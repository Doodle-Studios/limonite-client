# Milestone 5 plan: Playable singleplayer

**Status: approved on 2026-10-24; not started.** The owner answered all fifteen questions as
recommended ("Questions and answers", at the end), and asked for five changes, all folded in below:
- the physics is checked against the official client: Step 1's captured `PlayerAuthInput`
  positions are the ground truth for a jump's peak, walking, sprinting and sneaking speeds, and
  falling, and our physics replays the same inputs (Decision 5, Steps 1 and 6);
- Step 7 has a test that walks every Bedrock id, checks that all the Java states sharing it have the
  same collision and outline boxes, and lists any that don't (Decision 7, Step 7);
- each owner checkpoint is a go/no-go point. If a phase overruns, the owner can tick what's done
  and split the rest into its own milestone (Decision 1, Owner involvement);
- PLAN.md's Risks now say that Pumpkin trusts Bedrock movement and placement, so M7 must decide how
  to treat LAN guests (Decision 4);
- the supervisor goes back to the world list with an error when a child crashes, and stops with a
  message if a child dies at startup twice in a row (Decision 17, Step 16).

Question 3's answer is in PLAN.md too: container screens are milestone 5b, straight after M5.

The plan below is kept as approved; "Deviations" at the end will record where the real code
differs.
- **Before starting:** read `docs/STATUS.md` ("Notes for M5", "Special shapes to do", Dev
  environment, Gotchas), the Deviations in `m1-plan.md` to `m4-plan.md`, and
  `docs/recon/m0-pumpkin-bedrock.md` before touching the fork.
- **If the real code contradicts this plan,** stop and tell the owner (CLAUDE.md rule).
- **As you go:** teach. M5 is the project's first game programming: a fixed-step simulation,
  collision, prediction, light propagation and a UI framework. Explain the why behind each step,
  especially the ones marked "worth teaching". Update `docs/STATUS.md` as steps land.

## Context

M4 put the world on screen, but the host is still a camera. It flies through terrain, can't touch a
block, and sees no light, mobs, HUD or menus. M5 makes it a game to play alone. The host walks with
gravity and collision, breaks and places blocks, sees mobs move smoothly, and sees the world lit by
the sun and by torches. It gets a HUD, an inventory and a world list, and its settings and identity
persist between runs.

Three things are new for the project:
- **The client simulates.** The local player moves by its own physics every tick, ahead of the
  server, and edits are drawn before the server confirms them. That's PLAN.md's client-side
  prediction.
- **The client works out what the server never sends:** light (PLAN.md, Lighting).
- **The client gets its own UI** (PLAN.md, UI): a retained widget tree drawn through wgpu, with
  sprites, the font and the strings read at runtime from the install.

The fork changes only by client codecs, as in every milestone so far: readers and writers for the
play packets M5 uses, golden-tested against a new capture from the official client.

**Accept when:**
- **PLAN.md M5:** a full session of walking, building and saving with no visible input lag; reload
  restores the world.
