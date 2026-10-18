#[cfg(not(any(feature = "avd", feature = "waydroid")))]
compile_error!("quest_emu needs at least one device backend feature: `avd` or `waydroid`");

pub mod commands;
pub mod constants;
#[cfg(feature = "avd")]
pub mod downloader;
pub mod lldb;
pub mod adb;
pub mod apk;
#[cfg(feature = "avd")]
pub mod avd;
pub mod device;
#[cfg(feature = "waydroid")]
pub mod waydroid;
