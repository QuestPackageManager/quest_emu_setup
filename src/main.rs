use clap::Parser;

use crate::commands::{Command, GlobalContext};

#[cfg(not(any(feature = "avd", feature = "waydroid")))]
compile_error!("quest_emu needs at least one device backend feature: `avd` or `waydroid`");

mod commands;
mod constants;
#[cfg(feature = "avd")]
mod downloader;
mod lldb;
mod adb;
mod apk;
#[cfg(feature = "avd")]
mod avd;
mod device;
#[cfg(feature = "waydroid")]
mod waydroid;

#[derive(clap::Parser)]
struct Args {
    /// Skip all prompts and use default values
    #[arg(long, default_value_t = false, global = true)]
    yes: bool,

    #[command(subcommand)]
    command: commands::MainCommand,
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = Args::parse();

    let ctx = GlobalContext { yes: args.yes };

    args.command.execute(&ctx)?;

    Ok(())
}
