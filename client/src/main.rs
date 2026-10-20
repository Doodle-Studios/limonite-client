mod embedded;
mod login;
mod packets;
mod session;

use std::{
    io,
    path::{Path, PathBuf},
    process::ExitCode,
    time::{Duration, Instant},
};

use pumpkin_world::{CURRENT_BEDROCK_MC_PROTOCOL, CURRENT_BEDROCK_MC_VERSION};
use tracing::{error, info, warn};
use tracing_subscriber::{EnvFilter, filter::LevelFilter};

use crate::{
    embedded::EmbeddedServer,
    session::{Session, Stage},
};

const USAGE: &str = "usage: limonite-client [--data-dir <path>] [--name <name>]";

/// The whole scripted session must finish within this.
const SESSION_DEADLINE: Duration = Duration::from_secs(120);
/// How long to keep playing after spawning, so chunks keep arriving.
const AFTER_SPAWN: Duration = Duration::from_secs(1);

/// Command-line options. Every path is absolute by the time parsing returns.
struct Args {
    /// Where the embedded server keeps its world and data files.
    data_dir: PathBuf,
    /// The host's display name.
    name: String,
}

impl Args {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut data_dir = PathBuf::from("run/client");
        let mut name = "Limonite".to_string();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--data-dir" => {
                    data_dir = args.next().ok_or("--data-dir needs a path")?.into();
                }
                "--name" => name = args.next().ok_or("--name needs a name")?,
                other => return Err(format!("unknown argument `{other}`")),
            }
        }
        // Resolve now, against the directory we were started in: the working
        // directory changes later, and a relative path would change meaning.
        let data_dir = std::path::absolute(&data_dir)
            .map_err(|err| format!("can't resolve {}: {err}", data_dir.display()))?;
        Ok(Self { data_dir, name })
    }
}

fn main() -> ExitCode {
    // `RUST_LOG` picks what gets logged, e.g. `RUST_LOG=debug` or
    // `RUST_LOG=info,pumpkin_core::net::bedrock=debug`. Without it, `info` and above.
    // Pumpkin logs through the same global subscriber.
    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();
    tracing_subscriber::fmt().with_env_filter(filter).init();

    info!(
        "Limonite Client {} starting; embedded Pumpkin targets Bedrock \
         {CURRENT_BEDROCK_MC_VERSION} (protocol {CURRENT_BEDROCK_MC_PROTOCOL})",
        env!("CARGO_PKG_VERSION"),
    );

    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(err) => {
            error!("{err}\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    set_up_process();

    if let Err(err) = enter_data_dir(&args.data_dir) {
        error!(
            "Can't use the data directory {}: {err}",
            args.data_dir.display()
        );
        return ExitCode::FAILURE;
    }
    info!("Data directory: {}", args.data_dir.display());

    let server = match EmbeddedServer::start(embedded::config()) {
        Ok(server) => server,
        Err(err) => {
            error!("{err}");
            return ExitCode::FAILURE;
        }
    };

    let result = play(&server, &args);
    server.stop();
    result
}

/// The scripted host session: log in, spawn, and keep playing for a moment.
fn play(server: &EmbeddedServer, args: &Args) -> ExitCode {
    let deadline = Instant::now() + SESSION_DEADLINE;
    let mut session = match Session::connect(&server.connector, &args.name) {
        Ok(session) => session,
        Err(err) => {
            error!("Couldn't connect to the embedded server: {err}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(err) = session.run_until(deadline, |s| s.stage() == Stage::Spawned) {
        error!("Didn't spawn: {err}");
        return ExitCode::FAILURE;
    }
    let keep_playing_until = Instant::now() + AFTER_SPAWN;
    if let Err(err) = session.run_until(deadline, |_| Instant::now() >= keep_playing_until) {
        error!("Lost the session after spawning: {err}");
        return ExitCode::FAILURE;
    }
    info!(
        "Spawned; {} LevelChunk packets so far",
        session.level_chunks()
    );
    // Dropping the session closes the connection: the host leaves the game
    // before the server stops.
    drop(session);
    ExitCode::SUCCESS
}

/// Process-wide setup that Pumpkin's own `main` does before anything else.
fn set_up_process() {
    // Pumpkin's HTTP client is built without a TLS crypto provider, so the
    // process picks one before any client can be created.
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Pumpkin runs worldgen and chunk encoding on Rayon's global pool. Naming
    // its threads makes them recognisable in a debugger or profiler.
    if let Err(err) = rayon::ThreadPoolBuilder::new()
        .thread_name(|i| format!("Rayon-Worker-{i}"))
        .build_global()
    {
        warn!("Couldn't configure the Rayon pool: {err}");
    }
}

/// Creates `dir` and makes it the process's working directory.
///
/// Pumpkin reads and writes its world and data files relative to the working
/// directory. That directory belongs to the whole process, which is why every
/// path the client holds was made absolute before this runs.
fn enter_data_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::env::set_current_dir(dir)
}
