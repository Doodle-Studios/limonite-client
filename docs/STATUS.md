# Status

A living page: where the project is, how to build and run it, and what bites. Update it whenever a
milestone step lands. Architecture decisions live in `docs/PLAN.md`; this page is about progress
and environment.

_Last updated: 2026-10-23._

## Where we are

- **Current milestone:** 4, first pixels. **Planned and approved on 2026-10-22:**
  `docs/milestones/m4-plan.md`. **In progress: Steps 1 to 6 are done; next is Step 7** (chunk
  unloading).
  - **What it builds:** a window (winit), a wgpu device, CPU meshes per sub-chunk and a fly
    camera that flies the host. It also adds chunk unloading, and a `packs` crate that reads the
    vanilla packs from the user's Bedrock install at runtime.
  - **Done so far** (commits on `main`, nothing pushed):
    - Steps 1 and 2: the fork's `NetworkChunkPublisherUpdate` reader (`1082621`) and
      `Weather::downfall()` (`443b954`), bumped in `9337913`.
    - Step 3, `657a8b1`: the `packs` crate, a layered VFS over folder packs and `__brarchive`
      archives.
    - Step 4, `8c52765`: `--headless` for the scripted session; the default path finds the
      install and loads the vanilla stack.
    - Step 5, `33be32f`: Bedrock block names from the fork's block palette.
    - Step 6, `08ce0c0`: block appearance, the texture array's CPU half, and biome tints.
  - **One owner decision so far** (2026-10-23): `blocks.json` merges per block, then per field
    (m4-plan Deviation 1).
  - **Fork changes:** a client-side reader for `NetworkChunkPublisherUpdate`, and one deliberate
    non-seam edit: a `Weather::downfall()` accessor in `pumpkin-util/src/biome.rs`, an upstream
    candidate. The fork's `limonite` branch is at `443b954`, two commits ahead of
    `origin/limonite`.
- **Milestone 3 is done (2026-10-22).** Plan, results and deviations: `docs/milestones/m3-plan.md`.
  The `Arc` decision is made: bytes stay (PLAN.md, "Why bytes, not structs").
- **Pushed, both public, with the owner's OK** (M1 and M2 on 2026-10-20, M3 on 2026-10-22):
  - superproject `main` to `https://github.com/Doodle-Studios/limonite-client` (`origin`; `main`
    tracks `origin/main`);
  - the fork's `limonite` branch, at `e251ffb`, with the M1, M2 and M3 fork commits. It tracks
    `origin/limonite`.
- **No upstream PR is open.** The candidates are listed under "Upstream PR candidates".

  Ask the owner before pushing anything, and before opening an upstream PR.
- **Fork location:** the owner moved the fork to `Doodle-Studios/Pumpkin`. `.gitmodules` and
  `origin` point there. The old `LaDoodleInTheHat/Pumpkin` URL redirects, but don't use it.

## Done

