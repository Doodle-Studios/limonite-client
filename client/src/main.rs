mod app;
mod bench;
mod blocks;
mod checks;
mod embedded;
mod input;
mod install;
mod login;
mod map;
mod measure;
mod packets;
mod render;
mod session;
mod settings;
#[cfg(test)]
mod test_util;
mod world;

use std::{
    io,
    path::{Path, PathBuf},
    process::ExitCode,
    thread,
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
    settings::Settings,
    world::World,
};

const USAGE: &str = "usage: limonite-client [--bedrock <path>] [--data-dir <path>] [--name <name>] \
                     [--gamemode <mode>] [--radius <chunks>] [--bench <seconds>]\n       \
                     limonite-client --headless [--data-dir <path>] [--name <name>] \
                     [--gamemode <mode>] [--radius <chunks>] [--linger <seconds>] [--measure] \
                     [--map <path.png>] [--stream <blocks>]";

/// The view distance asked for without `--radius`, in chunks.
const DEFAULT_RADIUS: i32 = 8;
/// The most `--radius` accepts: the embedded server's Bedrock view distance.
const MAX_RADIUS: i32 = 32;

/// Login and spawn must finish within this.
const SPAWN_DEADLINE: Duration = Duration::from_secs(120);
/// The walk: 40 ticks at 0.2 blocks a tick is 8 blocks east.
const WALK_TICKS: u32 = 40;
/// How long to wait for the server to apply the walk before checking it.
const SETTLE: Duration = Duration::from_secs(5);
/// The join is over once no chunk has arrived for this long...
const CHUNKS_QUIET: Duration = Duration::from_secs(2);
/// ...or after this, whichever comes first. A radius-32 join into a fresh
/// world is about 3,200 chunks of worldgen, so this is generous.
const CHUNKS_MAX_WAIT: Duration = Duration::from_secs(180);
/// How long the world comparison keeps retrying while block updates land.
const WORLD_SETTLE: Duration = Duration::from_secs(2);
/// How many ticks a still-differing chunk gets for a fresh encode to decode
/// equal to the server.
const FRESH_DECODE_ATTEMPTS: u32 = 5;

/// Command-line options. Every path is absolute by the time parsing returns.
struct Args {
    /// Run the scripted session and its checks instead of opening a window.
    /// It needs no GPU and no Bedrock install, so it stays the regression
    /// suite.
    headless: bool,
    /// The Bedrock install: the folder that holds `data/resource_packs`.
    bedrock: Option<PathBuf>,
    /// What the player chose: the host's gamemode, from `--gamemode`.
    settings: Settings,
    /// Where the embedded server keeps its world and data files.
    data_dir: PathBuf,
    /// The host's display name.
    name: String,
    /// How long to stay in the world after the checks, still sending input,
    /// so the process can be inspected from outside.
    linger: Duration,
    /// Log what the `LocalChannel` cost (meaningful in release builds only).
    measure: bool,
    /// Where to write a top-down PNG map of the loaded chunks.
    map: Option<PathBuf>,
    /// The view distance to ask for, in chunks.
    radius: i32,
    /// After the join, fly this many blocks east and log how chunks stream.
    stream: Option<u32>,
    /// Fly the window's camera east for this long, log the frame times, and
    /// exit (`bench.rs`).
    bench: Option<Duration>,
}

