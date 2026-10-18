use std::{env, path::PathBuf};

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

/// Returns the path to the Android SDK
/// Checks the ANDROID_SDK_ROOT and ANDROID_HOME environment variables
/// If not set, defaults to {home}/Android/Sdk
pub fn android_sdk_path() -> PathBuf {
    std::env::var("ANDROID_SDK_ROOT")
        .or_else(|_| env::var("ANDROID_HOME"))
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            let home = dirs::home_dir()?;

            Some(home.join("Android/Sdk/"))
        })
        .expect("Could not find Android SDK path. Please set ANDROID_SDK_ROOT or ANDROID_HOME environment variable.")
}

/// Returns the path to the Android NDK
/// Checks the ANDROID_NDK_HOME environment variable
/// If not set, defaults to {home}/Android/Sdk/ndk-bundle
pub fn android_ndk_path() -> PathBuf {
    std::env::var("ANDROID_NDK_HOME")
        .ok()
        .map(PathBuf::from)
        // TODO: Find it in the SDK's ndk directory, if there are multiple versions installed
        // .or_else(|| {
        //     let home = dirs::home_dir()?;
        //     Some(home.join("Android/Sdk/ndk-bundle"))
        // })
        .expect(
            "Could not find Android NDK path. Please set ANDROID_NDK_HOME environment variable.",
        )
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

/// Returns the adb executable to run: the SDK's ({sdk}/platform-tools/adb[.exe])
/// if installed, otherwise `adb` from PATH (e.g. a Waydroid-only setup with
/// distro-packaged platform-tools).
pub fn adb_path() -> PathBuf {
    let mut path = android_sdk_path();
    path.push("platform-tools");
    path.push("adb");
    if cfg!(target_os = "windows") {
        path.set_extension("exe");
    }
    if path.exists() {
        path
    } else {
        PathBuf::from("adb")
    }
}

/// Maps an Android ABI (as reported by e.g. `adb shell getprop ro.product.cpu.abi`)
/// or an already-NDK-style name to the directory name NDK uses under
/// `lib/clang/<version>/lib/linux/`.
fn ndk_lib_arch(arch: &str) -> &str {
    match arch {
        "aarch64" | "arm64-v8a" | "arm64" => "aarch64",
        "x86" | "i386" | "i686" => "i386",
        "x86_64" | "amd64" => "x86_64",
        "arm" | "armeabi-v7a" | "armeabi" => "arm",
        other => other,
    }
}

/// Returns the path to the lldb-server executable for the given architecture
/// {ndk}/toolchains/llvm/prebuilt/{host}/lib/clang/<version>/lib/linux/{arch}/lldb-server
pub fn lldb_server_path(arch: &str) -> PathBuf {
    let ndk = crate::constants::android_ndk_path();
    let arch = ndk_lib_arch(arch);

    // The NDK only ships x86_64 prebuilt host toolchains (even on arm64 hosts,
    // e.g. Apple Silicon runs the darwin-x86_64 build via Rosetta), so the
    // host tag doesn't depend on the host's CPU architecture.
    #[cfg(target_os = "windows")]
    let host_os = "windows-x86_64";
    #[cfg(target_os = "linux")]
    let host_os = "linux-x86_64";
    #[cfg(target_os = "macos")]
    let host_os = "darwin-x86_64";

    // find clang version from ndk path
    let clang_path = ndk
        .join("toolchains")
        .join("llvm")
        .join("prebuilt")
        .join(host_os)
        .join("lib")
        .join("clang");

    let clang_versioned_path = {
        clang_path
            .read_dir()
            .expect("Failed to read clang directory")
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
            .max_by_key(|entry| entry.file_name())
            .expect("Failed to find clang version in NDK path")
            .path()
    };

    // let clang_version = {
    //     std::fs::read_dir(clang_path)
    //         .expect("Failed to read clang directory")
    //         .filter_map(|entry| entry.ok())
    //         .filter(|entry| entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
    //         .map(|entry| entry.file_name().into_string().unwrap())
    //         .max()
    //         .expect("Failed to find clang version in NDK path")
    // };

    clang_versioned_path
        .join("lib")
        .join("linux")
        .join(arch)
        .join("lldb-server")
}
