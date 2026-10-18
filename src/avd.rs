//! Android SDK emulator lifecycle: SDK/emulator/system image setup, AVD
//! creation, and starting/stopping AVDs.

use std::{
    io::Cursor,
    path::{Path, PathBuf},
    process,
};

use bytes::{BufMut, BytesMut};
use color_eyre::eyre::{Context, bail};
use itertools::Itertools;

use crate::{
    adb::{self, AdbDevice},
    commands::GlobalContext,
    constants::android_sdk_path,
    downloader,
};

#[cfg(target_os = "linux")]
pub const ANDROID_SDK_TOOLS: &str =
    "https://dl.google.com/android/repository/commandlinetools-linux-13114758_latest.zip";

#[cfg(target_os = "macos")]
pub const ANDROID_SDK_TOOLS: &str =
    "https://dl.google.com/android/repository/commandlinetools-mac-13114758_latest.zip";

#[cfg(target_os = "windows")]
pub const ANDROID_SDK_TOOLS: &str =
    "https://dl.google.com/android/repository/commandlinetools-win-13114758_latest.zip";

pub const DEFAULT_AVD_NAME: &str = "android13desktop";

/// Returns the default AVD system image, using an ABI that matches the host CPU.
/// QEMU2 (the Android Emulator backend) requires the system image architecture to
/// match the host architecture, so an aarch64 host (e.g. Apple Silicon, arm64 Linux)
/// needs an arm64-v8a image rather than the x86_64 one used on x86_64 hosts.
pub fn default_avd_image() -> String {
    let abi = if cfg!(target_arch = "aarch64") {
        "arm64-v8a"
    } else {
        "x86_64"
    };
    format!("system-images;android-33;android-desktop;{abi}")
}

/// Returns the path to the Android cmdline-tools latest directory
/// {sdk}/cmdline-tools/latest
pub fn cmdline_tools_path() -> PathBuf {
    let mut path = android_sdk_path();
    path.push("cmdline-tools");
    path.push("latest");
    path
}

/// Returns the path to the Android SDK Manager executable
/// {sdk}/cmdline-tools/latest/bin/sdkmanager[.bat]
pub fn sdkmanager_path() -> PathBuf {
    let mut path = cmdline_tools_path();
    path.push("bin");
    path.push("sdkmanager");
    if cfg!(target_os = "windows") {
        path.set_extension("bat");
    }
    path
}

/// Returns the path to the Android AVD Manager executable
/// {sdk}/cmdline-tools/latest/bin/avdmanager[.bat]
pub fn avdmanager_path() -> PathBuf {
    let mut path = cmdline_tools_path();
    path.push("bin");
    path.push("avdmanager");
    if cfg!(target_os = "windows") {
        path.set_extension("bat");
    }
    path
}

/// Returns the path to the Android AVDs
/// {home}/.android/avd or $ANDROID_AVD_HOME
pub fn avd_path() -> PathBuf {
    std::env::var("ANDROID_AVD_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            let home = dirs::home_dir()?;
            Some(home.join(".android").join("avd"))
        })
        .expect(
            "Could not find Android AVD path. Please set ANDROID_AVD_HOME environment variable.",
        )
}

/// Returns the path to the Android Emulator executable
/// {sdk}/emulator/emulator[.exe]
pub fn emulator_path() -> PathBuf {
    let mut path = android_sdk_path();
    path.push("emulator");
    path.push("emulator");
    if cfg!(target_os = "windows") {
        path.set_extension("exe");
    }
    path
}

/// Whether the emulator binary is installed at all.
pub fn is_emulator_installed() -> bool {
    emulator_path().exists()
}

/// Whether the emulator, platform-tools and `system_image` are all installed.
pub fn is_image_installed(system_image: &str) -> bool {
    is_emulator_installed()
        && android_sdk_path().join("platform-tools").exists()
        && android_sdk_path()
            .join(system_image.replace(";", "/"))
            .exists()
}

/// Whether adb sees a running emulator.
pub fn is_running() -> bool {
    adb::list_devices()
        .is_ok_and(|devices| devices.iter().any(|d| d.serial.starts_with("emulator-")))
}

pub struct SetupOptions {
    /// Download the SDK Manager without prompting if it's missing
    pub install_sdk: bool,
    /// Install the emulator and system image without prompting if missing
    pub install_emulator: bool,
    /// SDK Manager to use instead of `{sdk}/cmdline-tools/latest/bin/sdkmanager`
    pub sdk_manager_path: Option<PathBuf>,
    /// System image to install, e.g. `system-images;android-33;android-desktop;x86_64`
    pub system_image: String,
}