impl Args {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut headless = false;
        let mut bedrock = None;
        let mut data_dir = PathBuf::from("run/client");
        let mut name = "Limonite".to_string();
        let mut linger = Duration::ZERO;
        let mut measure = false;
        let mut map = None;
        let mut radius = DEFAULT_RADIUS;
        let mut stream = None;
        let mut bench = None;
        let mut settings = Settings::default();
        // The scripted session's flags, which mean nothing to the window.
        let mut headless_only = Vec::new();
        while let Some(arg) = args.next() {
            if matches!(arg.as_str(), "--linger" | "--measure" | "--map" | "--stream") {
                headless_only.push(arg.clone());
            }
            match arg.as_str() {
                "--headless" => headless = true,
                "--bedrock" => {
                    bedrock = Some(PathBuf::from(args.next().ok_or("--bedrock needs a path")?));
                }
                "--data-dir" => {
                    data_dir = args.next().ok_or("--data-dir needs a path")?.into();
                }
                "--name" => name = args.next().ok_or("--name needs a name")?,
                "--gamemode" => {
                    let mode = args.next().ok_or("--gamemode needs a mode")?;
                    settings.gamemode = mode.parse().map_err(|_| {
                        format!("--gamemode: `{mode}` isn't survival, creative, adventure or spectator")
                    })?;
                }
                "--linger" => {
                    let secs = args.next().ok_or("--linger needs a number of seconds")?;
                    let secs = secs
                        .parse()
                        .map_err(|_| format!("--linger: `{secs}` isn't a number of seconds"))?;
                    linger = Duration::from_secs(secs);
                }
                "--measure" => measure = true,
                "--map" => map = Some(PathBuf::from(args.next().ok_or("--map needs a path")?)),
                "--radius" => {
                    let chunks = args.next().ok_or("--radius needs a number of chunks")?;
                    radius = chunks
                        .parse()
                        .ok()
                        .filter(|r| (2..=MAX_RADIUS).contains(r))
                        .ok_or(format!("--radius: `{chunks}` isn't 2 to {MAX_RADIUS} chunks"))?;
                }
                "--stream" => {
                    let blocks = args.next().ok_or("--stream needs a number of blocks")?;
                    stream = Some(
                        blocks
                            .parse()
                            .map_err(|_| format!("--stream: `{blocks}` isn't a number of blocks"))?,
                    );
                }
                "--bench" => {
                    let secs = args.next().ok_or("--bench needs a number of seconds")?;
                    let secs = secs
                        .parse()
                        .ok()
                        .filter(|&secs| secs > 0)
                        .ok_or(format!("--bench: `{secs}` isn't a whole number of seconds above 0"))?;
                    bench = Some(Duration::from_secs(secs));
                }
                other => return Err(format!("unknown argument `{other}`")),
            }
        }
        if !headless && !headless_only.is_empty() {
            return Err(format!(
                "the scripted session's flags need --headless: {}",
                headless_only.join(", ")
            ));
        }
        if headless && bench.is_some() {
            return Err("--bench measures the window's frames, so it can't run with --headless".to_string());
        }
        // Resolve now, against the directory we were started in: the working
        // directory changes later, and a relative path would change meaning.
        let absolute = |path: &Path| {
            std::path::absolute(path).map_err(|err| format!("can't resolve {}: {err}", path.display()))
        };
        Ok(Self {
            headless,
            settings,
            bedrock: bedrock.as_deref().map(absolute).transpose()?,
            data_dir: absolute(&data_dir)?,
            name,
            linger,
            measure,
            map: map.as_deref().map(absolute).transpose()?,
            radius,
            stream,
            bench,
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

    if args.headless {
        if args.bedrock.is_some() {
            warn!("--bedrock is unused: the scripted session reads no assets");
        }
        headless(&args)
    } else {
        windowed(&args)
    }
}

/// The default path: the window.
///
/// The assets load first, before the server starts, so a missing install
/// fails at once.
fn windowed(args: &Args) -> ExitCode {
    let install = match install::find(args.bedrock.as_deref()) {
        Ok(install) => install,
        Err(err) => {
            error!("{err}");
            return ExitCode::FAILURE;
        }
    };
    let vanilla = match install::load_vanilla(&install) {
        Ok(vanilla) => vanilla,
        Err(err) => {
            error!("Couldn't load the vanilla packs: {err}");
            return ExitCode::FAILURE;
        }
    };
    let palette = match blocks::load_palette() {
        Ok(palette) => palette,
        Err(err) => {
            error!("{err}");
            return ExitCode::FAILURE;
        }
    };
    let looks = blocks::build_appearance(&palette, &vanilla);
    let tints = render::load_tints(&vanilla.stack);

    let server = match start_server(args) {
        Ok(server) => server,
        Err(code) => return code,
    };
    if args.bench.is_some() && cfg!(debug_assertions) {
        warn!("This is a debug build: the bench's frame times mean little. Use --release");
    }
    let needs = render::Needs {
        texture_layers: looks.textures.layers,
        uncapped: args.bench.is_some(),
    };
    let assets = app::Assets {
        table: looks.table,
        textures: looks.textures,
        tints,
    };
    let result = app::run(&server, &args.name, args.radius, args.settings, needs, assets, args.bench);
    server.stop();
    result
}

/// The scripted session (`--headless`): the M2 and M3 acceptance run, which
/// needs no GPU and no Bedrock install.
fn headless(args: &Args) -> ExitCode {
    let server = match start_server(args) {
        Ok(server) => server,
        Err(code) => return code,
    };
    let result = play(&server, args);
    server.stop();
    result
}

/// Sets up the process, changes into the data directory, and starts the
/// embedded server.
fn start_server(args: &Args) -> Result<EmbeddedServer, ExitCode> {
    set_up_process();
    if let Err(err) = enter_data_dir(&args.data_dir) {
        error!(
            "Can't use the data directory {}: {err}",
            args.data_dir.display()
        );
        return Err(ExitCode::FAILURE);
    }
    info!("Data directory: {}", args.data_dir.display());
    info!(
        "New players get gamemode {} (--gamemode)",
        args.settings.gamemode.name()
    );
    EmbeddedServer::start(embedded::config(&args.settings)).map_err(|err| {
        error!("{err}");
        ExitCode::FAILURE
    })
}

/// The scripted host session: log in, spawn, walk east, check the move on the
/// server, optionally linger, and report.
fn play(server: &EmbeddedServer, args: &Args) -> ExitCode {
    let mut session = match Session::connect(&server.connector, &args.name, args.radius) {
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
            session.check_gamemode(args.settings.gamemode);
            checks.push(walk_and_check(&mut session, server, &spawn));
            if args.measure || args.stream.is_some() {
                // These runs stay a minute or more. A host left standing
                // with its head in terrain dies in about 10 s, and the server
                // then ignores its input. Rising is a vertical move, so the
                // chunks the server sends don't change.
                info!("Rising to eye height {} for the measurements", measure::SAFE_EYE_Y);
                session.rise_to(measure::SAFE_EYE_Y);
            }
            // `PlayerSpawn` only means the server has queued enough chunks,
            // not that they've arrived, so wait for them before checking.
            let join_chunks = wait_for_chunks(&mut session);
            if args.measure {
                measure(&mut session, server, join_chunks);
            }
            if let Some(blocks) = args.stream {
                measure::stream(&mut session, &server.server, blocks, join_chunks);
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
    checks.push(checks::chunks_decoded(&session));
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
    let mut passed = checks::report(&checks);
    if let Some(path) = &args.map {
        passed &= write_map(&session, path);
    }
    // Dropping the session closes the connection: the host leaves the game
    // before the server stops.
    drop(session);

    if passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Writes the map of the loaded chunks, marking the host. Returns false if it
/// couldn't.
fn write_map(session: &Session, path: &Path) -> bool {
    let host = session.spawn().map(|_| session.position());
    if let Some(host) = host {
        info!(
            "Host at x {:.1}, z {:.1} (eyes at y {:.1})",
            host.x, host.z, host.y
        );
    }
    match map::write(session.world(), host, path) {
        Ok(map) => {
            let (width, height) = map.image.dimensions();
            info!(
                "Map: {} chunks, {width}×{height} px, {} unknown ids -> {}",
                map.chunks,
                map.unknown_ids.len(),
                path.display()
            );
            for id in &map.unknown_ids {
                debug!("Unknown block id on the map: {id:#010x}");
            }
            true
        }
        Err(err) => {
            error!("Couldn't write the map: {err}");
            false
        }
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
    // A chunk that still differs is either a decoder bug or a change the
    // server never sent. A fresh encode of the server's chunk tells them
    // apart: if our decoder reads it back equal to the server, the decoder is
    // right and the difference is drift. Retried on later ticks, since the
    // server can change the chunk between the encode and the comparison.
    let mut drifted = 0;
    let mut fresh_examples = Vec::new();
    differing.retain(|&(x, z)| {
        for _ in 0..FRESH_DECODE_ATTEMPTS {
            let Some(server_chunk) = level
                .loaded_chunks
                .get(&Vector2::new(x, z))
                .map(|entry| entry.value().clone())
            else {
                return true;
            };
            let mut attempt = Vec::new();
            if checks::fresh_decode_matches(&server_chunk, session.world().blocks(), &mut attempt) {
                drifted += 1;
                return false;
            }
            fresh_examples = attempt;
            session.tick();
            thread::sleep(TICK);
        }
        true
    });

    let took = start.elapsed().as_secs_f64();
    let not_loaded = positions.len() - compared;
    if differing.is_empty() && compared > 0 {
        let mut detail = format!(
            "{compared} chunks equal block for block on both layers and biome for biome: \
             {} on the full pass ({full_pass:.2} s), {} on a re-check",
            compared - first_differing,
            first_differing - drifted,
        );
        if drifted > 0 {
            for example in &examples {
                warn!("Changed by the server without an UpdateBlock: {example}");
            }
            detail += &format!(
                "; {drifted} hold {differ} values the server changed without sending an \
                 UpdateBlock (logged above), and a fresh encode of each decodes equal to the \
                 server"
            );
        }
        detail += &format!("; {not_loaded} no longer loaded on the server; {took:.2} s");
        return Check::new(NAME, true, detail);
    }
    for example in &examples {
        warn!("Difference: {example}");
    }
    for example in &fresh_examples {
        warn!("Difference in a fresh encode: {example}");
    }
    Check::new(
        NAME,
        false,
        format!(
            "{} of {compared} chunks still differ after {:.1} s of re-checks, {differ} values, \
             and a fresh encode doesn't decode equal to the server either (logged above); \
             {took:.2} s",
            differing.len(),
            WORLD_SETTLE.as_secs_f64(),
        ),
    )
}

/// Keeps playing until the join's chunks stop arriving, so the checks and
/// measurements cover a whole join.
fn wait_for_chunks(session: &mut Session) -> u32 {
    let start = Instant::now();
    let mut seen = session.level_chunks();
    let mut last_new = Instant::now();
    let _ = session.run_until(Instant::now() + CHUNKS_MAX_WAIT, |s| {
        if s.level_chunks() != seen {
            seen = s.level_chunks();
            last_new = Instant::now();
        }
        last_new.elapsed() >= CHUNKS_QUIET
    });
    info!(
        "{seen} LevelChunk packets in this join (waited {:.1} s)",
        start.elapsed().as_secs_f64()
    );
    seen
}

/// Logs the per-packet table, then the chunk cost phases and the numbers the
/// "special-case chunks with `Arc`" decision needs, then runs the block
/// change probe.
fn measure(session: &mut Session, server: &EmbeddedServer, join_chunks: u32) {
    if cfg!(debug_assertions) {
        warn!("This is a debug build: the timings below are meaningless");
    }
    session.stats().log_table();
    let start_game_throughput = session
        .start_game()
        .map(|start_game| measure::start_game_throughput(start_game, 1000));
    match measure::chunk_costs(&server.server, 10, || session.tick()) {
        Some(costs) => {
            measure::log_costs(session.stats(), &costs, join_chunks, start_game_throughput);
        }
        None => warn!("No loaded chunks to measure"),
    }
    measure::block_change_probe(session, &server.server);
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
