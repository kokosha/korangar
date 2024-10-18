#[cfg(feature = "debug")]
use korangar_debug::logging::{print_debug, Colorize};
use ron::ser::PrettyConfig;
use serde::{Deserialize, Serialize};

/// Audio Settings
#[derive(Clone, Serialize, Deserialize)]
pub struct AudioSettings {
    /// When minimized window produce audio
    pub audio_minimized: bool,
    /// When login window produce audio
    pub audio_login: bool,
}
/// Default function
impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            audio_minimized: true,
            audio_login: true,
        }
    }
}
/// Implement audio settings
impl AudioSettings {
    const FILE_NAME: &'static str = "client/audio_settings.ron";
    /// Create new audio settings
    pub fn new() -> Self {
        Self::load().unwrap_or_else(|| {
            #[cfg(feature = "debug")]
            print_debug!("failed to load audio settings from {}", Self::FILE_NAME.magenta());

            Default::default()
        })
    }
    /// Load audio settings
    pub fn load() -> Option<Self> {
        #[cfg(feature = "debug")]
        print_debug!("loading audio settings from {}", Self::FILE_NAME.magenta());

        std::fs::read_to_string(Self::FILE_NAME)
            .ok()
            .and_then(|data| ron::from_str(&data).ok())
    }
    /// Save audio settings
    pub fn save(&self) {
        #[cfg(feature = "debug")]
        print_debug!("saving audio settings to {}", Self::FILE_NAME.magenta());

        let data = ron::ser::to_string_pretty(self, PrettyConfig::new()).unwrap();
        std::fs::write(Self::FILE_NAME, data).expect("unable to write file");
    }
}
/// Drop audio settings
impl Drop for AudioSettings {
    fn drop(&mut self) {
        self.save();
    }
}
