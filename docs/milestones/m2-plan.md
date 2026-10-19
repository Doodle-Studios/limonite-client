# Milestone 2 plan: Headless embedded client

**Status:** approved by the owner on 2026-10-20. **Not started;** Step 1 is next. The owner
asked for four changes at approval, all folded in below:
- the fixed dev `xid` until M5, with a STATUS.md note on player data;
- absolute paths before the chdir, and the base-dir upstream candidate;
- the capture hook reverted with `git checkout`;
- a movement check that never panics.

The plan below is kept as approved; "Deviations" at the end records where the real code differs.
- **Before starting:** read `docs/STATUS.md` (environment, gotchas, "Notes for M2") and
  `docs/recon/m0-pumpkin-bedrock.md` (§2 accept path, §5 compression, §6 login).
- **If the real code contradicts this plan,** stop and tell the owner (CLAUDE.md rule).
- **As you go:** teach. The owner is learning Rust, so explain the why behind each step,
  especially the ones marked "worth teaching". Update `docs/STATUS.md` as steps land.

## Context

M1 put a `Transport` trait between Pumpkin and NetherNet. M2 is that seam's first real use. The
client binary starts Pumpkin on a background thread and joins it as the host through an
in-memory `LocalChannel`, with no sockets. It logs every packet it receives, spawns, walks, and
checks that the server saw the move. It is headless: no window until M4. The session it scripts is
the skeleton of the M4 main loop.

**Accept when** (PLAN.md M2):
- spawn completes;
- chunks arrive;
- the server sees the player move;
- no socket is opened.

## Decisions (owner, 2026-10-20)

1. **Host login: unsigned tokens.**
   - **What:** the client sends a normal `SLogin`. The token is an unsecured JWT
     (`{"alg":"none"}`, empty signature) with `xname`, `xid` and a `cpk`, and the raw token
     carries the 10 required `ClientData` fields.
   - **Fork diff:** none for login. The embedded server runs `online_mode = false` while LAN is
     closed.
   - **Rejected options:** (b) a local login seam: more fork code, and it duplicates
     LoginSuccess, ResourcePacksInfo and ClientData. (c) self-signed tokens: see Finding 2.
   - **M7 consequence:** when LAN opens, a small seam lets the local transport be trusted, so
     guests can still be Xbox-verified. Something like `Transport::is_trusted_local()` plus one
     condition in `login/login.rs`.
2. **Codec scope: only what M2 uses.**
   - **Writes** (the 5 packets the client sends): `SLogin`, `SResourcePackClientResponse`,
     `SRequestChunkRadius`, `SSetLocalPlayerAsInitialized`, `SPlayerAuthInput`.
   - **Reads** (the 4 packets the client decodes): `CPlayStatus`, `CStartGame` (in full),
     `CChunkRadiusUpdated`, `CDisconnect`.
   - **Everything else** is logged by name only; its codec lands with the milestone that needs it.
3. **Golden fixtures from the official client** (owner request).
   - **What:** the client's writers are tested against bytes the real Bedrock 1.26.52 client
     sent, not only against Pumpkin's reader. Step 1 captures them.
   - **`SLogin` is the exception.** It holds the owner's Xbox token, XUID, gamertag, device and
     PlayFab ids and skin, so its capture never enters git. See Step 1.
4. **The codecs live in the fork, in `pumpkin-protocol` behind a `client` feature**, as PLAN.md
   "What the fork changes" says. The client crate can't hold them (worth teaching):
   - **Orphan rule.** `PacketRead`/`PacketWrite` and the packet structs are both defined in
     `pumpkin-protocol`. Rust forbids implementing a foreign trait for a foreign type, so the
     client crate could only write free functions. Then `BedrockBatchEncoder::write_packet` and
     the blanket `Vec<T>`/`Option<T>` codecs couldn't be used.
   - **Privacy.** Some nested types have private fields, e.g. `ServerJoinInformation`
     (`start_game.rs:85`). Only code in their own module can build them, so their readers must
     be derives on the struct itself.
   - **No drift.** A derive next to the struct picks up an upstream field change automatically.
     A copy in the client would silently go stale. It's also an upstream candidate.
   - **Gated:** the stock `pumpkin` binary compiles exactly as before. The protocol crate already
     declares unused `serverbound`/`clientbound` features; we don't repurpose them, in case
     upstream has plans for them.
   - **Client-crate code** is everything that isn't a codec: token building, the id-to-name table,
     the session state machine and the stats.