- **This plan adds:**
  - **"No visible input lag", measured** (Decisions 3 and 9): mouse look moves the camera in the
    next frame; a movement key moves the player from the next tick, as in both vanilla clients; a
    predicted break or place is drawn in the next frame. The log summarises each, and the frame
    times, at the end of a session;
  - **"Reload restores the world", checked:** after Save & Quit and reopening from the world list,
    the blocks, the inventory, the host's position and gamemode, and the time of day are as they
    were. A two-run headless check does the same for scripted edits;
  - **light:** the client's sky and block light equal the server's own light engine, or each
    difference is explained (a headless check, like M3's world check);
  - entities move smoothly between server updates;
  - the world list creates, opens, copies and deletes worlds;
  - the headless M2 to M4 checks still pass, and `--bench` still flies M4's flight;
  - the owner judges a lit view a match for the official client at the same pose, allowing for the
    differences listed under "Out of scope".

## Decisions (owner, 2026-10-24)

1. **One milestone in seven phases, each ending in something to try** (Question 1).

   | Phase | Steps | Ends with |
   | --- | --- | --- |
   | A. The fork | 1 to 3 | the official client's play packets captured, and client codecs |
   | B. The player | 4 to 7 | **owner checkpoint:** walk a fresh world |
   | C. The HUD and items | 8 and 9 | a hotbar with icons |
   | D. Building | 10 to 12 | **owner checkpoint:** build something |
   | E. Light | 13 and 14 | **owner checkpoint:** lit frames by day and night |
   | F. Entities and worlds | 15 and 16 | **owner checkpoint:** the world list |
   | G. Measure and accept | 17 and 18 | the acceptance session, then the tick |

   - **Why not two milestones:** PLAN.md's acceptance needs nearly all of it at once. Building needs
     the hotbar and the inventory, and "reload restores the world" needs the world list and a saved
     identity. The checkpoints give the owner what a split would.
   - **Each checkpoint is a go/no-go point** (owner, at approval).
     - **At each one,** a short report: what the phase built, what's left, and how long it took
       against the phases before it.
     - **Go:** the next phase starts.
     - **No-go,** if a phase overruns: M5 is ticked with what's done, and the rest becomes its own
       milestone in PLAN.md, with M5's acceptance split between the two. The split is recorded in
       Deviations.

2. **One `Game` holds the client's whole picture** (worth teaching).
   - **What:** a new `game.rs` with `Game`: the `World` (blocks, and now light), the entities, the
     local player, the inventory and the clock. `Game::apply(packet)` is the only way packets change
     any of it.
   - **Why:** PLAN.md's wall between networking and rendering grows past chunks. `Session` decodes
     and hands over; the renderer and the UI only read `Game` ("UI reads world state; it never
     reads packets directly", PLAN.md, UI).
   - `Session` keeps its stage machine and `run_until`. `session.world()` stays as a shortcut, so
     the headless checks don't change.

3. **The simulation steps at 20 Hz; frames draw it interpolated** (worth teaching).
   - **Each tick (50 ms):** read the held actions, step the local player's physics once, and send
     one `PlayerAuthInput` with the result. Ticks already run apart from frames (`app.rs:186-205`);
     now they also move the player.
   - **Each frame:** draw the player at `lerp(last tick, this tick, alpha)`, where alpha is how far
     the frame is into the next tick. Mouse look goes straight to the camera every frame, never
     through the tick.
   - **Why:** jump height, fall distance and breaking time are all defined per tick, at the rate
     the server ticks. Interpolation keeps frames smooth at 165 Hz while the simulation runs at 20.
     The price is the vanilla one: a movement key shows up to one tick later. Looking around has no
     added delay.
   - **After a stall,** at most three ticks run back to back, as `MAX_CATCH_UP` does now
     (`app.rs:62-64`).
   - **`--bench` keeps M4's camera:** its scripted flight bypasses physics and interpolation, so it
     stays the flight M6 compares (Decision 20).

4. **Prediction: the client moves itself, and Pumpkin takes the result; the server's corrections
   still win** (worth teaching; Finding 1).
   - **Pumpkin takes a Bedrock player's position as sent,** with no speed or collision check, and
     never sends `CorrectPlayerMovePrediction`. So the client's predicted position *is* the
     server's, and predicting means not waiting.
   - **The corrections the client obeys:**
     - `MovePlayer` for its own runtime id, a teleport or a respawn: it snaps there and stops;
     - `SetActorMotion` for its own id: knockback;
     - `CorrectPlayerMovePrediction`, if a server ever sends one;
     - `UpdateAbilities` (may fly, flying, no clip, instant build), `SetPlayerGameType`, and
       `UpdateAttributes` (movement speed, health and hunger).
   - **Honest reports:** the server takes on-ground from `VerticalCollision` with a falling
     `delta.y`, and sprinting, sneaking and flying from the start and stop flags. So each input
     carries the flags the official client sends in the same situation (Step 1 shows which), set
     from the physics' real collisions, with the real `delta` and `move_vec`.
   - **Waiting for terrain:** the player doesn't move until the chunks around it have arrived, so a
     join can't fall through the world before they land.
   - **The same trust reaches LAN guests:** Pumpkin also uses whatever item a client says it holds
     when placing (Finding 3). That's harmless for the host, but not for M7's guests, so PLAN.md's
     Risks now record it, for M7 to decide (owner, at approval).

5. **Physics: vanilla's rules on Pumpkin's data** (worth teaching).
   - **The rules, per tick:** gravity 0.08 and vertical drag 0.98. Ground friction is the block's
     slipperiness (0.6, ice 0.98) times 0.91, and air friction 0.91. Ground acceleration is
     speed × 0.216 / slipperiness³, and air acceleration 0.02. Speed is the `minecraft:movement`
     attribute (0.1), ×1.3 sprinting and ×0.3 sneaking. A jump is 0.42 × the block's jump
     multiplier, plus a forward push when sprinting.
   - **A Rust reference exists:** Pumpkin's own mob physics (`entity/living.rs:1531-1606`,
     `entity/mod.rs:1401`). It needs a server world, so the client writes a pure version, tested
     without a server, and checks its constants against Pumpkin's.
   - **The official client is the ground truth** (owner, at approval; worth teaching).
     - Step 1 records clean runs of each motion on flat ground: walking, sprinting and sneaking in a
       straight line, standing jumps, and a fall of ten blocks.
     - Each `PlayerAuthInput` carries the position the official client's own physics predicted
       for that tick. So a run is a trace: the inputs it read, and where they took it.
     - Our physics replays each trace's inputs from the same start, over a flat floor at the
       trace's height, and its positions must match tick by tick: the jump's peak and how long it
       takes, the three walking speeds, and the fall. Bedrock's rules may differ from Java's in
       places; where they do, Bedrock wins, and the change is recorded in Deviations.
   - **Collision:** the player is a 0.6 × 1.8 box with its eyes at 1.62, lower while sneaking.
     - It moves along y first, then along x and z, the larger first, each clipped by the collision
       boxes in the swept region.
     - Then it tries again 0.6 higher, to step onto slabs and stairs.
     - Sneaking stops at edges.
   - **Collision boxes come from `pumpkin-data`** (Finding 9), through M3's Bedrock-to-Java table.
   - **Liquids and climbing, basic:** water and lava slow and drag, and holding jump swims up.
     Ladders and vines cap the fall and let you climb.
   - **Modes:**
     - survival and adventure walk;
     - creative also flies: a double tap on jump toggles it, as on Bedrock. Jump and sneak rise and
       sink, and landing ends it;
     - spectator flies through blocks, like M4's camera.

6. **The gamemode is chosen when a world is created, creative first** (Question 2).
   - The create-world screen offers survival and creative. The settings file says which is
     selected first.
   - **Why creative first:** survival works in M5 (physics, health, hunger, breaking by tool and
     hardness, death and respawn), but without crafting or containers it can't get far. Those come
     in milestone 5b, straight after M5 (Question 3), which revisits the default.
   - **`--gamemode` stays,** for `--data-dir` runs that skip the world list. As in M4, a host the
     world has saved keeps its own mode.

7. **Blocks M4 drew as stand-in cubes are drawn as Pumpkin's own boxes** (Question 4; worth
   teaching).
   - **Why now:** physics collides with each block's real shape. A stair drawn as a cube but
     collided as a stair, or an open door drawn as a cube that you walk through, would look broken.
   - **The shape:** a new `Shape::Boxes`, built from the outline boxes of the block's Java state in
     `pumpkin-data` (Finding 9).
     - Each box face shows the block's texture for that face, cut by its position, as slabs already
       are (`mesher.rs:301-313`). Java's own models texture stairs, fences, torches and ladders the
       same way.
     - Boxes stay inside their block, so the vertex format holds them.
   - **Connections come with the block:** Bedrock's fence, pane and bar states carry their
     connections, and stairs their corners (Finding 10). Pumpkin's Java logic sets them, so the
     client draws and collides with what it's sent, and never works a connection out.
   - **One Bedrock id, one set of boxes, checked** (owner, at approval): several Java states can
     share a Bedrock id (M3's table keeps the lowest). A test walks every Bedrock id and checks that
     all the Java states sharing it have the same collision and outline boxes. It lists any that
     don't, so each is looked at before the shapes rely on them (Step 7).
   - **Still approximate, and counted:** rails are flat, beds and chests are boxes, and dripstone is
     a column. The stand-in census becomes a census of approximate shapes, and STATUS.md's "Special
     shapes to do" stays the to-do list.

8. **Aiming: a voxel ray against the outline boxes** (worth teaching).
   - **The ray** steps from block to block along the look direction (Amanatides and Woo's
     traversal), and tests each block's outline boxes. It yields the block, the face and the exact
     point hit, which the place packet needs.
   - **The reach is Pumpkin's:** 4.5 blocks in survival and 5 in creative
     (`entity/player.rs:4234-4240`). The server allows one block more (`:4242-4250`), so an edit in
     reach is never refused for distance.
   - **The outline:** the aimed block's outline boxes as thin dark lines, as on Bedrock. Line lists
     are core WebGPU.

9. **Breaking and placing are predicted; the server has the last word** (worth teaching; Findings 2
   to 4).
   - **Why predict:** the server handles input at the start of its next tick, and sends block
     changes at the end of one (Finding 4). An unpredicted edit would show 50 to 100 ms late, three
     to six frames at 60 Hz.
   - **Breaking:**
     - creative breaks at once, and again every few ticks while held (the capture shows the
       interval);
     - survival takes the server's time. Progress per tick is the tool's mining speed ÷ hardness ÷
       30, or ÷ 100 if the tool can't harvest the block (`block/mod.rs:528-542`), with the hardness
       and the tool rules from `pumpkin-data`. A crack overlay, from the install's
       `destroy_stage_0` to `_9`, shows the progress;
     - the packets are the ones the official client sends: block actions in `PlayerAuthInput`
       (start, continue each tick, predict at the end, abort). The server accepts a predicted break
       from 65% progress (`play/mod.rs:80`).
   - **Placing:** a right click sends `InventoryTransaction` with a use-item-on-block, as captured.
     For its prediction, the client works out the server's target: the face's neighbour, or the
     aimed block itself when it's replaceable, like grass.
   - **What's predicted:**
     - every break, to air, or to water where the block was waterlogged;
     - a place only when its result is certain: the item's block has a single state, like dirt,
       stone or planks (Question 11). Logs, stairs and furnaces depend on how they're placed, so
       those wait the one or two ticks for the server.
   - **Reconciling** (worth teaching): each predicted position goes into a table, with a deadline of
     half a second for now.
     - A server `UpdateBlock` for it is noted, not drawn. Some updates in flight are older than the
       prediction, and drawing them would flicker.
     - When the server's state equals the prediction, the entry goes.
     - At the deadline, the server's last state is drawn, whatever it is. A refused edit snaps back.
   - **An urgent lane:** the edited sub-chunk, and the one across a border it's on, mesh in the same
     frame, ahead of the budgeted queue. Today the queue is only re-sorted on a few triggers
     (`meshes.rs:239-255`), so during a join an edit could wait 100 ms. Sub-chunks whose light the
     edit changed follow through the normal queue.
   - **Measured:** click to drawn, and click to confirmed by the server, logged and summarised at
     the end of a session.

10. **Items and the inventory mirror the server** (Findings 5 and 8).
    - **Item names** come from the join's `ItemRegistry`, and the creative list from
      `CreativeContent`.
    - **Item to block:** item stacks don't say which block an item places (Finding 8), so the client
      goes from item to Java item, to Java block, to its default state, to its Bedrock id, all in
      `pumpkin-data`.
    - **Containers** come from `InventoryContent` and `InventorySlot`, with the server's stack ids,
      and the selected slot from `PlayerHotbar`.
    - **Changes are item stack requests,** as the official client sends: creative takes from the
      list, and survival takes, places and swaps between slots. The client applies its own request
      at once; the response confirms it, or undoes it.
    - **The hotbar:** 1 to 9 and the wheel select (`MobEquipment`), a middle click picks the aimed
      block (`BlockPickRequest`), and Q drops.
    - **Icons:**
      - block items are small three-face blocks, drawn from the terrain's texture array. Plants and
        other flat blocks show their texture flat, as on Bedrock;
      - other items use `items/*.json`'s icon, then an `item_texture.json` key of the same name,
        then `textures/items/<name>`, then a small alias table;
      - anything else draws the missing texture. A census at startup lists those items, like M4's
        stand-in census.

11. **Light: the client propagates sky and block light itself** (PLAN.md, Lighting; Question 6;
    worth teaching).
    - **What a block does to light** comes from `pumpkin-data`, through the Bedrock-to-Java table:
      how much it emits, and how much it dims light passing through (Finding 9). A block with water
      on its second layer dims as much as the stronger of the two.
    - **Storage:** per sub-chunk, sky and block light as 4-bit levels. A sub-chunk whose levels are
      all the same, like open sky or solid rock, keeps one value.
    - **The algorithm is vanilla's,** flood fills over queues:
      - block light spreads from emitters, losing at least 1 per block;
      - sky light falls straight down at 15 through clear blocks, then spreads the same way;
      - a change runs a decrease pass first, then refills from the edges. The removal half is the
        part worth teaching.
    - **Arrival and edits:** a new chunk is lit with its loaded neighbours' borders, and lights
      them in turn. A replaced chunk is relit the same way. An edit relights at once, before its
      sub-chunk is meshed.
    - **Scheduling:** on the main thread, nearest first, within 3 ms a frame. Meshing waits for a
      sub-chunk's light, and light changes mark meshes dirty.
      - **Stop rule:** if, in a dev build at radius 8, light keeps terrain unmeshed for more than a
        few seconds after a join's last chunk, or an edit's relight and remesh takes longer than a
        frame, stop and propose a fix, probably a worker thread.
    - **Checked against the server** (Finding 12): Pumpkin runs a vanilla-style light engine and
      keeps each chunk's light. A headless check compares ours with it, level for level, on every
      chunk whose neighbours are loaded, with re-checks like M3's world check.

12. **Drawing light: smooth lighting and ambient occlusion, then the time of day, the sky and fog**
    (Question 5).
    - **Smooth lighting, Bedrock's default:** each vertex takes the average light of the four cells
      around its corner, on the face's open side. Ambient occlusion darkens a corner by how many of
      the three cells around it are solid.
    - **No bigger vertex** (worth teaching): the sky and block levels fill the vertex's spare byte
      (`mesher.rs:62-77`), and the occlusion takes two of the face byte's three free bits. The GPU's
      vertex memory stays where M4 measured it.
    - **Corners need diagonal neighbours:** M4's neighbourhood leaves its edges and corners air
      (`mesher.rs:4-10`). Now it fills them, so a chunk is meshed only once all eight neighbours
      have arrived, and the view's unmeshed outer ring gets its corners too.
    - **A switch:** `smooth_lighting = false` in the settings gives each face the light of the cell
      it faces, as Bedrock's own setting does.
    - **Brightness:** a 16 × 16 light map, sky by block, worked out each frame from the time of day
      with Java's light curve. Bedrock's own curve lives in its engine, so the comparison will
      judge it.
    - **The time of day** comes from `SetTime`. Between packets the client advances it a tick at a
      time, unless the game rules stop it.
    - **The sky** is a colour by the time of day, and **fog** fades the terrain into it, ending just
      inside the meshed area. That hides the view's edge, and the dark band M4 saw over water
      (STATUS.md, Notes for M5). Under water, the fog is denser and blue.

13. **Entities live in `hecs`, are drawn a tick in the past, and are drawn as their boxes**
    (Questions 7 and 8; Finding 11).
    - **`hecs`, not `bevy_ecs`** (PLAN.md open question): a small ECS with no scheduler, which fits a
      hand-written loop. `bevy_ecs` brings systems, schedules and change detection that M5 wouldn't
      use, and much more to compile.
    - **Components:** the runtime id; the Bedrock identifier and its `pumpkin-data` entity type, for
      the box's size; the last server poses with their arrival times; the item, for item entities;
      and the name, for players.
    - **Interpolation** (worth teaching): each entity is drawn where it was one tick ago, plus a
      margin, between the two server poses around that moment. So a frame always has two poses to
      blend.
      - The margin is measured, not guessed: the log reports how far apart updates arrive. It
        starts at 75 ms.
    - **Drawn as boxes,** the stand-ins for M8's models: lit by the light where they stand, and
      coloured by kind (monster, animal, item, player, other). Item entities are small spinning
      icons.
    - **An identifier `pumpkin-data` doesn't know** gets a 1-block box, and the log counts them.

14. **The client's own UI: a retained tree, laid out the JSON UI way** (PLAN.md, UI; worth
    teaching).
    - **Retained:** each screen builds its tree once, and changes properties when the game does,
      such as a slot's item. Layout runs again only when something changed, or the window resized.
    - **Laid out like JSON UI:** anchors, offsets and sizes (pixels, percentages or fill), stack
      panels and grids. Then M10's JSON UI subset maps onto the same tree, and the framework doesn't
      have to change.
    - **Widgets:** panels (nine-sliced images), images, labels, buttons, item slots and grids,
      scroll views and text fields.
    - **From the install, at runtime:**
      - sprites from `textures/ui` and `textures/gui`, with their nine-slice sizes from the JSON
        beside each (Finding 16), packed into one atlas at startup and never written to disk;
      - text in the bitmap font `font/default8.png`, each glyph as wide as its pixels, with a drop
        shadow;
      - strings from `texts/en_US.lang` (PLAN.md: "No hardcoded strings"). The few strings vanilla
        lacks go in the client's own lang file, in the repo.
    - **UI scale:** automatic, the largest whole scale at which a 320 × 240 screen fits, as Java's
      GUI scale does. The settings file can fix it (PLAN.md, Accessibility).
    - **Drawn last,** in its own pass over the frame: textured quads, alpha-blended. Block icons use
      a second pipeline, which samples the terrain's texture array.
    - **Input:** while a screen is open, the cursor is free, and the screen gets the clicks, the
      wheel and the keys. While playing, the cursor is captured.

