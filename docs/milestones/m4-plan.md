# Milestone 4 plan: First pixels

**Status:** approved by the owner on 2026-10-22, with all eleven questions answered as recommended
("Questions and answers", at the end). Execution hasn't started. The owner asked for four changes
at approval, all folded in below:
- the host's gamemode is a setting, creative by default in M4, not a constant in code. STATUS.md
  notes that M5 revisits it together with collision (Decision 10, Step 9);
- the block palette is loaded through one small loader module, so its source can change later
  without touching the renderer (Decision 4, Step 5);
- the `__brarchive` reader returns typed errors and never panics. It checks the format's magic
  number and version, and a test feeds it every prefix of a real archive (Decision 3, Step 3);
- vanilla files are only ever read at runtime from the install. They're never copied or cached
  into the repo, and screenshots stay in gitignored folders (Decision 15, Step 12).

The plan below is kept as approved; "Deviations" at the end will record where the real code
differs.
- **Before starting:** read `docs/STATUS.md` ("Notes for M4", Dev environment, Gotchas), the
  Deviations in `m1-plan.md`, `m2-plan.md` and `m3-plan.md`, and
  `docs/recon/m0-pumpkin-bedrock.md` before touching the fork.
- **If the real code contradicts this plan,** stop and tell the owner (CLAUDE.md rule).
- **As you go:** teach. Most of M4 is new ground for the project: a window, a GPU, shaders and
  matrices. Explain the why behind each step, especially the ones marked "worth teaching".
  Update `docs/STATUS.md` as steps land.

## Context

M3 left the host with a decoded, Bedrock-native picture of the world and a top-down map. M4 puts
that world on screen. It adds a window, a GPU device, CPU meshes of the loaded chunks and a fly
camera. Vanilla textures come from the owner's own Bedrock install, through the layered VFS that
PLAN.md describes.

Two things are new for the project. M4 is the first milestone whose main loop follows a window
rather than a script. It's also the first to read Mojang assets, which may only ever come from the
user's install. Most of the work is new client code; the fork gets two small changes.

**Accept when:**
- **PLAN.md M4:** terrain renders with vanilla textures loaded from the user's install via the
  VFS.
- **This plan adds:**
  - the install is found, and the vanilla pack stack loads without read or parse errors;
  - every Bedrock block id Pumpkin can send has a name and states, and every such name has
    textures;
  - the fly camera moves the host. Chunks stream in as it flies, and chunks the server's view has
    left are dropped, so the chunk count stays bounded on a long flight;
  - the owner judges the view a match for the official client at the same position, allowing for
    the differences listed under "Out of scope";
  - the headless M3 checks still pass, now behind `--headless`;
  - frame times at radius 8, 16 and 32 are recorded as the baseline M6 must beat.

## Decisions (owner, 2026-10-22)

1. **The window becomes the default; the scripted run moves behind `--headless`** (Question 1).
   - **The window:** `cargo run -p limonite-client` opens it.
   - **The scripted run:** `--headless` runs today's session unchanged: spawn, walk, chunk wait,
     the five checks, plus `--map`, `--measure`, `--stream` and `--linger`.
   - **Misuse:** any of those four flags without `--headless` is an error that says to add it.
   - **Why:** from now on the window is what the client is. The scripted run stays as the
     regression suite, which needs no GPU and no Bedrock install.

2. **The main loop becomes a winit event loop, and the 20 Hz tick runs inside it, apart from
   frames** (worth teaching).
   - **Threads:** winit owns the main thread (PLAN.md, Threads). The server stays on `Pumpkin-Main`,
     and the `LocalChannel` is still the only thing that crosses between them.
   - **`App`** implements winit 0.30's `ApplicationHandler`:
     - `resumed` creates the window and the GPU state, once.
     - `about_to_wait` runs on every pass of the loop:
       1. `session.drain()` applies whatever arrived;
       2. for each 50 ms that has passed, one `PlayerAuthInput` with the camera's pose. After a
          stall it sends at most a few, then sets the tick clock to now, rather than a burst;
       3. meshing, within its budget (Decision 6);
       4. a redraw request.
     - `RedrawRequested` moves the camera by the elapsed time and draws.
     - `window_event` and `device_event` handle keys, the mouse, focus, resizing and closing.
   - **Why ticks don't depend on frames:** the server drops a client that has been silent for
     10 s (Finding 12).
     - A minimised window may get no redraws at all. The loop then sleeps with
       `ControlFlow::WaitUntil(next tick)` and keeps ticking.
     - While the window is visible, vsync paces the frames.
   - **`Session`** keeps its headless API (`run_until`, `tick`) and gains `set_pose` and
     `send_input`, so `tick` becomes `drain` plus `send_input`.
   - **Shutdown keeps its order:** closing the window ends the loop. Then the session is dropped,
     so the host leaves, and the server stops and saves.
     - If the server ends the connection first, the loop logs it and ends the same way.

3. **Vanilla assets come from the user's install, through a new `packs` crate** (Question 11).
   PLAN.md's layout has a `packs/` crate: "layered VFS, resource pack loader".
   - **Finding the install:**
     - `--bedrock <path>` names the folder that holds `data/resource_packs`;
     - otherwise, on Windows, the client checks the GDK default
       `<drive>:\XboxGames\Minecraft for Windows\Content` on each drive (Finding 1);
     - not found is a clear error. Nothing is bundled to fall back on.
   - **The stack:** every pack under `data/resource_packs` whose manifest has vanilla's header
     UUID, bottom first, up to the base game version the server declares.
     - That version is Pumpkin's `CURRENT_BEDROCK_MC_VERSION`, 1.26.51.
     - On the owner's install that's 55 packs (Finding 2).
     - The cap only leaves out `vanilla_1.26.52`, which holds no assets. So it changes nothing M4
       draws; it's there for installs where it will matter.
   - **A pack is a folder,** whose files are loose or inside `__brarchive` archives (Finding 3).
     - Within one pack, a path resolves to the loose file first, then to the archive for its folder.
     - Zip and `.mcpack` packs wait for M8.
   - **The archive reader is strict** (owner, at approval):
     - it checks the format's magic number and its version before anything else;
     - every failure is a typed error, never a panic;
     - it checks every entry's range when it opens an archive, so one that's damaged or cut short
       is rejected at once.
   - **Lookups walk the stack top-down and return the first hit** (PLAN.md rule).
     - Texture paths have no extension, so each pack tries `.png`, then `.tga`.
     - Vanilla never depends on that order (Finding 5).
   - **Registry JSON merges per key, and the higher pack wins** (PLAN.md rule):
     - `blocks.json` per block name;
     - `textures/terrain_texture.json` per texture key.
   - **Bedrock JSON may contain comments,** so they're stripped before parsing. A file that still
     won't parse is logged with its pack and skipped, never a panic.
   - **What the crate knows:** pack formats only (manifests, archives, the registry schemas). It
     knows nothing about network ids; the client joins the two.
   - **Tests never use Mojang files.** Fixtures are synthetic packs built in a temp folder
     (Decision 15).
     - One `#[ignore]`d test reads the real install at runtime when `LIMONITE_BEDROCK` is set, like
       the `SLogin` golden test. It writes nothing.

