//! Waydroid device lifecycle: setup, start, stop and adb reachability.
//!
//! Waydroid quirks this module encodes:
//! - A `FROZEN` container is normal (Waydroid freezes it while no UI is open)
//!   and makes adb report the device as `offline`. Opening the UI
//!   (`waydroid show-full-ui`) unfreezes it.
//! - The device is only reachable over TCP adb (`<ip>:5555`) and has to be
//!   `adb connect`ed explicitly.
//! - `waydroid shell` needs root; everything on-device goes through adb instead.

use std::{
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use color_eyre::eyre::{Context, ContextCompat, bail};

use crate::adb::{self, AdbDevice};

const ADB_PORT: u16 = 5555;
const WAYDROID_CFG: &str = "/var/lib/waydroid/waydroid.cfg";
const WAYDROID_BASE_PROP: &str = "/var/lib/waydroid/waydroid_base.prop";
const ACCEPT_ADB_PROMPT: &str = "Accept the \"Allow USB debugging?\" prompt in the Waydroid window";

/// Parsed `waydroid status`.
#[derive(Debug, Clone, Default)]
pub struct WaydroidStatus {
    pub session_running: bool,
    /// `RUNNING`, `FROZEN`, `STOPPED`, ... (`None` when the session is stopped,
    /// since Waydroid omits the line then)
    pub container: Option<String>,
    pub ip: Option<String>,
}

impl WaydroidStatus {
    pub fn is_frozen(&self) -> bool {
        self.container.as_deref() == Some("FROZEN")
    }

    pub fn adb_serial(&self) -> Option<String> {
        self.ip.as_ref().map(|ip| format!("{ip}:{ADB_PORT}"))
    }
}

/// Whether the `waydroid` CLI is on PATH.
pub fn is_installed() -> bool {
    Command::new("waydroid")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Whether `waydroid init` has been run (images downloaded, config written).
pub fn is_initialized() -> bool {
    Path::new(WAYDROID_CFG).exists()
}

/// Whether ARM translation (libndk / libhoudini) is configured, judging by the
/// native bridge property in the base prop file.
pub fn has_arm_translation() -> bool {
    std::fs::read_to_string(WAYDROID_BASE_PROP).is_ok_and(|props| {
        props.lines().any(|line| {
            line.strip_prefix("ro.dalvik.vm.native.bridge=")
                .is_some_and(|v| !v.trim().is_empty() && v.trim() != "0")
        })
    })
}

pub fn status() -> color_eyre::Result<WaydroidStatus> {
    let output = Command::new("waydroid")
        .arg("status")
        .output()
        .context("Failed to run `waydroid status` (is Waydroid installed?)")?;
    if !output.status.success() {
        bail!("`waydroid status` exited with status {}", output.status);
    }

    let mut status = WaydroidStatus::default();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "Session" => status.session_running = value == "RUNNING",
            "Container" => status.container = Some(value.to_string()),
            "IP address" if !value.is_empty() && value != "UNKNOWN" => {
                status.ip = Some(value.to_string())
            }
            _ => {}
        }
    }
    Ok(status)
}

/// `status()`, or `None` if Waydroid isn't installed or can't report one.
pub fn try_status() -> Option<WaydroidStatus> {
    is_installed().then(status).and_then(Result::ok)
}

/// Makes the running Waydroid session reachable over adb and returns its
/// serial. Does not start anything: fails with a pointer to `start` instead.
pub fn ensure_connected() -> color_eyre::Result<String> {
    let status = status()?;
    if !status.session_running {
        bail!("The Waydroid session is not running. Start it with `quest_emu start waydroid`");
    }
    let serial = status
        .adb_serial()
        .context("Waydroid is running but did not report an IP address")?;

    if device_state(&serial)?.as_deref() != Some("device") {
        adb::connect_tcp(&serial)?;
    }

    match device_state(&serial)?.as_deref() {
        Some("device") => Ok(serial),
        state if status.is_frozen() => bail!(
            "Waydroid's container is FROZEN, so adb sees it as {}. \
             Open the UI (`waydroid show-full-ui`) or run `quest_emu start waydroid`",
            state.unwrap_or("missing")
        ),
        Some("unauthorized") => {
            bail!("Waydroid device {serial} is unauthorized in adb. {ACCEPT_ADB_PROMPT}")
        }
        state => bail!(
            "Waydroid device {serial} is {} in adb",
            state.unwrap_or("not listed")
        ),
    }
}

fn device_state(serial: &str) -> color_eyre::Result<Option<String>> {
    Ok(adb::list_devices()?
        .into_iter()
        .find(|d| d.serial == serial)
        .map(|d| d.state))
}

fn container_service_active() -> bool {
    Command::new("systemctl")
        .args(["is-active", "--quiet", "waydroid-container"])
        .status()
        .is_ok_and(|s| s.success())
}

