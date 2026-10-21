# Milestone 3 plan: Chunk map

**Status:** approved by the owner on 2026-10-20. "Deviations" and "Results" are filled in as the
work goes. The owner asked for three changes at approval, all folded in below:
- the world copy for the official-client comparison is taken only after a clean exit (Step 11.4);
- the reverse id table is deterministic, and its collisions are counted, logged, and noted in
  STATUS.md (Step 3);
- M3 can be ticked without the encode speedup if it turns out to be more than a clean single-file
  change (Step 7).

The plan below is kept as approved; "Deviations" at the end records where the real code differs.
- **Before starting:** read `docs/STATUS.md` ("Notes for M3", Dev environment, Gotchas),
  `docs/recon/m0-pumpkin-bedrock.md` §8, and the Deviations in `m1-plan.md` and `m2-plan.md`.
- **If the real code contradicts this plan,** stop and tell the owner (CLAUDE.md rule).
- **As you go:** teach. The owner is learning Rust, so explain the why behind each step,
  especially the ones marked "worth teaching". Update `docs/STATUS.md` as steps land.

## Context

M2 left the host in the world with 329 `LevelChunk` packets it only counted. M3 decodes them into
client-side world state, which is the wall between networking and rendering that M4's renderer will
read, and dumps a top-down map of the loaded chunks. M2 also left the `Arc` question open (PLAN.md,
"Why bytes, not structs"). M3 measures what it needs: the decode half of the cost, plus the three
checks the owner asked for. Then the owner decides.

**Accept when:**
- **PLAN.md M3:** the map image matches what the official client sees at the same seed and position.
- **This plan adds:**
  - every `LevelChunk` decodes without error;
  - the decoded world equals the server's own `ChunkData` block for block, on both layers, and
    biome for biome;
  - the three `Arc` checks are done, and the owner's decision is recorded in PLAN.md.

## Decisions (owner, 2026-10-20)

1. **The decoder lives in the client crate, not in `pumpkin-protocol`'s `client` feature.** The
   `client_codecs` pattern solved three problems in M2. None of them applies here (worth teaching):
   - **No struct to mirror.** `CLevelChunk` lives in `pumpkin-core`, borrows `&ChunkData`, and has
     only a writer. A reader can't fill it. The decoder's output is a new type, the client's own
     world state.
   - **The orphan rule doesn't bite.** The decoder's types are ours, so they can implement any
     trait, foreign ones included. In M2 the packet structs were Pumpkin's, which is why their
     codecs had to live in Pumpkin.
   - **No privacy problem.** Nothing private in Pumpkin needs building.
   - **Drift is still caught.** The encoder isn't in `pumpkin-protocol` either, so a reader there
     wouldn't sit next to it. Instead, the client's tests round-trip through
     `CLevelChunk::encode_chunk` (public, and `pumpkin-core` is already a dependency), so they
     fail as soon as the encoder changes.
   - **Where world state belongs:** PLAN.md's layout puts it in `client/`, and the fork diff stays
     small.
   - **What does go in the fork:** a one-line `CUpdateBlock` reader (Decision 3) and the encode
     speedup (Decision 4).
2. **World state is Bedrock-native: sub-chunks are kept as they arrive.** Each storage keeps its
   Bedrock network-id palette and its packed `u32` words in Bedrock's XZY order.
   - **One format for every transport.** LAN guests (M7) and P2P (M11) only ever get bytes.
   - **Matches PLAN.md:** palette-compressed 16×16×16 sub-chunks.
   - **Decode is cheap:** parsing, validating and copying. No reordering.
   - **Consequence for `Arc`:** a host on an `Arc` path would still have to turn Pumpkin's palettes
     (Java state ids, YZX order) into this format. That's `convert_be_network`, the same work the
     encoder does. Step 6 measures exactly that, so the decision weighs it.
   - **Rejected:** storing Pumpkin's own `BlockPalette`s (Java ids). It would make `Arc` cheaper for
     the host, but every byte-transport client would then translate Bedrock to Java ids, and
     Bedrock resource packs key textures by Bedrock blocks.
3. **The client also applies `UpdateBlock`.** This keeps world state true after a chunk arrives,
   and makes the server comparison exact instead of "nearly equal".
   - **Fork cost:** one `cfg_attr` derive. `CUpdateBlock` is a plain `PacketWrite` derive
     (`bedrock/client/update_block.rs`), and `BlockPos` already has a reader
     (`serial/deserializer.rs:209`).
   - **Out of scope:** chunk unloading via `NetworkChunkPublisherUpdate`. It goes to STATUS.md as
     an M4 note.
4. **The encode speedup is a deliberate exception to the seams rule.** It's an edit to an existing
   Pumpkin file, `pumpkin-world/src/chunk/palette.rs`, meant for upstream.
   - It goes in its own fork commit that touches only that file, so it cherry-picks onto upstream
     `master` unchanged.
   - It must produce byte-identical output.
   - It's recorded as an upstream PR candidate. Opening the PR needs the owner's OK.
