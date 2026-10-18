# Limonite Client — Design Plan

Oct 19, 2026 · @Krivi Ganta

## Overview

Limonite Client is a Minecraft Bedrock Edition client written in Rust, shipped as one binary that embeds a forked Pumpkin server for singleplayer. The client renders, predicts and presents; the embedded server owns world generation, physics, mob AI and saving. Nothing that Pumpkin already does gets rebuilt.

**Goals**

- Singleplayer with zero network overhead: the client and embedded server talk through an in-memory channel, not sockets.
- Open to LAN from inside a running world, without the host losing performance.
- A transport abstraction that can later carry peer-to-peer joining, Mojang-style.
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

The host's packets never leave memory. RakNet only exists while LAN is open, and P2P (dashed) is a later transport behind the same trait.

## Server backend: embedded Pumpkin fork

The backend is a source-level fork of [Pumpkin](https://github.com/Pumpkin-MC/Pumpkin), linked as a crate and run inside the client process. Pumpkin's Bedrock support is still work in progress upstream, so expect gaps and plan to upstream fixes where possible.

**What the fork changes**

- Adds a `Transport` trait: the one seam where a player's packets enter and leave the server.
- Strips the Java Edition listener behind a cargo feature, so it compiles out rather than being deleted.
- Adds a control API exposed only to the client binary: open/close LAN, pause, save, shut down. External players can never reach it.
- Moves Bedrock session setup (auth, compression, encryption) inside the RakNet transport, so local players skip it entirely.

**What the fork keeps untouched**: world generation, physics, mob AI, ticking, chunk storage, saving, commands.

**Transports**

| Transport | When active | Carries | Notes |
| --- | --- | --- | --- |
| `LocalChannel` | Always, for the host | Packet structs via `tokio::sync::mpsc`, large ones in `Arc<T>` | No serialisation, compression, encryption or auth |
| `RakNet` | Only while LAN is open | Encoded, compressed, encrypted bytes over UDP 19132 | Spawned and torn down by the control API |
| `P2P` | Later | Encoded bytes over a NAT-traversed link | Needs a signalling server, STUN, TURN fallback |

**Packet types are shared.** The client uses Pumpkin's own Bedrock packet definitions, not a separate protocol crate. One definition on both sides of the channel means the client and server can never disagree on a protocol version. This replaces the earlier idea of using bedrock-rs.

**Lazy broadcast encoding.** Many packets go to every player. With mixed transports, the local player receives the struct; the first remote recipient triggers serialisation, and the bytes are cached and reused for every other remote player. With LAN closed, nothing is ever serialised.

**Fork hygiene**

- The fork lives as a git submodule inside a cargo workspace (layout in Repository layout, below).
- New code goes in new modules; edits to existing Pumpkin files are kept to the seams listed above.
- Rebase on upstream regularly. A small diff keeps `git rebase upstream/master` painless, which matters while upstream is actively fixing Bedrock.
- Each seam gets a short doc comment saying why it exists.

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

- Chunks in a map keyed by chunk coordinates, stored as 16×16×16 sub-chunks with palette compression (bits per block = ceil(log2(palette size))).
- Chunk data shared as `Arc` so the channel and the mesher read the same allocation.
- Entities in an ECS (`bevy_ecs` or `hecs`, standalone). Pick one in milestone 2 and stick with it.

**Latency: what's fixed by what**

| Source of delay | Size | Fix |
| --- | --- | --- |
| Network round trip | 0 in singleplayer | `LocalChannel` |
| Server tick | Up to 50 ms (20 ticks/s) | Client-side prediction of the local player, corrected when the server disagrees |
| Other entities' movement | Updates once per tick | Interpolate between the last two server positions |

**Lighting.** Bedrock servers are not expected to send light data, so the client computes it: flood-fill sky light downward and block light outward, minus 1 per block. Relight incrementally on block change; this runs often and must be fast. Verify against what Pumpkin actually sends before building.

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

0. **Recon.** Clone Pumpkin, build it, enable Bedrock, offline mode, join with the official Bedrock client. Trace where a Bedrock connection is accepted and where packets are dispatched to a player.
   - Accept: official client joins the stock Pumpkin build; a short written note maps the accept and dispatch code paths and proposes the exact `Transport` seam.
1. **Workspace + fork seam.** Cargo workspace, fork as submodule, `Transport` trait added, RakNet moved behind it, Java listener behind a feature flag, `tracing` wired up.
   - Accept: official Bedrock client still joins the fork over RakNet; fork diff is small and confined to the seam; Java compiles out.
2. **Headless embedded client.** Client binary starts Pumpkin on a background tokio thread, connects through `LocalChannel`, logs every packet name it receives, then sends movement.
   - Accept: spawn completes, chunks arrive, the server sees the player move, with no socket opened.
3. **Chunk map.** Decode sub-chunks and palettes into world state; dump a top-down map image of loaded chunks.
   - Accept: map image matches what the official client sees at the same seed and position.
4. **First pixels.** `winit` window, `wgpu` device, CPU meshing of one chunk, fly camera.
   - Accept: terrain renders with vanilla textures loaded from the user's install via the VFS.
5. **Playable singleplayer.** Client prediction, entity interpolation, block break/place, client lighting, basic HUD and inventory in the own UI framework, world list.
   - Accept: a full session of walking, building and saving with no visible input lag; reload restores the world.
6. **GPU-driven renderer.** Compute meshing, GPU culling, indirect draws, capability tiers, benchmark + warnings.
   - Accept: measurable frame-time improvement over milestone 4 at the same render distance; tier chosen from features, not vendor.
7. **Open to LAN.** Control API toggles the RakNet transport live; lazy broadcast encoding.
   - Accept: a second device joins mid-session; host frame times unchanged with LAN closed and within noise with one guest.
8. **Resource packs.** Full VFS stacking and merge rules, flipbooks, Molang crate, entity models and animations, sounds, hot reload.
   - Accept: a popular third-party Bedrock pack renders blocks, items and mobs correctly; Molang suite passes on vanilla expressions.
9. **Iris packs.** Preprocessor, prelude, SPIR-V compile, pipeline emulation, mapping table.
   - Accept: the chosen 2–3 test packs load and look right versus Iris on Java at the same scene.
10. **Extras.** Native ray tracing path, JSON UI subset, server forms, Java pack import, particles.
    - Accept: per feature, defined when the milestone starts.
11. **P2P.** Signalling server, NAT traversal, `P2P` transport.
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
| Asset and licence issues | Can't distribute | Load vanilla assets from the user's install; client is GPL-3.0; never bundle packs |
| Public servers and Realms | Custom clients can trip anti-cheat or break terms | Out of scope; test only on the embedded server and own LAN |

**Open questions**

- [ ] Does Pumpkin send any light data to Bedrock clients, or is client lighting required?
- [ ] Which ECS: `bevy_ecs` or `hecs`?
- [ ] Which 2–3 Iris packs form the test suite?
- [ ] Exact form of the control API: function calls on a handle, or a dedicated control channel?
- [ ] How does the host's identity and permission level work when there's no login on `LocalChannel`?
- [ ] Which Bedrock protocol version is upstream Pumpkin targeting right now?

**References**

- [Pumpkin](https://github.com/Pumpkin-MC/Pumpkin) and its docs at [pumpkinmc.org](https://pumpkinmc.org/)
- [bedrock-rs](https://github.com/bedrock-crustaceans/bedrock-rs): no longer a dependency, still useful for reading packet layouts and NBT
- [Mojang bedrock-samples](https://github.com/Mojang/bedrock-samples): vanilla pack formats
- [GeyserMC](https://github.com/GeyserMC): Java ↔ Bedrock mappings
- [Iris](https://github.com/IrisShaders/Iris): reference for shader pack pipeline behaviour
- gophertunnel (Go) and PrismarineJS bedrock-protocol (JS): reference protocol implementations
