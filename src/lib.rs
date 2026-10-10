#[cfg(not(target_os = "linux"))]
compile_error!("waybar-pomodoro supports linux (its sleep detection uses linux clocks)");

pub mod clock;
pub mod config;
pub mod engine;
pub mod notify;
pub mod render;
pub mod store;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