5. **The map uses colours already in the binary.**
   - **Block colours** are Pumpkin's generated Java map colours: `Block::map_color`
     (`pumpkin-data/src/blocks.rs:28`) through `MapColor::from_id` (`generated/map_color.rs:461`).
   - **No new Mojang data:** `pumpkin-data` is already linked in. It stays under the existing
     datapack risk in PLAN.md.
   - **The PNG writer** is `image` 0.25.10 with only `png`, exactly as `pumpkin-core` declares it
     (`pumpkin-core/Cargo.toml:112`). So nothing new is downloaded or compiled.
6. **The official-client comparison runs the stock fork server on a copy of the client's world.**
   - **Same seed and chunks:** it's the same `world/` directory.
   - **No socket in the client:** the embedded server keeps NetherNet off.
   - **A console:** the server runs in a Terminal tab the owner can type into, for
     `tp`/`gamemode`/`time`.

## Findings while planning (fork at `03b498c`)

1. **The `LevelChunk` body Pumpkin writes** (`net/bedrock/level_chunk.rs:68`), with the cache off:
   1. `VarInt` x, `VarInt` z, `VarInt` dimension;
   2. `VarUInt` sub-chunk count (24 in the overworld);
   3. `bool` "request limit present" (always `false`), then `bool` `cache_enabled`;
   4. `VarUInt` blob count (0), then `VarUInt` payload length and the payload. The payload holds:
      - **one entry per sub-chunk:** `u8` version 9, `u8` storage count (1, or 2 with a water
        layer), `i8` sub-chunk y, then the storages;
        - **a storage** starts with a `u8` header, `bits << 1 | 1`. The low bit 1 means network
          ids.
        - **`bits` > 0:** `ceil(4096 / (32 / bits))` little-endian `u32` words, then a `VarInt`
          palette length and `VarInt` ids.
        - **`bits` == 0:** one `VarInt` id.
        - **Allowed widths:** 1–6, 8 or 16. Widths 3, 5 and 6 leave unused bits in each word.
        - **Index order:** `x·256 + z·16 + y`.
      - **one biome storage per sub-chunk,** in the same format, bottom-up and aligned with the
        sub-chunks. The values are `Biome::be_network_id`.
      - **a `u8` border-block count** (0);
      - **block actors:** Bedrock network NBT compounds, back to back to the end.
2. **`cache_enabled`** is `chunk_caching && client_cache_supported` (`net/bedrock/mod.rs:373`).
   - **Our client:** it never sends `ClientCacheStatus`, so it always gets the inline form above.
   - **The official client** probably takes the blob path. Its sub-chunk and biome bytes are the
     same buffers, so the map comparison still tests the palette layout.
3. **Block ids are hashes.**
   - `BlockState::to_be_network_id` is a table lookup (`STATE_ID_TO_BEDROCK`,
     `generated/block.rs:23570`).
   - `StartGame` says so in `LevelSettings::blocknetwork_ids_are_hashed`.
   - The client builds the reverse table once from `pumpkin-data`.
4. **Check 1, hypothesis from reading the code (to be measured, not assumed).**
   `BlockPalette::convert_be_network` (`palette.rs:780`) does this for each of a section's 4,096
   blocks:
   - `data.get(x, z, y)` turns the stored index into a block state;
   - then a std `HashMap` (SipHash) lookup turns the state back into its index.
   - With `PaletteStorage::Indexed` (`palette.rs:52`), the most common heterogeneous case, the
     stored `u8` already **is** the index.
   - `convert_be_water_network` (`:672`) and the biome version (`:568`) do the same round trip.
   - At about 20 ns per lookup and about 10 heterogeneous sections per chunk, that's in the region
     of the measured 850 µs.
5. **Check 2, answered from the code; Step 8 confirms it with numbers.**
   - **In-place mutation.** `ChunkData` is mutated in place through `&self`.
     `World::write_block_state_if` (`world/mod.rs:5074`) gets the shared `Arc` from
     `level.read_chunk_sync` (`pumpkin-world/src/level.rs:900`) and writes under the section's
     `RwLock` (`chunk/mod.rs:504`).
   - **The packet comes later.** `on_block_state_set` puts the change into `unsent_block_changes`
     (`world/mod.rs:5161`). `flush_block_updates` (`:1719`) sends it only at the **end of the
     tick** (`server/mod.rs:1152`), and then it waits in the outgoing queue.
   - **So a live `Arc` sees a change up to a tick early.** Intermediate states in one tick are never
     sent at all: the queue is a `HashMap` keyed by position, and the last write wins.
   - **Lock contention:** a reader of a live `Arc` (say, a mesher) takes the same `RwLock` the tick
     thread writes through.
   - **Conclusion:** the host would need a snapshot taken at send time, which is what encoding gives
     today. `encode_chunk` holds that read lock for its whole run, about 850 µs.
