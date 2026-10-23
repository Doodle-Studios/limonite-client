![title](docs/readme/title.png)

A bedrock edition client that is built entirely in rust from scratch with an inbuilt fork of [PumpkinMC](https://github.com/Pumpkin-MC/Pumpkin) for singleplayer use.

> [!WARNING]
> Limonite is under heavy construction
>
> **Expect small to no functionality**

> [!IMPORTANT]
> Current phase of development is : Milestone 5

> [!NOTE]
> All early development is made possible by using **Claude Opus 5.5** at Extra, Max, or Ultracode effort. Please do not spread hate because of it being **AI generated**

## Trying it

You need Minecraft Bedrock installed on Windows (the usual place is `C:\XboxGames\Minecraft for Windows`). Limonite reads the vanilla textures from your own install while it runs; it never copies or ships them.

```bash
cargo run -p limonite-client
```

A window opens on a singleplayer world, run by the embedded server, and you fly around it as the host. For now it's just the terrain with vanilla textures: no lighting, sky, mobs or HUD yet.

| Control | What it does |
| --- | --- |
| Click | Captures the mouse, so you can look around |
| W, A, S, D | Fly |
| Space / Left Shift | Up / down |
| Left Ctrl (hold) | Fly three times faster |
| Esc | Releases the mouse |
| P | Logs where you are as a `/tp` command |

| Flag | What it does |
| --- | --- |
| `--bedrock <path>` | Where Bedrock is, if it isn't in the usual place: the folder that holds `data/resource_packs` |
| `--data-dir <path>` | Where the world is kept (default `run/client`) |
| `--name <name>` | The host's display name (default `Limonite`) |
| `--gamemode <mode>` | The host's gamemode in a new world: survival, creative (the default), adventure or spectator |
| `--radius <chunks>` | The view distance to ask for, 2 to 32 (default 8) |
| `--bench <seconds>` | Flies east for that long and logs frame times; use a release build |

`--headless` runs the older scripted check instead, with no window and no Bedrock install needed. The client joins as the host, walks a few blocks east, checks that the world it decoded equals the server's own, block for block, and exits. Each check prints one `PASS` or `FAIL` line.

```bash
cargo run -p limonite-client -- --headless --map run/m3/map.png
```

| Flag (with `--headless`) | What it does |
| --- | --- |
| `--map <path.png>` | Writes a top-down map of the loaded chunks: one pixel per block column, north up, Java map colours, the host marked with a red cross |
| `--linger <seconds>` | Stays in the world afterwards |
| `--measure` | Logs what the in-process channel costs, chunk encode and decode split into phases, and how fast a block change reaches the client; only meaningful in a release build |
| `--stream <blocks>` | After joining, flies that many blocks east at creative flying speed and logs how fast new chunks arrive |

The first build needs the Pumpkin submodule, cloned with long paths on Windows. Setup, build times and known issues are in [docs/STATUS.md](docs/STATUS.md).