15. **Screens: the HUD, the inventory, the pause menu, the world list, death, and a command line**
    (Question 12).
    - **The HUD:** crosshair and hotbar, and in survival hearts, hunger, armour and air. The held
      item's name shows above the hotbar when it changes.
    - **The inventory** (E): creative has the creative list's four categories, with scrolling;
      survival has the 27 slots, the hotbar, armour and the off hand. No crafting grid: that's
      milestone 5b (Question 3).
    - **Pause** (Esc, or losing focus): Resume, and Save & Quit.
    - **The world list** (Decision 17).
    - **Death:** "You died!" and Respawn, with the packets the official client sends.
    - **A command line:** / or T opens a text field. Enter sends a command, or chat, and the
      server's replies show above the hotbar for a few seconds. It makes `/tp`, `/time` and
      `/gamemode` usable in our own client.

16. **A home folder holds the settings and a per-install identity** (STATUS.md, Notes for M5).
    - **`--home <dir>`,** `run/home` by default (gitignored), holds:
      - `settings.toml`;
      - `identity.toml`;
      - `worlds/`, with `backups/` and `deleted/` beside it.
    - **The settings file:** the name, the view radius, the field of view, the mouse sensitivity,
      the UI scale, smooth lighting, the first gamemode for new worlds, and the keymap.
      - **TOML,** through the `toml` crate, as Pumpkin's own config is.
      - **Missing fields** are filled in with defaults and written back. Unknown keys and bad
        values are logged and replaced, never a panic.
      - **Saved safely** (worth teaching): written to a temporary file, then renamed over the old
        one, so a crash mid-write can't leave half a file.
      - **Flags** (`--name`, `--radius`, `--gamemode`) override it for one run, without saving.
    - **The keymap** binds keys and mouse buttons by name. M4's defaults change where M5 needs the
      key: Ctrl sprints, Shift sneaks (and sinks when flying), and Esc pauses.
    - **The identity:** a random 16-digit `xid`, made on the first run. It replaces `login::DEV_XID`
      (`login.rs:17-21`).
      - The headless run uses it too, so a dev world's saved host becomes a new player once. That's
        the cost STATUS.md already warns of.

