use crate::{
    commands::Command,
    device::{Backend, DeviceArgs},
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
            #[cfg(feature = "waydroid")]
            Backend::Waydroid => crate::waydroid::stop(),
            #[cfg(feature = "avd")]
            Backend::Emulator => {
                let device = DeviceArgs {
                    backend: Some(Backend::Emulator),
                    ..self.device
                }
                .resolve_adb()?;
                crate::avd::stop(&device)
            }
        }
    }
}
