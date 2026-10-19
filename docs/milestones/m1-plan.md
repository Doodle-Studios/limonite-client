# Milestone 1 plan: Workspace + fork seam

**Status:** ✅ **done 2026-10-19.** It was approved by the owner the same day. The plan below is
kept as written; "Deviations" at the end records where the real code differed.
- **Before starting:** read `docs/STATUS.md` for the environment and gotchas, and
  `docs/recon/m0-pumpkin-bedrock.md` for why the seam looks like this.
- **If the real code contradicts this plan,** stop and tell the owner (CLAUDE.md rule).
- **As you go:** teach. The owner is learning Rust, so explain the why behind each step,
  especially the ones marked "worth teaching". Update `docs/STATUS.md` as steps land.

## Goal and acceptance (from `docs/PLAN.md`)

The milestone delivers:
- a Cargo workspace;
- the fork as a submodule;
- a `Transport` trait with NetherNet behind it;
- the Java listener behind a feature flag;
- `tracing` wired up.

**Accept when:**
- the official Bedrock client still joins the fork over NetherNet;
- the fork diff is small and confined to the seam;
- the Java listener compiles out.

## Checks already done (2026-10-19, fork at `aac0a82`)

None of Pumpkin's workspace-level config applies when building from our root. So:

- **`[patch]`:** none in any `Cargo.toml`. Nothing to mirror.
- **`.cargo/config(.toml)`:** none anywhere in `pumpkin/`. Nothing to mirror.
- **`rust-toolchain.toml`:** exists, with `channel = "stable"` and
  `components = ["rust-analyzer", "rust-src"]`. **Mirror it at the root.**
- **Profiles:** mirror them in the root `Cargo.toml`, copied from `pumpkin/Cargo.toml`:
  - `[profile.release]` with `lto = true`, `strip = "debuginfo"`, `codegen-units = 1`;
  - per-package `codegen-units = 16` for `pumpkin-data`, `pumpkin-core`, `pumpkin-world`,
    `pumpkin-protocol`, `pumpkin-wasm-host`, `pumpkin-wasm-host-common`,
    `pumpkin-wasm-host-v0_1` and `pumpkin-wasm-host-v0_2`;
  - `[profile.bench] debug = true`;
  - `[profile.profiling]` (inherits release, `debug = true`, `strip = false`).
  - One deliberate deviation: Pumpkin's `[profile.dev] debug = false` becomes
    `[profile.dev.package."*"] debug = false`. Dependencies match Pumpkin, but our own client
    keeps debug info, so backtraces have line numbers while learning.
- **The `pumpkin` crate's existing features:** `console-subscriber` and `plugin-v0_2`, both
  opt-in. **There's no `default` today.**
  - After step 5 adds `default = ["java-listener"]`, `--no-default-features` alone is exactly
    stock minus Java.
  - Re-check when you get to step 7. If any other default has appeared, use
    `--no-default-features --features <those>`.
- **Internal deps:** Pumpkin's `[workspace.dependencies]` declares every internal crate with
  `default-features = false`. That's why `java-listener` must be **opt-in** (see step 5).
- **`pumpkin-core` has no wasmtime dependency,** so the client can link it without building the
  heaviest crate.

## Step 2: Cargo workspace (superproject)

Target layout:

```
limonite-client/
├── Cargo.toml           # virtual workspace: members = ["client"], exclude = ["pumpkin"], resolver = "3"
├── Cargo.lock           # seeded from pumpkin/Cargo.lock
├── rust-toolchain.toml  # mirrored from pumpkin/
├── client/              # package `limonite-client` (current src/main.rs moves here)
└── pumpkin/             # fork (submodule after step 3)
```