| Milestone | Result | Commits |
| --- | --- | --- |
| 0. Recon | Official Bedrock 1.26.52 joined stock Pumpkin (fork at `aac0a82`) over NetherNet; note in `docs/recon/m0-pumpkin-bedrock.md`; PLAN.md corrected | `main`: `baa0cf0` scaffold, `dab65f0` recon note, `c2d9df1` PLAN.md corrections |
| 1. Workspace + fork seam | Cargo workspace with `client/`; fork as a submodule; `Transport` trait with NetherNet behind it; Java listener behind `java-listener`; `tracing` in the client. Bedrock 1.26.52 joined a `--no-default-features` server at `80b19bb` with `java.enabled = true` and no TCP 25565 listener | `main`: `1b6b2ad` plan, `ae0bcbe` workspace, `c1feeb2` submodule, `4ec46ac` tracing, `59fd378` profile cleanup, `83b63a9` bump. Fork `limonite`: `df1f8e524` Transport, `80b19bb9b` java-listener |
| 2. Headless embedded client | The client runs Pumpkin on a background thread and joins it as the host over an in-memory `LocalChannel`: unsigned-token login, spawn, 329 chunks, an 8-block walk the server saw (7.8), and no socket owned by the process. Client codecs in `pumpkin-protocol` behind `client`, golden-tested byte for byte against official-client captures. NetherNet regression join passed. `LocalChannel` cost measured; chunks dominate, and the `Arc` decision is deferred to M3's planning | `main`: `ebc85a9` plan, `4d67904` bump + deps, `65114e0` embedded server, `283ee14` login and spawn, `1c2a6ab` walk + check, `a107aeb` measure, `679a421` bump, `7a69f6f` block-actor timing, `4b32b93` packet names, then the tick. Fork `limonite`: `de0f7cedc` accept_bedrock, `7c1fa8063` LocalChannel, `85f58b5f6` client codecs, `03b498c47` NBT test fix |
| 3. Chunk map | The client decodes every `LevelChunk` into Bedrock-native world state and applies `UpdateBlock`. The decoded world equals the server's `ChunkData` block for block on both layers, and biome for biome: 329 chunks in the dev world, 3,725 at radius 32. `--map` writes a top-down map, which the owner judged a match for Bedrock 1.26.52 at the same seed and position (stock fork server over NetherNet, also the regression join). The fork's palette conversion is 4.8× faster (encode 696 → 145 µs per chunk), byte-identical. The `Arc` decision: bytes stay | `main`: `9b46385` plan, `0a5d84a` bump + deps, `cd8844f` decoder, `72ea959` world check, `5b6dd8c` map, `49def42` measure, `5aa5108` progress, `5402089` drift vs decoder bug, `fbbd472` bump, `cdfb2bd` keep the host alive, `fac65e2` results, `9068f3a` Arc decision, then the tick. Fork `limonite`: `dc3a0ac2e` UpdateBlock reader, `e251ffbda` palette speedup |

Key facts from M0 (details in the recon note, which is required reading before touching the fork):
- **Transport:** upstream Bedrock is **NetherNet** (WebRTC), not RakNet.
- **Protocol:** **exactly 2193** is accepted. That's Bedrock 1.26.51, and 1.26.52 also speaks 2193.
- **The seam:** `BedrockClient::session: Arc<NetherNetSession>`, which uses six methods.
- **LocalChannel:** carries uncompressed bytes. Compression only turns on via
  `RequestNetworkSettings`, so the host never sends it.
- **Offline login:** needs an unsigned token with a `cpk` key, plus 10 `ClientData` fields
  (recon §6).
- **Release blocker:** the build compiles Mojang's Java datapack into the binary. Tracked in
  PLAN.md Risks and Open questions.

Key facts from M1 (details and deviations in `docs/milestones/m1-plan.md`):
- **The seam:** `crates/pumpkin-core/src/net/bedrock/transport.rs` holds `trait Transport` and
  `impl Transport for NetherNetSession`. `BedrockClient::session` is now `Arc<dyn Transport>`.
- **Java listener:** `pumpkin-core` has an opt-in `java-listener` feature, and `lib.rs` gates the
  bind with `cfg!()`. The `pumpkin` binary has it on by default. The client depends on
  `pumpkin-core` without it.
- **Fork diff:** five files against `aac0a82`. Keep new fork code in new modules.

Key facts from M2 (details, results and deviations in `docs/milestones/m2-plan.md`):
- **The client's modules** (`client/src/`):
  - `embedded.rs`: the server thread and its config, built in code;
  - `login.rs`: the host's unsigned tokens;
  - `packets.rs`: id-to-name table and batch framing;
  - `session.rs`: the 20 Hz loop, the skeleton of the M4 main loop;
  - `checks.rs` and `measure.rs`.
- **The fork:**
  - `net/bedrock/local.rs` holds `LocalChannel`, and `PumpkinServer::accept_bedrock` is the
    shared accept path.
  - `pumpkin-protocol`'s `client` feature adds the client-side codecs: derives on the structs,
    and hand-written mirrors in `bedrock/client_codecs/`.
  - The fork diff is 32 files against `aac0a82`, and edits to existing files are small.
- **Framing on `LocalChannel`:** one packet per batch; no `0xFE` and no compression either way.
- **Golden fixtures:** in `client_codecs/fixtures/`, with provenance in its README.
  - The `SLogin` capture holds the owner's Xbox identity, so it lives only in
    `run/captures/slogin.bin` (gitignored).
  - Raw captures are in `run/captures/m2-official/`.

