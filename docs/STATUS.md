# Status

A living page: where the project is, how to build and run it, and what bites. Update it whenever a
milestone step lands. Architecture decisions live in `docs/PLAN.md`; this page is about progress
and environment.

_Last updated: 2026-10-25._

## Where we are

- **Current milestone:** 5, playable singleplayer. **Planned and approved on 2026-10-24:**
  `docs/milestones/m5-plan.md`. The owner answered all fifteen questions as recommended, and added
  five amendments. Not started.
  - **First:** Step 1, the official-client capture (owner present, about 30 minutes, Bedrock
    1.26.52).
  - **Checkpoints** after Steps 7, 11, 14 and 16 are go/no-go points: if a phase overruns, the
    owner can tick what's done and split the rest into its own milestone.
  - **Next:** milestone 5b, containers, added to PLAN.md at M5's approval; then M6.
- **Milestone 4 is done (2026-10-24).** Plan, results and deviations: `docs/milestones/m4-plan.md`.
  - **What it built:** a window (winit) with a wgpu device and a fly camera that flies the host;
    CPU meshes for every loaded sub-chunk, drawn with vanilla textures read at runtime from the
    user's Bedrock install through the new `packs` crate; chunk unloading; and `--bench`, the
    frame-time baseline M6 must beat.
  - **The owner accepted it:** a flight in a fresh world, then a side-by-side with Bedrock 1.26.52
    at the same pose: a match, apart from the planned differences, once bamboo had its own shape.
  - **Owner decisions in M4:**
    - 2026-10-23: `blocks.json` merges per block, then per field (m4-plan Deviation 1);
    - 2026-10-24: the client crate builds at `opt-level = 1` in dev builds, so meshing keeps up
      after a join (Deviation 13). CPU meshing gets no further work ("Notes for M6");
    - 2026-10-24: bamboo gets its own shape (Deviation 14), and a census of the other stand-in
      cubes becomes the to-do list under "Special shapes to do".
- **Fork changes in M4:** a client-side reader for `NetworkChunkPublisherUpdate`, and one
  deliberate non-seam edit: a `Weather::downfall()` accessor in `pumpkin-util/src/biome.rs`, an
  upstream candidate.
- **Pushed, both public, with the owner's OK** (M1 and M2 on 2026-10-20, M3 on 2026-10-22, M4 on
  2026-10-24, the README and the wiki on 2026-10-25):
  - superproject `main` to `https://github.com/Doodle-Studios/limonite-client` (`origin`; `main`
    tracks `origin/main`);
  - the fork's `limonite` branch, at `443b954`, with the M1 to M4 fork commits. It tracks
    `origin/limonite`. The fork goes first, since `main`'s submodule pointer names its commit;
  - the wiki, `https://github.com/Doodle-Studios/limonite-client.wiki.git` (`master`), from its
    clone in `run/wiki`. It goes after `main`, since its pages name `main`'s commit.
- **No upstream PR is open.** The candidates are listed under "Upstream PR candidates".

  Ask the owner before pushing anything, and before opening an upstream PR.
- **Fork location:** the owner moved the fork to `Doodle-Studios/Pumpkin`. `.gitmodules` and
  `origin` point there. The old `LaDoodleInTheHat/Pumpkin` URL redirects, but don't use it.
- **README and wiki (2026-10-25):** the README is rewritten for GitHub, with the owner's lemon
  added to the title, and the docs are mirrored to the GitHub wiki. Keeping both current: Dev
  environment, "The README and the wiki".

## Done

