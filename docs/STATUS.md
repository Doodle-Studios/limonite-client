# Status

A living page: where the project is, how to build and run it, and what bites. Update it whenever a
milestone step lands. Architecture decisions live in `docs/PLAN.md`; this page is about progress
and environment.

_Last updated: 2026-10-19._

## Where we are

- **Current milestone:** 1, Workspace + fork seam. **Planned and approved; not started.**
- **Next step:** execute `docs/milestones/m1-plan.md` from step 2.
- **Then:** Milestone 2, the headless embedded client (see "Notes for M2" below).

## Done

| Milestone | Result | Commits (superproject `main`, not pushed) |
| --- | --- | --- |
| 0. Recon | Official Bedrock 1.26.52 joined stock Pumpkin (fork at `aac0a82`) over NetherNet; note in `docs/recon/m0-pumpkin-bedrock.md`; PLAN.md corrected | `54a51e6` scaffold, `341cdfb` recon note, `1f86cbe` PLAN.md corrections |

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

## Dev environment

Windows 11, with Git Bash and PowerShell both used.

**Toolchain**
- rustc/cargo **1.99.1** stable (MSVC), with clippy.
- VS 2022 C toolchain and CMake are installed. There is **no Python**; use PowerShell for JSON.

**Pumpkin fork: `pumpkin/`**
- A clone of `https://github.com/LaDoodleInTheHat/Pumpkin`, branch **`limonite`**, at `aac0a82`
  (upstream 2026-10-18).
- `upstream` points at `https://github.com/Pumpkin-MC/Pumpkin.git`, whose default branch is
  `master`.
- **Until M1 step 3** it's a plain clone, gitignored by `/pumpkin/` in `.gitignore`. M1 turns it
  into a submodule and removes that line.

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
  profiles and `rust-toolchain.toml`. M1 mirrors them at the root.
- **Firewall:** Pumpkin binds `0.0.0.0`, so Windows Firewall may prompt. Allow it on Private
  networks only, or cancel; loopback works either way.
- **Untracked on purpose:** `.idea/` (RustRover) and `.claude/settings.local.json`.

## Notes for M2 (found while planning M1)

- **Logger:** Pumpkin's `init_logger` calls `.init()`, which panics if a global `tracing`
  subscriber already exists. The embedded server must not call it; the client owns logging.
- **Startup and shutdown hang:** `PumpkinServer::start` blocks on `LOGGER_IMPL.wait()`
  (`pumpkin-core/src/lib.rs:456` when `commands.use_console` is set, and **unconditionally at
  `:544`**). Before `start()`, call `pumpkin_core::LOGGER_IMPL.set(None)` and set
  `commands.use_console = false`. No fork change is needed.
- **Process setup:** `crates/pumpkin/src/main.rs` installs the `ring` rustls provider and builds a
  named Rayon pool before anything else. The embedded client must do the same.
- **`accept_bedrock` helper:** moving the NetherNet accept-branch body in `lib.rs` into an
  `accept_bedrock(transport, addr)` helper happens in M2, together with the `LocalChannel` branch
  that calls it.
- **Host login:** choose between unsigned tokens and a local login seam (recon §6).