1. **Show the owner the error first. They asked to see it.**
   - Write the root workspace and `client/` **without** `exclude`, then run
     `cargo metadata --format-version 1 > /dev/null` (or `cargo check -p limonite-client`).
   - Show Cargo's exact error. Expect something like "package … pumpkin-core … is a member of
     the wrong workspace".
   - Then add `exclude = ["pumpkin"]` and show that it resolves.
   - **Why it fails without `exclude`** (worth teaching): Cargo makes any path dependency inside
     the workspace folder a member automatically. But Pumpkin's crates already belong to
     Pumpkin's own workspace, and a package can only be in one. Excluding `pumpkin/` lets the
     client depend on `pumpkin-core` by path while each Pumpkin crate still inherits
     `version.workspace = true` from `pumpkin/Cargo.toml`.
2. **Seed the lockfile** by copying `pumpkin/Cargo.lock` over the root `Cargo.lock` before the
   first build.
   - **Why:** only the root lockfile counts. Seeding makes Cargo reuse the exact versions Pumpkin
     was tested with, and only add or prune what's different.
3. **Root `Cargo.toml`:**
   - `[workspace.package]`: edition 2024, license GPL-3.0, rust-version 1.96 (matching Pumpkin).
   - `[workspace.dependencies]`: pin `tracing 0.1.44` and `tracing-subscriber 0.3.23` (features
     `env-filter`, `fmt`, `ansi`). These are the same versions Pumpkin uses, so the build has one
     copy of each.
   - The mirrored profiles from "Checks already done".
4. **`client/Cargo.toml`:**
   - `pumpkin-core = { path = "../pumpkin/crates/pumpkin-core" }`. No `java-listener` feature, so
     the host binary can never bind a Java socket.
   - `pumpkin-world = { path = "../pumpkin/crates/pumpkin-world" }`, for the version constants.
   - `tracing` and `tracing-subscriber` from the workspace.
   - M2 will probably add `opt-level` for dependencies once worldgen runs in-process.
     Nothing more for M1.
5. Copy `pumpkin/rust-toolchain.toml` to the root.

**Commit:** "Convert to a cargo workspace with the client in client/".

## Step 3: Pumpkin as a submodule

1. Remove the `/pumpkin/` line from `.gitignore`. That's the PLAN.md M1 note: otherwise git
   ignores the submodule path.
2. Run `git submodule add -b limonite https://github.com/LaDoodleInTheHat/Pumpkin.git pumpkin`.
   Git sees the existing clone and adds it as is.
3. Run `git submodule absorbgitdirs`, which moves `pumpkin/.git` into `.git/modules/pumpkin`,
   the standard layout. Check that `core.longpaths` is still set afterwards.
4. The superproject now records the submodule at `aac0a82`, the commit M0 tested.

**Commit:** "Add the Pumpkin fork as a submodule".

## Step 4: `Transport` seam (fork, branch `limonite`)

**New file** `crates/pumpkin-core/src/net/bedrock/transport.rs`:

```rust
/// The one place a Bedrock player's bytes enter and leave the server.
///
/// Limonite seam: exists so a player can be connected by NetherNet (LAN or
/// remote peers) or by an in-process `LocalChannel` (the host) without game
/// code knowing which. Method names match `NetherNetSession`'s on purpose,
/// so existing call sites compile unchanged.
pub trait Transport: Send + Sync {
    fn send(&self, batch: Bytes) -> BoxFuture<'_, Result<(), String>>;
    fn recv(&self) -> BoxFuture<'_, Option<Bytes>>;
    fn flush_reliable(&self, timeout: Duration) -> BoxFuture<'_, Result<(), String>>;
    fn close(&self) -> BoxFuture<'_, ()>;
    fn is_closed(&self) -> bool;
    fn client_public_key(&self) -> Option<&PublicKey>;
}
```

The pieces it uses:
- `BoxFuture` is `futures::future::BoxFuture`. `pumpkin-core` already depends on `futures`.
- `PublicKey` is `pumpkin_auth::p384::PublicKey`.
- `Bytes` is `bytes::Bytes`.

The same file holds `impl Transport for NetherNetSession`:
- Each async method calls the inherent method with fully-qualified syntax, e.g.
  `Box::pin(NetherNetSession::send(self, batch))`. Inherent methods take priority over trait
  methods anyway, but spelling it out shows exactly which one runs.
