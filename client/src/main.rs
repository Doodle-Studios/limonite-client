mod checks;
mod embedded;
mod login;
mod measure;
mod packets;
mod session;
mod world;

use std::{
    io,
    path::{Path, PathBuf},
    process::ExitCode,
    time::{Duration, Instant},
};

use pumpkin_util::math::vector2::Vector2;
use pumpkin_world::{CURRENT_BEDROCK_MC_PROTOCOL, CURRENT_BEDROCK_MC_VERSION};
use tracing::{debug, error, info, warn};
use tracing_subscriber::{EnvFilter, filter::LevelFilter};

use crate::{
    checks::Check,
    embedded::EmbeddedServer,
    session::{Session, Stage, TICK},
    world::World,
};

const USAGE: &str =
    "usage: limonite-client [--data-dir <path>] [--name <name>] [--linger <seconds>] [--measure]";

/// Login and spawn must finish within this.
const SPAWN_DEADLINE: Duration = Duration::from_secs(120);
/// The walk: 40 ticks at 0.2 blocks a tick is 8 blocks east.
const WALK_TICKS: u32 = 40;
/// How long to wait for the server to apply the walk before checking it.
const SETTLE: Duration = Duration::from_secs(5);
/// The join is over once no chunk has arrived for this long...
const CHUNKS_QUIET: Duration = Duration::from_secs(2);
/// ...or after this, whichever comes first.
const CHUNKS_MAX_WAIT: Duration = Duration::from_secs(60);
/// How long the world comparison keeps retrying while block updates land.
const WORLD_SETTLE: Duration = Duration::from_secs(2);

/// Command-line options. Every path is absolute by the time parsing returns.
struct Args {
    /// Where the embedded server keeps its world and data files.
    data_dir: PathBuf,
    /// The host's display name.
    name: String,
    /// How long to stay in the world after the checks, still sending input,
    /// so the process can be inspected from outside.
    linger: Duration,
    /// Log what the `LocalChannel` cost (meaningful in release builds only).
    measure: bool,
}

impl Args {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut data_dir = PathBuf::from("run/client");
        let mut name = "Limonite".to_string();
        let mut linger = Duration::ZERO;
        let mut measure = false;
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
                "--measure" => measure = true,
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
            measure,
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

    let spawned = match (spawned, session.spawn()) {
        (Ok(()), Some(spawn)) => {
            checks.push(walk_and_check(&mut session, server, &spawn));
            // `PlayerSpawn` only means the server has queued enough chunks,
            // not that they've arrived, so wait for them before checking.
            wait_for_chunks(&mut session);
            if args.measure {
                log_measurements(&session, server);
            }
            true
        }
        _ => {
            checks.push(Check::new(
                "movement",
                false,
                "not checked: never spawned".to_string(),
            ));
            false
        }
    };