17. **Worlds: one folder each, a world list, and Save & Quit by starting afresh** (Question 10;
    Finding 13).
    - **A world is a folder,** `<home>/worlds/<id>/`, which is the embedded server's working
      directory for it: its `world/`, its `data/ops.json`, and so on. The id is the name made
      folder-safe, numbered if taken.
    - **`limonite-world.toml`** beside them holds the display name, the gamemode, the seed, and the
      dates created and last played. It's the client's own file.
    - **The list,** at startup, most recently played first: Play, Create, Copy and Delete.
      - **Create:** a name, survival or creative (Decision 6), and a seed, random if left empty.
        The server gets them as its seed and default gamemode.
      - **Copy:** to `<home>/backups/<id>-<time>/`, while the world isn't running.
      - **Delete:** after a confirmation, the folder moves to `<home>/deleted/`. Emptying that stays
        manual while the client is young.
    - **`--data-dir <dir>` skips the list** and opens that folder, as today, for the bench, the
      comparisons and the headless runs.
    - **Save & Quit:** the host leaves, and the server stops and saves, as closing the window does
      now. Then the process exits, and a new one opens on the world list.
      - **Why a new process:** Pumpkin's stop flags are process globals, and its stop token can't be
        reset once it has fired (Finding 13). So one process runs one server.
      - **How:** without `--data-dir` or `--headless`, the first process is a small supervisor. It
        runs the client as a child, and starts a new child each time one leaves for the world list.
        It never opens a window or a server itself, so a terminal or `cargo run` stays attached.
    - **When a child fails** (owner, at approval):
      - **A crash:** the supervisor starts a new child on the world list, with an error that says
        the game stopped unexpectedly, gives the exit status, and points to the log.
      - **A death at startup,** before the child's window is up: one is treated as a crash. Twice
        in a row, the supervisor stops with a message saying the client can't start, rather than
        loop.
      - **How it knows:** the child exits with one code for "back to the world list" and 0 for a
        normal quit. It tells the supervisor once its window is up, so a failure before that counts
        as a death at startup.

18. **The host is an operator, and the embedded server has no spawn protection** (PLAN.md open
    question; Question 9; Findings 3 and 7).
    - **Operator:** before the server starts, the world's `data/ops.json` gets the host at level 4,
      if it isn't there already. That's a world owner's power on Bedrock with cheats on: every
      command, and the abilities packet then calls the host an operator.
    - **The host's UUID** comes from pumpkin-auth's own claims parser, reading the token we send, as
      `login.rs`'s test already does. So it can't drift from the server's.
    - **The entry** uses `pumpkin-config`'s own `Op` type, so the file's format follows Pumpkin's.
    - **Spawn protection off** (`spawn_protection = 0` in `embedded::config`): Bedrock worlds have
      none, and Pumpkin's default of 16 blocks refuses a non-operator's building around spawn.
    - **StartGame's hard-coded 2 stays.** Our client reads the abilities instead. For M7's guests,
      StartGame would call everyone an operator, so that's a note for M7, and an upstream
      candidate.

19. **The fork changes only by client codecs, specified by a new capture** (Findings 14 and 15).
    - **The capture** (Step 1) repeats M2's: a temporary hook, never committed, records what the
      official client sends while the owner plays a short script. It's the spec for every packet
      the client sends.
    - **Readers** for about 25 clientbound packets: player state, entities and inventory. Most are a
      one-line `cfg_attr` derive beside the struct; about six need hand-written mirrors.
    - **Writers** for about a dozen serverbound packets. Two need hand-written mirrors, and the
      `PlayerAuthInput` writer gains its actions.
    - **Tests:** sent packets byte for byte against the capture; received ones by a round trip
      through Pumpkin's writer, with every field away from its default.
    - **Captures that identify the owner:** text and command packets carry the player's Xbox id or
      UUID. Like `SLogin`, they stay in `run/captures/`, with `#[ignore]`d golden tests; the
      committed tests use made-up values.
    - **No seam, and no game code changes.** Every change is in `pumpkin-protocol`, behind `client`.

20. **The M6 baseline stays M4's, and M5 records its own run beside it** (Question 14).
    - The bench flies the same path from the same template (`run/m4-template`).
    - But M5's frames also light, fog, and draw entities and the HUD. So Step 17 runs the bench
      again at radius 8, 16 and 32, and STATUS.md's "Notes for M6" gets both columns, for a
      like-for-like comparison.

21. **Capabilities and assets, as before.**
    - **The GPU:** still core WebGPU only: line lists, a 2D texture for the UI atlas, and alpha
      blending. No optional features.
    - **Assets** (CLAUDE.md; m4-plan Decision 15): sprites, the font, the strings and the item
      textures are read at runtime from the install, never copied into the repo. Screenshots stay
      in gitignored folders. The client's own lang file is ours.

22. **Dependencies** (Question 13). Checked on crates.io on 2026-10-24.
    - **New:** `hecs` 0.11.2, MIT or Apache-2.0, MSRV 1.81.
    - **Already in the lockfile through Pumpkin,** taken with Pumpkin's own specs: `toml` 1.1 (the
      settings), `rand` 0.10 (the identity) and `uuid` 1.28 (the operator entry).
    - **Not now:** `gilrs`, so gamepads wait (PLAN.md, Input; Question 15), and `egui`.
    - Each crate arrives with the first code that uses it.

**Fork changes in M5:** `pumpkin-protocol` only, behind `client`: a few dozen one-line derives
beside existing structs, about eight hand-written mirrors in `bedrock/client_codecs/`, and new
fixtures with their provenance. No seam, and no game code.

## Findings while planning (fork at `443b954`, client at `4072ce8`, Bedrock 1.26.52 installed)

Fork paths are under `pumpkin/crates/`, and client paths under `client/src/`.

1. **Pumpkin trusts a Bedrock player's movement.** `handle_player_auth_input`
   (`pumpkin-core/src/net/bedrock/play/player_auth_input.rs`):
   - ignores input until the client has loaded, and while the player is dead (`:13-20`);
   - takes on-ground from `VerticalCollision` with a negative `delta.y` (`:22-25`);
   - sets the position to the reported eye position minus the eye height, unchecked (`:27-47`);
   - updates sprinting, sneaking, crawling and flying from the start and stop flags (`:154-221`);
   - handles `block_actions`, and ignores the item interaction and item stack request that the
     same packet can carry (`:223-227`).
   - No code in `pumpkin-core` sends `CorrectPlayerMovePrediction`. A teleport sends `MovePlayer` in
     teleport mode (`entity/player.rs:4163`, Bedrock branch `:4209-4231`).
2. **Breaking** (`pumpkin-core/src/net/bedrock/play/player_action.rs`):
   - creative breaks on start (`:45-55`);
   - survival tracks progress with `calc_block_breaking` (`:68`; the formula at
     `pumpkin-core/src/block/mod.rs:528-542`);
   - a predicted break is accepted from 65% progress (`MIN_PREDICTED_BREAK_PROGRESS`,
     `play/mod.rs:80`). Otherwise the server sends `UpdateBlock` with the real state
     (`player_action.rs:201-203`);
   - abort stops mining (`:227-229`), and respawning is a player action too (`:233-241`).
   - Block actions inside `PlayerAuthInput` go through the same handler
     (`player_block_action.rs:5-22`).
3. **Placing** is `InventoryTransaction`'s use-item, click-block
   (`pumpkin-core/src/net/bedrock/play/inventory_action.rs:172-306`).
   - It tries the block's own use first, then the item's, then places (`:280-291`).
   - If the client's `item_in_hand` differs from the server's held item, the client's is used
     (`:202-209`).
   - **Spawn protection:** placing within 16 blocks of spawn is refused for players below level 4
     (`pumpkin-core/src/block/registry.rs:725-735`, `world/mod.rs:561-580`; the default of 16 at
     `pumpkin-config/src/lib.rs:225,248`). So today's host, with no operator entry, can't build
     near spawn.
