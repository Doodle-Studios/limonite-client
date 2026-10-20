//! The Pumpkin server the client embeds, running on its own thread.
//!
//! The client's main thread never runs async code. The server gets a
//! dedicated `Pumpkin-Main` thread with its own multi-threaded tokio runtime,
//! just as Pumpkin's own `main` would run it, and the two sides talk only
//! through a `LocalChannel` and a few shared handles.

use std::{sync::Arc, sync::mpsc, thread, time::Duration};

use pumpkin_config::PumpkinConfig;
use pumpkin_core::{
    PumpkinServer, data::VanillaData, net::bedrock::local::LocalConnector, server::Server,
};
use tokio::runtime::Runtime;
use tracing::{error, info};

/// How long shutdown waits for the runtime's leftover tasks after the world is saved.
const RUNTIME_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// The embedded server's configuration, built in code: no config file is read
/// or written.
///
/// Everything that would open a socket or reach the internet is off. The only
/// way in is the in-process `LocalChannel`.
pub fn config() -> PumpkinConfig {
    let mut config = PumpkinConfig::default();
    let networking = &mut config.advanced.networking;
    // No UDP or TCP 19132, and no LAN discovery on 7551.
    networking.bedrock.nethernet.enabled = false;
    // No Xbox OIDC key fetch at startup, and the host's unsigned token is accepted.
    networking.bedrock.online_mode = false;
    // Already compiled out (no `java-listener` feature); this keeps the config honest.
    networking.java.enabled = false;
    // Pumpkin's default sends a heartbeat to market.pumpkinmc.org.
    config.telemetry.enabled = false;
    config.advanced.plugins.enabled = false;
    // The client owns the terminal; Pumpkin must not read stdin.
    config.advanced.commands.use_console = false;
    config.advanced.commands.use_tty = false;
    config
}

/// A running embedded server.
pub struct EmbeddedServer {
    /// Opens in-process connections to the server.
    pub connector: LocalConnector,
    /// The server's own state, shared with its threads. Read-only use from the
    /// client: checks and measurements.
    pub server: Arc<Server>,
    thread: thread::JoinHandle<()>,
}

/// What the server thread sends back once the server exists.
type Ready = Result<(LocalConnector, Arc<Server>), String>;

impl EmbeddedServer {
    /// Starts the server and waits until it can accept connections.
    ///
    /// Pumpkin resolves its files (world, `ops.json`, ...) from the working
    /// directory, so the caller must have changed into the data directory first.
    ///
    /// # Errors
    /// If the thread or runtime can't be created, or the world fails to load.
    pub fn start(config: PumpkinConfig) -> Result<Self, String> {
        let (ready_tx, ready_rx) = mpsc::channel::<Ready>();
        let thread = thread::Builder::new()
            .name("Pumpkin-Main".into())
            .spawn(move || run_server(config, &ready_tx))
            .map_err(|err| format!("failed to spawn the server thread: {err}"))?;

        // `recv` fails only if the thread ended without reporting, e.g. a panic.
        match ready_rx.recv() {
            Ok(Ok((connector, server))) => Ok(Self {
                connector,
                server,
                thread,
            }),
            Ok(Err(err)) => {
                let _ = thread.join();
                Err(err)
            }
            Err(_) => {
                let _ = thread.join();
                Err("the server thread ended before the server started".to_string())
            }
        }
    }

    /// Stops the server, which kicks any players and saves the world, and waits
    /// for its thread to finish.
    ///
    /// A process can only run one server: Pumpkin's stop flags are process
    /// globals that are never reset.
    pub fn stop(self) {
        pumpkin_core::stop_server();
        if self.thread.join().is_err() {
            error!("The embedded server thread panicked while stopping");
        }
    }
}

/// The body of the `Pumpkin-Main` thread.
fn run_server(config: PumpkinConfig, ready: &mpsc::Sender<Ready>) {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("Pumpkin-Tokio")
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            let _ = ready.send(Err(format!("failed to build the server runtime: {err}")));
            return;
        }
    };
    runtime.block_on(serve(config, ready));
    shut_down(runtime);
}

async fn serve(config: PumpkinConfig, ready: &mpsc::Sender<Ready>) {
    // Pumpkin's `init_logger` is never called: the client already installed
    // the global `tracing` subscriber, and Pumpkin's events reach it. But
    // `PumpkinServer::start` waits on `LOGGER_IMPL` being set, at startup with
    // a console and always at the end. Setting it to "no logger" lets both
    // waits return at once.
    let _ = pumpkin_core::LOGGER_IMPL.set(None);

    let vanilla_data = VanillaData::load();
    let server = match PumpkinServer::new(
        config.basic,
        config.advanced,
        config.telemetry,
        vanilla_data,
        Vec::new(),
    )
    .await
    {
        Ok(server) => server,
        Err(err) => {
            let _ = ready.send(Err(format!("failed to start the embedded server: {err}")));
            return;
        }
    };

    let started = (server.local_listener.connector(), server.server.clone());
    if ready.send(Ok(started)).is_err() {
        return;
    }
    info!("Embedded server started");
    server.start().await;
    info!("Embedded server stopped");
}

fn shut_down(runtime: Runtime) {
    // Tasks still parked on the runtime (e.g. timers) are dropped; the world
    // has already been saved by `start` returning.
    runtime.shutdown_timeout(RUNTIME_SHUTDOWN_TIMEOUT);
}