- `NetherNetSession::flush_reliable` is `pub(in crate::net::bedrock)`, so the impl has to live
  inside `net/bedrock`.

**Why `BoxFuture` and not `async fn`** (worth teaching):
- `BedrockClient` stores an `Arc<dyn Transport>`, and Rust can only use a trait through `dyn` if
  the trait is dyn-compatible.
- A plain `async fn` in a trait returns a different hidden future type for each implementer,
  which `dyn` can't express.
- A boxed future gives every implementer the same return type. The cost is one small allocation
  per call, which is nothing next to a network send.

**Edits to existing files:** `crates/pumpkin-core/src/net/bedrock/mod.rs`, about 4 lines:
- add `pub mod transport;`;
- change the `session` field (`mod.rs:98`) and the `new()` parameter (`mod.rs:141`) from
  `Arc<NetherNetSession>` to `Arc<dyn Transport>`;
- replace `use self::nethernet::NetherNetSession;` with the `Transport` import.

**No other call sites change.** Two Rust rules make that work (worth teaching):
- **Unsized coercion:** `lib.rs:669` passes an `Arc<NetherNetSession>`, which converts to
  `Arc<dyn Transport>` automatically.
- **Trait objects:** methods of the trait behind a `dyn Trait` can be called without importing the
  trait, so `self.session.flush_reliable(..)` in `login/request_network_settings.rs:24` compiles
  unchanged.

**Change from the recon doc:** pulling out the `accept_bedrock` helper in `lib.rs` moves to M2,
where `LocalChannel` gives it a second caller. Doing it now would be code with no user.

**Commit (fork):** "Add Transport seam for Bedrock connections".

## Step 5: Java listener behind a feature (fork)

- **`crates/pumpkin-core/Cargo.toml`:** add `[features]` with `java-listener = []`, and a comment
  saying why it exists.
  - It's **opt-in, not default:** every consumer uses `default-features = false`, so a "default
    on" feature would silently turn Java off in the stock binary.
- **`crates/pumpkin/Cargo.toml`:** in the existing `[features]` section, add
  `default = ["java-listener"]` and `java-listener = ["pumpkin-core/java-listener"]`.
  - The stock binary keeps Java.
  - `--no-default-features` builds it without Java, which is how step 7 tests it.
- **`crates/pumpkin-core/src/lib.rs:279`:** change
  `if server.advanced_config.networking.java.enabled {` to
  `if cfg!(feature = "java-listener") && server.advanced_config.networking.java.enabled {`, with a
  short seam comment. That gates the TCP bind, query and Java LAN broadcast together.

**Why `cfg!()` and not `#[cfg]`** (worth teaching):
- `#[cfg]` deletes code before type-checking.
- `cfg!()` is just a compile-time `true`/`false`. The Java accept code still type-checks, but
  `tcp_listener` is always `None`, so no socket can be bound, and the optimizer drops the dead
  branch.
- Deleting the `select!` branch itself would need restructuring, because `tokio::select!` doesn't
  accept `#[cfg]` on individual branches. That would roughly 10× the diff.
- **PLAN.md wording when ticking M1:** "Java listener is compiled out: no TCP socket can be bound
  without the `java-listener` feature."

**Commit (fork):** "Gate the Java listener behind a java-listener feature".

## Step 6: `tracing` in the client

`client/src/main.rs`:
- Install a `tracing_subscriber::fmt` subscriber with `EnvFilter`, using `RUST_LOG` and defaulting
  to `info`.
- Log one startup line with the crate version and Pumpkin's Bedrock target
  (`pumpkin_world::CURRENT_BEDROCK_MC_VERSION` and `CURRENT_BEDROCK_MC_PROTOCOL`). That proves the
  client compiles and links against the fork through the workspace.
- The client doesn't start the server yet; that's M2. The pitfalls for M2 are already listed
  under "Notes for M2" in `docs/STATUS.md`.

**Commit:** "Wire tracing into the client and link pumpkin-core".

