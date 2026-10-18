use std::{
    io::Write,
    process::{Child, Command, Output, Stdio},
    thread,
    time::Duration,
};

use color_eyre::eyre::{Context, ContextCompat, bail};

use crate::constants::adb_path;

/// A handle to one adb-reachable device. Every adb invocation goes through
/// [`AdbDevice::command`], so `-s <serial>` is applied consistently when a specific
/// device was selected (see `device::DeviceArgs::resolve_adb`).
#[derive(Debug, Clone, Default)]
pub struct AdbDevice {
    /// `None` targets adb's default device (or `$ANDROID_SERIAL`).
    pub serial: Option<String>,
}

/// One row of `adb devices`.
#[derive(Debug, Clone)]
pub struct DeviceEntry {
    pub serial: String,
    /// `device`, `offline`, `unauthorized`, ...
    pub state: String,
}

/// Lists everything the adb server knows about, in any state.
pub fn list_devices() -> color_eyre::Result<Vec<DeviceEntry>> {
    let output = Command::new(adb_path())
        .arg("devices")
        .output()
        .context("Failed to run `adb devices`")?;
    if !output.status.success() {
        bail!("`adb devices` exited with status {}", output.status);
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .skip(1) // "List of devices attached"
        .filter_map(|line| {
            let (serial, state) = line.split_once('\t')?;
            Some(DeviceEntry {
                serial: serial.trim().to_string(),
                state: state.trim().to_string(),
            })
        })
        .collect())
}

