// # macOS / Linux path pattern
// <path-to-ndk>/toolchains/llvm/prebuilt/<host-os>/lib/clang/<version>/lib/linux/aarch64/lldb-server

// # Windows path pattern
// <path-to-ndk>\toolchains\llvm\prebuilt\windows-x86_64\lib\clang\<version>\lib\linux\aarch64\lldb-server

use std::{
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use color_eyre::eyre::{Context, bail};

use crate::{
    adb::AdbDevice,
    commands::Command,
    constants,
    device::DeviceArgs,
    lldb::{self, AttachInfo},
};

#[cfg_attr(feature = "clap", derive(clap::Parser))]
#[derive(Debug)]
pub struct DebugArgs {
    #[cfg_attr(feature = "clap", command(flatten))]
    device: DeviceArgs,

    #[cfg_attr(feature = "clap", command(subcommand))]
    action: DebugAction,
}

#[cfg_attr(feature = "clap", derive(clap::Subcommand))]
#[derive(Debug)]
pub enum DebugAction {
    /// Push the NDK's lldb-server binary onto the device and copy it into the
    /// target app's private storage. Does not start it; use `attach` for that.
    /// The app must be debuggable (`android:debuggable="true"`).
    Install {
        /// Android package name of the app to debug, e.g. "com.beatgames.beatsaber"
        package: String,

        /// The architecture the app's native code runs as (e.g. "aarch64", "x86_64").
        /// Auto-detected from the installed app via `adb` if omitted.
        arch: Option<String>,
    },

    /// Install lldb-server (see `install`), launch it as a debug platform server,
    /// then attach to the app as it's already running (pass `--relaunch` to
    /// force-stop and relaunch it fresh first) and open a VS Code attach session
    /// for it via the lldb-dap extension (llvm-vs-code-extensions.lldb-dap).
    Attach {
        /// Android package name of the app to debug, e.g. "com.beatgames.beatsaber"
        package: String,

        /// The architecture the app's native code runs as (e.g. "aarch64", "x86_64").
        /// Auto-detected from the installed app via `adb` if omitted.
        arch: Option<String>,

        /// The port to forward and listen on
        #[cfg_attr(feature = "clap", arg(short, long, default_value_t = 5039))]
        port: u16,

        /// Path to the VS Code executable used to open the attach session (default: "code")
        #[cfg_attr(feature = "clap", arg(long, default_value_t = "code".to_string()))]
        code_bin: String,

        /// Launch VS Code with the lldb-dap attach URI. By default the URI is only
        /// printed, since it can also be opened manually or on another machine.
        #[cfg_attr(feature = "clap", arg(long, default_value_t = false))]
        open: bool,

        /// .so files (or directories to search for .so files in) with debug symbols
        /// to preload in lldb (`target symbols add`)
        #[cfg_attr(feature = "clap", arg(long = "symbols"))]
        symbols: Vec<PathBuf>,

        /// Write the target's pid to this file (e.g. for a VS Code task to feed
        /// into a launch.json config via the Command Variable extension)
        #[cfg_attr(feature = "clap", arg(long))]
        pid_file: Option<PathBuf>,

        /// Force-stop and relaunch the app fresh before attaching, instead of
        /// attaching to it as it's already running
        #[cfg_attr(feature = "clap", arg(long, default_value_t = false))]
        relaunch: bool,
    },
}

impl Command for DebugArgs {
    fn execute(self, _ctx: &super::GlobalContext) -> color_eyre::Result<()> {
        let device = self.device.resolve_adb()?;
        match self.action {
            DebugAction::Install { package, arch } => {
                install_lldb_server(&device, &package, arch.as_deref())?;
            }
            DebugAction::Attach {
                package,
                arch,
                port,
                code_bin,
                open,
                symbols,
                pid_file,
                relaunch,
            } => {
                install_lldb_server(&device, &package, arch.as_deref())?;
                start_lldb_platform_server(&device, &package, port)?;
                attach(
                    &device,
                    &package,
                    port,
                    &code_bin,
                    open,
                    &symbols,
                    pid_file.as_deref(),
                    relaunch,
                )?;
            }
        }

        Ok(())
    }
}

fn resolve_arch(
    device: &AdbDevice,
    package: &str,
    arch: Option<&str>,
) -> color_eyre::Result<String> {
    if let Some(arch) = arch {
        return Ok(arch.to_string());
    }

    let arch = device.guest_arch(Some(package))?;
    if let Some(bridge) = device.native_bridge()? {
        let host_abi = device.getprop("ro.product.cpu.abi")?;
        if host_abi != arch {
            eprintln!(
                "Note: {package} runs as {arch} through {bridge} on this {host_abi} device. \
                 Debugging translated code is limited; pass the arch explicitly to override."
            );
        }
    }
    Ok(arch)
}

/// `run-as` only works for debuggable apps, so fail early with a clear message
/// instead of `run-as`'s raw error.
fn ensure_debuggable(device: &AdbDevice, package: &str) -> color_eyre::Result<()> {
    match device.is_debuggable(package)? {
        Some(true) => Ok(()),
        Some(false) => bail!(
            "{package} is not debuggable, so lldb-server can't be installed into it via `run-as`. \
             `debug` needs a build with android:debuggable=\"true\" in its manifest"
        ),
        None => bail!("{package} is not installed on {}", device.describe()),
    }
}

const APP_LLDB_SERVER_PATH: &str = "./lldb-server";

/// Installs lldb-server into `package`'s private storage, in order:
/// 1. `adb push` lldb-server to shared storage (can't push directly into an app's
///    private storage).
/// 2. `chmod` it executable there.
/// 3. `run-as` `cp` it into `package`'s private storage, since lldb-server has to
///    run under the app's own SELinux/UID context to be allowed to attach to it.
/// 4. `chmod` it executable again (the copy doesn't preserve permissions).
fn install_lldb_server(
    device: &AdbDevice,
    package: &str,
    arch: Option<&str>,
) -> color_eyre::Result<()> {
    ensure_debuggable(device, package)?;
    let arch = resolve_arch(device, package, arch)?;
    let lldb_server_path = constants::lldb_server_path(&arch);
    let device_tmp_path = "/data/local/tmp/lldb-server";

    println!("Pushing lldb-server ({arch}) to device...");
    device.push(&lldb_server_path.display().to_string(), device_tmp_path)?;
    device.chmod("755", device_tmp_path)?;

    println!("Copying lldb-server into {package}'s private storage...");
    device.run_as(package, &["cp", device_tmp_path, APP_LLDB_SERVER_PATH])?;
    device.run_as(package, &["chmod", "755", APP_LLDB_SERVER_PATH])?;

    println!("lldb-server installed in {package}'s private storage.");
    Ok(())
}


/// Forwards `port` and launches the lldb-server already installed (see
/// `install_lldb_server`) in `package`'s private storage as a platform server,
/// in the background, so it's still listening after this function returns.
fn start_lldb_platform_server(device: &AdbDevice, package: &str, port: u16) -> color_eyre::Result<()> {
    println!("Forwarding port {port}");
    device.forward_tcp(port, port)?;

    println!("Starting lldb-server platform server on the device...");
    device.spawn_run_as(
        package,
        &[
            APP_LLDB_SERVER_PATH,
            "platform",
            "--listen",
            &format!("*:{port}"),
            "--server",
        ],
    )?;

    // Give it a moment to start listening before callers try to connect.
    thread::sleep(Duration::from_millis(500));

    Ok(())
}

/// Attaches to `package` as it's already running, or (if `relaunch`) force-stops
/// and relaunches it fresh first, then opens an lldb-dap attach session pointed
/// at the lldb-server started by `start_lldb_platform_server`, in order:
/// 1. If `relaunch`: resolve the app's launcher activity component and
///    `am start -S` it, which force-stops the app and relaunches it fresh
///    in one step, so it comes back with a fresh pid.
/// 2. Wait for the process to show up and build its attach info (see
///    `wait_for_pid`/`build_attach_info`).
/// 3. Print it, and open it with `code --open-url` if requested.
fn attach(
    device: &AdbDevice,
    package: &str,
    port: u16,
    code_bin: &str,
    open_vscode: bool,
    symbols: &[PathBuf],
    pid_file: Option<&Path>,
    relaunch: bool,
) -> color_eyre::Result<()> {
    if relaunch {
        println!("Restarting {package}...");
        let component = device.resolve_launch_component(package)?;
        device.start_activity(&component, true)?;
    } else {
        println!("Attaching to already-running {package}...");
    }

    let pid = device.wait_for_pid(package)?;
    let info = lldb::build_attach_info(&pid, port, symbols)?;
    print_attach_info(port, &info);
    write_pid_file(pid_file, info.pid)?;

    if open_vscode {
        println!("Opening VS Code debug session...");
        match std::process::Command::new(code_bin)
            .arg("--open-url")
            .arg(&info.debug_uri)
            .status()
        {
            Ok(status) if !status.success() => {
                eprintln!("`{code_bin} --open-url` exited with status {status}");
            }
            Err(err) => {
                eprintln!(
                    "Failed to invoke VS Code (`{code_bin}`): {err}. \
                     Is it on PATH? Pass --code-bin to override, or open the attach URI above manually."
                );
            }
            _ => {}
        }
    }

    Ok(())
}

/// Prints the 4 lines needed to actually connect a debugger: the forwarded
/// port, the lldb-dap attach URI, the target pid, and the raw config behind
/// that URI (for pasting into a manual DAP launch instead).
fn print_attach_info(port: u16, info: &AttachInfo) {
    println!("Port: {port}");
    println!("Attach URI: {}", info.debug_uri);
    println!("PID: {}", info.pid);
    println!("Config: {}", info.config_json);
}

/// Writes the bare pid to `path`, if given, for a VS Code task to hand off to a
/// launch.json config (e.g. via the Command Variable extension) after this
/// process's `preLaunchTask` run finishes.
fn write_pid_file(path: Option<&Path>, pid: u32) -> color_eyre::Result<()> {
    let Some(path) = path else {
        return Ok(());
    };
    std::fs::write(path, pid.to_string())
        .with_context(|| format!("Failed to write pid file at {}", path.display()))
}