4. **Bedrock ids get their names from the fork's Bedrock block palette, embedded in the client**
   (Question 3).
   - **The file:** `pumpkin/assets/bedrock/block_states.nbt` lists every Bedrock block state,
     22,091 of them. Each has a `name`, its `states` and its `network_id`.
   - **Why these are our ids:** Pumpkin's codegen copies `network_id` verbatim into
     `STATE_ID_TO_BEDROCK` (Finding 7). They are exactly the ids in our chunks.
   - **How: one small loader module** (owner, at approval). `blocks/palette_source.rs` is the only
     code that knows where the palette comes from.
     - Today it `include_bytes!`s the fork's file and parses it once at startup with `pumpkin-nbt`,
       as codegen does.
     - It hands back a `Palette`: a map from network id to name and states.
     - Everything else, the appearance table and the renderer, sees only `Palette`. So the source
       can change later (a file read at runtime, a generated table, data from a server) without
       touching them.
   - **A true inverse, unlike M3's table** (worth teaching):
     - each id comes from one palette entry, so it names exactly one Bedrock state;
     - the Bedrock-to-Java table stays, for map colours and log names.
   - **Checked at startup:** every id in `STATE_ID_TO_BEDROCK` has a palette entry. The counts are
     logged, and a test checks it too.
   - **No fork change.** The cost is 3.9 MB of BDS-mined block names and states in the client
     binary.
     - It's interoperability data, not assets: no textures, models or sounds.
     - But it's Mojang-derived, so it joins the datapack row in PLAN.md's Risks: fine for dev
       builds, decide before any release.
   - **Rejected:**
     - **the user's install:** it has no block palette. Vanilla block states live in the engine
       (Finding 8).
     - **fork codegen that also emits names:** a large generated diff in an existing file, and not
       a seam.
     - **the Java table plus a Java-to-Bedrock name map:** not a true inverse, and it needs
       Geyser's mapping data anyway.

5. **Block appearance: textures come from the packs; shapes, tints and render classes come from a
   small table in the client, keyed by Bedrock name** (Question 6).
   - **Textures** come from the merged `blocks.json` (block name to a texture key per face) and
     `terrain_texture.json` (key to path).
     - **Faces:** a direction key wins, then `side` for the four sides. A plain string covers all
       six.
     - **Variants:** M4 takes variant 0 of a key's array. For terrain, that's the right one:
       fancy leaves (variant 1 is the fast-graphics opaque copy), plain dirt, and so on
       (Finding 6).
     - **Renamed blocks:** eight blocks have new names, but `blocks.json` still keys them by the old
       ones, so an alias table maps them (Finding 6). Among them: `grass_block` → `grass`,
       `sea_lantern` → `seaLantern`, and `iron_chain` → `chain`.
     - **`isotropic` faces** (grass tops, sand, ...) get a rotation picked by a hash of the
       position. The hash is ours: Bedrock's pattern isn't known, so it won't match tile for tile.
   - **Shapes:** the install has no geometry for vanilla blocks (Finding 8), so M4's shapes are a
     hand-written table:
     - **cube:** the default;
     - **cross:** plants, saplings, flowers, crops, sugar cane, cobweb, seagrass and kelp.
       Two-tall plants use the `down` texture for the lower half and `up` for the upper;
     - **liquid:** water and lava, with the top lowered when the same liquid isn't above;
     - **partial box:** slabs (`minecraft:vertical_half`), snow layers (`height`), carpets and
       lily pads;
     - **attached planes:** vines (`vine_direction_bits`), glow lichen and sculk veins
       (`multi_face_direction_bits`);
     - **invisible:** air, barriers, structure voids and light blocks.

     Everything else (stairs, fences, doors, torches, ...) is drawn as a cube, and the startup log
     counts those by name. A test checks that every name in the table exists in the palette, so a
     typo can't hide.
   - **Render classes:**
     - **opaque:** every texel of its face textures is opaque. It hides the faces of neighbours;
     - **cutout:** some texel is transparent (leaves, glass, plants), so pixels are alpha-tested;
     - **overlay:** `grass_block`'s sides. There the texture's alpha is a tint mask, not
       transparency (Finding 5);
     - **blend:** water, ice, stained glass, slime and honey.
   - **Tint kinds:**
     - grass: grass blocks, short and tall grass, ferns and sugar cane;
     - foliage: oak, jungle, acacia, dark oak and mangrove leaves, and vines;
     - birch and evergreen: their own leaves;
     - water;
     - none.
   - **Missing textures:** an unknown id, or a name or path the packs can't resolve, draws Bedrock's
     own `textures/misc/missing_texture`, and the log counts them.

6. **Meshing runs on the CPU, one mesh per sub-chunk, for every loaded chunk** (Questions 2 and
   8). Meshes keep culled faces and skip greedy merging, and they're built on the main thread
   within a time budget.
   - **How to read PLAN.md's "CPU meshing of one chunk":** one sub-chunk at a time, for every
     loaded chunk, not literally one chunk. M6's acceptance compares frame times "at the same
     render distance", so M4 must draw a whole view.
   - **Translate the palette, not each block** (worth teaching): each storage's palette is mapped
     to render entries once, and then its 4,096 indices are looked up.
   - **A padded 18×18×18 neighbourhood:** the sub-chunk plus a one-block border from its six
     neighbours, so face culling needs no bounds checks (worth teaching).
   - **When to mesh:** only once the chunk's four side neighbours are loaded.
     - So the view's outer ring stays unmeshed, as Java does.
     - Dirty sub-chunks are meshed nearest first, for up to 4 ms a frame.
   - **What makes a sub-chunk dirty:**
     - a chunk arriving: all of its sub-chunks, and its neighbours' borders;
     - an `UpdateBlock`: its sub-chunk, and a neighbour's when the block is on a border;
     - a dropped chunk: its meshes go.
   - **Hidden faces:** a face is skipped when its neighbour is an opaque or overlay cube, or the
     same liquid. Water on either layer counts, so a kelp forest has no inner water faces.
   - **Two passes:**
     1. opaque, cutout and overlay, writing depth;
     2. blend, sorted back to front by sub-chunk, without writing depth (worth teaching).
   - **Vertices are 16 bytes:** position in sixteenths of a block, texture layer, UV, face, class
     and tint. All meshes share one index buffer for the quad pattern.
   - **Measured, not guessed:** µs per sub-chunk, faces, GPU bytes and the queue length are all
     logged.
     - If meshing can't keep up at radius 8, stop and propose a worker thread. "Can't keep up"
       means terrain still popping in several seconds after the join.