4. **When the server answers.** It handles a player's packets at the start of that player's tick
   (`pumpkin-core/src/entity/player.rs:2717-2718`), and sends block changes once per world tick
   (`world/mod.rs:1528`, `:1719`). So an edit's `UpdateBlock` arrives one to two ticks after the
   click.
5. **The inventory.**
   - Item stack requests: creative take (`pumpkin-core/src/net/bedrock/play/item_stack_request.rs:37-76`),
     take and place (`:77-219`), swap (`:220-241`), drop (`:242-279`), destroy and consume
     (`:280-343`), and crafting (`:344-448`).
   - The hotbar slot is `MobEquipment` (`mob_equipment.rs:12-32`). Opening the inventory is
     `Interact`'s open-inventory, answered with `ContainerOpen` (`interaction.rs:5-18`).
   - The server syncs containers with `InventoryContent` and `InventorySlot`
     (`entity/player.rs:7726-7900`), and the selected slot with `PlayerHotbar` (`:7921-7930`).
6. **What a Bedrock player gets at join** (`pumpkin-core/src/world/mod.rs:2659-3147`):
   - `StartGame`, with server-authoritative block breaking on (`:2809`), and `permission_level: 2`
     behind a TODO (`:2754-2755`);
   - `ItemRegistry` (`:2843-2861`), `CreativeContent` (`:2863-2922`) and `CraftingData`;
   - the abilities (`:2934-2957`), and attributes: movement 0.1, gravity 0.08, air, health and
     hunger (`:2964-3031`).
   - Later, health and hunger come as `UpdateAttributes` (`entity/player.rs:4429-4460`), and the
     time as `SetTime` (`world/time.rs:120-131`, `:145-158`).
7. **Permission and identity.**
   - A player's level comes from `ops.json`, else from `default_op_level`
     (`pumpkin-core/src/entity/player.rs:790-799`; `data/op.rs:15-26`; the entry type at
     `pumpkin-config/src/op.rs:9-21`).
   - The abilities packet makes level 4 an operator, and everyone else a member
     (`entity/player.rs:3270-3284`).
   - A Bedrock player's UUID is an MD5 of `pocket-auth-1-xuid:<xid>`
     (`pumpkin-auth/src/jwt/mod.rs:500-533`).
8. **Items don't say which block they place.**
   - A network item stack takes its `block_runtime_id` from the mapping's `bedrock_block_state`
     (`pumpkin-protocol/src/bedrock/network_item.rs:348-370`), which the codegen always sets to 0
     (`pumpkin/tools/pumpkin-codegen/src/item.rs:1827`). Creative entries send 0 too
     (`pumpkin-core/src/world/mod.rs:2898-2915`).
   - `pumpkin-data` has the whole path: `JavaToBedrockItemMapping::from_bedrock`
     (`pumpkin-data/src/generated/item.rs:122944`), then `Block::from_item_id`
     (`generated/block.rs:598407`).
9. **`pumpkin-data` has what physics, light and breaking need:**
   - per Java state: emitted light, light opacity, hardness, and collision and outline boxes
     (`pumpkin-data/src/block_state.rs:14-38`, accessors `:207-245`; the boxes at
     `generated/block.rs:2287`);
   - per block: hardness, slipperiness, and the speed and jump multipliers (`blocks.rs:19-40`);
   - tools: `ToolImpl` and its rules (`data_component_impl/combat.rs:389-413`).
   - The client reaches them through its Bedrock-to-Java table (`client/src/world/mod.rs:277-353`),
     where the lowest Java state wins.
10. **Bedrock's block states carry connections and corners.** In the fork's palette
    (`pumpkin/assets/bedrock/block_states.nbt`):
    - `oak_fence`, `glass_pane` and `iron_bars` have 16 states each, keyed
      `minecraft:connection_east`, `_north`, `_south` and `_west`;
    - `oak_stairs` has 40, with `minecraft:corner`, `upside_down_bit` and `weirdo_direction`;
    - `cobblestone_wall` has 162, with `wall_connection_type_*`.
    - So a fence's arms and a stair's corner arrive in its id. The Bedrock-to-Java table should give
      back a Java state with the same arms and corner, so the same boxes; Step 7 tests it.
11. **Entities.**
    - `AddActor` carries the Bedrock identifier, such as `minecraft:cow`
      (`pumpkin-core/src/entity/mod.rs:405-441`). Players come as `AddPlayer`
      (`entity/player.rs:1125-1155`).
    - Every tick, `MoveActorDelta` sends each changed axis as an absolute `f32`, and the angles
      packed in bytes (`world/entity_tracker.rs:296-363`).
    - `SetActorMotion` follows velocity changes (`:247-262`), and `RemoveActor` removals
      (`:592-637`).
    - `pumpkin-data`'s `EntityType` has the width, height and eye height
      (`pumpkin-data/src/generated/entity_type.rs:12-34`), and `from_name` (`:7544`).
12. **The server has its own light.**
    - `pumpkin-world` runs a vanilla-style light engine: initial light during generation
      (`pumpkin-world/src/lighting/engine.rs:771-789`), and updates as blocks change
      (`lighting/runtime.rs`). Its default mode is full propagation (`pumpkin-config/src/lighting.rs`).
    - Each `ChunkData` keeps sky and block light per section (`pumpkin-world/src/chunk/mod.rs:78`,
      `:153-155`; `LightContainer::get` at `chunk/format/mod.rs:1377`). How those sections line up
      with the block sections is for Step 13 to check.
13. **Saving, and one server per process.**
    - Pumpkin autosaves every 6,000 ticks, five minutes (`pumpkin-config/src/world.rs:15-30`), and
      saves everything at shutdown (`pumpkin-core/src/lib.rs:520-553`). `Server::save_all` saves
      on demand (`server/mod.rs:585-614`).
    - `SHOULD_STOP`, `SERVER_IS_STOPPING` and `STOP_INTERRUPT` are process globals (`lib.rs:212-221`).
      The last is a `LazyLock<CancellationToken>`, which can't be reset once cancelled.
    - The world folder is `default_level_name` under the working directory
      (`pumpkin-config/src/lib.rs:212,243,255-258`), and the seed is `basic.seed` (`:187-188,231`).
14. **The client today.**
    - Ticks are counted apart from frames, and send the camera's pose (`app.rs:186-205`). The camera
      moves per frame (`app.rs:288-299`, `render/camera.rs:89-101`).
    - `Session` handles nine packet types (`session.rs:377-421`). Its input reports the camera's
      moves with fixed flags and no movement vector (`:511-557`).
    - The vertex has a spare byte, and three spare bits in the face byte (`render/mesher.rs:62-77`).
      The neighbourhood's edges and corners stay air (`:4-10`), and only side neighbours are
      required (`:160-172`).
    - Meshing re-sorts its queue only when the queue empties, when the camera changes sub-chunk, or
      100 ms after new work arrived (`render/meshes.rs:239-255`).
    - The keymap binds keys only (`input.rs:28-49`), `Settings` holds only the gamemode
      (`settings.rs:6-26`), and the identity is fixed (`login.rs:17-21`).
15. **The client codecs today** (`pumpkin-protocol/src/bedrock/client_codecs/`):
    - eight modules of hand-written mirrors (`mod.rs:18-25`). The `PlayerAuthInput` writer refuses
      actions (`player_auth_input.rs:17-29`), and the fixtures come from a session without a click
      (`fixtures/README.md`).
    - Of the clientbound packets M5 reads, most derive their writer, so one line adds the reader.
      `MovePlayer`, `MoveActorDelta`, `CreativeContent`, `ItemStackResponse`, and the actor
      metadata and synced properties that `AddActor` and `SetActorData` carry, are hand-written, so
      they need mirrors. Item stacks already read both ways (`network_item.rs:289-346`).
    - Of the serverbound packets, `InventoryTransaction` and `ItemStackRequest` have hand-written
      readers, so they need hand-written writers. The rest derive.