Key facts from M3 (details, results and deviations in `docs/milestones/m3-plan.md`):
- **New client modules:**
  - `world/` holds the world state: `storage.rs` (one palette storage), `chunk.rs`,
    `level_chunk.rs` (the decoder and its typed errors), and `mod.rs` (`World`, the block table).
  - `map.rs` draws the map.
- **The world check:** "world matches server" compares through independent code on each side.
  After one full pass it re-checks the chunks that differed. If a chunk still differs, a fresh
  encode of the server's copy tells server drift apart from a decoder bug.
- **The fork:** two commits in M3.
  - `dc3a0ac` derives a `CUpdateBlock` reader behind `client`.
  - `e251ffb` speeds up the palette conversion, touching only `palette.rs`, with the old code kept
    as a test oracle.
  - The fork diff is 34 files against `aac0a82`.
- **Measurements** (release; chunk cost in phases):
  - E, the encode: 145 µs per chunk;
  - C, the palette conversion: 130 µs;
  - D, our decode: 21 µs;
  - S, a snapshot: 8 µs.
  - A radius-32 join is 3,725 chunks in about 21 s.
- **Pumpkin bug:** its fire never tells clients about some changes (Upstream PR candidates).

## Dev environment

Windows 11, with Git Bash and PowerShell both used.

**Toolchain**
- rustc/cargo **1.99.1** stable (MSVC), with clippy.
- VS 2022 C toolchain and CMake are installed. There is **no Python**; use PowerShell for JSON.

**Pumpkin fork: `pumpkin/`**
- A git submodule of `https://github.com/Doodle-Studios/Pumpkin`, branch **`limonite`**, at
  `03b498c`: upstream `aac0a82` (2026-10-18) plus two M1 and four M2 commits. Its git directory
  is `.git/modules/pumpkin`.
- `upstream` points at `https://github.com/Pumpkin-MC/Pumpkin.git`, whose default branch is
  `master`.
- **Fresh clone:** clone `https://github.com/Doodle-Studios/limonite-client`, then set long
  paths before the submodule checks out. Run `git -c core.longpaths=true submodule update
  --init`, then `git -C pumpkin config core.longpaths true`.

**Build and run the client** (run at the repo root):

```bash
RUST_LOG=info,limonite_client=debug cargo run -p limonite-client -- --map run/m3/map.png
```

- **What it does:**
  1. Starts the embedded server and joins as the host.
  2. Walks 8 blocks east, then waits for the join's chunks.
  3. Logs one `PASS`/`FAIL` line each for spawn, movement, chunks, chunks decoded, and world
     matches server.
  4. Writes the map, if asked.

  The exit code is non-zero on any failure.
- **Debug logging:** `limonite_client=debug` logs every packet name received, and the world
  check's first-pass differences.
