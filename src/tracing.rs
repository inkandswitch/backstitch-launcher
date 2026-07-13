use crate::config::CommandConfig;
use tracing_subscriber::{EnvFilter, Layer, layer::SubscriberExt, util::SubscriberInitExt};

pub fn initialize_tracing(config: &CommandConfig) {
    if !config.verbose {
        return;
    }
    let stdout_layer = tracing_subscriber::fmt::layer().with_filter(
        EnvFilter::new("info").add_directive("backstitch_launcher=trace".parse().unwrap()),
    );
    match tracing_subscriber::registry().with(stdout_layer).try_init() {
        Err(e) => {
            tracing::error!("Failed to initialize tracing subscriber: {:?}", e);
        }
        Ok(_) => {
            tracing::info!("Tracing subscriber initialized");
        }
    }
}