| Milestone | Result | Commits |
| --- | --- | --- |
| 0. Recon | Official Bedrock 1.26.52 joined stock Pumpkin (fork at `aac0a82`) over NetherNet; note in `docs/recon/m0-pumpkin-bedrock.md`; PLAN.md corrected | `main`: `54a51e6` scaffold, `341cdfb` recon note, `1f86cbe` PLAN.md corrections |
| 1. Workspace + fork seam | Cargo workspace with `client/`; fork as a submodule; `Transport` trait with NetherNet behind it; Java listener behind `java-listener`; `tracing` in the client. Bedrock 1.26.52 joined a `--no-default-features` server at `80b19bb` with `java.enabled = true` and no TCP 25565 listener | `main`: `75ae93d` plan, `c27b3e4` workspace, `8b083d9` submodule, `7ed6d8e` tracing, `7ff616c` profile cleanup, `f8971b4` bump. Fork `limonite`: `df1f8e524` Transport, `80b19bb9b` java-listener |
| 2. Headless embedded client | The client runs Pumpkin on a background thread and joins it as the host over an in-memory `LocalChannel`: unsigned-token login, spawn, 329 chunks, an 8-block walk the server saw (7.8), and no socket owned by the process. Client codecs in `pumpkin-protocol` behind `client`, golden-tested byte for byte against official-client captures. NetherNet regression join passed. `LocalChannel` cost measured; chunks dominate, and the `Arc` decision is deferred to M3's planning | `main`: `b6e34c9` plan, `975b261` bump + deps, `d05e0e1` embedded server, `90e4f22` login and spawn, `441b7a1` walk + check, `b68d672` measure, `7d372f3` bump, `d4f045d` block-actor timing, `3d31577` packet names, then the tick. Fork `limonite`: `de0f7cedc` accept_bedrock, `7c1fa8063` LocalChannel, `85f58b5f6` client codecs, `03b498c47` NBT test fix |
| 3. Chunk map | The client decodes every `LevelChunk` into Bedrock-native world state and applies `UpdateBlock`. The decoded world equals the server's `ChunkData` block for block on both layers, and biome for biome: 329 chunks in the dev world, 3,725 at radius 32. `--map` writes a top-down map, which the owner judged a match for Bedrock 1.26.52 at the same seed and position (stock fork server over NetherNet, also the regression join). The fork's palette conversion is 4.8× faster (encode 696 → 145 µs per chunk), byte-identical. The `Arc` decision: bytes stay | `main`: `ee42446` plan, `e6c2d3e` bump + deps, `da890ae` decoder, `b2eff5b` world check, `dedeba9` map, `7804524` measure, `be0ef05` progress, `133bc94` drift vs decoder bug, `e904911` bump, `04f05ae` keep the host alive, `e0a5858` results, `55d0c36` Arc decision, then the tick. Fork `limonite`: `dc3a0ac2e` UpdateBlock reader, `e251ffbda` palette speedup |
| 4. First pixels | A window with a fly camera that flies the host, drawing every loaded sub-chunk with vanilla textures read at runtime from the user's install (55 vanilla packs, folders and `__brarchive` archives; 954 layers of 32×32). Block names from the fork's Bedrock palette; chunks the server's view has left are dropped. The owner accepted a flight in a fresh world and a side-by-side with Bedrock 1.26.52 at the same pose. M6 baseline in release: 2,794, 1,016 and 166 fps at radius 8, 16 and 32 | `main`: `02eabf1` bump, `cb6732e` packs crate, `508fcd7` install + `--headless`, `838c2a7` palette, `3a991cd` appearance, `c0cb67a` deviations, `50a2bcf` progress, `c254e45` unloading, `bddce75` mesher, `9f73aff` window, `d72703f` terrain, `b4cb2dc` dev opt-level, `fa01f0d` deviations, `5ccc2e5` progress, `34447e5` `--bench`, `545dc37` baseline, `4ea93d0` progress, `90397b3` bamboo, `3028183` census, then the tick. Fork `limonite`: `10826210f` publisher reader, `443b954b8` downfall |

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

Key facts from M4 (details, results and deviations in `docs/milestones/m4-plan.md`):
- **The `packs` crate** (`packs/`): `brarchive.rs` reads `__brarchive` archives with typed errors,
  `pack.rs` and `stack.rs` stack folder packs (`PackStack::vanilla` picks the vanilla packs), and
  `registry.rs` merges `blocks.json` and `terrain_texture.json`. PLAN.md, Resource packs, has
  the format and the rules.
