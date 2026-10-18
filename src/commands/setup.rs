use std::{io::Cursor, path::PathBuf};

use bytes::{BufMut, BytesMut};
use color_eyre::eyre::{Context, bail};

use crate::{
    commands::Command,
    constants::{
        self, ANDROID_SDK_TOOLS, android_sdk_path, cmdline_tools_path, emulator_path,
    },
    device::Backend,
    downloader,
    waydroid,
};

#[cfg_attr(feature = "clap", derive(clap::Args))]
pub struct SetupArgs {
    /// Device backend to set up (auto-detected if omitted)
    #[cfg_attr(feature = "clap", arg(long))]
    backend: Option<Backend>,

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
    #[cfg_attr(feature = "clap", arg(long = "image", default_value_t = constants::default_avd_image()))]
    system_image: String,

    /// Waydroid: system image type passed to `waydroid init -s`
    #[cfg_attr(feature = "clap", arg(long, default_value = "VANILLA"))]
    waydroid_system_type: String,
}

impl Command for SetupArgs {
    fn execute(self, ctx: &crate::commands::GlobalContext) -> color_eyre::Result<()> {
        let backend = Backend::resolve(self.backend);
        backend.remember();
        match backend {
            Backend::Waydroid => waydroid::setup(ctx, &self.waydroid_system_type),
            Backend::Emulator => self.setup_emulator(ctx),
        }
    }
}

impl SetupArgs {
    fn setup_emulator(self, ctx: &crate::commands::GlobalContext) -> color_eyre::Result<()> {
        let sdk_manager = constants::sdkmanager_path();

        println!("Using Android SDK path: {}", android_sdk_path().display());

        if !sdk_manager.exists() {
            let accepted = ctx.yes
                || self.install_sdk
                || dialoguer::Confirm::new()
                    .with_prompt(
                        "Android SDK Manager not found. Do you want to download and set it up?",
                    )
                    .interact()?;
            if accepted {
                setup_sdk_manager().context("Failed to set up SDK Manager")?;
            }
        }

        let android_emu_image_installed = constants::emulator_path().exists()
            && constants::android_sdk_path().join("platform-tools").exists()
            && constants::android_sdk_path()
                .join(self.system_image.replace(";", "/"))
                .exists();

        let android_emu_image = !android_emu_image_installed
            && (ctx.yes
                || self.install_emulator
                || dialoguer::Confirm::new()
                    .with_prompt("Do you want to install the Android Emulator and system image?")
                    .interact()?);
        if android_emu_image {
            install_tools(&sdk_manager, &self.system_image)?;
        }

        println!(
            "Add {} to your PATH.",
            sdk_manager.parent().unwrap().display()
        );
        println!(
            "Add {} to your PATH.",
            emulator_path().parent().unwrap().display()
        );

        Ok(())
    }
}

pub fn install_tools(sdk_manager: &PathBuf, image: &str) -> Result<(), color_eyre::eyre::Error> {
    let status = std::process::Command::new(sdk_manager)
        .arg("emulator")
        .arg("platform-tools")
        .arg(image)
        .status()
        .context("Failed to run sdkmanager")?;
    if !status.success() {
        bail!("sdkmanager exited with status: {}", status);
    };
    Ok(())
}

pub fn setup_sdk_manager() -> color_eyre::Result<()> {
    println!("Android SDK Tools not found, downloading...");
    println!("Adding to path: {}", android_sdk_path().display());

    let mut zip_tmp = BytesMut::new().writer();
    downloader::download_with_progress(None, ANDROID_SDK_TOOLS, &mut zip_tmp)
        .context("Failed to download Android SDK Tools")?;

    let zip_cursor = Cursor::new(zip_tmp.into_inner());

    let mut zip = zip::ZipArchive::new(zip_cursor).context("Failed to read downloaded zip file")?;
    zip.extract_unwrapped_root_dir(cmdline_tools_path(), zip::read::root_dir_common_filter)
        .context("Failed to extract Android SDK Tools")?;
    Ok(())
}