## Findings while planning (fork at `80b19bb`)

1. **The codecs are not "one derive per packet file"** (PLAN.md says they are).
   - 15 of 61 clientbound files and 9 of 29 serverbound files have hand-written codecs, which
     need hand-written mirrors.
   - Nested types need impls too.
   - `String::read` caps at 32,767 bytes and `Vec::read` at 65,536 items
     (`serial/deserializer.rs:152`, `:173`). That's fine for M2's packets; note it for later.
   - Good news: a Bedrock network NBT reader already exists (`NbtReadHelperBedrock`,
     `pumpkin-nbt/src/deserializer.rs:409`, used at `network_item.rs:644`). That makes a full
     `CStartGame` reader possible.
2. **Login has a third path the recon missed:** `AuthenticationType::SelfSigned`
   (`login/login.rs:42`) verifies the token against its own key.
   - It only works with `authentication.enabled = false`, which turns Xbox auth off for everyone.
     So it's no safer for LAN guests than unsigned tokens.
   - Add a line to recon §6.
3. **A socket at startup:** `Server::new` fetches Xbox OIDC keys over HTTPS whenever
   `bedrock.online_mode && bedrock.authentication.enabled` (`server/mod.rs:354`). M2's
   `online_mode = false` avoids it. M7 must fetch lazily when LAN opens.
4. **Pumpkin writes files relative to the process working directory:** `ops.json`,
   `banned-*.json`, `whitelist.json`, `usercache.json` and the world (`data/*.rs`,
   `basic.get_world_path()`).
5. **One server per process:** `SHOULD_STOP` / `STOP_INTERRUPT` are process globals that can't
   be reset. Note this for M5's world list.
6. **Liveness rules that bite a headless client:**
   - **10 s timeout:** the server drops a client after 10 s with no inbound packet
     (`net/bedrock/mod.rs:254`).
   - **Spawn needs chunks:** `PlayStatus::PlayerSpawn` is sent only after more than 4 chunks
     went out (`entity/player.rs:2881`).
   - **Movement is ignored until loaded:** `PlayerAuthInput` is dropped until the player is
     loaded (`play/player_auth_input.rs:13`).
   - **Positions are eye height:** `StartGame` and `PlayerAuthInput` positions are feet plus
     eye height.

## Step 1: Capture the official client's packets (temporary, never committed)

1. **Add a temporary capture hook** in the fork's working tree, in `BedrockClient::process_batch`
   (`net/bedrock/mod.rs`) just before `self.handle_game_packet(game_packet)`.
   - For ids 1, 8, 69, 113 and 144, it writes `game_packet.payload` to
     `captures/<id>-<seq>.bin` and logs one debug line per file. It keeps at most 200 files
     per id.
   - **Why files and not hex in the log:** the bytes stay exact, and there's no hex-parsing
     step with no Python available.
   - **Why at this point:** the payload has already been decompressed and split from its
     length/header, so it's exactly the packet body that `PacketRead`/`PacketWrite` handle.
     Compression and framing are separate layers (worth teaching).
2. **Build the capture server** with the STATUS.md release command at `80b19bb` plus the hook.
   Run it from `run/pumpkin/` (offline mode, Java off, telemetry off).
3. **The owner joins** from Bedrock 1.26.52 at `127.0.0.1:19132`:
   - stands still for a few seconds;
   - walks forward;
   - jumps once;
   - quits.

   No clicking, so `PlayerAuthInput` carries no item or block actions.