16. **The install has the HUD's and the menus' assets** (read at runtime; Bedrock 1.26.52):
    - `textures/ui`: `hotbar_0` to `_8` with start and end caps, `selected_hotbar_slot`, hearts,
      hunger, armour and air bubbles, and buttons and scroll bars. Nine-sliced sprites have a JSON
      beside them, such as `{"nineslice_size":1,"base_size":[4,4]}`;
    - `textures/gui/icons.png`, with the crosshair, and `textures/environment/destroy_stage_0` to
      `_9`;
    - `font/default8.png` and the `glyph_XX` pages;
    - `texts/en_US.lang`, loose, with the strings the menus need: `selectWorld.title`,
      `selectWorld.create`, `selectWorld.delete`, `menu.quit` ("Save & Quit"), `menu.returnToGame`,
      `container.inventory`, and `gameMode.creative` and `.survival`;
    - `textures/item_texture.json`, whose keys often hold variants (`axe`: wood to diamond). Only 44
      items have an `items/*.json` with an icon; the rest are mapped inside the engine. So item
      icons need a lookup order and a census (Decision 10).

## Step 1: Capture the official client's play packets (temporary, never committed; owner present)

1. **A temporary hook,** as in M2 Step 1: in `BedrockClient::process_batch`
   (`pumpkin-core/src/net/bedrock/mod.rs:616-645`), just before `handle_game_packet`. It writes every
   packet but the login (id 1) to `captures/<id>-<seq>.bin`, up to 5,000 per id.
2. **The capture server:** the stock fork at `443b954` plus the hook, built with STATUS.md's release
   command. It runs from `run/m5-capture/`, with M4's `pumpkin.toml` and a `data/ops.json` holding
   the owner at level 4.
3. **The owner plays a script** in Bedrock 1.26.52 on Fancy graphics, in about 30 minutes:
   1. in survival: stand, walk, sprint, sneak, jump, sprint-jump, and drop down three blocks;
   2. **the clean runs for the physics check** (Decision 5), on flat ground, each started from
      standing still and facing one way: walk straight for 5 s, sprint straight for 5 s, sneak
      straight for 5 s, and three standing jumps. Then build a 10-block pillar in creative,
      `/gamemode survival` at its top, and step off;
   3. `/give @s dirt 16`, `/give @s oak_planks 16` and `/give @s torch 4`;
   4. break dirt by hand to the end; start on stone and let go early;
   5. place dirt on the ground and against a wall, and a torch;
   6. press 1, 2 and 3, then scroll the wheel; drop one item with Q;
   7. open the inventory, move a stack, swap two, and close it;
   8. `/gamemode creative`; double-tap jump to fly, then rise, sink and land;
   9. break blocks, with one click and then held; take stone from the creative list into the
      hotbar; pick grass with a middle click; place blocks;
   10. `/gamemode survival`, `/kill`, and respawn;
   11. send a chat message, then `/time set noon`, and quit.
4. **Revert the hook** with `git -C pumpkin checkout -- crates/pumpkin-core/src/net/bedrock/mod.rs`.
   Confirm `git -C pumpkin status` is clean, then rebuild the stock server.
5. **Choose the fixtures,** one or two per packet and action, into `client_codecs/fixtures/`, and
   extend its README's provenance and table.
   - Text and command packets stay in `run/captures/m5-official/` (Decision 19).
   - **The physics traces:** the `PlayerAuthInput` packets of each clean run, in order, go into the
     client's own fixtures (`client/src/player/fixtures/`), with a README of their provenance and
     what each run shows. They carry no identity.
6. **Record in Results** which packets and flags the official client sends for each action. That's
   the spec for Steps 6, 9, 11 and 12.

## Step 2: Client codecs for play (fork, `pumpkin-protocol`)

1. **Player state, read:** `MovePlayer` (a mirror), `SetActorMotion`, `CorrectPlayerMovePrediction`,
   `UpdateAbilities`, `UpdateAttributes`, `SetPlayerGameType`, `SetHealth` and `SetTime`. `Respawn`
   already reads and writes.
   - **Commit (fork):** "Read player-state packets on the client side".
2. **Entities, read:** `AddActor`, `AddPlayer`, `AddItemActor`, `RemoveActor`, `MoveActorDelta` (a
   mirror), `MoveActorAbsolute`, `SetActorData` with its metadata (mirrors) and `TakeItemActor`.
   - **Commit (fork):** "Read entity packets on the client side".
3. **Inventory, read:** `ItemRegistry`, `CreativeContent` (a mirror), `InventoryContent`,
   `InventorySlot`, `PlayerHotbar`, `ItemStackResponse` (a mirror), `ContainerOpen` and
   `ContainerClose`.
   - **Commit (fork):** "Read inventory packets on the client side".
4. **The player's actions, written:** `PlayerAuthInput` with its block actions (and its item
   actions, if the capture has them), `InventoryTransaction` and `ItemStackRequest` (mirrors),
   `MobEquipment`, `Interact`, `PlayerAction`, `Animate`, `BlockPickRequest`, `CommandRequest` and
   `Text`.
   - The refusal test becomes a golden test of a captured block action.
   - **Commit (fork):** "Write the player's actions on the client side".
5. **Tests:** each written packet byte for byte against its fixture; each read packet by a round
   trip through Pumpkin's writer, with every field away from its default; the private fixtures
   through an environment variable, like `SLogin`'s.
6. **Fork checks, in `pumpkin/`:**
   - `cargo clippy -p pumpkin-protocol --all-targets`, plain and with `--features client`;
   - `cargo clippy -p pumpkin-core --all-targets --features pumpkin-protocol/client`;
   - `cargo test -p pumpkin-protocol --features client`.

## Step 3: The bump (superproject)

- **Commit:** "Bump pumpkin to the play codecs". No dependency changes yet.

## Step 4: The home folder, settings, identity, and the host as operator (client)

- **`home.rs`:** `--home` and its layout (Decision 16).
- **`settings.rs`:** the file, its fill-in and validation, the safe save, and the flags that
  override it.
- **`identity.rs`:** the per-install `xid`. `login::host_login` takes it, and `DEV_XID` goes.
- **`input.rs`:** bindings for keys, mouse buttons and the wheel, saved by name. The new actions are
  attack, use, pick, hotbar 1 to 9, inventory, drop, sneak, sprint, jump, pause and command.
- **`embedded.rs`:** spawn protection off; and before the server starts, the host's operator entry
  (Decision 18).
- **Tests:** the settings round trip, fill-in and bad values; the identity is stable across loads;
  the operator entry is added once and then kept; the host's UUID equals the one the server
  computes.
- **Commits:** "Keep settings and a per-install identity in a home folder"; "Make the host an
  operator of its worlds".

## Step 5: One `Game`, stepped at 20 Hz (client)

- **`game.rs`** (Decision 2), with `player/mod.rs` (the local player: runtime id, gamemode,
  abilities, attributes, health, hunger and air, and the last two ticks' poses) and `clock.rs` (the
  tick clock, alpha, and the time of day).
- **`Session`** hands every packet to `Game::apply`. That now applies `MovePlayer`, `SetActorMotion`,
  `CorrectPlayerMovePrediction`, `UpdateAbilities`, `SetPlayerGameType`, `UpdateAttributes`,
  `SetHealth`, `SetTime` and `GameRulesChanged`.
- **`app.rs`:** M4's flying moves the player each tick instead of the camera each frame. The camera
  draws it interpolated, and mouse look stays per frame (Decision 3). The bench keeps M4's
  per-frame flight.
- **A sixth headless check, "teleport":** after the walk, the check asks the server to move the
  host 5 blocks up (`Player::request_teleport`). The client must be there within a second, and
  report it in its next input.
- **Tests:** interpolation and alpha; the catch-up after a stall; each packet's effect.
- **Commit:** "Hold the client's picture in one Game, stepped at 20 Hz".

## Step 6: Walking, flying and the honest input (client)

- **`player/physics.rs`:** Decision 5's rules, as pure functions over a block lookup.
  **`player/movement.rs`:** the modes, and the double tap.
- **`PlayerAuthInput`** comes from the player's state: the flags, `delta`, `move_vec`, `raw_move`
  and the angles, as Step 1 recorded them for each situation.
