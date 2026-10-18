use clap::Parser;

use crate::commands::{Command, GlobalContext};

mod commands;
mod constants;
mod downloader;
mod lldb;
mod adb;
mod apk;
mod device;
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
