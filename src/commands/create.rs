use crate::{
    avd,
    commands::{Command, GlobalContext},
};

#[cfg_attr(feature = "clap", derive(clap::Parser))]
pub struct CreateArgs {
    /// Create an AVD (Android Virtual Device)
    #[cfg_attr(feature = "clap", arg(long = "avd", default_value_t = false))]
    create_avd: bool,

    /// Overwrite existing AVD (Android Virtual Device)
    #[cfg_attr(feature = "clap", arg(long = "overwrite", default_value_t = false))]
    overwrite_avd: bool,

    /// Name of AVD (Android Virtual Device)
    #[cfg_attr(feature = "clap", arg(long, default_value_t = avd::DEFAULT_AVD_NAME.to_string()))]
    name: String,

    /// Screen size for the AVD (Android Virtual Device), e.g. "1920x1080"
    #[cfg_attr(
        feature = "clap",
        arg(long = "screen-size", default_value = "1920x1080")
    )]
    screen_size: String,

    /// Limit the emulator FPS to save CPU/GPU resources (0 = unlimited)
    #[cfg_attr(feature = "clap", arg(long = "fps", default_value_t = 60))]
    fps_limit: u32,

    /// System image of AVD (Android Virtual Device)
    #[cfg_attr(feature = "clap", arg(long = "image", default_value_t = avd::default_avd_image()))]
    system_image: String,
}

impl Command for CreateArgs {
    fn execute(self, ctx: &GlobalContext) -> color_eyre::Result<()> {
        avd::create(
            ctx,
            &avd::CreateOptions {
                create_avd: self.create_avd,
                overwrite_avd: self.overwrite_avd,
                name: self.name,
                screen_size: self.screen_size,
                fps_limit: self.fps_limit,
                system_image: self.system_image,
            },
        )
    }
}
