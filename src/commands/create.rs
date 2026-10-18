use color_eyre::eyre::{Context, bail};

use crate::{
    commands::{Command, GlobalContext},
    constants::{self, avd_path},
    device::Backend,
};

#[cfg_attr(feature = "clap", derive(clap::Parser))]
pub struct CreateArgs {
    /// Device backend (auto-detected if omitted). Only the emulator has AVDs.
    #[cfg_attr(feature = "clap", arg(long))]
    backend: Option<Backend>,

    /// Create an AVD (Android Virtual Device)
    #[cfg_attr(feature = "clap", arg(long = "avd", default_value_t = false))]
    create_avd: bool,

    /// Overwrite existing AVD (Android Virtual Device)
    #[cfg_attr(feature = "clap", arg(long = "overwrite", default_value_t = false))]
    overwrite_avd: bool,

    /// Name of AVD (Android Virtual Device)
    #[cfg_attr(feature = "clap", arg(long, default_value_t = constants::DEFAULT_AVD_NAME.to_string()))]
    name: String,

    /// Screen size for the AVD (Android Virtual Device), e.g. "1920x1080"
    #[cfg_attr(feature = "clap", arg(long = "screen-size", default_value = "1920x1080"))]
    screen_size: String,

    /// Limit the emulator FPS to save CPU/GPU resources (0 = unlimited)
    #[cfg_attr(feature = "clap", arg(long = "fps", default_value_t = 60))]
    fps_limit: u32,

    /// System image of AVD (Android Virtual Device)
    #[cfg_attr(feature = "clap", arg(long = "image", default_value_t = constants::default_avd_image()))]
    system_image: String,
}

impl Command for CreateArgs {
    fn execute(self, ctx: &GlobalContext) -> color_eyre::Result<()> {
        if Backend::resolve(self.backend) == Backend::Waydroid {
            bail!(
                "Waydroid has no AVDs, so there is nothing to create. \
                 Use `setup --backend waydroid`, then `start --backend waydroid`"
            );
        }

        let android_emu_image_installed = constants::emulator_path().exists()
            && constants::android_sdk_path().join("platform-tools").exists()
            && constants::android_sdk_path()
                .join(self.system_image.replace(";", "/"))
                .exists();

        if !android_emu_image_installed {
            bail!(
                "The specified system image '{}' is not installed. Please run the 'setup' command to install the Android Emulator and system image.",
                self.system_image
            );
        }

        let mut avd_folder_name = self.name.clone();
        avd_folder_name.push_str(".avd");
        if avd_path().join(&avd_folder_name).exists() {
            let overwrite_avd = (ctx.yes || self.overwrite_avd)
                || dialoguer::Confirm::new()
                    .with_prompt("An existing AVD (Android Virtual Device) was found, do you want to delete this?")
                    .interact()?;

            match overwrite_avd {
                true => delete_emulator(&self.name)?,
                false => bail!("An existing AVD (Android Virtual Device) was found!"),
            }
        }

        let create_avd = ctx.yes
            || self.create_avd
            || dialoguer::Confirm::new()
                .with_prompt("Do you want to create an AVD (Android Virtual Device)?")
                .interact()?;
        if create_avd {
            create_emulator_with_screen_size(
                &self.name,
                &self.system_image,
                &self.screen_size,
                self.fps_limit,
            )?;
            println!(
                "Run the emulator using the '{} start --name {}'",
                std::env::var("CARGO_BIN_NAME").unwrap_or_else(|_| "quest_emu".to_string()),
                self.name
            );
        }

        Ok(())
    }
}

pub fn create_emulator(name: &str, image: &str) -> Result<(), color_eyre::eyre::Error> {
    let status = std::process::Command::new(constants::avdmanager_path())
        .arg("create")
        .arg("avd")
        .arg("-n")
        .arg(name)
        .arg("-k")
        .arg(image)
        .status()
        .context("Failed to create AVD (Android Virtual Device)")?;
    if !status.success() {
        bail!("avdmanager exited with status: {}", status);
    };
    Ok(())
}

/// Create an emulator with a specific screen size (e.g. "1080x1920")
pub fn create_emulator_with_screen_size(
    name: &str,
    image: &str,
    screen_size: &str,
    fps: u32,
) -> Result<(), color_eyre::eyre::Error> {
    // Create the AVD first
    create_emulator(name, image)?;
    // Set the screen size in config.ini
    let avd_dir = avd_path().join(format!("{}.avd", name));
    let config_path = avd_dir.join("config.ini");
    if config_path.exists() {
        use std::fs;
        use std::io::Write;
        let mut config = fs::OpenOptions::new().append(true).open(&config_path)?;
        writeln!(
            config,
            "hw.lcd.width={}",
            screen_size.split('x').next().unwrap_or("1080")
        )?;
        writeln!(
            config,
            "hw.lcd.height={}",
            screen_size.split('x').nth(1).unwrap_or("1920")
        )?;
        writeln!(config, "hw.lcd.vsync={fps}")?;
        writeln!(config, "hw.gpu.enabled=yes")?;
        writeln!(config, "hw.gpu.mode=auto")?;
    }
    Ok(())
}

pub fn delete_emulator(name: &str) -> Result<(), color_eyre::eyre::Error> {
    let status = std::process::Command::new(constants::avdmanager_path())
        .arg("delete")
        .arg("avd")
        .arg("-n")
        .arg(name)
        .status()
        .context("Failed to delete AVD (Android Virtual Device)")?;

    if !status.success() {
        bail!("avdmanager exited with status: {}", status);
    }
    Ok(())
}