7. **Textures go into one 2D texture array, one layer per texture** (PLAN.md, Renderer and Resource
   packs).
   - **Which:** only textures that some block uses. That's at most about 1,170 on the owner's
     install (Finding 6), plus the missing texture.
   - **Layer size:** the largest frame among them, scaled up by nearest neighbour for smaller ones
     (PLAN.md rule).
     - For vanilla that's 32, because of 22 special-block textures.
     - With mipmaps, that's about 6 MB.
   - **Frames:** a strip taller than it is wide is a flipbook, and M4 takes its first frame
     (animation is M8). `quad` textures are scaled to the layer size.
   - **Mipmaps** are built on the CPU with a box filter. Sampling is nearest up close and
     mipmapped in the distance.
   - **Why an array, not an atlas** (worth teaching):
     - nothing bleeds between neighbouring textures;
     - each texture gets its own mipmaps;
     - a face stores a layer number instead of UVs into one big image.

8. **Tints: per-biome grass and foliage colours from Bedrock's colormaps, one water colour, no
   blending** (Question 7).
   - **The colormaps** come from the install: `textures/colormap/grass.png`, `foliage.png`,
     `birch.png` and `evergreen.png`. They're sampled at the biome's temperature and downfall, as
     both Bedrock and Java do.
   - **The climate** comes from `pumpkin-data`. A Bedrock biome id leads to a Java biome through
     `be_network_id`, with the lowest winning and collisions counted. The Java biome gives the base
     temperature and the downfall.
   - **Downfall is private in `pumpkin-util`,** so the fork gets a one-line public accessor
     (Finding 9).
     - It's an edit to an existing file. Like M3's `palette.rs`, it's a deliberate exception to the
       seams rule, and an upstream candidate.
   - **Water:** the default surface colour and transparency from the install's
     `biomes_client.json`.
   - **One colour per column, with no blending between biomes.**
   - **Deferred: per-biome overrides and water colours** (Finding 9).
     - The overrides are swamp, badlands, mangrove, cherry and pale garden.
     - Both live in the install's `client_biome` files, keyed by Bedrock biome names.
     - But the mapping from our numeric biome ids to those names isn't in the install or in the
       fork's assets.

9. **Chunk unloading is in M4, driven by `NetworkChunkPublisherUpdate`** (Question 4).
   - **Why now:**
     - A fly camera crosses hundreds of chunks a minute, and each one keeps CPU state and a GPU
       mesh.
     - Stale chunks also break the world check (m3-plan Deviation 13).
   - **The packet** needs a reader in the fork's `client_codecs`, the M2 pattern for packets
     whose writers are hand-written (Step 1).
   - **The rule:** let r be the radius in chunks. Keep a chunk while it's within r + 3 chunks
     (straight-line distance) of the update's centre chunk; drop the rest, with their meshes.
   - **Why the margin** (worth teaching):
     - Pumpkin's view isn't the circle it publishes. It's a circle widened by 2 chunks on each axis,
       which reaches r + 2.83 chunks on the diagonals (Finding 10).
     - Pumpkin never resends a chunk it believes we still have. A chunk dropped while it's still in
       Pumpkin's view would leave a hole.
     - A test proves that our kept set covers Pumpkin's view at every radius from 2 to 32. It also
       reports the extra ring we keep.

10. **The fly camera is the host, and the embedded server makes new players creative**
    (Question 5).
    - **The host follows the camera:** the camera's eye position and angles go into each
      `PlayerAuthInput`, so the server streams chunks around wherever it flies.
    - **Why creative:** the camera flies through terrain.
      - A survival host with its head in a block suffocates (Finding 11).
      - Creative players are invulnerable to that.
    - **A setting, not a constant** (owner, at approval):
      - A new `Settings` struct (`settings.rs`) holds what M5 will save to a settings file. In M4
        that's the host's gamemode, creative by default, chosen with
        `--gamemode survival|creative|adventure|spectator`.
      - `embedded::config` takes it as a parameter and sets the server's `default_gamemode` from
        it.
      - That default only applies to players with no saved data. A saved player's gamemode wins on
        join (Finding 11), so a world that already has a host keeps that host's mode, and M4 runs
        start from fresh worlds.
      - The client logs the host's gamemode from `StartGame`, and warns if it isn't the one set.
      - M5 revisits the default together with collision (STATUS.md, Notes for M5).
    - **Coordinates** (worth teaching): the camera uses Bedrock's angles. Yaw 0 faces +Z, −90
      faces +X, and positive pitch looks down.
      - `look_direction` gives the camera's forward vector, and it's tested against the official
        client (Finding 12).
      - So what the server sees and what we draw can't disagree.
    - **Controls:**
      - W, A, S and D move; Space goes up, Shift down, and Ctrl faster;
      - the mouse looks around (raw motion). A click captures the cursor, and Esc releases it;
      - one key logs the pose as a ready-to-type `/tp` command, for the official-client
        comparison.

      Every binding goes through a keymap table (PLAN.md, Input). Saving and rebinding come with
      M5's settings.

11. **Capability rules: decide from what the adapter reports, never from who made it.**
    - **The adapter:** a high-performance preference, compatible with the window's surface.
      `WGPU_BACKEND` can force a backend for testing.
    - **Logged for context only:** `AdapterInfo` (name, backend, device type, driver). No code path
      reads the vendor, the name or the device type.
    - **Required: enough texture-array layers,** at least the number of block textures (about
      1,170).
      - WebGPU's default is 256, and desktop adapters typically report 2,048.
      - So M4 asks for the adapter's own limit. If that's too small, it fails with a message that
        names the limit and both numbers.
      - The fallback for smaller adapters is M6's tiers.
    - **Everything else is core WebGPU,** so M4 requests no optional features: `Depth32Float`, an
      sRGB colour format, 2D arrays, alpha blending, dynamic-offset uniforms.
    - **Present mode:** Fifo (vsync, always supported) by default.
      - `--bench` picks Immediate, else Mailbox, else Fifo, from what the surface reports, and logs
        which.
    - **Surface format:** the first sRGB format offered. Otherwise the first format, with a
      warning.
    - **Cursor grab:** try `Locked`, then `Confined`. Windows only offers `Confined`, and the client
      finds that out by trying, not by checking the OS.

