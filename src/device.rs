//! Device backend selection: which kind of Android device `quest_emu` manages
//! (`setup`/`create`/`start`/`stop`), and which adb device the adb-only
//! commands (`apk install`, `debug`) talk to.

use color_eyre::eyre::bail;
use itertools::Itertools;

#[cfg(feature = "avd")]
use crate::avd;
use crate::adb::{self, AdbDevice};
#[cfg(feature = "waydroid")]
use crate::waydroid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
pub enum Backend {
    /// The Android SDK emulator (AVDs)
    #[cfg(feature = "avd")]
    Emulator,
    /// A Waydroid container, reached over TCP adb
    #[cfg(feature = "waydroid")]
    Waydroid,
}

impl Backend {
    fn as_str(self) -> &'static str {
        match self {
            #[cfg(feature = "avd")]
            Backend::Emulator => "emulator",
            #[cfg(feature = "waydroid")]
            Backend::Waydroid => "waydroid",
        }
    }

    fn parse(s: &str) -> Option<Backend> {
        [
            #[cfg(feature = "avd")]
            Backend::Emulator,
            #[cfg(feature = "waydroid")]
            Backend::Waydroid,
        ]
        .into_iter()
            .find(|b| b.as_str() == s.trim())
    }

    /// `explicit`, or a best guess at what the user means:
    /// 1. A running Waydroid session.
    /// 2. A running emulator.
    /// 3. The backend last used by `setup`/`start` (see [`Backend::remember`]).
    /// 4. An installed Android Emulator.
    /// 5. An installed Waydroid.
    /// 6. The emulator, which `setup` knows how to install from scratch.
    pub fn resolve(explicit: Option<Backend>) -> Backend {
        if let Some(backend) = explicit {
            return backend;
        }

        let backend = Backend::detect();
        #[cfg(all(feature = "avd", feature = "waydroid"))]
        println!(
            "Using {} backend (auto-detected, override with --backend)",
            backend.as_str()
        );
        backend
    }

    fn detect() -> Backend {
        #[cfg(feature = "waydroid")]
        let waydroid_status = waydroid::try_status();
        #[cfg(feature = "waydroid")]
        if waydroid_status.as_ref().is_some_and(|s| s.session_running) {
            return Backend::Waydroid;
        }

        #[cfg(feature = "avd")]
        if avd::is_running() {
            return Backend::Emulator;
        }

        if let Some(last) = Backend::last_used() {
            return last;
        }
        #[cfg(feature = "avd")]
        if avd::is_emulator_installed() {
            return Backend::Emulator;
        }

        #[cfg(feature = "waydroid")]
        if waydroid_status.is_some() {
            return Backend::Waydroid;
        }

        // The emulator is the default, as `setup` can install it from scratch.
        #[cfg(feature = "avd")]
        return Backend::Emulator;
        #[cfg(not(feature = "avd"))]
        Backend::Waydroid
    }

    fn state_file() -> Option<std::path::PathBuf> {
        Some(dirs::data_dir()?.join("quest_emu").join("last_backend"))
    }

    fn last_used() -> Option<Backend> {
        Backend::parse(&std::fs::read_to_string(Backend::state_file()?).ok()?)
    }

    /// Records `self` as the backend to auto-detect next time nothing is running.
    /// Best effort: failing to write it only affects future guesses.
    pub fn remember(self) {
        let Some(path) = Backend::state_file() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, self.as_str());
    }
}

/// Selects the device adb commands target.
#[cfg_attr(feature = "clap", derive(clap::Args))]
#[derive(Debug, Clone, Default)]
pub struct DeviceArgs {
    /// Device backend to target (auto-detected if omitted)
    #[cfg_attr(feature = "clap", arg(long, global = true))]
    pub backend: Option<Backend>,

    /// adb serial of the device to target, e.g. "emulator-5554" or
    /// "192.168.240.112:5555". Takes precedence over --backend and $ANDROID_SERIAL.
    #[cfg_attr(feature = "clap", arg(short = 's', long, global = true))]
    pub serial: Option<String>,
}

impl DeviceArgs {
    /// Picks exactly one adb device, in order:
    /// 1. `--serial` (TCP serials are `adb connect`ed if needed).
    /// 2. `$ANDROID_SERIAL`, which adb honours on its own.
    /// 3. `--backend waydroid`: the running Waydroid session.
    ///    `--backend emulator`: the only online `emulator-*` device.
    /// 4. No backend: the only online device, else a running Waydroid session.
    ///
    /// Never silently picks one of several devices.
    pub fn resolve_adb(&self) -> color_eyre::Result<AdbDevice> {
        if let Some(serial) = &self.serial {
            if serial.contains(':') && !is_online(serial)? {
                adb::connect_tcp(serial)?;
            }
            return Ok(AdbDevice::new(Some(serial.clone())));
        }

        if self.backend.is_none() && std::env::var_os("ANDROID_SERIAL").is_some() {
            return Ok(AdbDevice::new(None));
        }

        let serial = match self.backend {
            #[cfg(feature = "waydroid")]
            Some(Backend::Waydroid) => waydroid::ensure_connected()?,
            #[cfg(feature = "avd")]
            Some(Backend::Emulator) => {
                only_online(|d| d.serial.starts_with("emulator-"), "emulator", || {
                    bail!("No emulator is running. Start one with `quest_emu start avd`")
                })?
            }
            None => only_online(|_| true, "device", || {
                #[cfg(feature = "waydroid")]
                if waydroid::try_status().is_some_and(|s| s.session_running) {
                    return waydroid::ensure_connected();
                }
                bail!(
                    "No adb device is connected. Start one with `quest_emu start avd` \
                     or `quest_emu start waydroid`"
                )
            })?,
        };
        Ok(AdbDevice::new(Some(serial)))
    }
}

fn is_online(serial: &str) -> color_eyre::Result<bool> {
    Ok(adb::list_devices()?
        .iter()
        .any(|d| d.serial == serial && d.state == "device"))
}

/// The serial of the single online device matching `filter`; `none` decides
/// what to do if there isn't one, and several is an error.
fn only_online(
    filter: impl Fn(&adb::DeviceEntry) -> bool,
    kind: &str,
    none: impl FnOnce() -> color_eyre::Result<String>,
) -> color_eyre::Result<String> {
    let devices = adb::list_devices()?;
    let online = devices
        .iter()
        .filter(|d| d.state == "device" && filter(d))
        .collect_vec();

    match online.as_slice() {
        [device] => Ok(device.serial.clone()),
        [] => {
            let offline = devices.iter().filter(|d| d.state != "device").collect_vec();
            if !offline.is_empty() {
                eprintln!(
                    "Note: adb lists devices that aren't ready: {}",
                    offline
                        .iter()
                        .map(|d| format!("{} ({})", d.serial, d.state))
                        .join(", ")
                );
            }
            none()
        }
        many => bail!(
            "Multiple {kind}s are connected ({}); pick one with --serial",
            many.iter().map(|d| &d.serial).join(", ")
        ),
    }
}
