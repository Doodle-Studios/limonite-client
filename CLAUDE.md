# Limonite Client

Limonite Client (`limonite-client`) is a Minecraft Bedrock Edition client in Rust, shipped as one binary that embeds a forked
Pumpkin server for singleplayer. The full design lives in `docs/PLAN.md`. Read it before
starting any task, and treat it as the source of truth for architecture decisions.

## Next session: start here

- **Progress and dev setup:** `docs/STATUS.md`. Read it first, and update it whenever a milestone
  step lands.
- **Milestones 0 to 4 are done** (M4 on 2026-10-24). **Milestone 5, playable singleplayer, isn't
  planned yet:** start it in plan mode. STATUS.md's "Notes for M5" and "Special shapes to do",
  and the Deviations in `docs/milestones/m4-plan.md`, are the starting points.
  - Both repos are public. The superproject is `Doodle-Studios/limonite-client` (`main`), and the
    fork is `Doodle-Studios/Pumpkin` (`limonite`, at `443b954` locally). Check STATUS.md for
    what's pushed. Ask the owner before pushing anything.
- **Before touching the fork, read** `docs/recon/m0-pumpkin-bedrock.md`, and the "Deviations"
  sections of `docs/milestones/m1-plan.md` to `m4-plan.md`.

## Architecture at a glance

```
┌──────────────────────── one binary ────────────────────────┐
│ main thread (winit)                                        │
│   Renderer (wgpu)  <──  World state  ──>  UI               │
│                             ▲                              │
│                             │ in-memory channel (bytes)    │
│                             ▼                              │
│ background thread (tokio): forked Pumpkin                  │
│   LocalChannel  <──>  Game core (worldgen, physics, AI)    │
│                        │                  │                │
│                   NetherNet (LAN)     P2P (later)          │
└────────────────────────┼──────────────────┼────────────────┘
                         ▼                  ▼
                     LAN guests        remote peers
```

All three transports implement the `Transport` trait. The host never touches the network.

## How to work on this repo

- Work one milestone at a time, in the order in `docs/PLAN.md`. A milestone is done only
  when its acceptance check passes.
- Start each milestone in plan mode: read the relevant code, propose the approach, wait for
  approval, then build.
- The owner is learning Rust. Teach as you go: explain the why behind each decision and every
  non-obvious piece of code. Don't just hand over finished code.
- Keep the Pumpkin fork diff small. Changes to existing Pumpkin files go only through the
  seams listed in the plan; new code goes in new modules. Each seam gets a doc comment saying
  why it exists.
- Detect GPU capabilities, never vendors. Measure performance, never guess from GPU names.
- Use `tracing` for all logging.
- If the real code contradicts the plan, stop and say so, propose the change, and update
  `docs/PLAN.md` once it's agreed.
- When a milestone or open question is resolved, tick it off in `docs/PLAN.md`.

## Rules

- Licence: GPL-3.0.
- Never bundle Mojang assets or third-party packs. Vanilla assets are only ever read at runtime
  from the user's install: never copied or cached into the repo, and screenshots stay in
  gitignored folders.
- Never connect to public servers or Realms while testing. Embedded server and own LAN only.
- Never attempt to decrypt Marketplace content.
- Always update the readme regarding any info that needs to be shared.