12. **Unlit, with constant per-face shading; lighting stays in M5** (STATUS.md, "Notes for M4";
    Question 9).
    - Each face gets a fixed brightness by direction, top brightest and bottom darkest, as
      Minecraft looks without smooth lighting.
    - That isn't lighting: there are no light levels, no sky light and no ambient occlusion. It
      only keeps cubes readable.

13. **A frame-time baseline for M6.**
    - **`--bench <seconds>`:**
      1. waits until the join's meshing settles;
      2. flies the camera east at creative speed, at a fixed height and pitch, with vsync off if
         the surface allows;
      3. logs a table and exits.
    - **The table:**
      - frame count, and the average, p50, p95 and p99 frame times;
      - draw calls, sub-chunk meshes, faces and GPU bytes;
      - mesh µs per sub-chunk;
      - chunks loaded, which also shows unloading keeping the count bounded.
    - **The runs:** radius 8, 16 and 32 in release builds, each on a fresh copy of one template
      world, as in M3.

14. **Dependencies** (Question 10). Versions from crates.io on 2026-10-22, all MIT or Apache-2.0.
    - **New:**
      - `wgpu` 30.0.1, default features. Its MSRV is 1.87, under our 1.96;
      - `winit` 0.30.13, the latest stable release (0.31 is in beta);
      - `glam` 0.34.1, for vectors and matrices;
      - `bytemuck` 1.25.2, already in the lockfile through `image`, plus its `derive` feature;
      - `pollster` 1.0.1, to wait on wgpu's async setup without a runtime on the main thread.
    - **Changed:** the workspace's `image` gains the `tga` feature, which adds no crate
      (`tga = []`).
    - **`packs`** uses `serde` (derive) and `serde_json`, at versions already in the lockfile.
    - **The lockfile** grows by the graphics stack: wgpu, naga, winit and their platform crates.
      The commit message records how many packages.

15. **Vanilla files are only ever read at runtime, from the install** (owner, at approval). This
    sharpens CLAUDE.md's rule against bundling Mojang assets.
    - **Nothing from the install goes into the repo:** no copies, no extracted fixtures, no texture
      caches, no generated atlases or merged JSON.
    - **Tests** use synthetic packs. The one real-install test reads at runtime and writes nothing
      (Decision 3).
    - **Screenshots,** ours and the official client's, go in gitignored folders under `run/`.
    - **Step 12 checks it:** `git ls-files` lists no `.tga` or `.brarchive` file, and no image but
      the README's own `docs/readme/title.png`.

**Fork changes in M4:**
- **`pumpkin-protocol`, `bedrock/client_codecs/`:** a reader for `CNetworkChunkPublisherUpdate`,
  in a new file plus one `mod` line (the M2 pattern).
- **`pumpkin-util/src/biome.rs`:** `Weather::downfall()`, which replaces the field's
  `#[expect(dead_code)]`. It's a deliberate non-seam exception, and an upstream candidate.

## Findings while planning (fork at `e251ffb`, Bedrock 1.26.52 installed)

1. **The install.** `Get-AppxPackage Microsoft.MinecraftUWP` reports version 1.26.5203.0 at
   `C:\Program Files\WindowsApps\Microsoft.MinecraftUWP_1.26.5203.0_x64__8wekyb3d8bbwe`.
   - **The same files** are at `C:\XboxGames\Minecraft for Windows\Content`, a GDK install.
   - **Access:** both are readable from the owner's account.
   - **Vanilla assets** are under `Content\data\resource_packs`.
2. **The vanilla stack.** 56 pack folders share vanilla's header UUID,
   `0575c61f-a5da-4b7f-9961-ffda2908861e`.
   - **The versions:** `vanilla` is version 0.0.1. Then come `vanilla_1.14` to `vanilla_1.26.52`,
     each with its own name as its header version.
   - **How they stack:** later versions add files or override earlier ones.
   - **The newest pack:** `vanilla_1.26.52` holds only its manifest and `contents.json`.
   - **Not in M4's stack** (separate packs): `vanilla_base`, the `chemistry*` packs (Education
     Edition), `experimental_*`, `editor`, `oreui`, `persona`, `platform_gdk_pc`, `vanilla_music`,
     `vanilla_trial` and `beta`.
   - **The cap:** `ResourcePackStack` declares base game version 1.26.51
     (`net/bedrock/login/resource_pack_response.rs:46`, `pumpkin-world/src/lib.rs:26`). That keeps
     55 of the 56.
   - **Manifests** carry `"pack_optimization_version": "0.1.0"`. From 1.21.90 on they add
     `"capabilities": ["pbr"]`.
3. **Archives.** In these packs, most files live in `__brarchive` archives.
   - **One archive per folder:** `<pack>/__brarchive/textures/blocks.brarchive` holds the files
     directly inside `textures/blocks/`. A subfolder gets its own archive:
     `__brarchive/textures/blocks/deepslate.brarchive`.
   - **Loose files:** files in a pack's root (`blocks.json`, `manifest.json`,
     `biomes_client.json`) are loose.
     - Some archived files also have a loose copy. `textures/terrain_texture.json` is one, and the
       two copies are byte-identical.
   - **The format,** worked out from the files (no documentation found):
     - an 8-byte magic, `7D 27 25 B1 A0 52 70 26`, then a `u32` entry count and a `u32` version
       (1), all little-endian;
     - 256 bytes per entry: a `u8` name length, the name in a 247-byte field, a `u32` offset from
       the start of the data, and a `u32` size;
     - the data starts at byte 16 + 256 × count. Each file is stored as it is: PNG, TGA or JSON,
       uncompressed.
   - **Sounds,** for later: sound archives list their `.fsb` files with size 0. The sounds
     themselves are loose files, and they're FMOD banks.
   - **Sizes:** `vanilla`'s `textures/blocks` archive has 703 entries (737 KB).
     `vanilla_1.21.90`'s has 3,427 (5.7 MB), most of them PBR maps.
4. **Registries, merged over the stack.** `blocks.json` has 1,352 block entries.
   `terrain_texture.json` has 1,367 texture keys under `texture_data`.
   - **A `blocks.json` entry:** `textures` is one key for all faces, or an object with `up`,
     `down`, `side`, `north`, `south`, `east` and `west`.
     - It may also have `carried_textures` (for the inventory), `isotropic` (a bool, or per face),
       `sound` and `ambient_occlusion_exponent`.
     - `format_version` is an array, not a block.
   - **A `terrain_texture.json` value:** `textures` is a path, an object with `path` and
     `overlay_color`, or an array of variants (the old data values).
     - `quad: 1` marks the flowing liquids' larger texture.
   - **Keys all resolve:** every texture key that `blocks.json` uses exists in
     `terrain_texture.json`.
   - **Two format surprises:**
     - `biomes_client.json` starts with a `/* ... */` comment, which `serde_json` rejects;
     - `flipbook_textures.json` is an array, not an object.