4. **Revert the hook** with
   `git -C pumpkin checkout -- crates/pumpkin-core/src/net/bedrock/mod.rs`. Confirm
   `git -C pumpkin status` is clean, then rebuild the stock server.
5. **Choose the fixtures.** Copy them into
   `pumpkin/crates/pumpkin-protocol/src/bedrock/client_codecs/fixtures/`:
   - `resource_pack_client_response-have_all_packs.bin` and `-completed.bin`;
   - `request_chunk_radius.bin`;
   - `set_local_player_as_initialized.bin`;
   - `player_auth_input-idle.bin`, `-walking.bin` and `-jumping.bin`.

   Add a short `README.md` recording the provenance: client version, protocol, fork commit,
   date, and "packet body after decompression, no length or header".
6. **`SLogin` stays out of git.** Its capture goes to `run/captures/slogin.bin`; `run/` is
   gitignored.
   - **Local-only test:** an `#[ignore]` test reads it from `LIMONITE_SLOGIN_FIXTURE` and checks
     that our writer reproduces it byte for byte.
   - **Committed test:** a constructed-bytes test in Pumpkin's existing style
     (`server/login.rs` tests).
   - **Recorded in this plan's results:** the non-identifying facts the capture shows. That
     means the `AuthenticationType`, which `ClientData` keys the real client sends, and the
     values of the 10 required keys that aren't identifying. Our token copies those values.

## Step 2: `accept_bedrock` helper (fork, behaviour-preserving)

`crates/pumpkin-core/src/lib.rs`:
- Move the NetherNet branch body (`lib.rs:668-698`) into
  `fn accept_bedrock(&self, transport: Arc<dyn Transport>, client_addr, master_client_id_counter, tasks, bedrock_clients)`,
  with a seam doc comment.
- PLAN.md's `accept_bedrock(transport, addr)` also needs the accept loop's shared state, which
  is the extra three parameters.
- The read loop now calls `transport.recv()` through `dyn`. That's one `BoxFuture` allocation
  per batch, negligible next to the batch.
- `process_nethernet_packet` keeps its name even though it's transport-agnostic (it just adds
  back `0xFE`). Renaming it would grow the diff.

**Commit (fork):** "Move the Bedrock accept body into accept_bedrock".

## Step 3: `LocalChannel` (fork)

**New file** `crates/pumpkin-core/src/net/bedrock/local.rs`, with a seam doc comment saying why it
exists:
- **`LocalChannel` (server end), `impl Transport`:**
  - **`send`:** pushes onto a **bounded** `tokio::sync::mpsc` (capacity 1024 batches).
    - **Why bounded** (worth teaching): if the client stalls, back-pressure stops Pumpkin's
      outgoing task. The packets then wait in Pumpkin's own queue, guarded by
      `MAX_PENDING_BYTES` (64 MB, `net/mod.rs:126`), exactly as for a slow NetherNet peer.
      An unbounded channel would hide a stall and grow without limit.
  - **`recv`:** `select!` between the inbound receiver and a `CancellationToken`.
    - **Why race the token:** `close()` must wake a pending `recv()`. Otherwise
      `PumpkinServer::start` hangs at shutdown in `tasks.wait()` while the read loop waits for
      bytes that never come.
  - **`flush_reliable`:** `Ok(())`. Nothing is in flight; a sent batch is already in the
    client's queue.
  - **`close` / `is_closed`:** cancel the token. `is_closed` is also true when the client end
    is gone.
  - **`client_public_key`:** `None`, so login accepts any well-formed `cpk` (recon §6).
- **`LocalClientEnd` (what the client holds):** a sender and a receiver.
  - The client uses `try_recv`/`try_send`, which work from a plain thread with no runtime. This
    is how the future winit main loop will use it.
- **`LocalListener` / `LocalConnector`:**
  - **`connect()`:** builds the pair, `try_send`s `(Arc<LocalChannel>, addr)` to the listener,
    and returns the client end.
  - **Never closes:** the listener keeps one connector itself, so `accept()` never returns
    `None`. Otherwise `select!` would spin on a closed channel.
  - **Address:** `addr` is `127.0.0.1:<connection number>`. It's only a key in
    `bedrock_clients`; nothing is bound.