6. **Chunk rate and radius (input for check 3).**
   - **Fixed rate for Bedrock:** 9 chunks per tick (180 per second). `INITIAL_CHUNKS_PER_TICK`
     (`net/chunk_sender.rs:23`) never changes, because only Java clients acknowledge batches.
   - **Radius cap:** requested radii are clamped to `networking.bedrock.view_distance`
     (`play/request_chunk_radius.rs:18`). Its default is 16 and its maximum 64. Radius 32 needs
     the embedded config raised.
   - **Movement:** Bedrock movement has no speed check (`play/player_auth_input.rs`), so a scripted
     flight works.

## Step 1: `CUpdateBlock` reader (fork, `pumpkin-protocol`)

- **The derive:** `client/update_block.rs` gets `#[cfg_attr(feature = "client", derive(PacketRead))]`
  and a `#[cfg(feature = "client")]` import, in the M2 pattern.
- **Test:** a round trip in `client_codecs/mod.rs` tests: Pumpkin's writer, then our reader, then
  the writer again. Use `CUpdateBlock::with_layer(pos, id, 1)` with a negative x and z.
- **Commit (fork):** "Read UpdateBlock on the client side".

## Step 2: Bump and client dependencies (superproject)

- **Bump:** commit the submodule bump.
- **`client/Cargo.toml` gains:**
  - path dependencies on `pumpkin-data` (block tables, map colours) and `pumpkin-nbt` (block actors);
  - `image` as a workspace dependency pinned to `pumpkin-core`'s spec (Decision 5).
- **Check:** `Cargo.lock` must only gain edges from `limonite-client`, not new packages.
- **Commit:** "Bump pumpkin to the UpdateBlock reader".

## Step 3: Decode chunks into world state (client)

**New module `client/src/world/`:**
- **`storage.rs`:** `Storage::Single(u32)` or `Storage::Packed { bits, words, palette }`.
  - `get(x, y, z)` computes `i = x<<8 | z<<4 | y`, then the word `i / per_word` and the shift
    `(i % per_word) * bits`, with `per_word = 32 / bits`.
  - **Palette indices are validated once, at decode** (worth teaching). Every index must be within
    the palette, so `get` can never fail. M4's mesher reads every block, so checking per read would
    cost far more.
- **`chunk.rs`:** `SubChunk` (1 or 2 layers) and `Chunk` (position, dimension, sub-chunks by y,
  biomes aligned with the sub-chunks, block actors as `NbtCompound`s).
  - It answers `block(x, y, z, layer)` and `biome(x, y, z)`. A missing sub-chunk reads as air.
- **`level_chunk.rs`:** the decoder: `decode(body: &[u8]) -> Result<Chunk, DecodeError>`, following
  Finding 1 byte for byte. Block actors are read with `NbtReadHelperBedrock` over a `Cursor`.
  - **Typed errors, never a panic:** truncated or trailing data, cache blobs, sub-chunk request
    modes, a version other than 9, persistent (low-bit 0) storages, unsupported widths, and
    out-of-range indices.
- **`mod.rs`:** `World`, a map from chunk position to `Chunk`, plus the id table (Finding 3).
  - **The id table maps Bedrock id to Java state, and it's deterministic** (owner, at approval).
    Several Java states can share one Bedrock id: for example, Java-only properties collapse, and
    `STATE_ID_TO_BEDROCK` repeats values. The table is built by walking the Java state ids in
    ascending order, and the **lowest id wins**. It counts the collisions, and logs the count at
    `info` and a few examples at `debug`.
  - **It isn't a true inverse.** It's good enough for map colours and for naming blocks in logs.
    STATUS.md says so (Step 11.6).
  - **Test:** building the table twice gives the same result. For a known shared Bedrock id, the
    lowest Java state wins, and the collision count is above zero.
  - `World::apply(&RawPacket)` takes `LevelChunk` and `UpdateBlock`, in the main-loop shape PLAN.md
    sketches.
  - `UpdateBlock` sets the block on its layer. For a chunk that isn't loaded, it's counted and
    ignored.

**`session.rs`:**
- **World state:** the session owns a `World`, and `CLevelChunk::PACKET_ID` goes to it in place of
  the counter. Decode time goes into `Stats` under that id, so the M2 table gains its missing
  "decode µs/pkt" for chunks.
- **Errors:** they're counted and logged at `warn`.
- **On `StartGame`:** record `blocknetwork_ids_are_hashed`.
- **Worth teaching:** chunks must be waited for, not assumed from `PlayerSpawn`. STATUS.md's
  "spawn and chunks race" note covers why.

