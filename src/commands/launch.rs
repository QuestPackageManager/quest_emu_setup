use crate::{
    commands::{Command, GlobalContext},
    device::DeviceArgs,
};

#[cfg_attr(feature = "clap", derive(clap::Parser))]
#[derive(Debug)]
pub struct LaunchArgs {
    #[cfg_attr(feature = "clap", command(flatten))]
    device: DeviceArgs,

    /// Android package name to launch, e.g. "com.beatgames.beatsaber"
    package: String,

    /// Force-stop the app first, so it relaunches fresh
    #[cfg_attr(feature = "clap", arg(short = 'S', long, default_value_t = false))]
    force_stop: bool,

    /// Wait for the app's process to show up before returning
    #[cfg_attr(feature = "clap", arg(long, default_value_t = false))]
    wait: bool,
}

impl Command for LaunchArgs {
    fn execute(self, _ctx: &GlobalContext) -> color_eyre::Result<()> {
        let device = self.device.resolve_adb()?;
        println!(
            "{} {} on {}",
            if self.force_stop { "Relaunching" } else { "Launching" },
            self.package,
            device.describe()
        );
        device.launch(&self.package, self.force_stop)?;

        if self.wait {
            let pid = device.wait_for_pid(&self.package)?;
            println!("{} is running (pid {pid})", self.package);
        }
        Ok(())
    }
}