- **Tests:**
  - a fall onto stone lands and reports `VerticalCollision`; a wall stops the player and reports
    `HorizontalCollision`;
  - a slab is stepped onto and a full block isn't; a jump peaks about 1.25 blocks up;
  - sprinting is 1.3 times walking; sneaking stops at an edge; ice slides;
  - water sinks, and jumping swims up; a ladder climbs;
  - a double tap toggles flight in creative only; a spectator passes through stone;
  - **the official client's traces** (Decision 5): each clean run from Step 1 replayed, compared
    tick by tick. The test reports the largest difference for each run, and fails beyond 0.001
    blocks. Results records the numbers, and Deviations any rule changed to match Bedrock.
- **Commits:** "Walk with gravity and collision"; "Fly in creative, and pass through blocks as a
  spectator".

## Step 7: Pumpkin's shapes for the stand-in cubes (client)

- **`blocks/shapes.rs`:** collision and outline boxes per Bedrock id, from `pumpkin-data` through
  the Bedrock-to-Java table. Physics, aiming and the mesher all read it.
- **`blocks/appearance.rs` and `render/mesher.rs`:** `Shape::Boxes` for every stand-in
  (Decision 7). A box face on the block's edge is hidden by an opaque neighbour, as a cube's face
  is; the others are always drawn.
- **The census** counts the blocks drawn as approximate boxes, by name.
- **Tests:**
  - a stair is two boxes, and an inner corner three; a fence with `connection_east` has an arm to
    the east; an open and a closed door; a torch; a ladder; the census;
  - **every Bedrock id** (owner, at approval): all the Java states that share it have the same
    collision and outline boxes. The test lists every id where they don't, with the states and
    their boxes, and fails. Each one is looked at with the owner; one that's explained (say, boxes
    that differ only where water sits) goes on the test's list of known exceptions, with its
    reason, so a new one still fails.
- **Owner checkpoint:** walk a fresh world in survival and in creative, over stairs and fences and
  through doors (about 10 minutes).
- **Commit:** "Draw Pumpkin's block shapes where M4 drew stand-in cubes".

## Step 8: The UI, and the HUD (client)

- **`ui/`:** `atlas.rs` (sprites and nine-slices), `font.rs`, `lang.rs`, `tree.rs` and `layout.rs`,
  `render.rs` with `ui.wgsl`, and `input.rs` (the pointer and focus) (Decision 14).
- **`ui/screens/hud.rs`:** the crosshair and hotbar, hearts, hunger, armour and air
  (Decision 15). The slots stay empty until Step 9.
- **Startup log:** the sprites, glyph pages and strings loaded, and what's missing.
- **Tests:** anchors, stacks and grids; the nine-slice maths; glyph widths from a synthetic sheet;
  lang parsing (comments, tabs, missing keys); hit testing; the shader parses and validates (naga,
  as in M4).
- **Commits:** "Add the client's own UI: sprites, text, strings and layout"; "Draw the HUD".

## Step 9: Items, icons and the hotbar (client)

- **`inventory/items.rs`:** the item registry, item to block, and icon resolution, with its census
  (Decision 10). **`render/icons.rs`:** three-face block icons from the terrain's texture array.
- **`inventory/mod.rs`:** containers and the selected slot from the server; selecting (1 to 9, the
  wheel), picking (a middle click) and dropping (Q), with Step 1's packets.
- **The HUD's hotbar** shows icons and counts, and the held item's name from the lang file.
- **Tests:** item to block for dirt, an oak log, a poppy and a torch; the icon lookup order; the
  census; the wheel wraps around the hotbar.
- **Commits:** "Know the server's items, and draw their icons"; "Select, pick and drop from the
  hotbar".

## Step 10: Aim at blocks (client)

- **`edit/aim.rs`:** the voxel ray and the reach (Decision 8). **`render/outline.rs`:** the outline,
  as lines after the opaque pass.
- **Tests:** the near face is hit; air and liquids are passed through; a slab is hit at its top,
  half a block up; a fence post's box; nothing beyond the reach.
- **Commit:** "Aim at blocks, and outline the one in reach".

## Step 11: Break and place, predicted (client)

- **`edit/mod.rs`:** the break and place state machine, with its packets as captured (Decision 9).
  **`edit/predict.rs`:** the prediction table. **`render/meshes.rs`:** the urgent lane. The crack
  overlay draws in the blend pass.
- **The latency summary** at the end of a session (Decision 9).
- **A headless "edits" check** (`--headless --edits <n>`): the scripted host places and breaks n
  blocks near itself with the real packets, and the world check confirms that the server applied
  them. Step 16 adds the reload half.
- **Tests:**
  - the table: confirmed, refused after the deadline, and an older update ignored;
  - survival times: dirt by hand in 15 ticks, stone by hand in 150, and stone with a wooden pickaxe
    in 23;
  - the place target: against a face, and into replaceable grass.
- **Owner checkpoint:** build something in creative and in survival (about 10 minutes).
- **Commits:** "Break blocks, predicted"; "Place blocks, predicted where the result is certain".

## Step 12: The inventory screens (client)

- **`ui/screens/inventory.rs`:** creative and survival (Decision 15), opened and closed with Step 1's
  packets.
- **`inventory/requests.rs`:** an item stack request for each move, applied at once, and confirmed
  or undone by the response (Decision 10).
- **Tests:** each move builds the request the capture shows; a refused request is undone.
- **Commit:** "Open the inventory: creative and survival screens".

## Step 13: Light (client, CPU)

- **`world/light.rs`:** the storage, the flood fills, arrival and edits, and the budget
  (Decision 11). Light changes mark meshes dirty.
- **A "light matches server" headless check** (`checks.rs`, run like the world check), against
  `ChunkData.light_engine` (Finding 12).
- **Tests:** a torch in a closed room (15 at the torch, one less per block); sky light under a roof,
  and through a hole in it; a torch removed (the decrease pass); water dims; light crosses chunk
  borders; open sky stays one value per sub-chunk.
- **The stop rule** (Decision 11), with numbers from a fresh copy of `run/m4-dev` at radius 8, in a
  dev build and in release.
- **Commits:** "Compute sky and block light on the client"; "Check the client's light against the
  server's".

## Step 14: Draw the light (client)

- **`render/mesher.rs`:** smooth light and occlusion per vertex, packed into the spare bits, and the
  full neighbourhood (Decision 12).
- **`render/terrain.wgsl` and `render/sky.rs`:** the light map, the time of day, the sky's colour
  and fog.
- **Tests:** occlusion in a corner, along an edge and in the open; smooth-light averages; the vertex
  packing round trip; daylight at noon, sunset and midnight; the shader validates.
- **Owner checkpoint:** frames at noon, at sunset and at night, and a torch-lit cave; the water's
  dark band is gone (about 5 minutes).
- **Commits:** "Draw the terrain lit, with smooth lighting"; "Follow the time of day, with the sky's
  colour and fog".

## Step 15: Entities (client)

- **`entities/mod.rs`:** the `hecs` world, its packets, and interpolation (Decision 13).
  **`render/entities.rs`:** the boxes and the item icons.
- **Logged:** how far apart entity updates arrive, and the unknown identifiers.
- **Tests:** a pose halfway between two updates; the delay buffer; removal; an unknown identifier.
- **Commits:** "Track entities with hecs, and interpolate their motion"; "Draw entities as their
  boxes".

## Step 16: Worlds, pausing, death and commands (client)

- **`worlds.rs`** and **`ui/screens/worlds.rs`:** the world list, and create, copy and delete
  (Decision 17).
- **`main.rs`:** the supervisor (Decision 17).
- **`ui/screens/pause.rs`** and **`death.rs`:** pause, Save & Quit, and respawn.
- **`ui/screens/command.rs`:** the command line, and the server's replies.
- **The headless reload check** (`--headless --check-edits`): a second run joins the world of an
  `--edits` run, and checks the edited blocks, and the host's position and inventory.
- **Tests:**
  - folder ids; the metadata round trip; delete moves and copy copies;
  - the supervisor's choice, as a pure function of how the last children ended (Decision 17): back
    to the list; back to the list with an error after a crash; and a stop after two deaths at
    startup in a row, but not after a death at startup that follows a good run;
  - the supervisor's arguments for the next child.
