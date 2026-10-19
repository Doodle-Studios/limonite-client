# Status

A living page: where the project is, how to build and run it, and what bites. Update it whenever a
milestone step lands. Architecture decisions live in `docs/PLAN.md`; this page is about progress
and environment.

_Last updated: 2026-10-20._

## Where we are

- **Current milestone:** 2, the headless embedded client. **Planned and approved on 2026-10-20;
  not started.** The plan is `docs/milestones/m2-plan.md`.
- **Next step:** M2 Step 1, capturing the official client's packets as golden test fixtures.
  The owner has to join once with Bedrock 1.26.52.
- **Pushed:** the fork's `limonite` branch, at `80b19bb`, on 2026-10-19, with the owner's OK. It
  tracks `origin/limonite`.
- **Not pushed:** superproject `main`, which has no remote yet. Ask the owner before pushing
  anything.
- **Fork location:** the owner moved the fork to `Doodle-Studios/Pumpkin`. `.gitmodules` and
  `origin` point there. The old `LaDoodleInTheHat/Pumpkin` URL redirects, but don't use it.

## Done

| Milestone | Result | Commits (not pushed) |
| --- | --- | --- |
| 0. Recon | Official Bedrock 1.26.52 joined stock Pumpkin (fork at `aac0a82`) over NetherNet; note in `docs/recon/m0-pumpkin-bedrock.md`; PLAN.md corrected | `main`: `54a51e6` scaffold, `341cdfb` recon note, `1f86cbe` PLAN.md corrections |
| 1. Workspace + fork seam | Cargo workspace with `client/`; fork as a submodule; `Transport` trait with NetherNet behind it; Java listener behind `java-listener`; `tracing` in the client. Bedrock 1.26.52 joined a `--no-default-features` server at `80b19bb` with `java.enabled = true` and no TCP 25565 listener | `main`: `75ae93d` plan, `c27b3e4` workspace, `8b083d9` submodule, `7ed6d8e` tracing, `7ff616c` profile cleanup, `f8971b4` bump. Fork `limonite`: `df1f8e524` Transport, `80b19bb9b` java-listener |

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

## Dev environment

Windows 11, with Git Bash and PowerShell both used.

**Toolchain**
- rustc/cargo **1.99.1** stable (MSVC), with clippy.
- VS 2022 C toolchain and CMake are installed. There is **no Python**; use PowerShell for JSON.

**Pumpkin fork: `pumpkin/`**
- A git submodule of `https://github.com/Doodle-Studios/Pumpkin`, branch **`limonite`**, at
  `80b19bb`: upstream `aac0a82` (2026-10-18) plus the two M1 seam commits. Its git directory is
  `.git/modules/pumpkin`.
- `upstream` points at `https://github.com/Pumpkin-MC/Pumpkin.git`, whose default branch is
  `master`.
- **Fresh clone** (the superproject has no remote yet): after cloning it, set long paths before
  the submodule checks out. Run `git -c core.longpaths=true submodule update --init`, then
  `git -C pumpkin config core.longpaths true`.

**Build and run the client** (run at the repo root):

```bash
cargo run -p limonite-client
```

- Expect `Limonite Client 0.1.0 starting; embedded Pumpkin targets Bedrock 1.26.51 (protocol 2193)`.
  `RUST_LOG` filters as usual; the default is `info`.
- The first build takes about 3 minutes, because it compiles `pumpkin-core` and its dependencies
  into the root `target/`, separate from `pumpkin/target/`.

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

## Notes for M2 (found while planning M1)

All of these are handled in `docs/milestones/m2-plan.md`; they stay here for the record.

- **Logger:** Pumpkin's `init_logger` calls `.init()`, which panics if a global `tracing`
  subscriber already exists. The embedded server must not call it; the client owns logging.
- **Startup and shutdown hang:** `PumpkinServer::start` blocks on `LOGGER_IMPL.wait()`
  (`pumpkin-core/src/lib.rs:461` when `commands.use_console` is set, and **unconditionally at
  `:549`**; line numbers as of fork `80b19bb`). Before `start()`, call `pumpkin_core::LOGGER_IMPL.set(None)` and set
  `commands.use_console = false`. No fork change is needed.
- **Process setup:** `crates/pumpkin/src/main.rs` installs the `ring` rustls provider and builds a
  named Rayon pool before anything else. The embedded client must do the same.
- **`accept_bedrock` helper:** moving the NetherNet accept-branch body in `lib.rs` into an
  `accept_bedrock(transport, addr)` helper happens in M2, together with the `LocalChannel` branch
  that calls it.
- **Host login:** choose between unsigned tokens and a local login seam (recon §6).
