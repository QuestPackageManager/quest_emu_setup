#[cfg(feature = "avd")]
use crate::avd;
use crate::{commands::Command, device::Backend};

#[cfg_attr(feature = "clap", derive(clap::Parser))]
#[derive(Debug)]
pub struct StartArgs {
    #[cfg_attr(feature = "clap", command(subcommand))]
    backend: StartBackend,
}

#[cfg_attr(feature = "clap", derive(clap::Subcommand))]
#[derive(Debug)]
pub enum StartBackend {
    /// Run an AVD in the Android Emulator, until it exits
    #[cfg(feature = "avd")]
    Avd {
        /// Name of the AVD to start
        #[cfg_attr(feature = "clap", arg(long, default_value_t = avd::DEFAULT_AVD_NAME.to_string()))]
        name: String,

        /// Start the emulator without loading a snapshot
        #[cfg_attr(feature = "clap", arg(long, default_value_t = false))]
        fresh: bool,

        /// Additional arguments to pass to the emulator
        #[cfg_attr(feature = "clap", arg(last = true))]
        args: Vec<String>,
    },

    /// Start the Waydroid container, session and UI, then connect adb to it
    #[cfg(feature = "waydroid")]
    Waydroid {
        /// Seconds to wait for Android to boot and adb to connect
        #[cfg_attr(feature = "clap", arg(long, default_value_t = 120))]
        timeout: u64,
    },
}

impl Command for StartArgs {
    fn execute(self, _ctx: &crate::commands::GlobalContext) -> color_eyre::Result<()> {
        match self.backend {
            #[cfg(feature = "avd")]
            StartBackend::Avd { name, fresh, args } => {
                Backend::Emulator.remember();
                avd::start(avd::StartOptions { name, fresh, args })
            }
            #[cfg(feature = "waydroid")]
            StartBackend::Waydroid { timeout } => {
                Backend::Waydroid.remember();
                crate::waydroid::start(std::time::Duration::from_secs(timeout))?;
                Ok(())
            }
        }
    }
}