- **Unit tests:**
  - both directions deliver in order;
  - `close()` wakes a pending `recv()` with `None`;
  - dropping the client end closes the channel;
  - `client_public_key()` is `None`.

**Edits to existing files:**
- `net/bedrock/mod.rs`: `pub mod local;`.
- `lib.rs`:
  - a `pub local_listener: LocalListener` field on `PumpkinServer`, built in `new`;
  - a `select!` branch for local connections that calls `accept_bedrock`.

**Commit (fork):** "Add LocalChannel for in-process Bedrock players".

## Step 4: Client codecs behind `client` (fork, `pumpkin-protocol`)

- **`Cargo.toml`:** add `client = []` with a comment saying why.
- **`bedrock/mod.rs`:** add `#[cfg(feature = "client")] mod client_codecs;`.
- **One-line derives,** `#[cfg_attr(feature = "client", derive(PacketRead))]` or `PacketWrite`,
  plus a `#[cfg(feature = "client")]` import of the trait, in 7 files:
  - `server/request_chunk_radius.rs` and `server/set_local_player_as_initialized.rs` (write);
  - `client/play_status.rs`, `client/chunk_radius_updated.rs`, `client/common.rs` (`GameType`)
    and `client/gamerules_changed.rs` (`GameRule`) (read);
  - `client/start_game.rs` (read): `CStartGame`, `LevelSettings`, `ServerJoinInformation`,
    `GatheringJoinInfo`, `StoreEntryPointInfo`, `PresenceInfo`, `ServerTelemetryData`,
    `ExperimentToggle`, `GamePublishSetting` and `BlockProperty`.
- **New module `bedrock/client_codecs/`** holds the hand-written mirrors. Each mirrors the
  existing hand-written opposite line by line:
  - **writers:** `SLogin`, `SResourcePackClientResponse` (its status-name strings come from the
    fixtures) and `SPlayerAuthInput`. The auth-input writer returns an error if the item, block
    or stack-request actions are `Some`; M5 adds those writers with block break/place;
  - **readers:** `CDisconnect`, `Experiments`, `RuleValue`, and `NbtCompound` through
    `NbtReadHelperBedrock`.
- **Tests:**
  - **Golden (sent packets):** Pumpkin's reader parses the fixture and consumes every byte. Our
    writer then re-encodes it to exactly the fixture's bytes.
    - If a field the reader drops makes byte equality impossible (for example, flag order in
      `PlayerAuthInput`), compare the decoded fields instead and record why in Deviations.
  - **Round trip (read packets):** Pumpkin's writer, then our reader, then Pumpkin's writer
    again gives identical bytes.
    - `CStartGame` is built the way `spawn_bedrock_player` builds it, plus one block property,
      experiment and gamerule, so the hand-written readers actually run.
- **Worth teaching:**
  - `cfg_attr` adds the derive only when the feature is on.
  - **Feature unification:** the client enables `client`, and since every crate in the
    client's build shares one copy of `pumpkin-protocol`, that copy has it on. The fork's own
    builds don't.

**Commit (fork):** "Add client-side Bedrock codecs behind a client feature".

## Step 5: Bump and client dependencies (superproject)

1. **Bump:** commit the submodule bump.
2. **Dependencies:** add workspace entries pinned to `pumpkin/Cargo.toml`'s versions, so the
   lockfile keeps one copy of each:
   - `tokio` (rt-multi-thread);
   - `rayon`;
   - `rustls` (ring);
   - `bytes`, `base64`, `serde_json`.
3. **`client/Cargo.toml`** gains:
   - `pumpkin-protocol` with `features = ["client"]`;
   - `pumpkin-config` and `pumpkin-auth` (for `p384` and the jwt helpers used in tests);
   - `pumpkin-util` if needed.

**Commit:** "Bump pumpkin to LocalChannel and client codecs".