- **New client modules:**
  - `install.rs` finds the Bedrock install and loads the vanilla stack;
  - `blocks/`: the palette (`palette_source.rs` is the only code that knows its source),
    appearance (shapes, textures, tints and render classes per Bedrock id), and the stand-in
    census;
  - `render/`: the device (`mod.rs`), camera, textures, tints, the mesher, the GPU meshes
    (`meshes.rs`), and the terrain pipelines and shader (`terrain.rs`, `terrain.wgsl`);
  - `app.rs` (the winit loop), `input.rs`, `settings.rs` and `bench.rs`.
- **The loop:** ticks are independent of frames. One `PlayerAuthInput` per 50 ms whatever the
  frame rate, so a minimised window stays connected.
- **Numbers:** a sub-chunk meshes in 144 µs in release and 585 µs in a dev build; a radius-8 view
  is about 1,937 meshes and 65 MB of vertex buffers. The palette parses in 35 to 43 ms in
  release.

## Dev environment

Windows 11, with Git Bash and PowerShell both used.

**Toolchain**
- rustc/cargo **1.99.1** stable (MSVC), with clippy.
- VS 2022 C toolchain and CMake are installed. There is **no Python**; use PowerShell for JSON.

**Pumpkin fork: `pumpkin/`**
- A git submodule of `https://github.com/Doodle-Studios/Pumpkin`, branch **`limonite`**, at
  `443b954`: upstream `aac0a82` (2026-10-18) plus the M1 to M4 commits. Its git directory is
  `.git/modules/pumpkin`.
- `upstream` points at `https://github.com/Pumpkin-MC/Pumpkin.git`, whose default branch is
  `master`.
- **Fresh clone:** clone `https://github.com/Doodle-Studios/limonite-client`, then set long
  paths before the submodule checks out. Run `git -c core.longpaths=true submodule update
  --init`, then `git -C pumpkin config core.longpaths true`.

**Run the client: the window** (run at the repo root):

```bash
cargo run -p limonite-client
```

- **What it does:** finds the Bedrock install, then loads the vanilla packs, the block palette,
  the appearance table and the textures. Each step logs one line, with counts and problems.
  Then it starts the embedded server, joins as the host, and opens the window.
  - **The install:** the GDK default, `<drive>:\XboxGames\Minecraft for Windows\Content`, or
    `--bedrock <path>` to the folder that holds `data/resource_packs`. Vanilla files are read at
    runtime and never copied.
- **Controls:** click in the window to capture the mouse, and move the mouse to look. W, A, S and
  D move, Space rises, Left Shift sinks, and holding Left Ctrl is three times faster. Esc
  releases the mouse. P logs the pose as a `/tp` command for the official client.
- **The title bar** shows the frame rate and frame time, chunks, meshes and the queue, visible
  meshes and draws, and the position. Closing the window saves the world and exits.
- **Window flags:**
  - `--gamemode <mode>`: survival, creative, adventure or spectator, for a host that joins a
    world for the first time; creative by default (the fly camera passes through terrain). A
    saved host keeps its own mode, with a warning.
  - `--bench <seconds>`: the M6 baseline flight ("Notes for M6"). Use a release build.
- **Dev builds** compile the client crate at `opt-level = 1` (root `Cargo.toml`), so meshing keeps
  up after a join. Overflow checks and debug assertions stay on.

**Run the client: the scripted session** (`--headless`, the M2 and M3 acceptance run; no GPU and
no Bedrock install needed):

