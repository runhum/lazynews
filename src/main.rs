use crate::app::App;

mod app;
mod comments_nav;
mod event;
mod hn;
mod input;
mod ui;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let terminal = ratatui::init();

    let result = App::new().run(terminal).await;

    ratatui::restore();

    result
}