**Tests:**
1. **Storage:**
   - hand-built words for every width (0, 1, 2, 3, 4, 5, 6, 8, 16), including the padded ones;
   - XZY order;
   - errors for a persistent header, width 7, an index outside the palette, and truncation.
2. **Round trip through Pumpkin's encoder.** Build a `ChunkData::empty(..)`, encode it with
   `CLevelChunk::encode_chunk(&chunk, 0, false, &actors)`, and decode it with ours. It holds:
   - an all-air section;
   - a 2-state section;
   - a 65-state section (8 bits);
   - a section with more than 256 states (16 bits, Pumpkin's `Dense` storage);
   - seagrass and a waterlogged block (the water layer);
   - mixed biomes (`set_relative_biome`);
   - a chest block actor.

   Then, at every position:
   - layer 0 equals `to_be_network_id(state)`;
   - layer 1 (or air, if absent) equals `to_be_network_id(bedrock_water_state(state))`;
   - the biome matches.

   The block actor is compared as decoded NBT, not bytes, because of the NBT order gotcha.
3. **Never panics:**
   - the same chunk encoded with `cache_enabled = true` returns the cache error;
   - every prefix of a small encoded chunk returns an error.

**Commit:** "Decode LevelChunk and UpdateBlock into world state".

## Step 4: Check the decoded world against the server (client)

- **Wait for chunks first.** `main.rs` now always waits for chunks to go quiet before the checks,
  not only with `--measure`. That reuses M2's `wait_for_chunks`.
- **New check "chunks decoded":** every `LevelChunk` decoded, and ids are hashed.
- **New check "world matches server":** for each decoded chunk the server still has loaded, it
  compares every position, through independent code on each side:
  - our layer 0 against `to_be_network_id` of
    `chunk.section.get_block_absolute_y(x, y, z)`;
  - our layer 1 against `to_be_network_id(bedrock_water_state(..))`;
  - our biome against `get_rough_biome_absolute_y` mapped to `be_network_id`.
- **Polling, as M2's movement check does:** it retries each tick for up to 2 s, so an
  `UpdateBlock` still in flight can't fail it. It passes on the first full match.
- **On a FAIL:** it logs up to 10 mismatches as block names, and its own duration. It never panics.
- **Why both sides use independent code** (worth teaching): our index math against Pumpkin's
  `get`. A shared mistake would hide.
- **Commit:** "Check the decoded world against the server".

## Step 5: The map (client)

- **Flag:** `--map <path>`, resolved to an absolute path before the chdir (M2 rule).
- **When:** written after the checks.
- **Pixels:** one per block column. North (−Z) is up and east (+X) is right, as on in-game maps.
- **The top block:** scan down from the highest sub-chunk.
  - Water on layer 1 wins and is drawn as water, darker with depth.
  - Otherwise, it's the first layer-0 block whose map colour isn't `NONE`.
- **Shading, Java-map style:** brighter if the column is higher than its north neighbour, darker if
  lower. Relief and coastlines then read clearly in the comparison.
- **Markers:**
  - unloaded chunks are transparent;
  - the host's position is a red marker;
  - unknown ids are magenta and counted.
- **Log line:** `Map: <n> chunks, <w>×<h> px, <k> unknown ids -> <path>`.
- **Unit tests** on a small hand-built `World`:
  - orientation: a block at +X lands right, one at −Z lands at the top;
  - water-layer precedence;
  - transparency for unloaded chunks;
  - the three shading levels.
- **Commit:** "Dump a top-down map of the loaded chunks".

## Step 6: Measurement (client, `--measure`; release numbers only)

1. **New flags:**
   - **`--radius <n>`** (default 8). The embedded config sets `networking.bedrock.view_distance = 32`
     so 16 and 32 aren't clamped (Finding 6), with a comment saying why.
   - **`--stream <blocks>`:** after the join settles, fly east at 0.55 blocks per tick (creative
     flying, 10.9 m/s).
     - It logs `LevelChunk` arrivals per second while moving.
     - It notes whether 9 per tick, the cap, was reached.
2. **Additions to `measure.rs`**, on the server's loaded chunks with 10 passes, as in M2:
   - **E:** `encode_chunk` plus the wrapper. That's M2's number, kept.
   - **C, the conversions,** timed one by one by calling Pumpkin's public functions per section:
     block `convert_be_network`, `convert_be_water_network` and biome `convert_be_network`.
     "Rest" is E − C, which is serialisation and the block actors.
     - It also logs a histogram of bits per entry for heterogeneous sections.
     - If C doesn't cover most of E, profile further with a sampling profiler on the root
       `profiling` profile (installing one needs the owner's OK).
   - **D:** our decode of each encoded chunk, plus the session's average for comparison.
   - **S:** a snapshot clone of `block_sections` and `biome_sections` under their read locks.
   - **The `Arc` arithmetic:** saving per chunk = (E + D) − (S + C), and the totals for this join.
     - **Worth teaching:** the `Arc` path still needs C, to reach our format (Decision 2), and S,
       to be correct (Finding 5).
3. **The check-2 probe:**
   1. On the main thread, set stone at y 318 above the host with
      `world.set_block_state(.., NOTIFY_ALL)`.
   2. Read it straight back through `loaded_chunks`.
   3. Tick until our `World` applies the matching `UpdateBlock`, then log both times.
   4. Restore air the same way, even on a timeout.

   - **If the call panics for lack of a runtime:** show the panic first. Then re-add the runtime
     `Handle` that M2 removed (Deviation 9) and call it under `handle.enter()`, as Pumpkin does
     around player ticks (`server/mod.rs:1175`).
4. **Commit:** "Measure chunk decode, encode phases and streaming".

## Step 7: Check 1: profile `encode_chunk`, then speed it up (fork)

1. **Template world.** One release run at `--radius 32` into a fresh `run/m3-template` generates
   the area. Every measured run below uses a fresh copy of it (`run/m3-<label>`).
   - **Why** (worth teaching): the same world and the same start position, so each comparison
     changes one variable.
2. **Baseline.** Run `--measure --radius 8` on a copy. Show the owner the phase table before
   touching code.
3. **Check upstream first.** Run `git -C pumpkin fetch upstream`. If upstream `master` already
   changed `convert_be_network`, stop and tell the owner.
4. **If C dominates, change `palette.rs`.** All output must be byte-identical:
   - **`Indexed` storage:** use the stored `u8` index directly. No `HashMap`, no palette lookup.
   - **`Dense` storage** (more than 256 entries, rare): build the key-to-index map once with
     `FxHashMap`.
   - **The water layer:** a per-palette-entry water flag, then the index.
   - **Biomes:** the same changes.
   - **Tests:** the current implementation moves into `#[cfg(test)]` as the oracle (worth
     teaching). The new code must equal it on: homogeneous sections; palettes of 2, 16, 65, 256 and
     257+ entries; water mixes; and biomes. Pumpkin's existing tests keep passing.
   - **If the table points elsewhere,** stop, show it, and propose a different change first.
   - **If the speedup turns out to be more than a clean single-file change** (owner, at approval),
     for example if it needs edits outside `palette.rs`: M3 can still be ticked without it.
     - The speedup becomes its own follow-up. Record it in STATUS.md, with the baseline table and
       what made it bigger.
     - Steps 9 and 10 then use the baseline E, and the decision says the speedup is pending.
     - The Step 11.5 diff then has no `palette.rs`.
5. **After.** Run the same command on a fresh copy and record before and after. Decode is
   unaffected, because the bytes are identical.
6. **Fork checks, in `pumpkin/`:**
   - `cargo clippy -p pumpkin-world --all-targets`;
   - `cargo test -p pumpkin-world --lib chunk`;
   - `cargo test -p pumpkin-core --lib net::bedrock`.
7. **Commits:**
   - fork: "Speed up Bedrock palette conversion";
   - superproject: "Bump pumpkin to the faster palette conversion".

   Add it to STATUS.md's upstream candidates.

## Step 8: Check 2: `ChunkData` mutation

Record Finding 5 with the probe's numbers from a release run: the change visible in the `Arc` at
about 0 ms, and `UpdateBlock` arriving X ms later. Also record S per chunk. The conclusion feeds the
decision: the host needs a snapshot, not a live `Arc`.

## Step 9: Check 3: join and streaming cost at radius 8, 16 and 32

- **Three release runs,** each on a fresh template copy: `--measure --radius R --stream 220`. Only R
  changes.
- **The table records, per radius:**
  - chunks per join;
  - the join totals of E (before and after Step 7), D, S and C;
  - the measured chunks per second while flying.
- **Streaming estimate:** chunks per second ≈ d·v/16, capped at 180.
  - **d** is the effective view diameter in chunks, taken from the join count (d = 2√(N/π)).
  - **v** is the speed in blocks per second.
  - The measured flight checks the formula. The formula then extrapolates to walking (4.3 m/s) and
    sprinting (5.6 m/s).
  - **Worth teaching:** moving one chunk exposes one new row of about d chunks.
- **Caveat:** flying past the template's edge means fresh worldgen, which may hold the rate below
  the formula. Note which limit applied.

## Step 10: The `Arc` decision (owner)

Present the Steps 7–9 table: per chunk E, C, D, S, and the saving (E + D) − (S + C), per join and per
second of flight at each radius.
- **Proposed rule:** adopt the `Arc` path only if, at radius 32, it saves at least one tick of CPU
  (50 ms) per join or 5% of a core while flying.
- **What it would cost:**
  - a new seam in `send_chunks`;
  - a second message type on `LocalChannel`, which carries `Bytes` today;
  - a second chunk path in the client.

  LAN guests need the encoder anyway, so a faster encoder helps every transport.
- **Expected outcome, a hypothesis only:** the speedup and the snapshot requirement leave too little
  to save, so bytes stay.
- **Either way:**
  - record the decision and the numbers in PLAN.md;
  - if `Arc` is adopted, it becomes the first item of M4's planning (with snapshots), not M3 work.

## Step 11: Verify, accept, tick

1. **Fork:**
   - `cargo clippy -p pumpkin-protocol --all-targets`, plain and with `--features client`;
   - `cargo clippy -p pumpkin-core --all-targets --features pumpkin-protocol/client`;
   - `cargo clippy -p pumpkin-world --all-targets`;
   - `cargo test -p pumpkin-protocol --features client`;
   - `cargo test -p pumpkin-core --lib net::bedrock`;
   - `cargo test -p pumpkin-world --lib chunk`.
2. **Client:** `cargo test -p limonite-client` and `cargo clippy -p limonite-client --all-targets`.
3. **Acceptance run (dev build):**
   `RUST_LOG=info,limonite_client=debug cargo run -p limonite-client -- --map <abs>/run/m3/map.png`.
   Expect `PASS` for spawn, movement, chunks, chunks decoded and world matches server, then the map
   line.
4. **Official-client comparison** (owner, about 10 minutes):
   1. **Set up:** copy `run/client/world` and `run/pumpkin/pumpkin.toml` into `run/m3-official/`.
      Build the stock server at the M3 fork commit (STATUS.md release command), and run it there in
      a Terminal tab.
      - **Copy only after a clean exit** (owner, at approval). The acceptance run must have ended on
        its own, with its normal save: exit code 0, and `Completed save!` then
        `Embedded server stopped` in its log. Also confirm `Get-Process limonite-client` returns
        nothing.
      - Never copy from a running, killed or failed session. A half-saved world could differ from
        what the map shows. If the run didn't exit cleanly, rerun it first.
   2. **Join:** the owner joins from Bedrock 1.26.52 at `127.0.0.1:19132`.
   3. **Console:** `gamemode creative <gamertag>`, `time set noon`, `weather clear`, then
      `tp <gamertag> <host x> 200 <host z> 180 90`. That faces north, looking straight down, so
      screen-up is the map's up. The client logs the host's position.
   4. **Screenshot:** Win+Alt+PrtScn, saved into `run/m3-official/`. It's gitignored, because it
      shows Mojang textures.
   5. **Compare** side by side with the map: coastlines, rivers, tree clusters and biome edges in
      the same places, with no mirroring or rotation. If coordinates can be shown, also spot-check
      2–3 landmarks.
   6. **Stop the server.** `Get-Process pumpkin` must return nothing.

   This join is also the NetherNet regression, and the first time the sped-up encoder meets the
   official client.
5. **Diff check:** `git -C pumpkin diff 03b498c --stat` lists only `update_block.rs`,
   `client_codecs/mod.rs` and `palette.rs`.
6. **Tick**, committed as "Tick Milestone 3":
   - **PLAN.md:**
     - tick M3;
     - record the `Arc` decision and the three checks under "Why bytes, not structs";
     - Client core's world-state lines: Bedrock-native storage, and the `Arc` line per the
       decision;
     - correct "bits per block = ceil(log2)": Bedrock uses 1–6, 8 or 16.
   - **STATUS.md:**
     - progress and the upstream candidate;
     - Notes for M4: chunk unloading isn't done; the world-state API; map colours are Java's;
       textures will need Bedrock block names; the Bedrock-to-Java id table isn't a true inverse
       (lowest Java state wins), with its collision count;
     - streaming numbers.
   - **This file:** Deviations and Results.

   Every commit ends with the Co-Authored-By line.
7. **Ask the owner** before pushing anything, or opening an upstream PR.

## Owner involvement

- **Step 7.2:** look at the baseline phase table before the fork change.
- **Step 10:** the `Arc` decision.
- **Step 11.4:** one official-client join (about 10 minutes).
- **Pushes and upstream PRs:** ask first.

## Verification (summary)

- **Unit and round-trip tests:** Steps 1, 3, 5 and 7.
- **Automated end-to-end checks:** Step 4, the decoded world equal to the server's, block for block.
- **The real thing:** Step 11.4, the official client at the same seed and position.
- **Numbers:** Steps 6–9 in release builds, each run changing one variable.

## Deviations (filled in during execution)

1. **"world matches server" re-checks chunk by chunk, not the whole world each tick.** The first
   dev run failed with 2 differences in 329 chunks, after a single full pass of 6.24 s. That
   pass is longer than the 2 s window, so no second pass ever ran. Both differences were blocks
   changing while the pass ran: `grass_block` turned to `dirt`, and redstone ore went from unlit
   to lit (both states are named `redstone_ore`). So the check now does one full pass, then
   re-compares only the chunks that differed, each tick for 2 s. Each chunk must still equal the
   server's exactly at one moment. Difference lines print the ids as well as the names (`72ea959`).
2. **The server changes some blocks without telling the client** (owner's decision,
   2026-10-21). A dev run in a fresh world failed: we had `fire` at (-257, -53, -428), the server
   had `air`, and no `UpdateBlock` arrived in 2 s. Pumpkin's `fire.rs` sets blocks with
   `BlockFlags::NOTIFY_NEIGHBORS` only (fire that can't survive, rain, spreading), and
   `on_block_state_set` queues client updates only with `NOTIFY_LISTENERS`. The plan assumed every
   change sends an update.
   - **The fix:** a chunk that still differs after the re-checks gets a fresh encode of the
     server's current copy, decoded with ours, for up to 5 ticks. If that equals the server, the
     decoder is right: the positions are logged as "changed by the server without an
     `UpdateBlock`" (WARN), and the check passes. Otherwise it fails (`5402089`).
   - The fire bug is an upstream candidate in STATUS.md.
3. **The decoder has a few more typed errors than the plan lists:** `BadVarInt`, `LayerCount` (not
   1 or 2), `SubChunkOrder` (sub-chunks must be consecutive), and `PaletteLength` (zero, or longer
   than the width can address, which also stops a corrupt length from allocating gigabytes).
   Border-block bytes are skipped, not rejected; Pumpkin sends none.
4. **The session gained `drain()`, and `tick()` is public.** The measurements run for longer than
   Pumpkin's 10 s client timeout, so they tick the session between passes. The probe polls
   `drain()` every half millisecond, which times the `UpdateBlock` more finely than a tick.
5. **The probe needed no runtime `Handle`.** `World::set_block_state` ran on the main thread
   without a panic, so M2's Deviation 9 stands.
6. **`--stream` flies with the eyes at y 330, above any terrain.** The server takes the reported
   position as it is, with no collision check, but a flight at walking height would pass through
   hillsides. The movement flags are the walk's.
7. **The chunk wait allows 180 s, not 60.** A radius-32 join into a fresh world is about 3,700
   chunks of worldgen. The wait still ends after 2 s without a new chunk; the template's took
   21 s.
8. **Map details:** the host's red cross has a white outline, because lava is Java's "fire"
   colour, pure red too. "Unknown ids" counts distinct ids. "chunks decoded" also reports the
   block actors decoded and the `UpdateBlock`s ignored for unloaded chunks.
9. **One behaviour change in the speedup, on a path that used to panic.** In `Dense` storage, a
   stored value missing from the palette made the old `convert_be_water_network` panic
   (`key_to_index[&value]`). The new code maps it to index 0, as the block conversion always did.
   Every other output is byte-identical, as the oracle tests show. Such a value only comes from a
   corrupt disk palette.
10. **The host died in the first streaming runs, so they were rerun** (`cdfb2bd`). Radius 16 and
    32 streamed 0 chunks during the flight, while radius 8 streamed 247.
    - **Evidence:**
      - the radius-32 packet table had an `SRespawn`, which Pumpkin sends when the player dies;
      - a radius-16 debug run showed a hurt event plus a health update every 0.5 s from the
        start, which is vanilla's in-wall damage rate;
      - the same radius-16 run without `--measure` streamed 455 chunks.
    - **Cause:** M2's scripted walk has no physics and keeps one height. The template run ended
      with the host's head in a hillside, so every copy starts there, and the host dies in about
      10 s. Radius 32 died during its 21 s join, and radius 16 during the measurement. Pumpkin
      ignores a dead player's input.
    - **Fix:** with `--measure` or `--stream`, the host rises to eye height 330 right after the
      movement check. It's a vertical move, so the chunk centre and the chunks sent don't change.
      The stream log now reports how far the server's player moved.
    - **Comparisons stay one-variable:**
      - before against after is `m3-baseline` against `m3-after-r8`, both unlifted, and only the
        fork differs;
      - the radius series is three lifted runs, and only R differs.
11. **A radius-32 join is 3,725 chunks, not π·32² ≈ 3,217.** So the streaming estimate takes the
    view diameter from the measured join count, as planned (d = 2√(N/π)), not from R.
12. **The speedup stops at one commit.** C is still about 89% of the new E. The packer divides
    by a runtime word size for each position, and `network_indices` builds a buffer per section.
    Tightening both may cut C further (not measured), but it would be a second `palette.rs` commit, and
    it wouldn't change the `Arc` decision, since both paths pay C. It's a follow-up in STATUS.md.
13. **Chunks the host leaves behind go stale, as the plan expected.** After the radius-8 flight,
    the world check passed with one WARN: at (10, 85, -36) we had `dirt`, the server
    `grass_block`. Grass spreads with `NOTIFY_ALL`, so the server did send that change, but only
    to players whose view holds the chunk. The host was 14 chunks away by then, and our client
    doesn't unload chunks yet (out of scope, Decision 3). The server kept that chunk loaded near
    spawn, so the check compared it, and the fresh encode correctly put the difference down to
    drift.

## Results

All numbers are from release builds (`CARGO_PROFILE_RELEASE_LTO=false
CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16`), 2026-10-21. Each run used a fresh copy of one template
world, `run/m3-template`: seed 1794192434310962158, generated by a radius-32 run (3,725 chunks in
21 s). Logs are in `run/m3-logs/`, which is gitignored.

**Step 7: encode phases, before and after the speedup** (`--measure --radius 8`; per chunk,
averaged over the server's 961 loaded chunks, 10 passes per phase; the join was 348 chunks):

| Phase | Before (fork `dc3a0ac`) | After (fork `e251ffb`) | Speedup |
| --- | ---: | ---: | ---: |
| **E**: `encode_chunk` + batch wrapper | 696.0 µs | 145.2 µs | 4.8× |
| **C**: palette conversions | 679.9 µs (98% of E) | 128.6 µs (89%) | 5.3× |
| – blocks | 392.9 | 74.2 | 5.3× |
| – water layer | 24.3 | 6.6 | 3.7× |
| – biomes | 262.6 | 47.7 | 5.5× |
| **E − C**: writing words and palettes, block actors | 16.1 µs | 16.6 µs | |
| **D**: our decode | 21.2 µs | 21.4 µs | |
| **S**: snapshot of the sections | 5.9 µs | 7.8 µs | |
| `Arc` saving (E + D) − (S + C) | 31.4 µs | 30.3 µs | |

- **Sections:** 13,810 homogeneous. Heterogeneous by width: 1 bit 62, 2 bits 194, 3 bits 3,609,
  4 bits 4,913, 5 bits 389, 6 bits 87. 516 have a water layer. No section needed 8 or 16 bits, so
  Pumpkin's `Dense` block storage never appeared.
- **Byte-identical output:** `CLevelChunk::encode_chunk` output is unchanged, per the oracle
  tests. A mutation that swapped the packer's z and y loops failed all four oracle tests, plus
  Pumpkin's own water-order test.

**Step 8: `ChunkData` mutation.** The probe ran in every measured run. `set_block_state` returned
after 173–197 µs, and the shared `ChunkData` already held the stone when read straight after.
Our world applied the matching `UpdateBlock` 14.5–60.7 ms after the call (0.3–1.2 ticks),
depending on where in the server tick the call landed. S, the snapshot an `Arc` path would need,
costs 6–10 µs per chunk. Finding 5 holds: a live `Arc` sees changes up to a tick before their
packets, so the host would need a snapshot.

**Step 9: join and streaming cost** (`--measure --radius R --stream 220`; the host rises to eye
height 330 first, Deviation 10; per-chunk costs after the speedup):

| R | Chunks per join | E before (696.0 µs each) | E after | D | S | C | `Arc` saving per join |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 8 | 348 | 242 ms | 51.5 ms | 7.4 ms | 2.7 ms | 45.0 ms | 11.2 ms |
| 16 | 1,057 | 736 ms | 157.1 ms | 23.7 ms | 8.7 ms | 141.2 ms | 31.0 ms |
| 32 | 3,725 | 2,593 ms | 568.0 ms | 82.4 ms | 37.9 ms | 486.4 ms | 126.1 ms |

| R | d = 2√(N/π) | Estimate d·v/16 | Measured while flying (11.0 m/s) | Peak second | `Arc` saving per second of flight | E after per second |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 8 | 21.0 | 14.4/s | 12.4/s | 19 | 0.40 ms (0.04% of a core) | 1.8 ms |
| 16 | 36.7 | 25.2/s | 22.8/s | 35 | 0.67 ms (0.07%) | 3.4 ms |
| 32 | 68.9 | 47.4/s | 43.6/s | 67 | 1.47 ms (0.15%) | 6.6 ms |

- **Chunks come in bursts:** a whole new row arrives within about a second of each chunk crossing.
  For example, radius 8 got `[0, 19, 19, 0, 19, 19, ...]` per second.
- **The formula's accuracy:** it overestimates by 8–15%, because the row that enters is a little
  shorter than the full diameter.
- **What limited the rate:** movement, not the cap. The peak second reached 67 of 180.
- **New worldgen:** the radius-32 flight needed chunks beyond the template, and generation kept
  up; the measured rate was 92% of the estimate.
- **Walking and sprinting**, extrapolated with d·v/16:
  - radius 8: 5.6 and 7.4 chunks/s;
  - radius 16: 9.9 and 12.8;
  - radius 32: 18.5 and 24.1.
