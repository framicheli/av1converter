use crate::app::{App, CurrentScreen};
use ratatui::prelude::*;

use crate::components::{config, encode, file_explorer, finish, home};

pub fn ui(f: &mut Frame, app: &mut App) {
    match app.current_screen {
        CurrentScreen::Home => {
            home::draw_home(f, app);
        }
        CurrentScreen::FileExplorer => {
            file_explorer::draw_file_explorer(f, app);
        }
        CurrentScreen::Config => {
            config::draw_config(f, app);
        }
        CurrentScreen::Encode => {
            encode::draw_encode(f, app);
        }
        CurrentScreen::Finish => {
            finish::draw_finish(f, app);
        }
    }
}
