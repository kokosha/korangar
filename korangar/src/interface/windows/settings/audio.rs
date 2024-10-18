use korangar_interface::elements::{Headline, PrototypeElement, Slider, StateButtonBuilder};
use korangar_interface::event::Render;
use korangar_interface::size_bound;
use korangar_interface::state::TrackedStateBinary;
use korangar_interface::windows::{PrototypeWindow, Window, WindowBuilder};
use serde::{Serialize, Deserialize};

use crate::interface::application::InterfaceSettings;
use crate::interface::elements::MutableRange;
use crate::interface::layout::{ScreenSize};
use crate::interface::theme::{DefaultMain,  ThemeDefault};
use korangar_interface::elements::ElementWrap;
use crate::interface::windows::WindowCache;




#[derive(Default)]
pub struct AudioSettingsWindow<AudioMinimized, AudioLogin>
where  AudioMinimized:  TrackedStateBinary<bool>,
       AudioLogin: TrackedStateBinary<bool>,
{
    audio_minimized:AudioMinimized,
    audio_login:AudioLogin,
    reference: f32,
    change_event: Option<ChangeEvent>,

}

impl<AudioMinimized, AudioLogin>  AudioSettingsWindow <AudioMinimized, AudioLogin>
where  AudioMinimized:  TrackedStateBinary<bool>,
       AudioLogin: TrackedStateBinary<bool>,
{
    pub const WINDOW_CLASS: &'static str = "audio_settings";
    pub fn new(audio_minimized: AudioMinimized, audio_login: AudioLogin) -> Self {
        Self {
            audio_minimized,
            audio_login,
            reference,
            change_event
        }
    }
}

impl <AudioMinimized, AudioLogin> PrototypeWindow<InterfaceSettings> for AudioSettingsWindow <AudioMinimized, AudioLogin>
where  AudioMinimized:  TrackedStateBinary<bool>,
       AudioLogin: TrackedStateBinary<bool> 
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
            Slider::new(&self.reference, 0.0, 1.0, self.change_event).wrap(),
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