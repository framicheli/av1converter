mod app;
mod components;
mod ui;

use anyhow::Result;

use app::App;

fn main() -> Result<()> {
    let terminal = ratatui::init();
    let result = App::new().run(terminal);
    ratatui::restore();
    result
}