- **Flags:**
  - `--data-dir <path>`: default `run/client`, gitignored. The client changes into it, because
    Pumpkin writes its world there.
  - `--name <name>`: the host's display name, default `Limonite`.
  - `--linger <secs>`: stay in-world afterwards, for outside inspection; it logs the process id.
  - `--map <path.png>`: a top-down map of the loaded chunks, written after the checks.
  - `--radius <chunks>`: the view distance to ask for, 2 to 32 (default 8). The embedded server
    allows up to 32.
  - `--stream <blocks>`: after the join, fly that many blocks east and log how chunks stream.
  - `--measure`: the cost table, chunk cost in phases, and the block-change probe. Only
    meaningful in a release build:

    ```bash
    CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 cargo run --release -p limonite-client -- --measure
    ```

    With `--measure` or `--stream`, the host rises to eye height 330 first (Gotchas, "The host
    has no physics").
  - **Repeatable measurements:** run each on a fresh copy of one template world (`--data-dir
    run/m3-<label>`), as M3 did.
- **Run time:** a dev run takes about 25 s plus the linger. The world check's full pass is about
  6 s in a dev build and under 1 s in release at radius 8. On a fresh world, `StartGame` came
  1.85 s after the resource packs.
- **First builds:** about 3 minutes for dev, and 5 for release with the env vars. They compile
  `pumpkin-core` into the root `target/`, separate from `pumpkin/target/`.
- **Socket check** while it lingers, in PowerShell:

  ```powershell
  Get-NetTCPConnection -OwningProcess <pid>; Get-NetUDPEndpoint -OwningProcess <pid>
  ```

  Both should return nothing.

**Mojang server jar** (needed only when `pumpkin/assets/datapack/` is missing, e.g. a fresh clone)
- File: `run/mojang/server.jar`, Minecraft **Java 26.3**, from Mojang's version manifest.
- SHA-1: `33680f5f2ac32864d6d7cf5e56a705fdb3e05f4c`, 62,294,556 bytes.
- Point the build at it with
  `PUMPKIN_MINECRAFT_SERVER_JAR="C:\Users\LaDoodleInTheHat\Desktop\big projects\limonite-client\run\mojang\server.jar"`.
  It must be a Windows-style path, because the build script is a native Windows program.

**Build the fork's server** (run in `pumpkin/`):

```bash
CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 cargo build --release --locked -p pumpkin
```

- Takes about 5.5 minutes from clean.
- The env vars skip the slow fat-LTO link without editing fork files. Always use the same ones,
  or Cargo rebuilds everything.
- Add `--no-default-features` for a Bedrock-only server: stock minus the Java listener. That's
  how M1 tested it. The output path is the same, so check which build `pumpkin.exe` is.

**Lint the fork** (run in `pumpkin/`): Pumpkin denies clippy pedantic and nursery lints.

```bash
cargo clippy -p pumpkin-core --all-targets
```

- Run it again with `--features java-listener`. Then run
  `cargo test -p pumpkin-core --lib net::bedrock`.
- For the client codecs, run `cargo clippy -p pumpkin-protocol --all-targets`, plain and with
  `--features client`. Also run `cargo clippy -p pumpkin-core --all-targets --features
  pumpkin-protocol/client`, because that's the feature-unified build the client produces. Then
  run `cargo test -p pumpkin-protocol --features client`.
- The `SLogin` golden test is `#[ignore]`d, because its capture can't be committed. Run it
  locally:

  ```bash
  LIMONITE_SLOGIN_FIXTURE="C:\Users\LaDoodleInTheHat\Desktop\big projects\limonite-client\run\captures\slogin.bin" cargo test -p pumpkin-protocol --features client --lib reproduces_the_official_client_login -- --ignored
  ```

**Run the server**
- Run from `run/pumpkin/` (gitignored); it writes its world and config there:

  ```bash
  RUST_LOG="info,pumpkin_core::net::bedrock=debug" ../../pumpkin/target/release/pumpkin.exe
  ```

- `run/pumpkin/pumpkin.toml` overrides `networking.bedrock.online_mode = false`,
  `networking.java.enabled = false` and `telemetry.enabled = false`. Pumpkin fills in all other
  defaults on first start.
- Expect `Bedrock NetherNet signaling is listening on 0.0.0.0:19132`, then
  `Accepted Bedrock NetherNet connection` and `… joined the game` when the client connects.

**Join from the official client**
- Bedrock → Play → Servers → Add Server, address `127.0.0.1`, port `19132`.
- Loopback works with no exemption needed.

## Gotchas

- **Long paths:** set `git config core.longpaths true` in any Pumpkin clone. Some paths exceed
  Windows `MAX_PATH`, and the checkout fails half-way without it.
- **NBT order:** `NbtCompound` keeps its tags in a `HashMap`, so two equal compounds serialise
  their tags in different orders. Compare decoded NBT in tests, never bytes, unless the
  compound has one tag.
- **Which structs go clientbound:** Pumpkin sends some serverbound structs to clients too:
  `SText`, `SActorEvent`, `SAnimate`, `SRespawn` and `SContainerClose`. Look in
  `bedrock/server/` as well as `bedrock/client/` when a packet id seems to be missing.
- **The host's position persists:** the dev world saves the player per UUID, which comes from
  the fixed dev `xid`. Every run starts where the last one ended, so delete `run/client/` for a
  fresh world.
- **Captures are private:** `run/captures/slogin.bin` contains the owner's Xbox token and
  identity. Never commit it, paste it, or print its contents.
- **Exact protocol:** Pumpkin accepts one Bedrock protocol only. Don't update the Bedrock client
  mid-milestone; if it updates past 2193, the official client can no longer join.
- **Stopping the server:** stopping a background shell doesn't always kill `pumpkin.exe` on
  Windows. Check with `Get-Process pumpkin`.
- **Telemetry:** Pumpkin's defaults to **on** (heartbeat to `market.pumpkinmc.org`). Keep it off in
  every config.
- **Lockfile and profiles:** building from the repo root ignores Pumpkin's `Cargo.lock`,
  profiles and `rust-toolchain.toml`. The root has mirrored copies, so when the fork changes them:
  - **After a rebase onto upstream,** copy `pumpkin/Cargo.lock` over the root `Cargo.lock` again
    and build. Shared crates then match the versions the fork was tested with.
  - **Profiles:** copy any new or changed ones into the root `Cargo.toml`. The `pumpkin-wasm-host*`
    overrides are left out on purpose: no root crate builds the plugin host.
- **Misleading banner:** a `--no-default-features` server with `networking.java.enabled = true`
  still prints `Connect using port: Java Edition: 0.0.0.0:25565`. That line is
  `crates/pumpkin/src/main.rs:194` echoing the config. Nothing is bound, so check sockets with
  `Get-NetTCPConnection -LocalPort 25565 -State Listen`, not the log. Left unfixed to keep the fork
  diff small; the embedded server never runs that `main.rs`.
- **Fork lints:** Pumpkin denies `clippy::use_self`, so code inside `impl … for NetherNetSession`
  writes `Self::send(..)`, not `NetherNetSession::send(..)`. It also denies `doc_markdown`, so put
  CamelCase words like `NetherNet` in backticks in doc comments.
- **Firewall:** Pumpkin binds `0.0.0.0`, so Windows Firewall may prompt. Allow it on Private
  networks only, or cancel; loopback works either way.
- **Untracked on purpose:** `.idea/` (RustRover) and `.claude/settings.local.json`.
- **The host has no physics.** The scripted walk keeps one height, so it can walk into a hillside.
  - **What happens:** the server suffocates the host: a hurt event every 0.5 s, dead in about
    10 s. Pumpkin then ignores the dead player's input, so it "stops moving".
  - **Why it carries over:** the dev world saves the host's position, so a later run can start
    inside terrain.
  - **Measured runs:** with `--measure` or `--stream`, the host rises to eye height 330 first.
  - **If acceptance runs start failing this way,** delete `run/client/` for a fresh world.
- **Pumpkin's log file keeps INFO only.** `logs/latest.log` in the server's directory has no
  debug lines, e.g. `Accepted Bedrock NetherNet connection`. Read those in the console.
- **Some block changes never reach clients.** Pumpkin's fire uses `NOTIFY_NEIGHBORS` alone
  (Upstream PR candidates). The world check tells that kind of drift apart from a decoder bug.

## Notes for M4 (found in M3)

All of these are folded into `docs/milestones/m4-plan.md`.

- **Chunk unloading isn't done.** The client keeps every chunk it receives.
  - **What should drive it:** `NetworkChunkPublisherUpdate` gives the view's centre and radius,
    which says what to drop.
  - **Why it matters:** chunks left behind go stale. The server only sends block updates for
    chunks inside a player's view (m3-plan Deviation 13).
- **The world-state API** (`client/src/world/`):
  - **The world:** `World::apply(&RawPacket)` takes `LevelChunk` and `UpdateBlock`.
    `World::chunk(x, z)` and `World::chunks()` read it.
  - **A chunk:** `Chunk::block(x, y, z, layer)` and `Chunk::biome(x, y, z)` take x and z within
    the chunk and an absolute y. A missing sub-chunk or layer reads as air.
  - **Sub-chunks:** `Chunk::sub_chunks[i].layers` holds the `Storage`s. `Storage::get` does no
    per-read check, because indices are checked once at decode.
  - **For a mesher:** walk the sub-chunks directly and skip all-air ones
    (`Storage::Single(AIR)` on every layer), as `map.rs` does.
- **Map colours are Java's** (`Block::map_color`), reached through the Bedrock-to-Java table. Fine
  for the map, not for rendering.
- **Textures will need Bedrock block names.** The world holds Bedrock network ids, which are
  hashes (`StartGame` says so). Vanilla Bedrock textures are keyed by Bedrock block names and
  states, but `pumpkin-data` only maps Java states to Bedrock ids. M4 needs a Bedrock id →
  Bedrock name-and-states table; plan where it comes from without bundling Mojang data.
- **The Bedrock-to-Java table isn't a true inverse.** It has 15,953 Bedrock ids for 35,723 Java
  states: 19,770 Java states share an id with a lower one, and the lowest Java state wins. Use it
  only for log names and map colours.
- **Decode runs on the main thread:** about 21 µs per chunk measured in a loop, and 32 µs in the
  live session. That's about 6 ms per second during a radius-32 join (at most 180 chunks a
  second), and about 1.4 ms per second flying at radius 32.