## Step 6: Embedded server on a background thread (client)

New `client/src/embedded.rs`. `main.rs` does, in order:
1. **Logging:** set up `tracing` as now. Pumpkin's events reach this subscriber because there is
   one global dispatcher; Pumpkin's `init_logger` is never called.
2. **Process setup, mirroring `crates/pumpkin/src/main.rs`:**
   - install the `ring` rustls provider;
   - build the named Rayon global pool.
3. **Data directory:** `--data-dir <path>`, default `run/client`.
   - **Resolve first:** turn every client path (data dir, and any fixture or capture path) into an
     absolute path before changing directory, so nothing the client holds changes meaning
     afterwards.
   - **Then chdir:** create the directory and `std::env::set_current_dir` into it, because
     Pumpkin resolves its files from the working directory (Finding 4).
   - **Worth teaching:** the working directory belongs to the whole process, which is why every
     relative path has to be resolved before the change.
   - **Record in STATUS.md as an upstream PR candidate:** give Pumpkin a base-dir parameter so
     the chdir can go.
4. **Config in code:** start from `PumpkinConfig::default()`, then override:
   - `networking.bedrock.nethernet.enabled = false` (no UDP or TCP 19132, no LAN discovery);
   - `networking.bedrock.online_mode = false` (no OIDC fetch, unsigned login);
   - `networking.java.enabled = false` (compiled out anyway);
   - `telemetry.enabled = false`;
   - `plugins.enabled = false`;
   - `commands.use_console = false` and `commands.use_tty = false`.

   Management, query and chat reports are already off by default; leave them off.
5. **Server thread:** spawn a `Pumpkin-Main` thread running a multi-threaded tokio runtime. On it:
   1. `pumpkin_core::LOGGER_IMPL.set(None)`, which prevents the hangs at `lib.rs:461` and `:549`;
   2. `VanillaData::load()`;
   3. `PumpkinServer::new(..)`;
   4. send `(local_listener.connector(), Arc<Server>, runtime Handle)` back to `main` over a
      `std::sync::mpsc`;
   5. `start().await`.
6. **Shutdown:** `pumpkin_core::stop_server()`, then join the thread, which saves the world.
   - Worth teaching: `Handle::block_on` from a thread outside the runtime, and why the server
     can start only once per process (Finding 5).

**Commit:** "Start the embedded Pumpkin server on a background thread".

## Step 7: Host login and spawn (client)

**`client/src/login.rs`** builds the `SLogin`:
- **`jwt` field:** `{"AuthenticationType": <value the capture showed>, "Token": "<unsecured JWT>"}`.
- **Token payload:** `xname` (`--name`, default `Limonite`), a fixed development `xid`, and a
  `cpk`.
  - **The `cpk`** is the P-384 generator point as base64 uncompressed SEC1 (97 bytes, accepted by
    `build_public_key_from_b64`). Nothing ever verifies a signature against it, so any valid
    curve point works, and the generator makes it obviously a placeholder.
  - **Identity:** the fixed dev `xid` stays until M5 (owner-approved), when a per-install
    identity saved with the settings replaces it.
  - **Consequence:** the player UUID is derived from the `xid`, so dev-world player data
    (inventory, position) won't carry over after the M5 switch. Record that in STATUS.md.
- **Raw token:** `ClientData` JSON with the 10 required keys. The values for `DeviceOS`, the
  input modes, `UIProfile` and `GameVersion` are copied from the capture.
- **Unit tests** run the server's own parsers on our output:
  - `pumpkin_auth::jwt::extract_oidc_token_player_claims`;
  - `extract_cpk_from_token`;
  - `serde_json::from_slice::<ClientData>`.

**`client/src/packets.rs`:**
- **Name table:** id to name for every Pumpkin clientbound type, plus `CLevelChunk`. It's built
  with a small macro from each type's `PACKET_ID`, so the ids can't drift. Unknown ids log as
  `Unknown(n)`.