5. **Texture files.**
   - **Extensions:** paths have none; the files are `.png` or `.tga`.
     - Only two names have both in one pack: `tallgrass` and `grass_side_snowed`, in `vanilla`.
     - `vanilla_1.21.90` overrides both with a `.tga`, so the order inside a pack never matters for
       vanilla.
   - **The TGAs** are uncompressed 32-bit, with the origin at the bottom left. `image` flips them.
   - **Alpha means different things:**
     - `grass_side.tga` is drawn opaque. Its alpha marks the 45 grey overlay pixels that take the
       grass colour; its 211 dirt pixels have alpha 0.
     - In `leaves_oak.tga` and `tallgrass.tga`, alpha 0 is a hole.
   - **Baked grass colours:** `grass_side`'s 17 variants carry `overlay_color`s (16 greens, then a
     snowy one). M4 tints the mask with the biome colour instead. The official-client comparison
     checks that choice.
   - **Sizes in the block archives:**
     - 2,346 are 16×16;
     - flipbook strips are 16×N (water is 16×512: 32 frames), and the flowing liquids 32×N;
     - 22 are 32×32: shelves, the decorated pot, dried ghast tentacles;
     - a few are odd sizes: the conduit at 24×12, the end portal at 16×17.
   - **PBR:** from 1.21.90 on, `*.texture_set.json` files (Vibrant Visuals) sit beside the
     textures. Their `color` names the same file, so the plain texture is the colour.
6. **Coverage.** The fork's palette has 1,477 distinct block names. With the eight renamed blocks
   aliased, 1,310 have a `blocks.json` entry.
   - **The other 167** are Education Edition blocks and two engine-internal ones.
     - Education: 119 `element_*`, 34 `hard_*` glass blocks and panes, the chemistry tables, the
       coloured and underwater torches, and underwater TNT.
     - Engine-internal: `unknown` and `client_request_placeholder_block`.
     - None has a Java equivalent, so Pumpkin never sends them.
   - **The eight renames:**
     - `grass_block` (key `grass`), `sea_lantern` (`seaLantern`), `iron_chain` (`chain`);
     - five engine blocks with camelCase keys: `invisible_bedrock`, `moving_block`,
       `piston_arm_collision`, `sticky_piston_arm_collision` and `trip_wire`.
   - **Textures needed:** the resolvable names use 1,171 distinct texture paths, counting every
     variant; variant 0 alone needs fewer. That's above WebGPU's default of 256 array layers, and
     below the common 2,048.
   - **Variants:** 148 blocks use a key with more than one variant.
     - Leaves have two: fancy, then a fast-graphics opaque copy.
     - Crops vary by growth, and doors, candles and others have variants too.
     - For terrain, variant 0 is right.
7. **The palette in the fork.** `assets/bedrock/block_states.nbt` is 3.9 MB: CloudburstMC's 26.50
   dump, vendored with Geyser's notice.
   - **Format:** 22,091 network-NBT compounds back to back. Each has `name`, `states`,
     `network_id`, `name_hash` and `version`.
   - **How Pumpkin uses it:** its codegen reads the file (`tools/pumpkin-codegen/src/block.rs:1033`)
     and takes `network_id` verbatim. Its doc comment explains why: "Bedrock network IDs are
     hashes of the block state" (`:2029`).
   - **The result** is `STATE_ID_TO_BEDROCK` (`pumpkin-data/src/generated/block.rs:23570`): ids for
     35,723 Java states, 15,953 of them distinct.
   - **No names today:** the binary holds the ids, but no Bedrock names.
8. **No block geometry in the install.** Only four blocks have data-driven models, all in
   `vanilla_1.26.50`, each with its own culling rules: `shelf_mushroom_large` and `_small`,
   `straw_bed_foot` and `_head`. Every other vanilla shape lives in engine code, so a client must
   know the shapes itself.
9. **Biome data.**
   - **In the install:**
     - eight colormaps in `textures/colormap.brarchive`: `grass`, `foliage`, `birch`, `evergreen`,
       `dry_foliage`, `swamp_grass`, `swamp_foliage` and `mangrove_swamp_foliage`;
     - in `biomes_client.json`, a default water colour of `#44AFF5` at transparency 0.65;
     - 87 `*.client_biome.json` files, keyed by Bedrock biome names. For example, `swampland` takes
       its foliage from `swamp_foliage`, its grass from `swamp_grass`, and its water is `#617b64`.
   - **In our chunks: numeric biome ids.**
     - `pumpkin-data`'s `Biome::ALL` (`generated/biome.rs:9320`) maps them through `be_network_id`
       to Java biomes.
     - `weather.base_temperature()` is public. `downfall` is private and unused
       (`pumpkin-util/src/biome.rs:69-78`, `#[expect(dead_code)]`).
   - **Numeric id to Bedrock name isn't anywhere we can read:**
     - Geyser's `biomes.json` maps Java names to numeric ids.
     - The vanilla entries of Pumpkin's `BiomeDefinitionList` carry id 0xFFFF
       (`tools/pumpkin-codegen/src/bedrock_biome.rs:495`), because the client knows vanilla ids
       itself.
     - Bedrock and Java names differ for about twenty biomes: `swampland` is `swamp`, `mesa` is
       `badlands`, and so on.
10. **The server's view, and unloading.**
    - **The publisher update:** Pumpkin sends `NetworkChunkPublisherUpdate` with the player's block
      position and `view_distance × 16` blocks (`world/chunker.rs:72-75`).
      - It only sends one when the view's centre chunk or distance changes.
    - **The shape of the view:** `Cylindrical::is_within_distance`
      (`pumpkin-world/src/cylindrical_chunk_iterator.rs:41`).
      - A chunk is in view when, after taking 2 off each axis distance, it's inside the circle of
        radius r.
      - So the view reaches r + 1 chunks along the axes, and r + 2.83 on the diagonals.
      - That's why a radius-32 join was 3,725 chunks, not π·32² ≈ 3,217 (m3-plan Deviation 11).
    - **Leaving the view:** `ChunkSender::unload_chunk` (`net/chunk_sender.rs:163`) forgets the
      chunk.
      - It sends Java clients `UnloadChunk`, and Bedrock clients nothing. A Bedrock client decides
        for itself, from the publisher update.
      - A chunk is resent only after it has left the view and come back.
    - **The packet has no reader:** only a hand-written writer
      (`bedrock/client/network_chunk_publisher_update.rs:36`).
