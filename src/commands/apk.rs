use std::path::{Path, PathBuf};

// use bytes::{BufMut, Bytes, BytesMut};
use color_eyre::eyre::{Context, ContextCompat, bail, eyre};
use itertools::Itertools;
use mbf_res_man::version_grabber;
use semver::Version;

use crate::{adb::AdbDevice, apk, commands::Command, device::DeviceArgs};

#[cfg_attr(feature = "clap", derive(clap::Parser))]
#[derive(Debug)]
pub struct ApkArgs {
    #[cfg_attr(feature = "clap", command(flatten))]
    device: DeviceArgs,

    #[cfg_attr(feature = "clap", command(subcommand))]
    action: ApkAction,
}

#[cfg_attr(feature = "clap", derive(clap::Subcommand))]
#[derive(Debug)]
pub enum ApkAction {
    /// Download an APK from Oculus Graph and optionally patch and install it
    Download {
        /// Oculus auth token, can be found in the browser devtools when logged in to oculus.com
        #[cfg_attr(feature = "clap", arg(long))]
        token: String,
        /// The ID of the APK to download, e.g. "com.beatgames.beatsaber" is 2448060205267927
        #[cfg_attr(feature = "clap", arg(long, default_value = "2448060205267927"))]
        graph_app_id: String,
        /// The version of the APK to download, e.g. "1.0.0"
        fuzzy_version: String,

        /// Output path, defaults to current directory
        output: Option<PathBuf>,

        /// Patches the APK after download to work in the emulator
        #[cfg_attr(feature = "clap", arg(long, default_value_t = true))]
        patch: bool,

        /// Installs APK and obb after download
        #[cfg_attr(feature = "clap", arg(long, default_value_t = false))]
        install: bool,

        /// Delete the downloaded version folder after a successful --install
        #[cfg_attr(
            feature = "clap",
            arg(long, default_value_t = false, requires = "install")
        )]
        cleanup: bool,
    },
    /// Patch an APK to work in the emulator.
    Patch {
        /// Path to the APK to patch
        path: PathBuf,
    },
    /// Install an APK and its OBB files to the device
    Install {
        /// An APK file, or a folder containing exactly one APK plus any OBB files:
        /// <folder>/
        ///     <anything>.apk
        ///     main.<version_code>.<package>.obb (optional)
        ///     patch.<version_code>.<package>.obb (optional)
        /// OBBs next to an APK file are picked up too. Nothing is deleted.
        #[cfg_attr(feature = "clap", arg(verbatim_doc_comment))]
        path: PathBuf,
    },
}

impl Command for ApkArgs {
    fn execute(self, _ctx: &crate::commands::GlobalContext) -> color_eyre::Result<()> {
        match self.action {
            ApkAction::Patch { path } => {
                apk::patch(&path)?;
            }
            ApkAction::Install { path } => {
                let (apk_path, folder) = if path.is_dir() {
                    let apks = files_with_extension(&path, "apk")?;
                    match apks.as_slice() {
                        [apk] => (apk.clone(), path.clone()),
                        [] => bail!("No .apk file found in {}", path.display()),
                        many => bail!(
                            "Multiple .apk files found in {}, pass one directly: {}",
                            path.display(),
                            many.iter().map(|p| p.display()).join(", ")
                        ),
                    }
                } else if path.exists() {
                    let folder = path.parent().map(Path::to_path_buf).unwrap_or_default();
                    (path.clone(), folder)
                } else {
                    bail!("APK file not found at path: {}", path.display());
                };

                let obb_files = files_with_extension(&folder, "obb")?;
                let device = self.device.resolve_adb()?;
                do_install(&device, &apk_path, &obb_files)?;
            }
            ApkAction::Download {
                token,
                graph_app_id,
                fuzzy_version,
                output,
                patch,
                install,
                cleanup,
            } => {
                let versions = version_grabber::get_live_versions(
                    &token,
                    Version::new(0, 0, 0),
                    &graph_app_id,
                )
                .map_err(|e| eyre!(e))?;

                let matching_version: &str = versions
                    // 1) Try exact match
                    .iter()
                    .find_map(|(v, _)| {
                        let exact = v.non_semver == fuzzy_version;
                        exact.then_some(&v.non_semver)
                    })
                    // 2) If no exact match, try a fuzzy match (contains / contained-by)
                    .or_else(|| {
                        versions.iter().find_map(|(v, _)| {
                            let ns = &v.non_semver;
                            let matches = ns.contains(&fuzzy_version) || fuzzy_version.contains(ns);
                            matches.then_some(ns)
                        })
                    })
                    // convert Option<&String> -> Option<&str> and fallback to the original input
                    .map(|s| s.as_str())
                    .unwrap_or(&fuzzy_version);

                println!("Downloading {} version {}", graph_app_id, matching_version);

                let output = output.unwrap_or("./apk".into());
                let downloaded = version_grabber::download_version(
                    &token,
                    &versions,
                    matching_version,
                    false,
                    &output,
                    false,
                )
                .map_err(|e| eyre!(e))?
                .context("Version not found")?;

                let version_folder = output.join(&downloaded.main.version);
                let apk_path = version_folder.join(format!("{}.apk", &downloaded.main.id));

                println!(
                    "Downloaded {} version {}",
                    downloaded.main.id, matching_version
                );
                match patch {
                    true => {
                        println!("Patching APK");
                        apk::patch(&apk_path)?
                    }
                    false => {
                        println!(
                            "You may need to patch the APK to work in the emulator using `apk patch`",
                        );
                    }
                }

                if install {
                    let obb_files = files_with_extension(&version_folder, "obb")?;
                    let device = self.device.resolve_adb()?;
                    do_install(&device, &apk_path, &obb_files)?;
                    if cleanup {
                        std::fs::remove_dir_all(&version_folder).with_context(|| {
                            format!("Failed to remove {}", version_folder.display())
                        })?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// Files directly inside `dir` with the given extension, sorted by name.
fn files_with_extension(dir: &Path, extension: &str) -> color_eyre::Result<Vec<PathBuf>> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .with_context(|| format!("Failed to read directory {}", dir.display()))?
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
        })
        .collect();
    files.sort();
    Ok(files)
}

/// Installs `apk_path` on `device`, then pushes `obb_files` into
/// `/sdcard/Android/obb/<package>/`, with the package read from the APK's
/// manifest (not from file names, which may be e.g. an Oculus store id).
/// Never deletes anything on the host.
fn do_install(
    device: &AdbDevice,
    apk_path: &Path,
    obb_files: &[PathBuf],
) -> Result<(), color_eyre::eyre::Error> {
    let package = apk::read_package_name(apk_path)?;
    println!(
        "Installing {} ({package}) to {}",
        apk_path.display(),
        device.describe()
    );

    device
        .install(&apk_path.display().to_string())
        .with_context(|| format!("Failed to install {}", apk_path.display()))?;

    if !obb_files.is_empty() {
        let obb_device_path = format!("/sdcard/Android/obb/{package}");
        device
            .mkdir(&obb_device_path)
            .context("Failed to create obb directory")?;

        for obb in obb_files {
            println!("Pushing {}", obb.display());
            let local_path = obb.display().to_string();
            let remote_path = format!("{obb_device_path}/{}", obb.file_name().unwrap().to_string_lossy());
            device
                .push(&local_path, &remote_path)
                .with_context(|| format!("Failed to push {}", obb.display()))?;
        }
    }

    println!("Successfully installed {package}");
    Ok(())
}
