#[derive(Copy, Clone, Debug, Default)]
pub enum WindowsState{
    #[default]
    Login, 
    Game,
}

#[derive(Copy, Clone, Debug, Default)]
pub struct Windows {
    is_minimized: bool,
    state: WindowsState,
}