11. **Gamemode.**
    - **Joining:** the config's default is survival (`pumpkin-config/src/lib.rs:238`).
      - `force_gamemode` is only read by `/defaultgamemode`
        (`command/commands/defaultgamemode.rs:26`), never on join.
      - A saved player's `playerGameType` wins (`entity/player.rs:7146`).
    - **Damage:** creative sets `abilities.invulnerable` (`player.rs:7169`).
      - `Player::damage_with_context` returns early for an invulnerable player, except for kill and
        void damage (`player.rs:4385-4394`).
      - In-wall damage comes from `entity/living.rs:1456`.
    - **What the client sees:** `StartGame` carries `player_gamemode`
      (`bedrock/client/start_game.rs:25`), and the client already decodes it.
12. **Today's loop** (`client/src/session.rs`).
    - `Session::run_until` sleeps until the next 50 ms tick (`:247`).
    - `tick` drains, then sends one `PlayerAuthInput` (`:272`). The server drops a client it hasn't
      heard from in 10 s (`:270`).
    - `drain` never blocks (`:294`).
    - `look_direction` (`:519`) matches the official client's `camera_orientation` to within 1e-4
      (the test at `:535`).

## Step 1: Two small fork changes

1. **The publisher reader:** a new file, `bedrock/client_codecs/network_chunk_publisher_update.rs`.
   - It implements `PacketRead for CNetworkChunkPublisherUpdate`, mirroring the hand-written
     writer: a `BlockPos`, the `VarUInt` radius, a `u32` count, then pairs of `VarInt` x and z.
   - **Test:** a round trip with a negative position and two cached chunks: Pumpkin's writer, then
     our reader, then the writer again, byte for byte.
   - **Commit (fork):** "Read NetworkChunkPublisherUpdate on the client side".
2. **The downfall accessor** (Question 7): `pub const fn downfall(&self) -> f32` on `Weather`.
   - Its doc comment says why it exists: clients colour grass and leaves by it.
   - The field's `#[expect(dead_code)]` goes, because the field is now used.
   - **Commit (fork):** "Expose a biome's downfall".
3. **Fork checks, in `pumpkin/`:**
   - `cargo clippy -p pumpkin-protocol --all-targets`, plain and with `--features client`;
   - `cargo clippy -p pumpkin-core --all-targets --features pumpkin-protocol/client`;
   - `cargo clippy -p pumpkin-util --all-targets`;
   - `cargo test -p pumpkin-protocol --features client`.

## Step 2: The bump (superproject)

- Commit the submodule bump: "Bump pumpkin to the publisher reader and biome downfall".
- No dependency changes yet. Each new crate arrives with the first code that uses it.

## Step 3: The `packs` crate

- **The crate:** a new workspace member `packs/`, package `limonite-packs`, GPL-3.0, with edition
  and rust-version from the workspace.
- **Modules:**
  - `brarchive.rs`: opens an archive from its bytes and reads the header and index (Finding 3);
    `get(name)` returns an entry's bytes. Strict, as the owner asked at approval (Decision 3):
    - **The magic number and the version are checked first.** The magic is
      `7D 27 25 B1 A0 52 70 26`, and the version must be 1, the only one seen.
    - **Typed errors, never a panic:**
      - a wrong magic;
      - an unsupported version;
      - a header or index cut short;
      - a name longer than its 247-byte field, or not UTF-8;
      - an entry that reaches past the end of the file.

      Offsets and sizes are added with checked arithmetic, so a huge value can't overflow.
    - **Every entry's range is checked when the archive is opened** (worth teaching).
      - All 1,244 archives in the owner's install end exactly where their last entry does.
      - So an archive cut short anywhere fails to open, instead of failing later on one entry.
    - **Absent entries:** a zero-size entry counts as absent.
  - `pack.rs`: a folder pack. `read(path)` returns the loose file if there is one, else the entry
    from the folder's archive. Archive indexes are cached per folder.
  - `manifest.rs`: the header's name, UUID and version, and `min_engine_version`.
  - `stack.rs`: `PackStack`, bottom first.
    - `read(path)` walks it top-down, and `read_texture(path)` tries `.png`, then `.tga`, in each
      pack.
    - `vanilla(resource_packs_dir, base_game_version)` builds the vanilla stack (Decision 3).
  - `json.rs`: strips comments without touching strings, then parses with `serde_json`.
  - `registry.rs`: typed `BlocksJson` and `TerrainTextures`, merged per key across the stack.