/// Downloads the SDK Manager if needed, then installs the emulator,
/// platform-tools and system image through it, prompting for each unless
/// `--yes` or the matching option is set.
pub fn setup(ctx: &GlobalContext, options: &SetupOptions) -> color_eyre::Result<()> {
    let sdk_manager = options
        .sdk_manager_path
        .clone()
        .unwrap_or_else(sdkmanager_path);

    println!("Using Android SDK path: {}", android_sdk_path().display());

    if !sdk_manager.exists() {
        let accepted = ctx.yes
            || options.install_sdk
            || dialoguer::Confirm::new()
                .with_prompt("Android SDK Manager not found. Do you want to download and set it up?")
                .interact()?;
        if accepted {
            setup_sdk_manager().context("Failed to set up SDK Manager")?;
        }
    }

    let install_image = !is_image_installed(&options.system_image)
        && (ctx.yes
            || options.install_emulator
            || dialoguer::Confirm::new()
                .with_prompt("Do you want to install the Android Emulator and system image?")
                .interact()?);
    if install_image {
        install_tools(&sdk_manager, &options.system_image)?;
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

pub fn install_tools(sdk_manager: &Path, image: &str) -> Result<(), color_eyre::eyre::Error> {
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

pub struct CreateOptions {
    /// Create the AVD without prompting
    pub create_avd: bool,
    /// Delete an existing AVD of the same name without prompting
    pub overwrite_avd: bool,
    pub name: String,
    /// e.g. "1920x1080"
    pub screen_size: String,
    /// 0 = unlimited
    pub fps_limit: u32,
    pub system_image: String,
}

/// Creates the AVD described by `options`, replacing an existing one of the
/// same name if confirmed.
pub fn create(ctx: &GlobalContext, options: &CreateOptions) -> color_eyre::Result<()> {
    if !is_image_installed(&options.system_image) {
        bail!(
            "The specified system image '{}' is not installed. Please run `quest_emu setup avd` to install the Android Emulator and system image.",
            options.system_image
        );
    }

    if avd_path().join(format!("{}.avd", options.name)).exists() {
        let overwrite_avd = (ctx.yes || options.overwrite_avd)
            || dialoguer::Confirm::new()
                .with_prompt("An existing AVD (Android Virtual Device) was found, do you want to delete this?")
                .interact()?;

        match overwrite_avd {
            true => delete_emulator(&options.name)?,
            false => bail!("An existing AVD (Android Virtual Device) was found!"),
        }
    }

    let create_avd = ctx.yes
        || options.create_avd
        || dialoguer::Confirm::new()
            .with_prompt("Do you want to create an AVD (Android Virtual Device)?")
            .interact()?;
    if create_avd {
        create_emulator_with_screen_size(
            &options.name,
            &options.system_image,
            &options.screen_size,
            options.fps_limit,
        )?;
        println!(
            "Run the emulator using '{} start avd --name {}'",
            std::env::var("CARGO_BIN_NAME").unwrap_or_else(|_| "quest_emu".to_string()),
            options.name
        );
    }

    Ok(())
}

pub fn create_emulator(name: &str, image: &str) -> Result<(), color_eyre::eyre::Error> {
    let status = std::process::Command::new(avdmanager_path())
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
    let status = std::process::Command::new(avdmanager_path())
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

pub struct StartOptions {
    /// Name of the AVD to start
    pub name: String,
    /// Start without loading a snapshot
    pub fresh: bool,
    /// Additional arguments to pass to the emulator
    pub args: Vec<String>,
}

/// Runs the emulator for `options.name` in the foreground, until it exits.
pub fn start(options: StartOptions) -> color_eyre::Result<()> {
    // use emulator @android13desktop -selinux permissive -feature QtRawKeyboardInput

    println!("Starting emulator with AVD name: {}", options.name);

    let mut command = process::Command::new(emulator_path());
    command
        .arg(format!("@{}", options.name))
        .arg("-selinux")
        .arg("permissive")
        .arg("-feature")
        .arg("QtRawKeyboardInput");

    if options.fresh {
        command.arg("-no-snapshot-load");
    }

    if !options.args.is_empty() {
        command.args(options.args);
    }

    println!(
        "{} {}",
        emulator_path().to_string_lossy(),
        command.get_args().map(|s| s.display()).join(" ")
    );

    let status = command.status().context("Failed to start emulator")?;

    if !status.success() {
        bail!("Emulator exited with status: {}", status);
    }

    Ok(())
}

/// Shuts down the emulator behind `device` (`adb emu kill`).
pub fn stop(device: &AdbDevice) -> color_eyre::Result<()> {
    println!("Stopping emulator {}...", device.describe());
    device.status(&["emu", "kill"])
}
