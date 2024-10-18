use korangar_interface::elements::{ElementWrap, Headline, StateButtonBuilder};
use korangar_interface::size_bound;
use korangar_interface::state::TrackedStateBinary;
use korangar_interface::windows::{PrototypeWindow, Window, WindowBuilder};

use crate::interface::application::InterfaceSettings;
use crate::interface::layout::ScreenSize;
use crate::interface::windows::WindowCache;

#[derive(Default)]
pub struct AudioSettingsWindow<AudioMinimized, AudioLogin>
where
    AudioMinimized: TrackedStateBinary<bool>,
    AudioLogin: TrackedStateBinary<bool>,
{
    audio_minimized: AudioMinimized,
    audio_login: AudioLogin,
}

impl<AudioMinimized, AudioLogin> AudioSettingsWindow<AudioMinimized, AudioLogin>
where
    AudioMinimized: TrackedStateBinary<bool>,
    AudioLogin: TrackedStateBinary<bool>,
{
    pub const WINDOW_CLASS: &'static str = "audio_settings";

    pub fn new(audio_minimized: AudioMinimized, audio_login: AudioLogin) -> Self {
        Self {
            audio_minimized,
            audio_login,
        }
    }
}

impl<AudioMinimized, AudioLogin> PrototypeWindow<InterfaceSettings> for AudioSettingsWindow<AudioMinimized, AudioLogin>
where
    AudioMinimized: TrackedStateBinary<bool>,
    AudioLogin: TrackedStateBinary<bool>,
{
    fn window_class(&self) -> Option<&str> {
        Self::WINDOW_CLASS.into()
    }

    fn to_window(
        &self,
        window_cache: &WindowCache,
        application: &InterfaceSettings,
        available_space: ScreenSize,
    ) -> Window<InterfaceSettings> {
        let elements = vec![
            StateButtonBuilder::new()
                .with_text("Audio when minimized")
                .with_event(self.audio_minimized.toggle_action())
                .with_remote(self.audio_minimized.new_remote())
                .build()
                .wrap(),
            StateButtonBuilder::new()
                .with_text("Audio during login screen")
                .with_event(self.audio_login.toggle_action())
                .with_remote(self.audio_login.new_remote())
                .build()
                .wrap(),
            Headline::new("Main Sound".to_string(), size_bound!(100%, 12)).wrap(),
        ];

        WindowBuilder::new()
            .with_title("Audio Settings".to_string())
            .with_class(Self::WINDOW_CLASS.to_string())
            .with_size_bound(size_bound!(200 > 300 < 400, ?))
            .with_elements(elements)
            .closable()
            .build(window_cache, application, available_space)
    }
}