```bash
RUST_LOG=info,limonite_client=debug cargo run -p limonite-client -- --headless --map run/m3/map.png
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
- **Flags for both:**
  - `--data-dir <path>`: default `run/client`, gitignored. The client changes into it, because
    Pumpkin writes its world there.
  - `--name <name>`: the host's display name, default `Limonite`.
  - `--radius <chunks>`: the view distance to ask for, 2 to 32 (default 8). The embedded server
    allows up to 32.
  - `--gamemode <mode>`, as above.
- **`--headless` only:**
  - `--linger <secs>`: stay in-world afterwards, for outside inspection; it logs the process id.
  - `--map <path.png>`: a top-down map of the loaded chunks, written after the checks.
  - `--stream <blocks>`: after the join, fly that many blocks east and log how chunks stream.
  - `--measure`: the cost table, chunk cost in phases, and the block-change probe. Only
    meaningful in a release build:

    ```bash
    CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 cargo run --release -p limonite-client -- --headless --measure
    ```

    With `--measure` or `--stream`, the host rises to eye height 330 first (Gotchas, "The host
    has no physics").
- **Repeatable measurements:** run each on a fresh copy of one template world (`--data-dir
  run/m<N>-<label>`), as M3 and M4 did.
- **Run time:** a headless dev run takes about 25 s plus the linger. The world check's full pass is
  about 6 s in a dev build and under 1 s in release at radius 8. On a fresh world, `StartGame`
  came 1.85 s after the resource packs.
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
- **In a Terminal tab, leave out the debug level** (m4-plan Deviation 16). NetherNet's
  per-datagram debug lines flood the tab, and Ctrl-C stops reaching the server. Plain INFO shows
  joins and commands, and stops cleanly on Ctrl-C.

**Official-client comparison** (M3 and M4): copy into the comparison folder, such as
`run/m4-official/`, only after a clean exit of our client:
- the world folder;
- `run/pumpkin/pumpkin.toml`;
- **`data/ops.json` with the owner at level 4** (copy `run/m3-official/data/ops.json`). Without it
  the server refuses every in-game command, while still logging it as executed.

Then run the stock fork server there, join from Bedrock, and type `/gamemode creative`,
`/time set noon`, `/weather clear` and the `/tp` our client logged with P. Screenshots stay in
the gitignored folder.

**Join from the official client**
- Bedrock → Play → Servers → Add Server, address `127.0.0.1`, port `19132`.
- Loopback works with no exemption needed.

**The README and the wiki**
- **The README** (`README.md`) is the GitHub landing page, kept current by CLAUDE.md's rule. Its
  current-phase alert, milestone badge and roadmap table follow this page.
  - **The title** (`docs/readme/title.png`) is a render of `docs/readme/models/title.bbmodel`, a
    Blockbench project in the Minecraft Title Generator plugin's format, with the lemon
    (`lemon.bbmodel`) beside the CLIENT plate. Re-render it with the plugin's Render: resolution
    2048, antialiasing on.
  - **The icons** on the feature cards are SVGs in `docs/readme/icons/`, stroked in the plate's
    lime, `#818600`. No emojis, at the owner's request.
  - **After pushing a changed image,** github.com can show the old one for about 5 minutes, while
    GitHub's raw-file cache expires.
- **The wiki** (`https://github.com/Doodle-Studios/limonite-client/wiki`) mirrors `docs/PLAN.md`,
  this page, `docs/recon/` and `docs/milestones/`, one page each. It's never edited by hand
  (CLAUDE.md, Rules).
  - **After committing doc changes,** run `pwsh scripts/sync-wiki.ps1`. It regenerates every page
    into `run/wiki`, a gitignored clone of the wiki, cloned if missing. It stops if the docs have
    uncommitted changes, and never commits or pushes.
  - **Then** check `git -C run/wiki diff`, and commit and push the wiki after `main`, with the
    owner's OK.
- **The owner's original models** from before the lemon (`title.bbmodel`, `lemon.bbmodel`, and
  their `.obj`, `.mtl` and texture exports) are in `run/readme-models-before-lemon/`, on the
  owner's machine only.

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
- **A running client locks its `.exe`.** On Windows, `cargo build` can't replace
  `target/debug/limonite-client.exe` while a window is open, so close it first. `cargo test` and
  `cargo clippy` neither touch nor rebuild it: run `cargo build` before starting the `.exe`
  directly, or it's stale.
- **Closing the window from a script:** `taskkill /PID <pid>`, without `/F`, sends `WM_CLOSE`. The
  host leaves, the world saves, and the exit code is 0. `/F` skips the save.
- **A harmless shutdown warning:** `Failed to send NetherNet packet … local client is gone` can
  appear as the client exits. The server sends a last packet after the host has left; nothing is
  lost.
