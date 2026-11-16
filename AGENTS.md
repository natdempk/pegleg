# Agent instructions

You are probably here to set up the project, search for runs, or play and record them for a person. Read this
first; the folder READMEs have the details.

## Setup

1. `./bootstrap.sh`. It finds Peggle Deluxe in Steam, extracts the levels into `game/`, builds `sim/` and
   `solver/`, runs the golden test, and builds `play/bin/bass.dll`. Rerunning it is safe.
2. If it can't find the game, ask the person where "Peggle Deluxe" is installed (`PEGGLE_DIR=...`), or for their
   Steam login to download it with steamcmd (`STEAM_USER=...`). steamcmd asks for the password and Steam Guard
   code interactively: have the person run that step themselves.
3. Rust missing: install from https://rustup.rs (ask before installing system packages).

## Where things run

| Task | Where |
|---|---|
| Simulate, search (`peggle-solver`, `tas/pipeline.py screen/refine`), `game_check`, tests | Any OS with Rust |
| Play in the real game (`tas/play.sh`, `pipeline.py play/video`) | Windows with Steam running: from WSL, Git Bash or PowerShell on that machine |

Playback opens a game window on the person's desktop and plays for 10–60 s per level. Tell them before you
start, and don't run two playbacks at once. Steam must be running and logged in; if `play.ps1` says it isn't, ask
the person to start it.

## Common tasks

- **Best plan for one level:** `solver/target/release/peggle-solver LEVEL --objective O --chars ... --seeds 1-16 --out plans/`.
  Then `tas/play.sh plans/S_L.txt $(cat plans/S_L.seed) runs/LEVEL`. See `solver/README.md` for objectives and
  settings. For a category, copy the settings from `tas/pipeline.py`'s `CATEGORIES`.
- **A whole category:** `tas/pipeline.py CATEGORY OUT all`. It takes hours. Run it in the background and log to a
  file (`nohup tas/pipeline.py ... > OUT.log 2>&1 &`). Every phase is resumable.
- **Beat an existing result:** search more seeds (`SEEDS=17-64`) and wider sets for that level. Seed choice
  matters more than beam width. Compare against `tas/runs/<category>/results.tsv`.
- **Replay a finished run:** see `tas/README.md`.

## Checking results

- After playback, `game_check` must print `N/N shots ok` and `real won at shot N`. Then the real game did exactly
  what the plan predicted. The real final score is the last `st5 ... total N` line in `harness.txt`.
- If shots differ, first check the run conditions before suspecting the simulator. 3D acceleration must be off
  (`play.ps1` does it), the profile must be fresh (`play.ps1` does it), and the seed must be the plan's `.seed`.
- Report results to the person in a table (level, master, seed, shots, real total or time, won, exact), and
  keep `results.tsv` files up to date. They're the record.

## Changing the simulator or solver

- The simulator must stay bit-exact. After any change in `sim/`, run
  `cd sim && cargo test --release --test golden` **and** `SIM_NO_FAST=1 cargo test --release --test golden`.
  Both must pass.
- Optimisations to `sim/` or `solver/` must not change plans. Run the same solver command before and after and
  compare the output files byte for byte.
- Plans must stay human-makeable: window-pixel aims and clicks at frame starts.
- Code comments name the game's functions and data by address (`FUN_xxxxxxxx`, `LogicMgr+0x108`). Keep that
  convention when you add game knowledge.

## Resources

- The solver uses all CPU cores but two by default (`RAYON_NUM_THREADS` to change). If the machine is someone's
  desktop, keep some free.
- Search output goes in `plans/`, `out/` and `runs/`, which are git-ignored. Don't commit run data or anything
  from `game/`.