- **Tests:**
  - hand-built archives covering every field, plus a zero-size entry;
  - every proper prefix of a well-formed archive returns an error, never a panic (owner, at
    approval);
  - each corrupt field returns its own typed error: a wrong magic, version 2, a name length over
    247, an offset past the end, and a count too large for the index to fit;
  - in one pack, the loose file beats the archived one; across packs, the higher pack wins;
  - the registries merge per key;
  - a comment marker inside a string survives the comment stripping;
  - the `#[ignore]`d test against the real install. It also runs the prefix test on a real
    archive, read at runtime: the smallest archive in the vanilla stack that holds data (443 bytes
    on the owner's install). It writes nothing (Decision 15).
- **Commit:** "Add the packs crate, a layered VFS over Bedrock packs".

## Step 4: Find the install and load the vanilla stack (client)

- **Two paths in `main.rs`** (Decision 1):
  - `--headless` runs today's session, unchanged. It never needs the install.
  - The default path has no window yet. It loads the assets, logs, and exits; Steps 5 and 6 add
    to it, and Step 9 turns it into the window. So every step ends in something that runs.
- **`install.rs`:** `--bedrock` first, then the GDK default on each drive. A folder counts when
  `data/resource_packs/vanilla/manifest.json` exists in it.
- **At startup, before the server starts:** build the stack, so a missing install fails at once.
- **Log:** the install path, the pack count with the first and last version, the `blocks.json` and
  `terrain_texture.json` entry counts, and the time taken.
  - **Expected** on the owner's install: 55 packs, with 1,352 and 1,367 entries.
- **Commit:** "Load the vanilla packs from the Bedrock install".

## Step 5: Name Bedrock ids (client)

- **`blocks/palette_source.rs`, the loader** (Decision 4): the only module that knows where the
  palette comes from.
  - Today it parses the embedded `block_states.nbt`, and returns a `Palette` or a typed error.
- **`blocks/palette.rs`, the `Palette` type:**
  - `Palette::state(id)` returns the name, without `minecraft:`, and the states as typed values;
  - it also runs the coverage check below.

  Nothing outside the loader names the file or the NBT format.
- **Log:** entries, distinct names, and the parse time in debug and release. Also how many of the
  ids in `STATE_ID_TO_BEDROCK` were found: it should be all of them.
- **Tests:**
  - known blocks: `Block::GRASS_BLOCK.default_state` is `grass_block`, an oak log carries its
    `pillar_axis`, and water carries `liquid_depth` 0;
  - every id Pumpkin can send is found.
- **Commit:** "Name Bedrock block ids from the fork's block palette".

## Step 6: Block appearance (client, CPU only)

- **`blocks/appearance.rs`:** a render entry for every palette id: shape, class, tint, face layers,
  `isotropic` flags and shape parameters (Decision 5).
- **`render/textures.rs`, the CPU half:**
  1. decode each texture in use (PNG or TGA, through `image`);
  2. take the first frame, and scale it to the layer size;
  3. build the mipmaps.

  The texels' alpha also decides each block's render class.
- **`render/tint.rs`:** the colormaps, the climate table and the water colour (Decision 8).
- **Startup log:**
  - names with textures. Expected: 1,310 of 1,477 palette names; the other 167 are Education or
    engine-internal, and Pumpkin never sends them (Finding 6);
  - the blocks drawn as stand-in cubes, by name;
  - missing textures;
  - the layer count and size, the texture bytes, and the time taken.
- **Tests,** on synthetic packs:
  - face keys, variant 0, aliases and `isotropic`;
  - render classes from alpha, and the grass overlay;
  - a flipbook's first frame, upscaling, and the mip chain;
  - every name in the shape and tint tables exists in the palette;
  - tint sampling at a colormap's corners.
- **Commit:** "Resolve block textures, shapes and tints from the packs".

## Step 7: World changes and chunk unloading (client)

- **Change tracking:** `World` records what changed: chunks added or replaced, blocks changed with
  their position, and chunks dropped. `take_changes()` hands them over.
  - It's opt-in (`track_changes`), so the headless run doesn't keep a log that nobody reads.
- **`Applied::Block`** gains the block's position.
- **Unloading:** `World::apply` takes `NetworkChunkPublisherUpdate` and drops every chunk outside
  r + 3 (Decision 9).
- **Tests:**
  - the drop rule;
  - the coverage test against Pumpkin's own `Cylindrical`, for radii 2 to 32 (Decision 9);
  - change tracking for each packet type.
- **Unused until Step 10:** change tracking, and the mesher in Step 8, have no caller in the binary
  yet. They carry `#[cfg_attr(not(test), expect(dead_code, reason = "..."))]`, as M3 did for
  `Chunk::dimension`, and Step 10 removes it.
- **Commit:** "Drop chunks the server's view has left".

## Step 8: The mesher (client, no GPU)

- **`render/mesher.rs`:** `mesh(neighbourhood, appearance, tints)` returns the opaque and blend
  vertices for one sub-chunk. It's a pure function, so it's tested without a GPU.
- **Tests:**
  - one stone block gives 6 faces, and two touching give 10;
  - stone beside glass keeps the face between them;
  - a cross plant gives 4 quads, both sides of two planes;
  - water beside water, or beside waterlogged seagrass, has no face between them;
  - water under air gets a lowered top;
  - borders read the neighbour, and a chunk with a missing neighbour isn't meshed;
  - a slab, a snow layer and a vine;
  - the `isotropic` rotation is the same on every run.
- **Commit:** "Mesh sub-chunks on the CPU".

## Step 9: The window, the device and the fly camera (client)

- **`main.rs`:** the default path from Steps 4 to 6 keeps loading the packs, palette, appearance
  and textures. Then, instead of exiting, it starts the server, connects, and runs the event loop.
- **`app.rs`:** the `ApplicationHandler` (Decision 2).
- **`render/mod.rs`:** the instance, surface, adapter and device, under the capability rules
  (Decision 11).
  - It configures the surface and handles resizing, and clears to a sky colour.
- **`render/camera.rs`:** the fly camera, its view and projection matrices (glam, right-handed,
  depth from 0 to 1), and its frustum.
- **`input.rs`:** the keymap and the state of each action.
- **`session.rs`:** `set_pose` and `send_input`; `tick` becomes `drain` plus `send_input`.
- **`settings.rs`:** `Settings`, with the host's gamemode, creative by default, and `--gamemode` to
  choose another (Decision 10).
  - Both paths use it, so headless runs get the same default.
  - A comment says why creative: the fly camera passes through terrain, and M5 revisits it with
    collision.
- **`embedded.rs`:** `config` takes the gamemode from `Settings` instead of a constant.
- **Checks:**
  - the server's player follows the camera. At debug level, the client logs both positions once a
    second;
  - chunks keep arriving while the camera flies;
  - after 30 s minimised, the host is still connected.
- **Commit:** "Open a window with a fly camera that flies the host".

## Step 10: Draw the terrain, the first pixels (client)

- **The texture array:** uploaded, with its mipmaps.
- **`terrain.wgsl`:**
  - unpacks the vertex, and places it relative to the camera;
  - samples the array, then applies the tint, the overlay mask or the alpha test, and the face
    shading.

  A second pipeline draws the blend pass.
- **`render/meshes.rs`:** the GPU meshes, one per sub-chunk.
  - It keeps the dirty set, waits for neighbours, and stays within the budget (Decision 6).
  - It culls against the frustum, and sorts for the blend pass.
- **Camera-relative positions** (worth teaching): vertex positions are local to their sub-chunk.
  - Each draw gets its sub-chunk's origin minus the camera, worked out in `f64`.
  - So the picture doesn't jitter far from 0, 0, where an `f32` has too few digits left for a
    world position.
- **The title bar** shows the frame rate and frame time, chunks, meshes and position.
- **The owner looks at the first frames.**
- **Commit:** "Draw the loaded chunks with vanilla textures".

## Step 11: The M6 baseline (`--bench`; release numbers only)

1. **The template world:** one fresh release run at `--radius 32` into `run/m4-template` generates
   the area. Its host is creative (Decision 10).
2. **Three runs,** each on a fresh copy: `--bench 30 --radius R`, with R = 8, 16 and 32. Only R
   changes.
3. **Record** the tables in this file's Results, plus the highest chunk count compared with the
   join.
- **Commit:** "Measure frame times for the M6 baseline".

## Step 12: Verify, accept, tick

1. **Fork:** the Step 1 checks.
   - `git -C pumpkin diff e251ffb --stat` should list only `client_codecs/mod.rs`, the new reader
     file and `pumpkin-util/src/biome.rs`.
2. **Client and packs:** `cargo test --workspace` and
   `cargo clippy --workspace --all-targets`.
   - **No Mojang files in the repo** (Decision 15): `git ls-files '*.tga' '*.brarchive'` is empty,
     and `git ls-files '*.png' '*.jpg'` lists only `docs/readme/title.png`.
3. **The headless regression:** the M3 acceptance run with `--headless` gives five PASS lines and
   the map.
4. **The windowed acceptance** (owner):
   - a fresh world and a dev build;
   - the startup log lines from Steps 4 to 6;
   - a flight around: terrain with vanilla textures.
5. **The official-client comparison** (owner, about 15 minutes), following M3's procedure:
   1. **In our client:** fly to a spot with variety: water, trees, a biome edge. Log the pose,
      take a screenshot (Win+Shift+S), and exit cleanly.
      - Screenshots from both clients go in `run/m4-official/`, which is gitignored
        (Decision 15).
   2. **Copy the world** into `run/m4-official/`, but only after a clean exit: exit code 0,
      `Completed save!` and `Embedded server stopped` in the log, and no `limonite-client` process
      left.
   3. **Run the stock server** there, built at the M4 fork commit.
   4. **Join** from Bedrock 1.26.52 with the graphics mode on Fancy, not Vibrant Visuals. Then type
      `/time set noon`, `/weather clear` and the logged `/tp`.
   5. **Compare the two side by side:** block textures, tints, water, plants and the shape of the
      terrain. Ignore the differences listed under "Out of scope".
   6. **Also watch the view's edge** while flying in the official client. If holes appear, Pumpkin's
      wider view (Finding 10) may be leaving the official client without chunks. That would be an
      upstream candidate.
   7. **Stop the server:** `Get-Process pumpkin` must return nothing.
6. **Tick,** committed as "Tick Milestone 4":
   - **PLAN.md:**
     - tick M4;
     - the Renderer and Resource packs facts found here: the archive format, the vanilla stack
       rule and the layer size;
     - the palette in the Risks table.
   - **STATUS.md:**
     - progress;
     - Notes for M5 found in M4, next to the gamemode note that planning already added;
     - the windowed dev setup: `--bedrock`, `--gamemode`, the controls, `--headless` and `--bench`.
   - **README.md:** the window by default, the controls and the flags.
   - **This file:** Deviations and Results.

   Every commit ends with the Co-Authored-By line.
7. **Ask the owner** before pushing anything, or opening an upstream PR. The downfall accessor is a
   candidate, and so is the view-shape question if Step 12.5 shows holes.

## Owner involvement

- **Approval:** done on 2026-10-22, with four amendments (Status, at the top).
- **Step 10:** look at the first frames (a few minutes).
- **Steps 12.4 and 12.5:** the acceptance flight and the official-client comparison (about 20
  minutes).
- **Pushes and upstream PRs:** ask first.

## Verification (summary)

- **Unit tests,** with no GPU and no Mojang files: Steps 1, 3, 5, 6, 7 and 8, plus the camera
  maths.
- **Startup checks,** logged on every run: the stack, the palette's coverage of Pumpkin's ids, and
  the texture and shape coverage.
- **Automated end to end:** the headless M3 checks (Step 12.3).
- **The real thing:** the owner's flight, then the official client at the same pose (Steps 12.4
  and 12.5).
