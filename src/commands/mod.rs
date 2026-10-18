pub mod apk;
pub mod create;
pub mod setup;
pub mod start;
pub mod stop;
pub mod debug;

pub struct GlobalContext {
    pub yes: bool,
}

pub trait Command {
    fn execute(self, ctx: &GlobalContext) -> color_eyre::Result<()>;
}

#[cfg_attr(feature = "clap", derive(clap::Parser))]
pub enum MainCommand {
    /// Create for Quest emulator. Downloads sdkmanager, emulator, system images, etc.
    Create(create::CreateArgs),
    /// Start the device: the Android Emulator with a specified AVD, or Waydroid
    Start(start::StartArgs),
    /// Stop the running emulator or Waydroid session
    Stop(stop::StopArgs),
    /// Commands for patching APKs
    Apk(apk::ApkArgs),
    /// Setup the Android SDK and Emulator, or Waydroid with ARM translation
    Setup(setup::SetupArgs),
    /// Debug commands for the device
    Debug(debug::DebugArgs),
}

impl Command for MainCommand {
    fn execute(self, ctx: &GlobalContext) -> color_eyre::Result<()> {
        match self {
            MainCommand::Create(args) => args.execute(ctx)?,
            MainCommand::Apk(args) => args.execute(ctx)?,
            MainCommand::Start(args) => args.execute(ctx)?,
            MainCommand::Stop(args) => args.execute(ctx)?,
            MainCommand::Setup(setup_args) => setup_args.execute(ctx)?,
            MainCommand::Debug(debug_args) => debug_args.execute(ctx)?,
        }

        Ok(())
    }
}