- **Streaming numbers** (release, flying at 11 m/s):
  - **measured:** 12, 23 and 44 chunks a second at radius 8, 16 and 32;
  - **walking**, extrapolated: 6, 10 and 19.
  - **The shape:** chunks come in bursts, about a row within a second of each chunk crossing.
  - **The model:** the rate is about d·v/16 (8–15% high). The 9-per-tick cap was never reached.
- **Spawn and chunks race:** `PlayStatus::PlayerSpawn` can arrive before any `LevelChunk`. The
  client waits for chunks to stop arriving before its checks.
- **No light data** comes with chunks (M0). Plan M4's first pixels as unlit or full-bright;
  lighting is M5.

Notes for M3 (found in M2) were all handled; see `docs/milestones/m3-plan.md`.

## Notes for M5 (found in M2, and when M4 was planned)

- **Gamemode and collision** (owner, when approving M4's plan): the host's gamemode is a setting
  (`--gamemode`), creative by default in M4.
  - **Why creative:** M4's fly camera passes through terrain, and a survival host with its head in
    a block suffocates.
  - **M5 revisits the default together with collision,** once the client has physics.
  - **Saved worlds:** the setting only applies to players without saved data. A saved host keeps
    its own mode (m4-plan Finding 11).
- **One server per process:** `SHOULD_STOP` and `STOP_INTERRUPT` are process globals, never
  reset. Switching worlds from a world list needs a process restart, or an upstream reset.
- **Host identity:** replace `login::DEV_XID` with a per-install identity saved with the
  settings. The player's UUID comes from the `xid`, so dev-world player data (inventory,
  position) won't carry over when it changes.
