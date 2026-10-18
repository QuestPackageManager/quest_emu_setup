use crate::{
    commands::Command,
    device::{Backend, DeviceArgs},
    waydroid,
};

#[cfg_attr(feature = "clap", derive(clap::Parser))]
#[derive(Debug)]
pub struct StopArgs {
    #[cfg_attr(feature = "clap", command(flatten))]
    pub device: DeviceArgs,
}

impl Command for StopArgs {
    fn execute(self, _ctx: &crate::commands::GlobalContext) -> color_eyre::Result<()> {
        match Backend::resolve(self.device.backend) {
            Backend::Waydroid => waydroid::stop(),
            Backend::Emulator => {
                let device = DeviceArgs {
                    backend: Some(Backend::Emulator),
                    ..self.device
                }
                .resolve_adb()?;
                println!("Stopping emulator {}...", device.describe());
                device.status(&["emu", "kill"])
            }
        }
    }
}
