#[cfg(feature = "avd")]
use std::path::PathBuf;

#[cfg(feature = "avd")]
use crate::avd;
use crate::{commands::Command, device::Backend};

#[cfg_attr(feature = "clap", derive(clap::Args))]
pub struct SetupArgs {
    #[cfg_attr(feature = "clap", command(subcommand))]
    backend: SetupBackend,
}

#[cfg_attr(feature = "clap", derive(clap::Subcommand))]
pub enum SetupBackend {
    /// Install the Android SDK, Emulator and system image
    #[cfg(feature = "avd")]
    Avd {
        /// Install the Android SDK tools
        /// This includes the SDK Manager, platform tools, and build tools
        /// By default, this is prompted if the tools are not found
        #[cfg_attr(feature = "clap", arg(long = "sdk", default_value_t = false))]
        install_sdk: bool,

        /// Install the Android Emulator and system image
        #[cfg_attr(feature = "clap", arg(long = "emulator", default_value_t = false))]
        install_emulator: bool,

        /// Path to the Android SDK Manager
        #[cfg_attr(feature = "clap", arg(long))]
        sdk_manager_path: Option<PathBuf>,

        /// System image of AVD (Android Virtual Device)
        #[cfg_attr(feature = "clap", arg(long = "image", default_value_t = avd::default_avd_image()))]
        system_image: String,
    },

    /// Initialize Waydroid's images and check ARM translation is configured
    #[cfg(feature = "waydroid")]
    Waydroid {
        /// System image type passed to `waydroid init -s`
        #[cfg_attr(feature = "clap", arg(long, default_value = "VANILLA"))]
        system_type: String,
    },
}

impl Command for SetupArgs {
    fn execute(
        self,
        // Only the emulator setup prompts.
        ctx: &crate::commands::GlobalContext,
    ) -> color_eyre::Result<()> {
        match self.backend {
            #[cfg(feature = "avd")]
            SetupBackend::Avd {
                install_sdk,
                install_emulator,
                sdk_manager_path,
                system_image,
            } => {
                Backend::Emulator.remember();
                avd::setup(
                    ctx,
                    &avd::SetupOptions {
                        install_sdk,
                        install_emulator,
                        sdk_manager_path,
                        system_image,
                    },
                )
            }
            #[cfg(feature = "waydroid")]
            SetupBackend::Waydroid { system_type } => {
                Backend::Waydroid.remember();
                crate::waydroid::setup(&system_type)
            }
        }
    }
}
