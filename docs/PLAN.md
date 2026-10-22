# Limonite Client — Design Plan

Oct 19, 2026 · @Krivi Ganta

## Overview

Limonite Client is a Minecraft Bedrock Edition client written in Rust, shipped as one binary that embeds a forked Pumpkin server for singleplayer. The client renders, predicts and presents; the embedded server owns world generation, physics, mob AI and saving. Nothing that Pumpkin already does gets rebuilt.

**Goals**

- Singleplayer with zero network overhead: the client and embedded server talk through an in-memory channel, not sockets.
- Open to LAN from inside a running world, without the host losing performance.
- A transport abstraction that can later carry peer-to-peer joining, Mojang-style (NetherNet, Bedrock's WebRTC transport).
- Drag-and-drop Iris shader packs (made for Java Edition) and Bedrock resource packs.
- Squeeze the GPU: GPU-driven rendering, hardware ray tracing where the hardware has it, honest performance warnings where it doesn't.

**Non-goals (for now)**

- Rewriting world generation, physics or AI.
- Java Edition networking: the Java listener is stripped from the fork.
- Marketplace content: encrypted packs are not supported.
- Full JSON UI and Ore UI parity on day one.

**Guiding principles**

1. Seams over spaghetti: every cross-cutting change to Pumpkin goes through a small, named seam so upstream rebases stay cheap.
2. Detect capabilities, never brands: a GPU feature flag decides a code path, a vendor name never does.
3. Measure, don't guess: performance tiers come from timed frames, not GPU names.
4. Shared cores first: the layered pack filesystem, the Molang interpreter and the Java to Bedrock mapping table feed most features, so they get built well and early.
5. Log everything from day one with `tracing`, because bugs here can live in the client, the fork, or a pack.

**Note for the implementing agent.** The owner is learning Rust and wants every step taught, not just done: explain the why behind each decision and each non-obvious piece of code as you go. Keep the fork diff small and document each seam you add. When a decision in this plan turns out wrong against the real code, say so and propose the change rather than forcing the plan.

## Architecture

One process, two threads, one seam. The main thread runs the window, renderer and UI; a background thread runs the forked Pumpkin server. They meet only at the `Transport` trait, so the server doesn't know or care whether a player is the host in the same process, a LAN guest, or a future P2P peer.

&#91;embedded content: architecture · client, embedded server, transports\]

The host's packets never leave memory. NetherNet only listens while LAN is open, and P2P (dashed) is a later transport behind the same trait. (The embedded diagram above predates M0 and still says RakNet; read it as NetherNet.)

## Server backend: embedded Pumpkin fork

The backend is a source-level fork of [Pumpkin](https://github.com/Pumpkin-MC/Pumpkin), linked as a crate and run inside the client process. Pumpkin's Bedrock support is still work in progress upstream, so expect gaps and plan to upstream fixes where possible.

**Upstream speaks NetherNet, not RakNet** (found in M0; details in `docs/recon/m0-pumpkin-bedrock.md`).
- NetherNet is Mojang's WebRTC transport: HTTP signalling on TCP 19132, then ICE and server-list status sharing UDP 19132, then DTLS-encrypted data channels. LAN discovery runs on UDP 7551.
- Legacy RakNet clients are rejected.
- Pumpkin accepts exactly one Bedrock protocol: 1.26.51 / 2193 at fork commit `aac0a82`.

**What the fork changes**

- Adds a `Transport` trait: the one seam where a player's packets enter and leave the server.
  - It replaces the `BedrockClient::session: Arc<NetherNetSession>` field, keeping its six method names (`send`, `recv`, `flush_reliable`, `close`, `is_closed`, `client_public_key`) so call sites don't change.
  - The NetherNet branch of the accept loop is an `accept_bedrock` helper (M2) that local connections reuse. Besides the transport and address, it takes the accept loop's shared state: the client-id counter, the task tracker and the client table.
- Adds `LocalChannel` (`net/bedrock/local.rs`, M2): an in-memory `Transport` for the host, over two bounded tokio channels, plus an always-present `local_listener` on `PumpkinServer`. It binds nothing.
- Puts the Java Edition listener behind an opt-in `java-listener` cargo feature in `pumpkin-core`, so it compiles out rather than being deleted. That covers the TCP bind, query and LAN broadcast. The stock `pumpkin` binary turns the feature on by default; Limonite's client never does. Java protocol types stay compiled, because they're used in 93 files of `pumpkin-core`.
- Adds a control API exposed only to the client binary: open/close LAN, pause, save, shut down. External players can never reach it.
- Lets local players skip session setup without moving code:
  - Encryption is DTLS inside NetherNet, so `LocalChannel` never does it.
  - Compression only turns on when the client sends `RequestNetworkSettings`, and the host never sends it.
  - The host logs in with an unsigned token over `LocalChannel` (M2), with no fork change. The embedded server stays offline while LAN is closed; M7 adds a trusted-local seam so LAN guests can still be Xbox-verified.
- Adds client-side packet codecs behind a `client` feature in `pumpkin-protocol`. Clientbound packets only derive `PacketWrite` and most serverbound ones only `PacketRead`; the client needs the opposite. This is the one change to existing files that isn't a seam, and a good upstream candidate.
  - It isn't one derive per packet file (M2 finding). Plain structs get a `cfg_attr` derive next to their definition. Types whose codec is hand-written get a hand-written mirror in the new `bedrock/client_codecs` module.
  - Each milestone adds the codecs it uses; M2 has the 5 packets the host sends and the 4 it decodes.
  - Sent packets are tested against bytes the official client sent (`client_codecs/fixtures/`), and must re-encode byte for byte. Read packets round-trip through Pumpkin's writer.

**What the fork keeps untouched**: world generation, physics, mob AI, ticking, chunk storage, saving, commands.

**Transports**

| Transport | When active | Carries | Notes |
| --- | --- | --- | --- |
| `LocalChannel` | Always, for the host | Uncompressed Bedrock batch bytes via `tokio::sync::mpsc` | No WebRTC, DTLS, fragmentation, compression or auth. One in-process encode/decode remains; M2 measured it (see below) |
| `NetherNet` | Only while LAN is open | Compressed batches over WebRTC data channels (DTLS-encrypted, ≤10 kB fragments) | Upstream's existing transport. Listener, status responder and LAN discovery are started and stopped by the control API |
| `P2P` | Later | NetherNet with our own signalling | ICE/STUN already exist in NetherNet; needs a signalling server and TURN fallback |

**Why bytes, not structs, on `LocalChannel`.** Pumpkin turns each packet into bytes in game code, before the transport ever sees it (`BedrockClient::serialize_packet`, called from 54 `ClientPlatform::Bedrock` sites across 13 files). Passing structs would mean rewriting all of those call sites, which breaks the small-diff rule. Bytes still skip everything expensive. M2 found that chunk packets dominate, so M3 measured whether to special-case them with `Arc`.

**Decided in M3: bytes stay, chunks included** (owner, 2026-10-21). The host's chunks go through the encoder and the client's decoder like every other packet.
- **What M2 measured:** chunks were 86% of the bytes in a radius-8 join (329 chunks, 7.2 of 8.4 MB). Encoding cost about 850 µs per chunk in release, and everything else well under a millisecond. Results are in `docs/milestones/m2-plan.md`.
- **The three checks, done in M3** (release builds; full tables in `docs/milestones/m3-plan.md`, Results):
  1. **Encode speed.** The palette conversion to Bedrock's format was 98% of `encode_chunk`: a SipHash lookup per block to recover an index Pumpkin already held. The fork's fix (`e251ffb`, `palette.rs` only, an upstream candidate) made the encode 4.8× faster, from 696 to 145 µs per chunk, with byte-identical output.
  2. **`ChunkData` mutation.** Pumpkin changes chunks in place, and sends the block update at the end of the tick. A probe saw a change in the shared `ChunkData` at once, and its `UpdateBlock` 15–60 ms later. So an `Arc` path would need a snapshot (S, 6–10 µs per chunk). Because the client keeps Bedrock-native world state, it would also still need the palette conversion (C, about 130 µs per chunk). It would only skip writing the bytes and decoding them: **(E + D) − (S + C) ≈ 30 µs per chunk**, before or after the speedup.
  3. **Join and streaming cost:**

     | Radius | Chunks per join | `Arc` saving per join | Chunks per second flying at 11 m/s | `Arc` saving while flying |
     | ---: | ---: | ---: | ---: | ---: |
     | 8 | 348 | 11 ms | 12 | 0.04% of a core |
     | 16 | 1,057 | 31 ms | 23 | 0.07% of a core |
     | 32 | 3,725 | 126 ms | 44 | 0.15% of a core |

- **The rule, per second:** adopt `Arc` only if, at radius 32, it saves at least 5% of one core, sustained, during a join or while moving.
  - **Why per second:** a per-join total isn't a spike. The server sends a Bedrock client at most 180 chunks a second (9 per tick), so a radius-32 join spreads over about 19 s.
  - **Result:** the radius-32 join saves 126 ms over about 19 s, roughly 0.7% of a core, and flying saves 0.15%. Both are far below 5%, so bytes stay.
  - The approved plan had stated the rule per join (50 ms). Read that way, radius 32 would have passed; the owner restated it per second when deciding.
- **What `Arc` would have cost:**
  - a new seam in `send_chunks`;
  - a second message type on `LocalChannel`;
  - a second chunk path in the client.

  LAN guests need the encoder anyway, so the faster encoder helps every transport.
- **Revisit only** if a profile shows chunk encode or decode on the frame's critical path, e.g. at radii past 32 or much faster movement. Moving one chunk exposes about one row of the view, so the rate scales with radius × speed.

**Packet types are shared.** The client uses Pumpkin's own Bedrock packet definitions, not a separate protocol crate. One definition on both sides of the channel means the client and server can never disagree on a protocol version. This replaces the earlier idea of using bedrock-rs. The structs need codecs in both directions first (see "What the fork changes").

**Broadcast encoding.** Pumpkin already serialises each broadcast once per recipient, with that recipient's compression settings. The host's copy is never compressed. Caching one encoding across all remote recipients is a possible M7 optimisation, but measure first.

**Fork hygiene**

- The fork lives as a git submodule inside a cargo workspace (layout in Repository layout, below).
- New code goes in new modules; edits to existing Pumpkin files are kept to the seams listed above.
- Rebase on upstream regularly. A small diff keeps `git rebase upstream/master` painless, which matters while upstream is actively fixing Bedrock.
- Each seam gets a short doc comment saying why it exists.
- On Windows, clone with `git config core.longpaths true`; some Pumpkin paths exceed `MAX_PATH`.

**Repository layout**

```
limonite-client/         # cargo workspace
├── pumpkin/             # fork, git submodule
├── client/              # renderer, input, world state, UI
├── packs/               # layered VFS, resource pack loader
├── molang/              # Molang parser + evaluator
├── iris-compat/         # shader pack loader + GLSL translation
├── mappings/            # Java <-> Bedrock block and texture names
└── Cargo.toml
```

**Licence.** Pumpkin is GPL-3.0. Embedding a modified fork generally means the distributed client is GPL-3.0 too. That's accepted for this project: the client is licensed GPL-3.0.

## Client core

The client holds a picture of the world and never the truth: the server decides what exists, the client predicts and draws. World state is the wall between networking and rendering. The renderer never touches packets; the transport never touches the GPU.

**Main loop shape**

```rust
loop {
    while let Some(packet) = transport.try_recv() {
        world.apply(packet);          // chunks, entities, player corrections
    }
    let input = input.sample();
    world.predict_local_player(&input);  // move now, don't wait for the tick
    transport.send(build_player_auth_input(&input, &world.player));
    renderer.draw(&world, interpolation_alpha);
}
```

**World state**

- Chunks in a map keyed by chunk coordinates, kept exactly as Bedrock sends them (`client/src/world/`, M3):
  - **sub-chunks:** 16×16×16, holding block network ids. Each is palette-compressed at the narrowest of Bedrock's widths that addresses its palette (1–6, 8 or 16 bits per block), in Bedrock's XZY order.
  - **layers:** one or two, the second for water around a block.
  - **biomes:** aligned with the sub-chunks.
  - **Why this format:** one format for every transport.
- Chunks reach the client as bytes on every transport, the host's included. M3 measured an `Arc` path and it isn't worth it (see "Why bytes, not structs"). Inside the client the world state owns the decoded sub-chunks, and how the mesher reads them is M4's call.
- Entities in an ECS (`bevy_ecs` or `hecs`, standalone). Pick one in milestone 5 and stick with it: M2 stores no entities, and interpolation in M5 is the first user.

**Latency: what's fixed by what**

| Source of delay | Size | Fix |
| --- | --- | --- |
| Network round trip | 0 in singleplayer | `LocalChannel` |
| Server tick | Up to 50 ms (20 ticks/s) | Client-side prediction of the local player, corrected when the server disagrees |
| Other entities' movement | Updates once per tick | Interpolate between the last two server positions |

**Lighting.** Bedrock servers are not expected to send light data, so the client computes it: flood-fill sky light downward and block light outward, minus 1 per block. Relight incrementally on block change; this runs often and must be fast. Verified in M0: Pumpkin's `LevelChunk` carries block storage, biomes and block actors only, with no light, so client lighting is required.

**Input.** `winit` for keyboard and mouse, `gilrs` for controllers. All bindings go through a rebindable keymap from day one; nothing hardcodes a key.

**Threads.** `winit` owns the main thread (macOS requires it). Pumpkin's tokio runtime runs on a background thread. The transport channel is the only thing that crosses between them.

**Other core pieces**

- World list: create, delete, back up worlds through the embedded server's save format.
- Settings persisted to a config file, including graphics tier and pack stack.
- `tracing` everywhere, with a crash report that bundles the last log lines, GPU info and the active pack stack.

## Renderer

The renderer is built on `wgpu`, which runs on Vulkan (Windows, Linux), Metal (macOS) and DX12. Raw Vulkan via `ash` is reserved for paths wgpu doesn't expose yet, reached through wgpu's HAL escape hatch. This gives portability for most of the renderer and full power where it matters, at the cost of platform-specific code in those few paths.

**Why not raw Vulkan everywhere:** months of memory and synchronisation plumbing before the first block, and macOS only gets Vulkan through MoltenVK, which would lose Metal's ray tracing on M3 and newer.

**GPU-driven rendering**

- Compute-shader chunk meshing: only faces touching air are emitted, then greedy-merged into larger quads.
- Compute-shader frustum and occlusion culling per chunk section.
- Indirect draws: the GPU writes its own draw list, the CPU issues one call per pass.
- Block textures as a 2D texture array (one layer per texture), so a face stores a layer index instead of atlas UVs. See Resource packs for the size cost.

**Capability tiers** are chosen from `adapter.features()` and `adapter.limits()`, never from the vendor name.

| Tier | Gate | Adds |
| --- | --- | --- |
| Base | Any wgpu adapter | Forward renderer, CPU fallback meshing if compute is unavailable |
| GPU-driven | Compute + indirect draw support | Compute meshing, GPU culling, indirect draws |
| Ray traced | Hardware ray query or ray tracing pipeline exposed (wgpu experimental, or Vulkan/Metal via escape hatch) | RT shadows, reflections, GI as a native path |

Hardware RT is available on Nvidia RTX, AMD RX 6000 and newer, Intel Arc, and Apple M3 and newer; GTX-class cards don't have it. Apple Silicon also has unified memory, so chunk uploads skip the PCIe copy; prefer mapped buffers on that path.

**Benchmark and warnings**

1. First launch, and first load of a shader pack: render a fixed benchmark scene for a few seconds and record frame times.
2. Suggest a tier from the measured times, not the GPU name (a laptop and desktop card with the same name differ a lot).
3. During play, track a rolling frame-time average. If it stays over budget, show a dismissible hint suggesting a lower setting or lighter pack.
4. Store the results so the benchmark doesn't rerun every launch; rerun on driver or GPU change.

**Debug overlay** (`egui`): frame time graph, chunk count, draw calls, packet log, current tier and adapter info.

## Shaders: Iris pack compatibility

Iris packs (the Java Edition format, OptiFine-derived) load by drag and drop. A pack is a full pipeline description, so when one is active the renderer switches into an Iris-shaped pipeline. Target 2 to 3 popular packs as the test suite rather than "every pack"; Iris itself took years to mature.

**What a pack contains, and what the client must provide**

| Pack part | Client must provide |
| --- | --- |
| `gbuffers_*.vsh/.fsh` (terrain, entities, water, sky…) | Geometry passes per category, writing to the pack's render targets |
| `shadow.vsh/.fsh` | A shadow pass rendered from the sun/moon |
| `deferred*`, `composite*`, `final` | Full-screen passes in order |
| `colortex0`–`colortex15`, depth and shadow textures | Render targets with the formats and sizes the pack requests |
| Uniforms (`sunPosition`, `gbufferModelView`, `frameTimeCounter`, …) | A uniform provider filled each frame |
| `shaders.properties` | Parser for options, pass toggles, buffer settings |
| `block.properties` | Java block names to IDs, fed through the mapping table to Bedrock blocks |

**Translation pipeline**

1. Run the pack's preprocessor ourselves: `#include`, `#define`, `#ifdef`, option values from the user's settings.
2. Inject a prelude that emulates legacy OpenGL built-ins (`gl_ModelViewMatrix`, `texture2D`, `gl_FragData`, …) and declares the uniforms and attributes Iris would supply.
3. Compile to SPIR-V with `shaderc`/glslang. naga's GLSL frontend is not expected to handle older pack GLSL.
4. Hand SPIR-V to wgpu (passthrough on Vulkan) or translate for Metal.
5. Cache compiled output keyed by pack hash + option set.

**Mapping table.** Java and Bedrock name many blocks differently. A shared `mappings` crate handles Java to Bedrock block and texture names; GeyserMC maintains this kind of data and is the first place to look. The same table powers Java resource pack import.

**Ray tracing.** Iris packs never use hardware RT, since OpenGL has no access to it. RT is a native path (or a pack extension the client defines), not something packs get automatically.

**Licensing.** Many shader packs have restrictive licences. The client loads packs the user supplies; it never bundles or redistributes them.

## Resource packs

Bedrock resource packs are loaded through a layered virtual filesystem; Molang is the one genuinely hard dependency. Vanilla assets are loaded from the user's own Bedrock install at runtime and never shipped in the binary. Mojang's public `bedrock-samples` repo is the format reference.

**Layered VFS**

- The pack stack is an ordered list; lookups walk it top-down and return the first hit.
- Two resolution rules: binary assets (PNGs, sounds) override; registry JSONs (`terrain_texture.json`, `item_texture.json`, `blocks.json`, `sound_definitions.json`…) merge per key, higher pack wins on conflicts.
- Packs can be folders, `.zip` or `.mcpack`; the VFS hides the difference.
- Hot reload: watch pack folders with `notify` and rebuild only what changed.

**Textures**

- Block textures go into a 2D texture array; mixed resolutions are upscaled to the stack's largest size.
- Memory = width × height × 4 bytes × layers: 1000 textures at 16×16 is about 1 MB, at 512×512 about 1 GB. Feed this into the performance warnings.
- Animated textures from `flipbook_textures.json` are extra layers plus a time-driven frame index in the shader.

**Molang** (own crate, reused by animations, render controllers, particles and parts of UI)

1. Tokeniser and parser to an AST, run once at pack load.
2. Tree-walking evaluator, run per frame, with `query.*`, `variable.*`, `math.*` bound to entity state.
3. Build a test suite from expressions found in the vanilla packs.

**Entities.** Geometry JSON (bones and cubes), animations and render controllers combine per entity definition. Skinning happens on the GPU.

**Particles.** Bedrock particle JSON + Molang, simulated in a compute shader.

**Audio.** `sounds.json` and `sound_definitions.json` from the VFS, 3D positional playback with distance falloff, probably via `kira`.

**Server-sent packs.** Servers offer packs and stream them in chunks over the protocol. Cache downloads by UUID + version. In singleplayer this step is skipped and packs load straight from disk.

**Java pack import.** Optional converter from the Java layout (`assets/minecraft/textures/block/…`) using the shared mapping table.

**Unsupported.** Encrypted Marketplace packs; the client never attempts to decrypt them.

## UI

UI is three separate systems: the client's own retained-mode UI, server forms, and a later JSON UI subset for resource packs. `egui` is used only for developer tools.

| System | Purpose | Approach | When |
| --- | --- | --- | --- |
| Own UI framework | Menus, HUD, inventory, settings, world list, chat | Retained widget tree rendered through wgpu | Milestone 5 onward |
| Server forms | Simple, modal and custom forms servers send as JSON in packets | Render with the own UI framework | With LAN/external play |
| JSON UI subset | Pack-restyled HUD and inventory | Interpret controls, `@` inheritance, variables, bindings | After the core UI is stable |
| Dev tools | Debug overlay, chunk viewer, packet inspector | `egui` | From milestone 1 |

**Why retained for the game UI:** animated, styled, game-looking screens and an eventual mapping onto JSON UI's control tree both want a persistent widget tree. Immediate mode stays for tools, where speed of writing wins.

**JSON UI scope.** Mojang is moving newer screens to Ore UI, which packs cannot modify, so JSON UI is slowly becoming legacy. Support the HUD and inventory first; expand only if packs people actually use need more.

**Cross-cutting rules**

- Localisation: every user-facing string goes through a lookup into `texts/*.lang`. No hardcoded strings.
- Accessibility: UI scale, sound subtitles, colourblind-friendly palettes, all planned from the start.
- UI reads world state; it never reads packets directly.

## Milestones

Each milestone ends in something runnable and is done only when its acceptance check passes. Do them in order; later ones assume the seams from earlier ones.

0. ✅ **Recon.** Clone Pumpkin, build it, enable Bedrock, offline mode, join with the official Bedrock client. Trace where a Bedrock connection is accepted and where packets are dispatched to a player.
   - Accept: official client joins the stock Pumpkin build; a short written note maps the accept and dispatch code paths and proposes the exact `Transport` seam.
   - **Done 2026-10-19.** Bedrock 1.26.52 joined the fork at `aac0a82` over NetherNet in offline mode. The note is `docs/recon/m0-pumpkin-bedrock.md`.
1. ✅ **Workspace + fork seam.** Cargo workspace, fork as submodule, `Transport` trait added, NetherNet moved behind it, Java listener behind a feature flag, `tracing` wired up.
   - When `pumpkin/` becomes a submodule, remove the `/pumpkin/` line from `.gitignore`; otherwise git ignores the submodule path.
   - Accept: official Bedrock client still joins the fork over NetherNet; fork diff is small and confined to the seam; the Java listener compiles out.
   - **Done 2026-10-19.** Bedrock 1.26.52 joined the fork at `80b19bb` over NetherNet. The fork diff is five files (+81/−4). Java listener is compiled out: no TCP socket can be bound without the `java-listener` feature. A `--no-default-features` server with `networking.java.enabled = true` had no TCP 25565 listener. Plan and deviations: `docs/milestones/m1-plan.md`.
2. ✅ **Headless embedded client.** Client binary starts Pumpkin on a background tokio thread, connects through `LocalChannel`, logs every packet name it receives, then sends movement.
   - Decide the host's login path (unsigned tokens vs a local login seam; recon §6) and add the client-side packet codecs.
   - Measure `LocalChannel`'s encode/decode cost.
   - Accept: spawn completes, chunks arrive, the server sees the player move, with no socket opened.
   - **Done 2026-10-20** with the fork at `03b498c`.
     - The client starts Pumpkin on a `Pumpkin-Main` thread and logs in as the host with unsigned tokens over `LocalChannel`. It spawns, receives 329 chunks and walks 8 blocks east.
     - The server's own player moved 7.8 blocks. While in-world the process owned no TCP or UDP socket, and the startup log has no listening or fetch lines.
     - Bedrock 1.26.52 still joins over NetherNet after the `accept_bedrock` move.
     - Sent packets re-encode byte for byte against official-client captures.
     - Measurements, and the `Arc` decision M3 made from them, are under "Why bytes, not structs". Plan, results and deviations: `docs/milestones/m2-plan.md`.
3. ✅ **Chunk map.** Decode sub-chunks and palettes into world state; dump a top-down map image of loaded chunks.
   - Accept: map image matches what the official client sees at the same seed and position.
   - **Done 2026-10-22** with the fork at `e251ffb`.
     - **Decoding:** `client/src/world/` decodes every `LevelChunk` into Bedrock-native world state and applies `UpdateBlock`.
     - **The live check:** the decoded world equals the server's own `ChunkData`, block for block on both layers and biome for biome. That held for 329 chunks in the dev world and 3,725 at radius 32. A difference that only comes from a change the server never sent is told apart from a decoder bug.
     - **The map:** `--map` writes the top-down map. The owner compared it with Bedrock 1.26.52 at the same seed and position, joined over NetherNet to the stock fork server, and judged it a match.
     - **Encoder speedup:** the fork's Bedrock palette conversion is 4.8× faster (encode 696 → 145 µs per chunk) with byte-identical output. It's an upstream candidate.
     - **`Arc`:** the decision is made; bytes stay (see "Why bytes, not structs").
     - Plan, results and deviations: `docs/milestones/m3-plan.md`.
4. **First pixels.** `winit` window, `wgpu` device, CPU meshing of one chunk, fly camera.
   - Accept: terrain renders with vanilla textures loaded from the user's install via the VFS.
5. **Playable singleplayer.** Client prediction, entity interpolation, block break/place, client lighting, basic HUD and inventory in the own UI framework, world list.
   - Accept: a full session of walking, building and saving with no visible input lag; reload restores the world.
6. **GPU-driven renderer.** Compute meshing, GPU culling, indirect draws, capability tiers, benchmark + warnings.
   - Accept: measurable frame-time improvement over milestone 4 at the same render distance; tier chosen from features, not vendor.
7. **Open to LAN.** Control API starts and stops the NetherNet listener, status responder and LAN discovery live; the host's packets stay uncompressed; optionally cache one encoding across remote recipients, if measurements justify it.
   - Turn online mode on when LAN opens, so guests are Xbox-verified. Add a small trusted-local seam (something like `Transport::is_trusted_local()` plus one condition in `login/login.rs`) so the host's unsigned token is still accepted.
   - Fetch the Xbox OIDC keys lazily when LAN opens. `Server::new` fetches them at startup whenever online mode and authentication are on (`server/mod.rs:354`); with LAN closed, the embedded server must open no socket.
   - Accept: a second device joins mid-session; host frame times unchanged with LAN closed and within noise with one guest.
8. **Resource packs.** Full VFS stacking and merge rules, flipbooks, Molang crate, entity models and animations, sounds, hot reload.
   - Accept: a popular third-party Bedrock pack renders blocks, items and mobs correctly; Molang suite passes on vanilla expressions.
9. **Iris packs.** Preprocessor, prelude, SPIR-V compile, pipeline emulation, mapping table.
   - Accept: the chosen 2–3 test packs load and look right versus Iris on Java at the same scene.
10. **Extras.** Native ray tracing path, JSON UI subset, server forms, Java pack import, particles.
    - Accept: per feature, defined when the milestone starts.
11. **P2P.** Signalling server and TURN fallback on top of the NetherNet transport; ICE and STUN already exist there.
    - Accept: two machines on different networks join with no port forwarding.

## Risks, open questions, references

The biggest risk is upstream Pumpkin's Bedrock support being incomplete; the biggest scope trap is Iris compatibility.

**Risks**

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Pumpkin's Bedrock side is WIP | Missing packets or behaviour block milestones | Upstream fixes; keep the official client as a control to tell fork bugs from client bugs |
| Fork drifts from upstream | Rebases become painful, Bedrock fixes are missed | Seam-only edits, frequent rebases |
| Iris scope explosion | Shader work eats the project | Fixed 2–3 pack test suite; defer the long tail |
| wgpu RT is experimental | RT path churns between wgpu versions | Keep RT behind the escape hatch and the capability gate |
| Bedrock protocol changes each game update | External LAN guests on newer clients can't join | Pin the guest client version; follow upstream Pumpkin's protocol bumps |
| Pumpkin compiles Mojang's vanilla Java datapack into the binary | A distributed Limonite binary would contain Mojang data (worldgen, loot tables, recipes, tags), breaking the "never bundle Mojang assets" rule. `pumpkin-data`'s build script downloads the Java server jar, and generated code `include_str!`s its JSON | Local dev builds only until solved. Decide before any release; one option is to fetch the server jar from Mojang on the user's machine at first run and load the datapack at runtime |
| Bedrock auto-updates past Pumpkin's exact protocol | The official client, our control, can't join at all: Pumpkin accepts one exact protocol | Record the client version in each milestone's test; rebase onto upstream protocol bumps; avoid updating the test client mid-milestone |
| Asset and licence issues | Can't distribute | Load vanilla assets from the user's install; client is GPL-3.0; never bundle packs |
| Public servers and Realms | Custom clients can trip anti-cheat or break terms | Out of scope; test only on the embedded server and own LAN |

**Open questions**

- [x] Does Pumpkin send any light data to Bedrock clients, or is client lighting required? **No light data; client lighting is required** (M0).
- [ ] Which ECS: `bevy_ecs` or `hecs`?
- [ ] Which 2–3 Iris packs form the test suite?
- [ ] Exact form of the control API: function calls on a handle, or a dedicated control channel?
- [x] How does the host's identity work when there's no Xbox login on `LocalChannel`? **The host sends an unsigned token: a fixed dev identity now, a per-install one saved with the settings in M5** (M2; recon §6).
- [ ] What permission level does the host get? Pumpkin's Bedrock `StartGame` hard-codes 2 with a TODO. Decide in M5.
- [ ] How does the embedded server get Mojang's vanilla datapack without the binary bundling it? (Found in M0; see the Risks table. Must be settled before any release.)
- [x] Which Bedrock protocol version is upstream Pumpkin targeting right now? **1.26.51, protocol 2193, exact match only** (fork commit `aac0a82`, M0).

**References**

- [Pumpkin](https://github.com/Pumpkin-MC/Pumpkin) and its docs at [pumpkinmc.org](https://pumpkinmc.org/)
- [bedrock-rs](https://github.com/bedrock-crustaceans/bedrock-rs): no longer a dependency, still useful for reading packet layouts and NBT
- [Mojang bedrock-samples](https://github.com/Mojang/bedrock-samples): vanilla pack formats
- [GeyserMC](https://github.com/GeyserMC): Java ↔ Bedrock mappings
- [Iris](https://github.com/IrisShaders/Iris): reference for shader pack pipeline behaviour
- gophertunnel (Go) and PrismarineJS bedrock-protocol (JS): reference protocol implementations
- Pumpkin's NetherNet implementation: `pumpkin/crates/pumpkin-core/src/net/bedrock/nethernet/`; the M0 walkthrough is in `docs/recon/m0-pumpkin-bedrock.md`
