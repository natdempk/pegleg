# pegleg

A bit-exact simulator of **Peggle Deluxe** (Steam), a search that finds tool-assisted (TAS) runs with it, and
the glue to play those runs in the real game, check them, and record them.

The simulator reproduces the game exactly: a simulated shot ends in the same place, to the last bit of every
float, as the real one. That's checked against 274 recorded real-game runs on every build. A beam search then tries
millions of shots per level and picks the plan that best fits a goal (fastest clear, highest score, fewest shots,
or clearing every peg). Plans are inputs a person could make: an aim at a window pixel and a click at a normal
moment.

## Runs made with it

| Run | Goal | Result |
|---|---|---|
| [Min-time](tas/runs/min-time/README.md) | Clear each level fastest (speedrun.com Any% IL timing) | 55 levels in 884.90 s, vs 1533.38 s for the records |
| [Ultra Extreme Fever](tas/runs/uef/README.md) | Hit every peg, fastest | 55 levels in 1841.19 s, vs 3086.03 s for the records |
| [Max score](tas/runs/max-score/README.md) | Highest score per level | 235,502,410 points; all 55 Cyberscore level records beaten |
| [Showcase](tas/runs/showcase/README.md) | Fewest shots, then highest score | All 55 levels in 162 shots |

Every run is in `tas/runs/` as plans you can replay in your own copy of the game.

## Quick start

You need Peggle Deluxe in your Steam library, Python 3 and [Rust](https://rustup.rs).

```sh
git clone https://github.com/natdempk/pegleg && cd pegleg
./bootstrap.sh
```

`bootstrap.sh` does four things:

1. It finds the game in your Steam libraries. If it's somewhere else, set `PEGGLE_DIR=/path/to/Peggle Deluxe`.
   If you don't have a local install, set `STEAM_USER=<login>` and it downloads the game with steamcmd.
2. It extracts the level files into `game/`.
3. It builds everything and runs the bit-exactness test.
4. It builds the playback DLL.

No game files are in this repository.

Find a plan, then play it in the real game:

```sh
solver/target/release/peggle-solver 1-1 --chars 3 --seeds 1-8 --out plans/   # fastest 1-1 for Splork
tas/play.sh plans/0_0.txt $(cat plans/0_0.seed) runs/1-1                     # Windows, Steam running
```

The solver works anywhere Rust does. Real-game playback runs on Windows, either from PowerShell through WSL or
from Git Bash on that machine, because the game is started through Steam. See [play/README.md](play/README.md).

## Layout

| Folder | What's in it |
|---|---|
| [`sim/`](sim/README.md) | The simulator (Rust library), `game_check` (compares real runs with the simulator), and the golden test |
| [`solver/`](solver/README.md) | `peggle-solver`: beam search over shots for one level, which writes a playable script |
| [`play/`](play/README.md) | The playback DLL: it plays scripts inside the real game and logs state for checking. Also the Windows runner |
| [`tas/`](tas/README.md) | Glue scripts: the four-category pipeline, `play.sh`, video recording and stitching, and the finished runs |

## What's legitimate about these runs (and what isn't)

- **Inputs are human-makeable.** Each shot is an aim at a window pixel and a click at a frame boundary. Fast
  forward is the game's own right-mouse-button feature.
- **The RNG is seeded.** The playback DLL reseeds the game's random number generator at fixed points from one
  seed per level (recorded with each plan). The seed decides which pegs are orange, purple and green, and how
  Lucky Spins and coin flips land. Without it no run could be reproduced. It's the same thing as a TAS on an
  emulator starting from a fixed seed: these are TAS runs, not human runs.
- **The beginner aim assist stays off.** On 1-1, for the last 6 balls of a fresh profile, the game nudges the
  aim toward a shot its AI prefers, using the aim guide's first hit. The playback DLL makes the aim guide report
  a hit at its first step, so the assist never turns the gun; the simulator and all runs assume that.
- **3D acceleration is off during playback.** With it on, Direct3D switches the CPU's floating-point unit to
  24-bit precision. The game's physics then differs in the last bits and plans drift. The playback script
  turns the option off for the run and restores it afterwards.

## For AI agents

See [AGENTS.md](AGENTS.md).

## License

Public domain: the Unlicense (see [LICENSE](LICENSE)). Peggle Deluxe is a trademark of PopCap Games / Electronic Arts. This project is not
affiliated with them and contains none of their files; you need your own copy of the game.
