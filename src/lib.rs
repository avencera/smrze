mod app;
mod audio;
mod cli;
mod console;
mod foundation_models;
#[cfg(target_os = "macos")]
mod foundation_models_bridge;
mod input;
mod models;
mod output;
mod paths;
mod speakers;
mod summary;
mod transcript;
mod utils;
mod workers;

pub use app::run;
