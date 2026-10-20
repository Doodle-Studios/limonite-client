# Status

A living page: where the project is, how to build and run it, and what bites. Update it whenever a
milestone step lands. Architecture decisions live in `docs/PLAN.md`; this page is about progress
and environment.

_Last updated: 2026-10-20._

## Where we are

- **Current milestone:** 3, the chunk map. **Not planned yet.** Start it in plan mode; read
  "Notes for M3" below first.
- **Milestone 2 is done (2026-10-20).** Plan, results and deviations: `docs/milestones/m2-plan.md`.
- **Pushed, both public, on 2026-10-20 with the owner's OK:**
  - superproject `main` to `https://github.com/Doodle-Studios/limonite-client` (`origin`; `main`
    tracks `origin/main`);
  - the fork's `limonite` branch, at `03b498c`, with the M1 and M2 fork commits. It tracks
    `origin/limonite`.

  Ask the owner before pushing anything.
- **Fork location:** the owner moved the fork to `Doodle-Studios/Pumpkin`. `.gitmodules` and
  `origin` point there. The old `LaDoodleInTheHat/Pumpkin` URL redirects, but don't use it.

## Done

| Milestone | Result | Commits (all pushed) |
| --- | --- | --- |
| 0. Recon | Official Bedrock 1.26.52 joined stock Pumpkin (fork at `aac0a82`) over NetherNet; note in `docs/recon/m0-pumpkin-bedrock.md`; PLAN.md corrected | `main`: `54a51e6` scaffold, `341cdfb` recon note, `1f86cbe` PLAN.md corrections |
| 1. Workspace + fork seam | Cargo workspace with `client/`; fork as a submodule; `Transport` trait with NetherNet behind it; Java listener behind `java-listener`; `tracing` in the client. Bedrock 1.26.52 joined a `--no-default-features` server at `80b19bb` with `java.enabled = true` and no TCP 25565 listener | `main`: `75ae93d` plan, `c27b3e4` workspace, `8b083d9` submodule, `7ed6d8e` tracing, `7ff616c` profile cleanup, `f8971b4` bump. Fork `limonite`: `df1f8e524` Transport, `80b19bb9b` java-listener |
| 2. Headless embedded client | The client runs Pumpkin on a background thread and joins it as the host over an in-memory `LocalChannel`: unsigned-token login, spawn, 329 chunks, an 8-block walk the server saw (7.8), and no socket owned by the process. Client codecs in `pumpkin-protocol` behind `client`, golden-tested byte for byte against official-client captures. NetherNet regression join passed. `LocalChannel` cost measured; chunks dominate, and the `Arc` decision is deferred to M3's planning | `main`: `b6e34c9` plan, `975b261` bump + deps, `d05e0e1` embedded server, `90e4f22` login and spawn, `441b7a1` walk + check, `b68d672` measure, `7d372f3` bump, `d4f045d` block-actor timing, `3d31577` packet names, then the tick. Fork `limonite`: `de0f7cedc` accept_bedrock, `7c1fa8063` LocalChannel, `85f58b5f6` client codecs, `03b498c47` NBT test fix |

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
RUST_LOG=info,limonite_client=debug cargo run -p limonite-client -- --linger 30
```

- **What it does:** starts the embedded server, joins as the host, walks 8 blocks east, then
  logs one `PASS`/`FAIL` line each for spawn, movement and chunks. The exit code is non-zero
  on any failure.
- **Debug logging:** `limonite_client=debug` logs every packet name received.
- **Flags:**
  - `--data-dir <path>`: default `run/client`, gitignored. The client changes into it, because
    Pumpkin writes its world there.
  - `--name <name>`: the host's display name, default `Limonite`.
  - `--linger <secs>`: stay in-world afterwards, for outside inspection; it logs the process id.
  - `--measure`: the cost table. Only meaningful in a release build:

    ```bash
    CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 cargo run --release -p limonite-client -- --measure
    ```

- **Run time:** a dev run takes about 5 s plus the linger. The dev build is fast enough: on a
  fresh world, `StartGame` came 1.85 s after the resource packs.
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

## Notes for M3 (found in M2)

- **Chunk layout:** `CLevelChunk::encode_chunk` (`net/bedrock/level_chunk.rs:68`) writes the
  layout M3 decodes, summarised in recon §8. M2 only counts `LevelChunk` packets.
- **Where the decoder lives:** if it needs Pumpkin types that only Pumpkin's own module can
  build, follow the `client_codecs` pattern. Otherwise the client crate is fine.
- **The `Arc` decision is deferred to M3's planning** (owner, 2026-10-20). Plan these three
  checks before deciding (PLAN.md, "Why bytes, not structs"):
  1. **Profile `encode_chunk`** (about 850 µs per 22 KB chunk in release), and look for a cheap
     speedup that could go upstream first. `convert_be_network`, the palette conversion, is the
     first suspect.
  2. **Check how Pumpkin mutates `ChunkData`.** Would a client holding a live `Arc` see a block
     change before the matching block-update packet arrives? If so, it would need a clone anyway.
  3. **Estimate the join and streaming costs** (moving through new chunks) at render distances 8,
     16 and 32, not just one join at 8.
- **Finish the measurement:** time chunk *decode* per chunk; that's the other half of what
  `Arc` would save.
- **Spawn and chunks race:** `PlayStatus::PlayerSpawn` can arrive before any `LevelChunk`.
  Don't treat "spawned" as "chunks loaded".
- **Batches:** each one holds exactly one packet; the outgoing task sends packets one by one.
- **Packet names:** the name table lists every clientbound packet. Unknown ids log as
  `Unknown(n)`, so a new one shows up in the debug log.

## Notes for M5 (found in M2)

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
- **Chunk encode speed** (see Notes for M3).

Notes for M2 (found while planning M1) were all handled; see `docs/milestones/m2-plan.md`.