/// `adb connect <addr>`. adb exits 0 even when the connection fails, so the
/// output is checked too.
pub fn connect_tcp(addr: &str) -> color_eyre::Result<()> {
    let output = Command::new(adb_path())
        .args(["connect", addr])
        .output()
        .with_context(|| format!("Failed to run `adb connect {addr}`"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !output.status.success() || !stdout.contains("connected to") {
        bail!("`adb connect {addr}` failed: {stdout}");
    }
    Ok(())
}

impl AdbDevice {
    pub fn new(serial: Option<String>) -> Self {
        Self { serial }
    }

    /// A bare `adb` command targeting this device.
    pub fn command(&self) -> Command {
        let mut command = Command::new(adb_path());
        if let Some(serial) = &self.serial {
            command.args(["-s", serial]);
        }
        command
    }

    /// Human-readable name for messages.
    pub fn describe(&self) -> &str {
        self.serial.as_deref().unwrap_or("the default adb device")
    }

    /// Runs `adb <args>`, inheriting stdio, and fails on a non-zero exit.
    pub fn status(&self, args: &[&str]) -> color_eyre::Result<()> {
        let status = self
            .command()
            .args(args)
            .status()
            .with_context(|| format!("Failed to run `adb {}`", args.join(" ")))?;
        if !status.success() {
            bail!("`adb {}` exited with {status}", args.join(" "));
        }
        Ok(())
    }

    /// Runs apk install, failing on a non-zero exit with stderr attached.
    pub fn install(&self, apk: &str) -> color_eyre::Result<()> {
        self.status(&["install", "-r", apk])
            .with_context(|| format!("Failed to install {apk}"))
    }

    /// Runs `adb uninstall <package>`, failing on a non-zero exit with stderr attached.
    pub fn uninstall(&self, package: &str) -> color_eyre::Result<()> {
        self.status(&["uninstall", package])
            .with_context(|| format!("Failed to uninstall {package}"))
    }

    /// Runs `adb shell mkdir -p <path>`, failing on a non-zero exit with stderr attached.
    pub fn mkdir(&self, path: &str) -> color_eyre::Result<()> {
        self.status(&["shell", "mkdir", "-p", path])
            .with_context(|| format!("Failed to create directory {path}"))
    }

    /// Runs `adb push <local> <remote>`, failing on a non-zero exit with stderr attached.
    pub fn push(&self, local: &str, remote: &str) -> color_eyre::Result<()> {
        self.status(&["push", local, remote])
            .with_context(|| format!("Failed to push {local} to {remote}"))
    }

    /// Runs `adb pull <remote> <local>`, failing on a non-zero exit with stderr attached.
    pub fn pull(&self, remote: &str, local: &str) -> color_eyre::Result<()> {
        self.status(&["pull", remote, local])
            .with_context(|| format!("Failed to pull {remote} to {local}"))
    }

    /// Runs `adb shell <args>`, echoes its output, and fails on a non-zero exit.
    /// Returns the captured output for commands that report failure in it
    /// while still exiting 0 (see `start_activity`).
    pub fn shell(&self, args: &[&str]) -> color_eyre::Result<Output> {
        let output = self
            .command()
            .arg("shell")
            .args(args)
            .output()
            .with_context(|| format!("Failed to run `adb shell {}`", args.join(" ")))?;
        std::io::stdout().write_all(&output.stdout)?;
        std::io::stderr().write_all(&output.stderr)?;
        if !output.status.success() {
            bail!("`adb shell {}` exited with {}", args.join(" "), output.status);
        }
        Ok(output)
    }

    /// Runs `adb shell chmod <mode> <path>`.
    pub fn chmod(&self, mode: &str, path: &str) -> color_eyre::Result<()> {
        self.shell(&["chmod", mode, path])
            .map(drop)
            .with_context(|| format!("Failed to chmod {path}"))
    }

    /// Runs `adb shell run-as <package> <args>`, i.e. as the app's own user,
    /// relative to its private data directory. Requires a debuggable app.
    pub fn run_as(&self, package: &str, args: &[&str]) -> color_eyre::Result<()> {
        let mut full = vec!["run-as", package];
        full.extend_from_slice(args);
        self.shell(&full)
            .map(drop)
            .with_context(|| format!("Failed to run `{}` as {package}", args.join(" ")))
    }

    /// Spawns `adb shell run-as <package> <args>` in the background with its
    /// output discarded, for long-running processes like servers.
    pub fn spawn_run_as(&self, package: &str, args: &[&str]) -> color_eyre::Result<Child> {
        self.command()
            .args(["shell", "run-as", package])
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("Failed to spawn `{}` as {package}", args.join(" ")))
    }

    /// Runs `adb forward tcp:<local> tcp:<remote>`.
    pub fn forward_tcp(&self, local: u16, remote: u16) -> color_eyre::Result<()> {
        self.status(&["forward", &format!("tcp:{local}"), &format!("tcp:{remote}")])
            .with_context(|| format!("Failed to forward tcp:{local} to tcp:{remote}"))
    }

    /// Runs `adb shell am start -n <component>`, with `-S` to force-stop the
    /// app first when `force_stop` is set.
    ///
    /// `am start` prints failures to stderr and still exits 0 (`Error type 3` /
    /// `Error: Activity class {...} does not exist`), so the output is checked
    /// instead of the exit status.
    pub fn start_activity(&self, component: &str, force_stop: bool) -> color_eyre::Result<()> {
        let mut args = vec!["am", "start"];
        if force_stop {
            args.push("-S");
        }
        args.extend(["-n", component]);

        let output = self
            .shell(&args)
            .with_context(|| format!("Failed to start {component}"))?;
        if [output.stdout, output.stderr]
            .iter()
            .any(|out| String::from_utf8_lossy(out).contains("Error"))
        {
            bail!("Failed to start {component}");
        }
        Ok(())
    }

    /// Starts `package`'s launcher activity (see `resolve_launch_component`),
    /// force-stopping it first when `force_stop` is set.
    pub fn launch(&self, package: &str, force_stop: bool) -> color_eyre::Result<()> {
        let component = self.resolve_launch_component(package).with_context(|| {
            format!("Is {package} installed on {}?", self.describe())
        })?;
        self.start_activity(&component, force_stop)
    }

    /// Runs `adb shell <args>` and returns trimmed stdout, failing on a non-zero
    /// exit with stderr (or stdout, since many shell tools report there) attached.
    pub fn shell_output(&self, args: &[&str]) -> color_eyre::Result<String> {
        let output = self
            .command()
            .arg("shell")
            .args(args)
            .output()
            .with_context(|| format!("Failed to run `adb shell {}`", args.join(" ")))?;

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let detail = if stderr.is_empty() { &stdout } else { &stderr };
            bail!(
                "`adb shell {}` exited with {}{}",
                args.join(" "),
                output.status,
                if detail.is_empty() { String::new() } else { format!(": {detail}") }
            );
        }
        Ok(stdout)
    }

    /// `getprop <key>`; empty string if unset.
    pub fn getprop(&self, key: &str) -> color_eyre::Result<String> {
        self.shell_output(&["getprop", key])
    }

    /// Polls `adb shell pidof` until `package` shows up as a running process.
    pub fn wait_for_pid(&self, package: &str) -> color_eyre::Result<String> {
        println!("Waiting for {package} to start...");
        (0..20)
            .find_map(|_| {
                let output = self.command().args(["shell", "pidof", package]).output().ok()?;
                let pid = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if pid.is_empty() {
                    thread::sleep(Duration::from_millis(500));
                    None
                } else {
                    Some(pid)
                }
            })
            .context("Timed out waiting for the app to start")
    }

    /// Resolves `package`'s launcher activity component (`package/.Activity`) via
    /// `cmd package resolve-activity`, for use with `am start -n`. Needed because
    /// `am start -a android.intent.action.MAIN -c android.intent.category.LAUNCHER -p
    /// <package>` fails to resolve for activities that declare additional categories
    /// alongside LAUNCHER (e.g. Quest apps also declaring `com.oculus.intent.category.VR`,
    /// which makes them non-`isDefault` and unresolvable by that implicit-intent form)
    /// even though the exact same activity resolves fine here and via `monkey`.
    pub fn resolve_launch_component(&self, package: &str) -> color_eyre::Result<String> {
        let output = self
            .command()
            .args([
                "shell",
                "cmd",
                "package",
                "resolve-activity",
                "--brief",
                "-c",
                "android.intent.category.LAUNCHER",
                package,
            ])
            .output()
            .context("Failed to resolve the app's launcher activity via adb")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout
            .lines()
            .map(str::trim)
            .find(|line| line.contains('/'))
            .map(str::to_string)
            .with_context(|| {
                format!(
                    "Could not resolve a launcher activity for {package}: {}",
                    stdout.trim()
                )
            })
    }

    /// The native-bridge library the device translates foreign-ABI code with
    /// (e.g. `libndk_translation.so` on Waydroid and x86_64 emulator images),
    /// or `None` if it runs everything natively.
    pub fn native_bridge(&self) -> color_eyre::Result<Option<String>> {
        let bridge = self.getprop("ro.dalvik.vm.native.bridge")?;
        Ok((!bridge.is_empty() && bridge != "0").then_some(bridge))
    }

    /// The ABI `package`'s native code actually executes as. This is *not*
    /// necessarily the device's own ABI: on an x86_64 device with ARM
    /// translation (Waydroid, emulator images with libndk_translation), an
    /// arm64-only app reports `ro.product.cpu.abi=x86_64` but runs arm64 code.
    ///
    /// Resolution order:
    /// 1. The package's `primaryCpuAbi` as recorded by the package manager.
    /// 2. With `package` unknown/not installed and a native bridge present,
    ///    `arm64-v8a` if the device advertises it (Quest apps are arm64).
    /// 3. `ro.product.cpu.abi`.
    pub fn guest_arch(&self, package: Option<&str>) -> color_eyre::Result<String> {
        if let Some(package) = package
            && let Some(abi) = self.package_primary_abi(package)?
        {
            return Ok(abi);
        }

        if self.native_bridge()?.is_some() {
            let abilist = self.getprop("ro.product.cpu.abilist")?;
            if abilist.split(',').any(|abi| abi == "arm64-v8a") {
                return Ok("arm64-v8a".to_string());
            }
        }

        let abi = self.getprop("ro.product.cpu.abi")?;
        if abi.is_empty() {
            bail!(
                "Could not detect the CPU ABI of {} (empty response from adb); \
                 pass the architecture explicitly",
                self.describe()
            );
        }
        Ok(abi)
    }

    /// `primaryCpuAbi` from `dumpsys package <package>`, if installed and known.
    fn package_primary_abi(&self, package: &str) -> color_eyre::Result<Option<String>> {
        let dump = self.shell_output(&["dumpsys", "package", package])?;
        Ok(dump
            .lines()
            .find_map(|line| line.trim().strip_prefix("primaryCpuAbi="))
            .map(str::trim)
            .filter(|abi| !abi.is_empty() && *abi != "null")
            .map(str::to_string))
    }

    /// Whether `package` is installed and marked debuggable, which `run-as`
    /// (and therefore lldb-server installation) requires.
    pub fn is_debuggable(&self, package: &str) -> color_eyre::Result<Option<bool>> {
        let dump = self.shell_output(&["dumpsys", "package", package])?;
        let Some(flags) = dump
            .lines()
            .map(str::trim)
            .find_map(|line| line.strip_prefix("pkgFlags="))
        else {
            // Not installed (or a dumpsys format we don't recognise).
            return Ok(None);
        };
        Ok(Some(flags.contains("DEBUGGABLE")))
    }
}
