use pumpkin_world::{CURRENT_BEDROCK_MC_PROTOCOL, CURRENT_BEDROCK_MC_VERSION};
use tracing::info;
use tracing_subscriber::{EnvFilter, filter::LevelFilter};

fn main() {
    // `RUST_LOG` picks what gets logged, e.g. `RUST_LOG=debug` or
    // `RUST_LOG=info,pumpkin_core::net::bedrock=debug`. Without it, `info` and above.
    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();
    tracing_subscriber::fmt().with_env_filter(filter).init();

    info!(
        "Limonite Client {} starting; embedded Pumpkin targets Bedrock \
         {CURRENT_BEDROCK_MC_VERSION} (protocol {CURRENT_BEDROCK_MC_PROTOCOL})",
        env!("CARGO_PKG_VERSION"),
    );
}