- **Framing:**
  - inbound: `BedrockBatchDecoder::get_game_packet`;
  - outbound: `BedrockBatchEncoder::write_packet` with the leading `0xFE` stripped. The server
    side adds it back.

**`client/src/session.rs`** runs a 20 Hz state machine on the main thread. Each tick it drains
`try_recv`, logs each packet name at `debug`, and reacts:
1. connect, then send `SLogin`;
   - with no `RequestNetworkSettings`, compression stays off (recon §5);
2. on `ResourcePacksInfo`, send `HAVE_ALL_PACKS`; on `ResourcePackStack`, send `COMPLETED`;
3. on `StartGame`:
   - record the runtime id and the eye-height position, and log the seed and spawn;
   - send `RequestChunkRadius(8)`;
   - start sending `PlayerAuthInput` every tick. It keeps the 10 s timeout from firing and is
     ignored until the player is loaded (Finding 6);
4. count `LevelChunk` packets;
5. on `PlayStatus::PlayerSpawn`, send `SetLocalPlayerAsInitialized(runtime id)`;
6. on `Disconnect`, log the reason.

The whole session has a 120 s deadline. On timeout it exits with code 1 and says which state it
was stuck in.

**Commit:** "Log the host in over LocalChannel and spawn".

## Step 8: Movement and the server-side check (client)

1. **Walk:** after spawn, walk +X at about 0.2 blocks per tick for 40 ticks. `position`, `delta`
   and `move_vec` move together; `y` stays constant. The server trusts the reported position.
   This milestone tests the transport, not physics.
2. **Check:** read the server's own player through the shared `Arc<Server>`:
   `get_all_players().first()`, then `.position()`.
   - **No player:** if the list is empty, report `FAIL` ("server has no player") and carry on to
     shutdown. The check never indexes or unwraps, so it never panics.
   - **Pass:** the player moved at least 4 blocks in +X from the `StartGame` feet position.
   - **Why this is a real check** (worth teaching): the value went through the whole pipeline,
     not an echo: `LocalChannel` → `process_batch` → the incoming mpsc →
     `progress_player_packets` → the `SegQueue` → the tick thread → `handle_player_auth_input` →
     `set_pos`.
3. **Linger:** `--linger <secs>` keeps the session alive, still sending input, so the external
   socket check has a window.
4. **Report:** log one `PASS`/`FAIL` line per acceptance check; the exit code is non-zero on any
   failure.

**Commit:** "Walk the host and check the move server-side".

## Step 9: Measure `LocalChannel`'s encode/decode cost (client, `--measure`)

Numbers come from builds with the STATUS.md release env vars. Debug numbers are meaningless.
1. **Per packet name:** count and total bytes received, plus client framing time (batch to
   `RawPacket`s) and decode time for the 4 decoded types. Logged as an `info` table at the end.
   - **Copies:** the outgoing task copies each packet once (`Bytes::copy_from_slice`,
     `net/bedrock/mod.rs:234`). Its cost follows from the byte totals; no fork instrumentation.
2. **Server-side chunk encode:** after spawn, through the shared `Arc<Server>` and runtime
   `Handle`, time `CLevelChunk::encode_chunk` plus the batch wrapper on the loaded spawn chunks.
   Do 10 passes and report µs and bytes per chunk.
3. **Not measured in M2:**
   - **chunk *decode*:** the decoder arrives in M3, which finishes this measurement;
   - **channel latency:** microseconds, dominated by the 50 ms tick.
4. **Decision rule** for PLAN.md's "special-case chunks with `Arc`":
   - Record the chunk share of bytes and of encode time, and the full-join encode cost against
     the 50 ms tick.
   - Recommend `Arc` only if chunks dominate and a join's encode is a noticeable fraction of a
     tick. The decision is recorded in PLAN.md either way.

**Commit:** "Measure LocalChannel encode and decode cost".

## Step 10: Verify, then tick

