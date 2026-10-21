![title](docs/readme/title.png)

A bedrock edition client that is built entirely in rust from scratch with an inbuilt fork of [PumpkinMC](https://github.com/Pumpkin-MC/Pumpkin) for singleplayer use.

> [!WARNING]
> Limonite is under heavy construction
>
> **Expect small to no functionality**

> [!IMPORTANT]
> Current phase of development is : Milestone 3

> [!NOTE]
> All early development is made possible by using **Claude Opus 5.5** at Extra, Max, or Ultracode effort. Please do not spread hate because of it being **AI generated**

## Trying it

There's no window yet. The client starts its embedded server, joins it as the host, walks a few blocks east, checks that the world it decoded equals the server's own, block for block, and exits. Each check prints one `PASS` or `FAIL` line.

```bash
cargo run -p limonite-client -- --map run/m3/map.png
```

| Flag | What it does |
| --- | --- |
| `--map <path.png>` | Writes a top-down map of the loaded chunks: one pixel per block column, north up, Java map colours, the host marked with a red cross |
| `--data-dir <path>` | Where the world is kept (default `run/client`) |
| `--name <name>` | The host's display name (default `Limonite`) |
| `--linger <seconds>` | Stays in the world afterwards |
| `--measure` | Logs what the in-process channel costs, chunk encode and decode split into phases, and how fast a block change reaches the client; only meaningful in a release build |
| `--radius <chunks>` | The view distance to ask for, 2 to 32 (default 8) |
| `--stream <blocks>` | After joining, flies that many blocks east at creative flying speed and logs how fast new chunks arrive |

The first build needs the Pumpkin submodule, cloned with long paths on Windows. Setup, build times and known issues are in [docs/STATUS.md](docs/STATUS.md).