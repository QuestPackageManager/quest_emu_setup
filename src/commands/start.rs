use std::{process, time::Duration};

use color_eyre::eyre::{Context, bail};
use itertools::Itertools;

use crate::{
    commands::Command,
    constants::{self, emulator_path},
    device::Backend,
    waydroid,
};

#[cfg_attr(feature = "clap", derive(clap::Parser))]
#[derive(Debug)]
pub struct StartArgs {
    /// Device backend to start (auto-detected if omitted)
    #[cfg_attr(feature = "clap", arg(long))]
    pub backend: Option<Backend>,

    /// Waydroid: seconds to wait for Android to boot and adb to connect
    #[cfg_attr(feature = "clap", arg(long, default_value_t = 120))]
    pub timeout: u64,

    /// Name of the AVD to start
    #[cfg_attr(feature = "clap", arg(long, default_value_t = constants::DEFAULT_AVD_NAME.to_string()))]
    pub name: String,

    /// Start the emulator without loading a snapshot
    #[cfg_attr(feature = "clap", arg(long, default_value_t = false))]
    pub fresh: bool,

    /// Additional arguments to pass to the emulator
    #[cfg_attr(feature = "clap", arg(last = true))]
    pub args: Vec<String>,
}

impl Command for StartArgs {
    fn execute(self, ctx: &crate::commands::GlobalContext) -> color_eyre::Result<()> {
        let backend = Backend::resolve(self.backend);
        backend.remember();
        match backend {
            Backend::Waydroid => {
                waydroid::start(ctx, Duration::from_secs(self.timeout))?;
                Ok(())
            }
            Backend::Emulator => self.start_emulator(),
        }
    }
}

impl StartArgs {
    fn start_emulator(self) -> color_eyre::Result<()> {
        // use emulator @android13desktop -selinux permissive -feature QtRawKeyboardInput

        println!("Starting emulator with AVD name: {}", self.name);

        let mut command = process::Command::new(emulator_path());
        command
            .arg(format!("@{}", self.name))
            .arg("-selinux")
            .arg("permissive")
            .arg("-feature")
            .arg("QtRawKeyboardInput");

        if self.fresh {
            command.arg("-no-snapshot-load");
        }

        if !self.args.is_empty() {
            command.args(self.args);
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
}