1. **Fork lints and tests** (in `pumpkin/`):
   - `cargo clippy -p pumpkin-core --all-targets`, plain and with `--features java-listener`;
   - `cargo clippy -p pumpkin-protocol --all-targets`, plain and with `--features client`;
   - `cargo test -p pumpkin-core --lib net::bedrock`;
   - `cargo test -p pumpkin-protocol --features client`;
   - the ignored `SLogin` golden test, run locally with `LIMONITE_SLOGIN_FIXTURE` set.
2. **Client tests:** `cargo test -p limonite-client`.
3. **Acceptance run:**
   `RUST_LOG=info,limonite_client=debug cargo run -p limonite-client -- --linger 30`.
   - The first run is a dev build, deliberately. If world generation makes `StartGame` or spawn
     slow, or trips the 10 s timeout, show that log first. Only then add
     `[profile.dev.package."*"] opt-level = 3` at the root (its own commit,
     "Optimise dependencies in dev builds").
   - Expect, in order:
     - the packet names;
     - `StartGame` with its seed and spawn;
     - more than 4 `LevelChunk` packets;
     - `PlayerSpawn`;
     - `PASS` for spawn, chunks and movement.
4. **No sockets:**
   - **During the linger:** `Get-NetTCPConnection -OwningProcess <pid>` and
     `Get-NetUDPEndpoint -OwningProcess <pid>` both return nothing.
   - **Startup log:** no "listening" lines and no "Failed to fetch" lines.
   - **Control:** the same two cmdlets against the stock `pumpkin.exe` list `19132`, which shows
     the probe works.
   - **Limitation:** this is a point-in-time check. The startup log and the config cover the rest.
5. **NetherNet regression** (Step 2 moved its code):
   - Rebuild the stock server at the M2 fork commit.
   - **The owner joins** from Bedrock 1.26.52 and walks.
   - Stop the server, and confirm with `Get-Process pumpkin`.
6. **Measure:** `cargo run --release -p limonite-client -- --measure`, with the STATUS.md env
   vars. Put the table in Deviations/Results.
7. **Diff check:** `git -C pumpkin diff aac0a82 --stat` should list:
   - M1's five files;
   - `net/bedrock/local.rs`;
   - `pumpkin-protocol`'s `Cargo.toml` and `bedrock/mod.rs`;
   - the 7 packet files;
   - `bedrock/client_codecs/` and its fixtures.
8. **Tick:**
   - **PLAN.md:**
     - tick M2;
     - apply the corrections below;
     - resolve the host-identity open question.
   - **STATUS.md:**
     - progress, and Notes for M3;
     - Notes for M5: one server per process; the per-install identity, and that dev-world player
       data won't carry over when it replaces the fixed dev `xid`; the permission level;
     - an upstream PR candidate: a Pumpkin base-dir parameter so the client's chdir can go.
   - **Recon §6:** a line on `SelfSigned`.
   - **This file:** Deviations.

   Commit as "Tick Milestone 2". Every commit ends with the Co-Authored-By line.
9. **Ask the owner before pushing** the fork or anything else.

**PLAN.md corrections to apply when ticking** (approved with this plan):
- **"What the fork changes", codecs:**
  - The codecs aren't one derive per file; hand-written mirrors live in a new module.
  - Each milestone adds the codecs it uses.
  - Sent packets are tested against official-client fixtures.
- **"What the fork changes", host login:** unsigned token over `LocalChannel`; the server stays
  offline while LAN is closed.
- **M7 entry:** add the trusted-local seam and a lazy OIDC fetch when LAN opens.
- **Client core:** "Pick [the ECS] in milestone 2" becomes M5. M2 stores no entities;
  interpolation in M5 is the first user.
- **Open questions:**
  - **Host identity:** resolved (unsigned token; a fixed dev identity now, a per-install one in
    M5).
  - **Permission level:** stays open, for M5.

## Owner involvement

- **Step 1:** one join with the official client (about 2 minutes: stand, walk, jump, quit).
- **Step 10.5:** one regression join.
- **Pushes:** ask before any push.

## Deviations (filled in during execution)