- **Permission level:** Pumpkin's Bedrock `StartGame` hard-codes 2 with a TODO. It's still an
  open question in PLAN.md.
- **Block actions:** the `PlayerAuthInput` writer refuses item, stack-request and block actions.
  Add those writers with block break and place.
- **Resource pack names:** only the HAVE_ALL_PACKS and COMPLETED status names are known (from
  the capture). Capture the others before sending them.
- **ECS:** pick `bevy_ecs` or `hecs` (PLAN.md open question).

## Upstream PR candidates

- **A base-dir parameter for Pumpkin**, so the embedding client needn't change the process's
  working directory (`main.rs`'s `enter_data_dir`). Pumpkin resolves the world, `ops.json`,
  bans, whitelist and user cache from the working directory.
- **The `client` codec feature in `pumpkin-protocol`**, with its golden fixtures.
- **Compression ignores its threshold:** every packet is deflated, even tiny ones (recon §5).
- **Faster Bedrock palette conversion:** fork commit `e251ffb`, which touches only
  `pumpkin-world/src/chunk/palette.rs` and cherry-picks onto upstream `master` unchanged. It
  replaces a SipHash lookup per block with a per-palette table. The output is byte-identical,
  checked against the old code kept as a test oracle. Before and after numbers are in
  `docs/milestones/m3-plan.md`, Results.
- **Fire never tells clients about its changes** (found in M3, still in upstream `master`
  `44d881c`). `block/blocks/fire/fire.rs` sets blocks with `BlockFlags::NOTIFY_NEIGHBORS` only in
  three places: fire that can't survive, fire put out by rain, and fire spreading. But
  `World::on_block_state_set` only queues a client update with `NOTIFY_LISTENERS`. So clients keep
  seeing fire that went out, and never see fire that spread. Vanilla uses flag 3 (`NOTIFY_ALL`)
  there. pumpkin-core has seven `set_block_state` calls with `NOTIFY_NEIGHBORS` alone, worth
  auditing together.
- **A second encode step, not tried yet** (m3-plan Deviation 12). The palette conversion is still
  about 89% of the faster encode: the packer divides by a runtime word size for each position,
  and `network_indices` builds a buffer per section. Measure it before proposing anything.

Notes for M2 (found while planning M1) were all handled; see `docs/milestones/m2-plan.md`.