- **Numbers:** Step 11, in release builds, changing one variable per run.

## Out of scope

- **Lighting (M5):** no light levels, sky light, block light, smooth lighting or ambient occlusion.
  Only the constant face shading.
- **Special block shapes beyond Decision 5:** stairs, fences, walls, panes, doors, torches, rails,
  beds, chests, signs and so on. They're drawn as cubes, and counted.
- **Everything that isn't a block:** block entities, entities, the sky, clouds, fog, weather,
  particles, the HUD, and the `egui` debug overlay.
- **Texture extras:** animated textures (flipbooks), and connected or random textures beyond
  `isotropic`.
- **Tint fidelity:** biome-specific overrides, per-biome water colours, and blending between
  biomes (Decision 8).
- **PBR texture sets (Vibrant Visuals):** `*.texture_set.json` and their `_mers` and normal maps.
- **Pack features (M8):** zip and `.mcpack` packs, user pack stacks, hot reload, Molang.
- **GPU-driven rendering (M6):** greedy meshing, GPU culling, indirect draws, compute meshing.
- **Meshing on a worker thread,** unless Step 10's numbers demand it. Then stop and ask.
- **Settings (M5):** gamepads (`gilrs`), a settings file, and rebinding.
- **Other dimensions:** nothing stops them rendering, but they aren't tested.

## Questions and answers (owner, 2026-10-22)

The owner answered all eleven as recommended. Two answers came with amendments, folded in above:
- **Question 3:** the palette loads through one small loader module (Decision 4);
- **Question 5:** the gamemode is a setting, not a constant, and M5 revisits it with collision
  (Decision 10).

The questions as they were asked:

1. **Should the window be the default, with the scripted run behind `--headless`** (Decision 1)?
   Recommended: yes.
2. **Is PLAN.md's "CPU meshing of one chunk" right to read as meshing each sub-chunk of every
   loaded chunk** (Decision 6)? Recommended: yes, because M6 compares frame times at the same
   render distance.
3. **May the client binary hold the Bedrock block palette** (Decision 4)? That's 3.9 MB of
   BDS-mined names and states, filed with the datapack under PLAN.md's release-blocker risk.
   Recommended: yes; without it, ids can't be matched to textures.
4. **Should chunk unloading land in M4, with a 3-chunk margin** (Decision 9)? Recommended: yes.
5. **Should creative be the embedded server's default gamemode, with M4 runs on fresh worlds**
   (Decision 10)? Recommended: yes.
   - **The alternative:** keep survival, and stop the camera at solid blocks, so there's no flying
     through terrain.
6. **Should M4 draw only the Decision 5 shapes, with every other block as a cube that the log
   counts?** Recommended: yes.
   - **The alternative:** skip unsupported shapes, which leaves holes instead of wrong cubes.
7. **Should grass and foliage be tinted per biome from Pumpkin's climate data** (Decision 8)? That
   needs the one-line `Weather::downfall()` fork accessor. Recommended: yes.
   - **The alternative, with no fork change:** one plains colour everywhere until M5.
8. **Should meshing run on the main thread within a 4 ms budget,** moving to a worker thread only if
   Step 10's numbers demand it? Recommended: yes.
9. **Should faces get constant per-face shading** (Decision 12), or stay fully flat until M5?
   Recommended: face shading.
10. **Are these dependencies OK** (Decision 14)? wgpu 30.0.1, winit 0.30.13, glam 0.34.1, bytemuck's
    `derive` feature and pollster, plus `tga` on `image`. Recommended: yes.
11. **Should the VFS be a separate `packs` crate now, as PLAN.md's layout has it, or a module in the
    client** (Decision 3)? Recommended: the crate.

## Deviations (filled in during execution)

## Results (filled in during execution)