    let chunks = session.level_chunks();
    checks.push(Check::new(
        "chunks",
        chunks > 4,
        format!("{chunks} LevelChunk packets received (needs more than 4)"),
    ));
    checks.push(checks::chunks_decoded(
        chunks,
        session.chunks_decoded(),
        session.world_errors(),
        session.ids_hashed(),
    ));
    if spawned {
        checks.push(check_world(&mut session, server));
        linger(&mut session, args.linger);
    } else {
        checks.push(Check::new(
            "world matches server",
            false,
            "not checked: never spawned".to_string(),
        ));
    }
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

/// Compares every decoded chunk the server still has loaded with the
/// server's copy, block for block on both layers and biome for biome.
///
/// The server keeps ticking while we compare, and a block change (a sheep
/// eating grass, a mob lighting redstone ore) reaches us as an `UpdateBlock`
/// a tick or more later. A full pass takes several ticks, longer than a whole
/// second in a dev build, so some chunk will usually be caught mid-change.
/// So after the full pass, the chunks that differed are compared again each
/// tick for up to [`WORLD_SETTLE`]. Each chunk must still equal the server's
/// exactly at one moment, so a real decode error can never pass.
fn check_world(session: &mut Session, server: &EmbeddedServer) -> Check {
    const NAME: &str = "world matches server";
    let Some(level) = server.server.worlds.load().first().map(|world| world.level.clone()) else {
        return Check::new(NAME, false, "the server has no world".to_string());
    };
    // Compares one of our chunks: `None` if the server no longer has it,
    // otherwise how many values differ.
    let compare = |world: &World, (x, z): (i32, i32), examples: &mut Vec<String>| {
        let ours = world.chunk(x, z)?;
        let server_chunk = level
            .loaded_chunks
            .get(&Vector2::new(x, z))
            .map(|entry| entry.value().clone())?;
        Some(checks::compare_chunk(&server_chunk, ours, world.blocks(), examples, 10))
    };

    let start = Instant::now();
    let mut examples = Vec::new();
    let mut compared = 0usize;
    let mut differing = Vec::new();
    let positions: Vec<_> = session.world().chunks().map(|c| (c.x, c.z)).collect();
    for &pos in &positions {
        match compare(session.world(), pos, &mut examples) {
            Some(0) => compared += 1,
            Some(_) => {
                compared += 1;
                differing.push(pos);
            }
            None => {}
        }
    }
    let full_pass = start.elapsed().as_secs_f64();
    let mut first_differing = differing.len();
    debug!(
        "World comparison: {compared} chunks in {full_pass:.2} s, {first_differing} differ; \
         re-checking those"
    );
    for example in &examples {
        debug!("Full-pass difference: {example}");
    }

    let mut differ = 0;
    if !differing.is_empty() {
        let _ = session.run_until(Instant::now() + WORLD_SETTLE, |s| {
            examples.clear();
            differ = 0;
            differing.retain(|&pos| match compare(s.world(), pos, &mut examples) {
                Some(0) => false,
                None => {
                    // Unloaded by the server meanwhile: no longer comparable.
                    compared -= 1;
                    first_differing -= 1;
                    false
                }
                Some(n) => {
                    differ += n;
                    true
                }
            });
            differing.is_empty()
        });
    }
    let took = start.elapsed().as_secs_f64();
    let not_loaded = positions.len() - compared;
    if differing.is_empty() && compared > 0 {
        return Check::new(
            NAME,
            true,
            format!(
                "{compared} chunks equal block for block on both layers and biome for biome: \
                 {} on the full pass ({full_pass:.2} s), {first_differing} on a re-check; \
                 {not_loaded} no longer loaded on the server; {took:.2} s",
                compared - first_differing,
            ),
        );
    }
    for example in &examples {
        warn!("Difference: {example}");
    }
    Check::new(
        NAME,
        false,
        format!(
            "{} of {compared} chunks still differ after {:.1} s of re-checks, {differ} values \
             (first {} logged above); {took:.2} s",
            differing.len(),
            WORLD_SETTLE.as_secs_f64(),
            examples.len(),
        ),
    )
}

/// Keeps playing until the join's chunks stop arriving, so the checks and
/// measurements cover a whole join.
fn wait_for_chunks(session: &mut Session) {
    let mut seen = session.level_chunks();
    let mut last_new = Instant::now();
    let _ = session.run_until(Instant::now() + CHUNKS_MAX_WAIT, |s| {
        if s.level_chunks() != seen {
            seen = s.level_chunks();
            last_new = Instant::now();
        }
        last_new.elapsed() >= CHUNKS_QUIET
    });
    info!("{seen} LevelChunk packets in this join");
}

/// Logs the per-packet table, then the chunk encode cost and the numbers the
/// "special-case chunks with `Arc`" decision needs.
fn log_measurements(session: &Session, server: &EmbeddedServer) {
    if cfg!(debug_assertions) {
        warn!("This is a debug build: the timings below are meaningless");
    }
    session.stats().log_table();
    let start_game_throughput = session
        .start_game()
        .map(|start_game| measure::start_game_throughput(start_game, 1000));
    match measure::chunk_encode(&server.server, 10) {
        Some(encode) => measure::log_decision(session.stats(), &encode, start_game_throughput),
        None => warn!("No loaded chunks to measure"),
    }
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
