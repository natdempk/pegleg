# play: real-game playback

The playback DLL (`hook.c`, built into `bin/bass.dll`) runs a TAS script inside the real Peggle Deluxe. The game
loads it in place of its sound library, `bass.dll`. Every sound call is forwarded to the original, which is kept
as `bass_orig.dll`. Inside the game it:

- **runs the script:** `level`, `char`, `shotx` and so on (see `solver/README.md`), at exact game ticks, from
  inside the game's own update loop rather than with your mouse;
- **seeds the RNG** (`PEGGLE_RNG_SEED`) at fixed points: level start, turn end, Lucky Spin, coin flip and Zen
  Shot. Peg colours and luck then depend only on the seed and the inputs;
- **holds fast forward** and makes the clicks a player would make between frames: skipping the intro, cancelling
  the near-win slow motion, confirming Level Complete;
- **ignores the real mouse** while a script runs, so moving your mouse over the window can't change a shot;
- **logs** the run: `harness.txt` (commands, fire ticks, frames, final total) and `oracle.bin` (board state at
  each ready point, which `game_check` compares with the simulator).

## Running it

`tas/play.sh` (WSL or Git Bash) calls `play.ps1`, or you can call it directly:

```powershell
powershell -ExecutionPolicy Bypass -File play\play.ps1 -Script plans\0_0.txt -Seed 12 -Out runs\1-1 -Fast
```

Requirements: Peggle Deluxe installed through Steam, **Steam running and logged in**, and `bin/bass.dll` built
(`../bootstrap.sh` or `build.sh`). For `-Video`, `ffmpeg` on PATH (`winget install ffmpeg`).

For the duration of the run it changes three things, and restores them afterwards, also on error or Ctrl+C:

| What | Why |
|---|---|
| `bass.dll` in the game folder → the playback DLL (the original is kept as `bass_orig.dll`) | Loads the player into the game |
| `C:\ProgramData\Steam\Peggle\userdata` → a fresh profile (`profile/users.dat`), yours kept as `userdata.pegleg-backup` | Every run starts the same way. The profile stores which tips the game has shown, and those change the game (the first coin flip is free until its tip has been shown) |
| Registry `HKCU\Software\Steam\Peggle` `Is3D` = 0 | The "3D acceleration" option. With it on, Direct3D switches the FPU to 24-bit precision and the physics no longer match the simulator |

The game is started through its normal Steam launcher (`Peggle.exe`), with Steam's folder on `PATH` so the
launcher finds `Steam.dll`, as when Steam starts it. A game window appears and plays by itself; one level takes
10–60 s with `-Speed 8` (the default). Run one playback at a time. The launcher starts the game itself as
`C:\ProgramData\PopCap Games\Peggle\popcapgame1.exe`; `play.ps1` waits for and stops both processes. If the game
stops making progress for 60 s (for example on an error dialog, which can open behind other windows), it is
stopped and everything is restored. If a run was interrupted hard (power loss), the next run detects the
leftover `bass_orig.dll` and profile backup and still restores them.

Options: `-Speed N` (game clock multiplier; 1 = real time), `-Fast` (fast forward and human-timed clicks; solver
plans need it, and `tas/play.sh` always passes it), `-NoFF` (with `-Fast`: human-timed clicks without fast
forward; `tas/play.sh` uses it for scripts with `flip` lines and with `--no-ff`), `-Video` (real time, records `run.mp4` of the game window
and a sound log; the OS cursor glides to each aim point, so don't touch the mouse), `-Progress N`,
`-TimeoutSec N`, `-Game DIR`.

## Building the DLL

```sh
sh play/build.sh     # Zig 0.17.1 C compiler, downloaded into play/.zig on first use; no admin rights
```

## Environment variables the DLL reads

| Variable | Meaning |
|---|---|
| `PEGGLE_SCRIPT`, `PEGGLE_HLOG`, `PEGGLE_LOG`, `PEGGLE_OUT` | Script to run; harness log; state dump file; directory for the other outputs |
| `PEGGLE_RNG_SEED` | The seed (from `S_L.seed`) |
| `PEGGLE_SPEED` | Game clock multiplier |
| `PEGGLE_FF`, `PEGGLE_CLICKS`, `PEGGLE_HUMANFIRE` | Fast forward held; between-frame clicks for intro, slow motion and Level Complete; shots fired only at frame starts |
| `PEGGLE_PROGRESS` | Profile progress set at each `level` command (55 = everything unlocked; greens and purples need > 2) |
| `PEGGLE_FRMLOG`, `PEGGLE_SOUNDLOG` | Per-frame log lines; sound log for the video soundtrack |

Others (`PEGGLE_MOVALL`, `PEGGLE_BHPLOG`, `PEGGLE_TUBELOG`, `PEGGLE_TRACE_CCD`, `PEGGLE_AILOG`, ...) add extra records to `oracle.bin`
for investigating simulator differences; see the comments in `hook.c`.