- **The camera starts where the host was saved,** like the scripted run. Press P before closing if
  you'll want the spot again: the window can log a pose, but can't go to one yet.

Notes for M4 (found in M3) were all handled; see `docs/milestones/m4-plan.md`. Notes for M3 (found
in M2) were too; see `docs/milestones/m3-plan.md`.

## Notes for M5 (found in M2, M4's planning and M4)

M5's plan takes these up (`docs/milestones/m5-plan.md`): lighting and fog in Decisions 11 and 12,
special shapes in 7, the pose in 15 (a command line, so `/tp`), gamemode and collision in 5 and 6,
one server per process in 17, the identity in 16, the permission level in 18, block actions in
Step 2, and the ECS in 13. Plant offsets and tints stay out of scope, and M5 sends no new resource
pack status.

Found in M4:
- **Lighting:** chunks still carry no light data, and M4 draws only constant face shading. The
  sky, fog and clouds aren't drawn either.
  - **Fog will hide the view's edge.** Over water, rays that enter near the edge of the meshed
    area leave it before they reach the seabed, so a darker band of sky shows through the water
    (m4-plan Results, Step 10).
- **Plant offsets:** Java and Bedrock nudge flowers, grass and other plants off-centre per block.
  M4 draws every cross centred; only the bamboo stalk is nudged. Moving crosses, and bamboo's
  leaves, needs vertex positions that can leave their block: a bias in the vertex format.
- **Tints:** one water colour everywhere (`#44AFF5` at 65%), no per-biome overrides and no blending
  between biomes. The official client's deep ocean is darker (m4-plan Decision 8).
- **Special shapes:** a to-do list, sorted by how often they occur, is under "Special shapes to
  do".
- **Going back to a pose:** the window can log one (P), but can't go to one. A `--pose` flag, or a
  teleport once the host has commands, would make comparisons easier.

Found earlier:

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

## Notes for M6 (found in M4)

- **CPU meshing isn't worth optimising further** (owner, 2026-10-24): M6 replaces it with compute
  meshing. Parallel or worker-thread meshing comes back only if M6 keeps a CPU fallback path.
  - **Where it stands:** one sub-chunk at a time on the main thread, nearest first, within 4 ms a
    frame (m4-plan Decision 6).
  - **Its speed at radius 8:** 144 µs per sub-chunk in release, and 585 µs in a dev build. The
    client crate builds at `opt-level = 1` in dev builds for this; unoptimised, it took 1,956 µs
    (m4-plan Deviation 13).
- **The baseline M6 must beat** is in m4-plan Results, Step 11: frame times, draw calls and GPU
  totals at radius 8, 16 and 32. At radius 32 that's 166 fps, p99 15.6 ms, about 10,700 draw
  calls a frame, and 1.16 GB of vertex buffers.
  - **To compare,** run the same flight the same way: a release build with the env vars under
    Dev environment, and `--data-dir <a fresh copy of run/m4-template> --radius R --bench 30`.
  - **Keep `run/m4-template`.** It's gitignored, so it exists only on the owner's machine, and a
    new template gets a new seed, whose numbers won't compare.
  - **The flight leaves the template's area** at radius 16 and 32, so the server generates chunks
    during those runs. That's the same for M6, so it's fair, but a bigger template would take
    worldgen out of the numbers.

## Notes for M7 (found while planning M5)

- **Pumpkin trusts what a Bedrock client reports:** its position, unchecked, and the item it says
  it holds when placing (m5-plan Findings 1 and 3). PLAN.md's Risks has the row; M7 decides how to
  treat LAN guests.
- **StartGame tells every player permission level 2,** an operator (`world/mod.rs:2754-2755`, a
  TODO). M5 makes the host a real operator through `ops.json`, and reads the abilities packet
  instead, so a guest would be told it's an operator too. An upstream candidate.
- **Spawn protection is off** in the embedded config from M5 (m5-plan Decision 18), so guests could
  build at spawn.
