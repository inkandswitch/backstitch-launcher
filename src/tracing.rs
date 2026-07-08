use crate::config::CommandConfig;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub fn initialize_tracing(config: &CommandConfig) {
    if config.verbose.is_none_or(|v| !v) {
        return;
    }
    let stdout_layer = tracing_subscriber::fmt::layer();
    match tracing_subscriber::registry().with(stdout_layer).try_init() {
        Err(e) => {
            tracing::error!("Failed to initialize tracing subscriber: {:?}", e);
        }
        Ok(_) => {
            tracing::info!("Tracing subscriber initialized");
        }
    }
}