- **Owner checkpoint:** the world list (about 5 minutes).
- **Commits:** "Pick, create and delete worlds from a world list"; "Pause, quit to the world list,
  and respawn"; "Type commands, and show the server's replies".

## Step 17: The M6 bench again (release numbers only)

- `--bench 30 --radius R` for R = 8, 16 and 32, each on a fresh copy of `run/m4-template`, run as M4
  ran it (STATUS.md, "Notes for M6").
- **Record** an M5 column beside M4's baseline, in Results and in STATUS.md.
- **Commit:** "Measure the M6 bench again, with M5's frames".

## Step 18: Verify, accept, tick

1. **Fork:** the Step 2 checks. `git -C pumpkin diff 443b954 --stat` lists only `pumpkin-protocol`
   files.
2. **Client:** `cargo test --workspace` and `cargo clippy --workspace --all-targets`.
   - **No Mojang files** (Decision 21): no `.tga` or `.brarchive` file is tracked, the only tracked
     image is `docs/readme/title.png`, and the only `.lang` file is the client's own.
3. **Headless,** with `--headless`: the five M3 checks, teleport, light matches server and edits;
   then the reload check.
4. **The acceptance session** (owner, about 30 minutes, a dev build and a fresh home):
   - create a world in the list, in survival first; walk, sprint, jump, swim and climb; die, and
     respawn;
   - then in creative: build a small house with stairs, a door, glass and torches, and break parts
     of it;
   - use the inventory in both modes; Save & Quit; reopen the world from the list, and find
     everything as it was left;
   - the latency summary and the frame times go in Results, and the owner judges "no visible input
     lag".
5. **The official-client comparison** (owner, about 15 minutes), with M4's procedure (STATUS.md),
   now at noon, at sunset, at night, and in a torch-lit cave: light, sky, fog and shapes. Ignore the
   differences listed under "Out of scope".
6. **Tick,** committed as "Tick Milestone 5":
   - **PLAN.md:** tick M5; answer the ECS and permission-level questions; the Lighting paragraph;
     gamepads deferred (Input); and check milestone 5b's scope against what M5 built. Milestone 5b
     and the Bedrock trust risk went in at approval;
   - **STATUS.md:** progress, the new dev setup (the home folder, the world list, settings, the
     controls and flags), Gotchas, Notes for M6, and the shapes to-do list;
   - **README.md:** the world list, the controls, the settings file and the flags;
   - **CLAUDE.md:** the next session's starting point; **this file:** Deviations and Results.
7. **Ask the owner** before pushing anything, or opening an upstream PR.

## Owner involvement

- **Approval:** done on 2026-10-24, with five amendments (Status, at the top).
- **Step 1:** the capture session (about 30 minutes). Keep Bedrock at 1.26.52 for the whole
  milestone (STATUS.md, Gotchas, "Exact protocol").
- **Checkpoints:** after Steps 7, 11, 14 and 16 (5 to 10 minutes each). Each is a go/no-go point
  (Decision 1): the owner hears what's done, what's left and how long it took, and decides whether
  to go on or to tick M5 and split the rest off.
- **Steps 18.4 and 18.5:** the acceptance session and the comparison (about 45 minutes).
- **Pushes and upstream PRs:** ask first.

## Verification (summary)

- **Unit tests,** with no GPU and no Mojang files: physics, shapes, aiming, prediction, light,
  interpolation, layout, fonts and strings, settings, the identity, worlds, the supervisor and
  requests.
- **Against the official client:** our physics replays its captured traces tick by tick (Step 6),
  and every Bedrock id's Java states agree on their boxes (Step 7).
- **Golden tests** of every packet the client sends, against the official client (Step 2).
- **Headless, end to end:** M3's five checks, plus teleport, light matches server, edits and
  reload.
- **Measured:** input-to-screen latency, frame times, the cost of light and meshing, and the bench
  (Step 17).
- **The real thing:** the owner's checkpoints, the acceptance session, and the official client at
  the same pose.

## Out of scope

- **Crafting and containers:** crafting tables, chests, furnaces and the other block screens. They
  are milestone 5b, straight after M5 (Question 3).
- **Combat and item use:** attacking entities, eating, bows, buckets and riding.
- **What entities look like** (M8): models, textures, animations and name tags. **Sounds** (M8)
  and **particles** (M10).
- **The sky's objects:** the sun, moon, stars, clouds and weather.
- **An options screen and a rebinding screen:** the settings file instead. **Gamepads** (`gilrs`).
- **Pausing the world** in the pause menu: that's the control API (a PLAN.md open question), with
  M7.
- **Fidelity beyond Pumpkin's data:** Bedrock's own light curve and block shapes, plant offsets,
  per-biome water and animated textures (STATUS.md, Notes for M5).
- **Languages other than `en_US`,** and chat beyond its last few lines.
- **Other dimensions and portals,** and **LAN** (M7).

## Questions and answers (owner, 2026-10-24)

The owner answered all fifteen as recommended, and asked for five changes, folded in above (Status,
at the top). Question 3's answer made PLAN.md's milestone 5b.

The questions as they were asked:

1. **Keep M5 as one milestone in phases** (Decision 1)? Recommended: yes.
   - **The alternative:** split it into 5a (the player, building, light and entities) and 5b (the
     UI, the inventory screens and worlds). But 5a would need a stopgap hotbar.
2. **Should new worlds start creative** (Decision 6)? Recommended: creative first, survival
   offered.
   - **The alternative:** survival first, as Bedrock does.
3. **Where do container screens go?** No milestone in PLAN.md has crafting, chests or furnaces, and
   survival can't get far without them. Recommended: add them to PLAN.md now as their own
   milestone, straight after M5, while M5's UI is fresh.
   - **The alternatives:** fold them into M5, which makes it bigger, or place them later.
4. **Draw the stand-in blocks as Pumpkin's boxes** (Decision 7)? Recommended: yes, so what you see
   is what you collide with.
   - **The alternative:** keep M4's cubes, which then disagree with collision.
5. **Smooth lighting and ambient occlusion now** (Decision 12)? Recommended: yes, as Bedrock's
   default look, with a setting for flat light.
   - **The alternative:** flat light in M5, and smooth light later.
6. **Light on the main thread within a budget, with a stop rule** (Decision 11)? Recommended: yes,
   as M4 did for meshing.
   - **The alternative:** a worker thread from the start.
7. **`hecs` 0.11.2 for the ECS** (Decision 13)? Recommended: yes.
   - **The alternative:** `bevy_ecs` 0.20.
8. **Draw entities as boxes until M8** (Decision 13)? Recommended: yes, so interpolation can be
   seen.
   - **The alternative:** draw none until M8.
9. **The host's permission** (Decision 18): an operator at level 4 through `ops.json`, spawn
   protection off, and StartGame's hard-coded 2 left for M7? Recommended: yes.
   - **The alternative:** a fork seam that makes every local player an operator.
10. **Back to the world list through a fresh process** (Decision 17)? Recommended: yes, with no fork
    change.
    - **The alternative:** a fork seam that resets Pumpkin's stop flags and token, about five files
      and an upstream candidate, so one process can run world after world.
11. **Predict a place only when its result is certain** (Decision 9)? Recommended: yes, measured,
    adding rules for any block family in which the owner sees lag.
    - **The alternative:** client-side placement rules for every block family now.
12. **A minimal command line, with the server's replies** (Decision 15)? Recommended: yes.
    - **The alternative:** leave commands to the official client.
13. **The capture session and the dependencies** (Decisions 19 and 22): about 25 minutes with
    Bedrock 1.26.52, and `hecs` 0.11.2, plus `toml`, `rand` and `uuid` from the lockfile?
    Recommended: yes.
14. **Keep M4's bench and baseline, and add an M5 run beside it** (Decision 20)? Recommended: yes.
15. **Leave gamepads and an options screen for later,** with the settings file meanwhile?
    Recommended: yes.

## Deviations (filled in during execution)

## Results (filled in during execution)