- **Pumpkin ignores item interactions and item stack requests inside `PlayerAuthInput`**
  (`player_auth_input.rs:223-227`). Whether an official-client guest sends them there is for M7 to
  check.

## Special shapes to do (found in M4)

M4 draws cubes; slabs, snow layers, carpets and lily pads; crossed planes for plants; thin planes
for vines and lichen; liquids; and bamboo. Every other block whose Java shape isn't a full cube is
a stand-in cube. The owner asked for this list, sorted by frequency, as the to-do list for a
special-shapes pass.

**The census:** the stand-in blocks in the acceptance world (m4-plan Results). It covers 3,725
chunks at radius 32 around the comparison pose, at a jungle edge by the ocean, with lush caves,
mineshafts and trial chambers below: 109,881 blocks of 43 names. The window logs the same census
once a join's meshing settles, so a new world refreshes it.

| Block | Count | Shape to draw |
| --- | --- | --- |
| `leaf_litter` | 86,229 | Flat segments on the floor, like pink petals |
| `oak_fence` | 7,235 | A post, with arms toward its neighbours |
| `pointed_dripstone` | 6,991 | A tapering spike, by thickness |
| `rail` | 2,994 | A flat plane, or a slope |
| `iron_chain` | 1,249 | Two thin crossed planes, along its axis |
| `big_dripleaf` | 1,140 | A leaf platform on a stem |
| `waxed_oxidized_cut_copper_stairs` | 1,019 | Stairs |
| `azalea` | 835 | A bush: a box top over crossed planes |
| `flowering_azalea` | 455 | As `azalea` |
| `cocoa` | 257 | A small box by age, on a log |
| `ladder` | 213 | A plane against one face |
| `chest` | 177 | A smaller box (a block entity) |
| `waxed_oxidized_copper_trapdoor` | 132 | A thin box, open or shut |
| `torch` | 124 | A thin stick, or leaning on a wall |
| `small_dripleaf_block` | 121 | Crossed leaves on stems |
| `decorated_pot` | 108 | A pot (a block entity) |
| `spore_blossom` | 73 | Hanging flower planes |
| `grass_path` | 57 | A cube 15/16 tall |
| `flower_pot` | 56 | A small pot |
| `wooden_button` | 55 | A small box on a face |
| `white_wool_stairs` | 54 | Stairs |
| `candle` | 52 | One to four small candles |
| `red_candle` | 34 | As `candle` |
| `waxed_cut_copper_stairs` | 34 | Stairs |
| `waxed_oxidized_copper_door` | 34 | A thin box, open or shut |
| `hopper` | 26 | A funnel |
| `bed` | 24 | A low box (a block entity) |
| `farmland` | 20 | A cube 15/16 tall |
| `acacia_stairs` | 16 | Stairs |
| `stone_brick_stairs` | 15 | Stairs |
| `trip_wire` | 10 | A thin line |
| `acacia_fence` | 9 | As `oak_fence` |
| `sandstone_stairs` | 7 | Stairs |
| `straw_bed` | 6 | Not yet known |
| `stone_button` | 4 | As `wooden_button` |
| `tripwire_hook` | 4 | A hook on a face |
| `wooden_pressure_plate` | 4 | A thin slab |
| `glass_pane` | 2 | A thin pane, with arms toward its neighbours |
| `waxed_copper_door` | 2 | As the copper door |
| `acacia_pressure_plate` | 1 | A thin slab |
| `campfire` | 1 | Crossed logs |
| `oxidized_copper_chest` | 1 | As `chest` |
| `stonecutter_block` | 1 | A box 9/16 tall |

- **By family:** stairs are 6 names (1,145 blocks), fences 2, and doors and trapdoors 3. The block
  entities (chests, pots and beds) want their own models, which M4 leaves out.
- **Done in M4:** bamboo (m4-plan Deviation 14).

## Upstream PR candidates

- **`Weather::downfall()`** (fork `443b954`, M4): a one-line accessor in
  `pumpkin-util/src/biome.rs`, in place of an `#[expect(dead_code)]` on the field. The client
  tints grass and foliage from each biome's temperature and downfall.
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
