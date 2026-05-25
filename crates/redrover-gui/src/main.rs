// Hide the console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod discord;
mod install;
mod view;

use tracing_subscriber::{fmt, EnvFilter};

fn main() -> iced::Result {
    let _ = fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with_target(false)
        .try_init();

    iced::application("Redrover", app::App::update, app::App::view)
        .theme(|_| iced::Theme::TokyoNight)
        .window_size(iced::Size::new(420.0, 520.0))
        .run_with(app::App::new)
}
