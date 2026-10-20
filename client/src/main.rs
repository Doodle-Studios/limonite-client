mod checks;
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
    checks::Check,
    embedded::EmbeddedServer,
    session::{Session, Stage, TICK},
};

const USAGE: &str =
    "usage: limonite-client [--data-dir <path>] [--name <name>] [--linger <seconds>]";

/// Login and spawn must finish within this.
const SPAWN_DEADLINE: Duration = Duration::from_secs(120);
/// The walk: 40 ticks at 0.2 blocks a tick is 8 blocks east.
const WALK_TICKS: u32 = 40;
/// How long to wait for the server to apply the walk before checking it.
const SETTLE: Duration = Duration::from_secs(5);

/// Command-line options. Every path is absolute by the time parsing returns.
struct Args {
    /// Where the embedded server keeps its world and data files.
    data_dir: PathBuf,
    /// The host's display name.
    name: String,
    /// How long to stay in the world after the checks, still sending input,
    /// so the process can be inspected from outside.
    linger: Duration,
}

impl Args {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut data_dir = PathBuf::from("run/client");
        let mut name = "Limonite".to_string();
        let mut linger = Duration::ZERO;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--data-dir" => {
                    data_dir = args.next().ok_or("--data-dir needs a path")?.into();
                }
                "--name" => name = args.next().ok_or("--name needs a name")?,
                "--linger" => {
                    let secs = args.next().ok_or("--linger needs a number of seconds")?;
                    let secs = secs
                        .parse()
                        .map_err(|_| format!("--linger: `{secs}` isn't a number of seconds"))?;
                    linger = Duration::from_secs(secs);
                }
                other => return Err(format!("unknown argument `{other}`")),
            }
        }
        // Resolve now, against the directory we were started in: the working
        // directory changes later, and a relative path would change meaning.
        let data_dir = std::path::absolute(&data_dir)
            .map_err(|err| format!("can't resolve {}: {err}", data_dir.display()))?;
        Ok(Self {
            data_dir,
            name,
            linger,
        })
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

/// The scripted host session: log in, spawn, walk east, check the move on the
/// server, optionally linger, and report.
fn play(server: &EmbeddedServer, args: &Args) -> ExitCode {
    let mut session = match Session::connect(&server.connector, &args.name) {
        Ok(session) => session,
        Err(err) => {
            error!("Couldn't connect to the embedded server: {err}");
            return ExitCode::FAILURE;
        }
    };

    let spawned = session.run_until(Instant::now() + SPAWN_DEADLINE, |s| {
        s.stage() == Stage::Spawned
    });
    let mut checks = vec![match &spawned {
        Ok(()) => Check::new(
            "spawn",
            true,
            "PlayerSpawn received; SetLocalPlayerAsInitialized sent".to_string(),
        ),
        Err(err) => Check::new("spawn", false, err.to_string()),
    }];

    match (spawned, session.spawn()) {
        (Ok(()), Some(spawn)) => {
            checks.push(walk_and_check(&mut session, server, &spawn));
            linger(&mut session, args.linger);
        }
        _ => checks.push(Check::new(
            "movement",
            false,
            "not checked: never spawned".to_string(),
        )),
    }

    let chunks = session.level_chunks();
    checks.push(Check::new(
        "chunks",
        chunks > 4,
        format!("{chunks} LevelChunk packets received (needs more than 4)"),
    ));
    // Dropping the session closes the connection: the host leaves the game
    // before the server stops.
    drop(session);

    if checks::report(&checks) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Walks east, then waits until the server's copy of the player has moved
/// (or [`SETTLE`] passes) and checks it.
fn walk_and_check(session: &mut Session, server: &EmbeddedServer, spawn: &session::Spawn) -> Check {
    info!("Walking east for {WALK_TICKS} ticks");
    session.walk(WALK_TICKS);
    let walk_deadline = Instant::now() + TICK * (WALK_TICKS + 20);
    if let Err(err) = session.run_until(walk_deadline, |s| !s.is_walking()) {
        return Check::new("movement", false, format!("walk interrupted: {err}"));
    }
    // The server applies input on its own tick thread, a tick or more later.
    // Polling its state each tick, rather than sleeping a fixed time, keeps
    // a slow (debug) server from failing the check. Timing out here is fine:
    // the check below reports what the server has.
    let _ = session.run_until(Instant::now() + SETTLE, |_| {
        checks::movement(&server.server, spawn).passed()
    });
    checks::movement(&server.server, spawn)
}

/// Stays in the world for `duration`, still sending input, so the process can
/// be inspected from outside (e.g. for open sockets).
fn linger(session: &mut Session, duration: Duration) {
    if duration.is_zero() {
        return;
    }
    info!(
        "Lingering for {} s; process id {}",
        duration.as_secs(),
        std::process::id()
    );
    let until = Instant::now() + duration;
    if let Err(err) = session.run_until(until + TICK, |_| Instant::now() >= until) {
        warn!("Session ended while lingering: {err}");
    }
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