## Step 7: Verify, then bump and tick

1. **Fork lints and tests** (in `pumpkin/`). Pumpkin denies clippy pedantic and nursery lints, so
   the new code must pass them:
   - `cargo clippy -p pumpkin-core --all-targets`
   - `cargo clippy -p pumpkin-core --all-targets --features java-listener`
   - `cargo test -p pumpkin-core --lib net::bedrock`
2. **Client build** (root):
   - Run `cargo build -p limonite-client`. The first build compiles pumpkin-core's dependencies
     into the root `target/`.
   - Then `cargo run -p limonite-client` should print the tracing line with `1.26.51 / 2193`.
3. **Java compiled out, and the join still works:**
   1. Build the fork's server with `--no-default-features` and the LTO env vars from STATUS.md.
      Add `--features <x>` only if the re-check found another default.
   2. Run it from `run/pumpkin/` with `[networking.java] enabled = true` set **on purpose**.
   3. Check that `Get-NetTCPConnection -LocalPort 25565 -State Listen` finds nothing and the log
      shows no Java listener.
   4. **The owner joins** from Bedrock 1.26.52 at `127.0.0.1:19132` and walks around.
   5. Afterwards, set `java.enabled = false` again and stop the server, confirming with
      `Get-Process pumpkin`.
4. **Diff check:** `git -C pumpkin diff aac0a82 --stat` should list only
   `net/bedrock/transport.rs`, `net/bedrock/mod.rs`, `lib.rs`, `crates/pumpkin-core/Cargo.toml`
   and `crates/pumpkin/Cargo.toml`.
5. **Bump and tick:**
   - The superproject commits the submodule bump: "Bump pumpkin to the Transport seam".
   - Tick M1 in `docs/PLAN.md` with the Java wording above and update `docs/STATUS.md`, then
     commit "Tick Milestone 1".
   - Every commit ends with the Co-Authored-By line.
6. **Ask the owner before pushing.** The `limonite` branch has to be on the fork for the
   submodule commit to be fetchable from other machines. Never push without a yes.

## Deviations (what actually happened)

1. **The `exclude` error was a different message with the same cause.** Without `exclude`, Cargo
   failed with "error inheriting `version` from workspace root manifest's
   `workspace.package.version` … was not defined", not "member of the wrong workspace". Cargo had
   made `pumpkin-core` a member of our workspace, so its `version.workspace = true` looked in our
   root manifest, which has no version.
2. **The impl uses `Self::send(self, ..)`, not `NetherNetSession::send(self, ..)`.** Pumpkin denies
   `clippy::use_self`. It's still the inherent method, because inherent methods take priority over
   trait methods. For the same reason, doc comments put `NetherNet` in backticks (`doc_markdown`).
3. **The four `pumpkin-wasm-host*` release overrides were dropped from the root** (`7ff616c`). No
   root crate builds them, and Cargo warned about each one on every build. A comment in the root
   `Cargo.toml` says when to restore them. The owner delegated this decision.
4. **The startup banner was left alone.** `crates/pumpkin/src/main.rs:194` prints the Java address
   whenever `networking.java.enabled` is set, even with the listener compiled out. The socket check
   showed no TCP 25565 listener; only TCP 19132 was bound. Fixing the banner would make a sixth fork
   file, so it's a STATUS.md gotcha instead. The owner delegated this decision.
5. **The join test ran twice.** The first run was stopped unjoined while the owner was away. The
   second run, with the same binary and config, joined (fork `80b19bb`, 2026-10-19 15:46).
   Sequence: NetherNet accept, `HAVE_ALL_PACKS`, `COMPLETED`, `<gamertag> joined the game`, render
   distance 8 → 16, `initialized (Runtime ID: 1)`.

Results: clippy was clean both ways, 34/34 `net::bedrock` tests passed, the `--no-default-features`
release build took 4 m 59 s, and the client printed
`Limonite Client 0.1.0 starting; embedded Pumpkin targets Bedrock 1.26.51 (protocol 2193)`.
