use crate::app::App;

mod app;
mod event;
mod hn;
mod ui;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let terminal = ratatui::init();

    let result = App::new().run(terminal).await;

    ratatui::restore();

    result
}