/// Brings Waydroid up and returns its adb serial, in order:
/// 1. Start `waydroid-container` (root) if it isn't active.
/// 2. Spawn `waydroid show-full-ui`, detached: it starts the session if needed
///    and opening the UI unfreezes the container.
/// 3. Wait for the session to report an IP, then `adb connect` until the
///    device is online and has finished booting.
///
/// Idempotent: on an already-running instance this just re-opens the UI and
/// connects adb.
pub fn start(timeout: Duration) -> color_eyre::Result<String> {
    if !is_installed() {
        bail!("Waydroid is not installed. Run `quest_emu setup waydroid` first");
    }
    if !is_initialized() {
        bail!("Waydroid has not been initialized. Run `quest_emu setup waydroid` first");
    }

    if !container_service_active() {
        bail!(
            "Waydroid's container service is not running. Start it with `sudo systemctl start waydroid-container`"
        );
    }

    println!("Opening the Waydroid UI...");
    let mut ui = Command::new("waydroid");
    ui.arg("show-full-ui")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Own process group, so the session survives Ctrl+C in this terminal.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut ui, 0);
    ui.spawn().context("Failed to run `waydroid show-full-ui`")?;

    let deadline = Instant::now() + timeout;
    println!("Waiting for the Waydroid session...");
    let serial = poll_until(deadline, || {
        Ok(status()?
            .session_running
            .then(|| status().ok()?.adb_serial())
            .flatten())
    })
    .context("Timed out waiting for the Waydroid session to start")?;

    println!("Connecting adb to {serial}...");
    let mut hinted_unauthorized = false;
    poll_until(deadline, || {
        match device_state(&serial)?.as_deref() {
            Some("device") => return Ok(Some(())),
            Some("unauthorized") if !hinted_unauthorized => {
                hinted_unauthorized = true;
                println!("Note: adb reports {serial} as unauthorized. {ACCEPT_ADB_PROMPT}.");
            }
            _ => {}
        }
        // Fails until adbd is up; just retry.
        let _ = adb::connect_tcp(&serial);
        Ok(None)
    })
    .with_context(|| format!("Timed out waiting for adb to reach {serial}"))?;

    println!("Waiting for Android to finish booting...");
    let adb = AdbDevice::new(Some(serial.clone()));
    poll_until(deadline, || {
        Ok((adb.getprop("sys.boot_completed").unwrap_or_default() == "1").then_some(()))
    })
    .context("Timed out waiting for Android to finish booting")?;

    if !has_arm_translation() {
        eprintln!(
            "Warning: no ARM translation is configured, so arm64-only apps (e.g. Quest apps) \
             won't install. Run `quest_emu setup waydroid`."
        );
    }

    println!("Waydroid is ready: {serial}");
    Ok(serial)
}

fn poll_until<T>(
    deadline: Instant,
    mut f: impl FnMut() -> color_eyre::Result<Option<T>>,
) -> color_eyre::Result<T> {
    loop {
        if let Some(value) = f()? {
            return Ok(value);
        }
        if Instant::now() >= deadline {
            bail!("timed out");
        }
        thread::sleep(Duration::from_millis(1000));
    }
}

/// Disconnects adb and stops the Waydroid session (the container service is
/// left running, as it's cheap and system-managed).
pub fn stop() -> color_eyre::Result<()> {
    if let Some(serial) = status().ok().and_then(|s| s.adb_serial()) {
        let _ = Command::new(crate::constants::adb_path())
            .args(["disconnect", &serial])
            .stdout(Stdio::null())
            .status();
    }

    println!("Stopping the Waydroid session...");
    let status = Command::new("waydroid")
        .args(["session", "stop"])
        .status()
        .context("Failed to run `waydroid session stop`")?;
    if !status.success() {
        bail!("`waydroid session stop` exited with {status}");
    }
    Ok(())
}

/// Checks that Waydroid is installed, initialized (`waydroid init`) and has ARM
/// translation configured. Every fix needs root, so this never runs them:
/// it stops with the command for the user to run instead.
pub fn setup(system_type: &str) -> color_eyre::Result<()> {
    if !is_installed() {
        bail!(
            "Waydroid is not installed. Install it with your distro's package manager, e.g.:\n  \
             Debian/Ubuntu: curl -s https://repo.waydro.id | sudo bash && sudo apt install waydroid\n  \
             Arch:          sudo pacman -S waydroid  (or the AUR package)\n  \
             Fedora:        sudo dnf install waydroid\n\
             then re-run this command."
        );
    }

    if !is_initialized() {
        bail!(
            "Waydroid has not been initialized. Download its Android images with \
             `sudo waydroid init -s {system_type}`, then re-run this command."
        );
    }
    println!("Waydroid is initialized.");

    if !has_arm_translation() {
        bail!(
            "Waydroid has no ARM translation configured, and Quest apps are arm64-only, \
             so they won't install yet.\n\
             \n\
             ARM translation means installing libndk_translation (generally best on AMD CPUs) \
             or libhoudini (Intel) into Waydroid's system overlay \
             (/var/lib/waydroid/overlay) and enabling it with \
             `ro.dalvik.vm.native.bridge` in {WAYDROID_BASE_PROP}.\n\
             The usual way is waydroid_script (https://github.com/casualsnek/waydroid_script), \
             see its README for setup:\n  \
             waydroid session stop\n  \
             sudo python3 main.py install libndk\n  \
             sudo systemctl restart waydroid-container\n\
             Then re-run this command to verify."
        );
    }
    println!("ARM translation is configured.");

    println!("Waydroid is set up. Start it with `quest_emu start waydroid`");
    Ok(())
}
