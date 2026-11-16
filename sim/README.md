# sim: bit-exact Peggle Deluxe simulator

A Rust library (`peggle-sim`) that plays Peggle Deluxe (Steam build) adventure levels exactly as the game does:
ball physics, every peg, brick and mover shape, the ten masters' powers, scoring and style bonuses, Extreme
Fever, the ball tube, and the timing of when the next shot can be fired.

"Exact" means bit for bit. The game does its physics in x87 floating point. The simulator does the same
operations in the same order, with the same rounding points, so every ball position, peg hit and score matches.
Where the game draws random numbers, the playback DLL (`play/`) reseeds them from the level's seed at fixed
points. The simulator draws the same numbers.

## Proof: the golden test

`tests/golden/` holds 274 recorded real-game runs, taken from the TAS sets as they were built plus a set of
closed-loop runs. For each one the test replays the inputs in the simulator and compares every peg, ball count,
score and power counter at every point where the game is ready for the next shot, and the final total.

```sh
cargo test --release --test golden          # needs ../game/levels (run ../bootstrap.sh once)
SIM_NO_FAST=1 cargo test --release --test golden   # same, with the fast paths off (reference behaviour)
```

Any change to the simulator must keep both passing. The fast paths, such as the broadphase and the skipping of
settled objects, must not change results; `SIM_NO_FAST=1` turns them off for an A/B comparison.

## Tools

| Binary | What it does |
|---|---|
| `game_check RUN_DIR...` | Compares real-game runs (from `tas/play.sh`) with the simulator, shot by shot. Prints `N/N shots ok` and `real won at shot N` when everything matches. `GC_REAL_FIRE=1` replays the real fire ticks (use it for runs with fast forward); `GC_VERBOSE=1` prints every shot. |
| `dump_level FILE.dat...` | Prints a level file summary (`--json` for everything). |
| `census LEVELDIR NAME...` | Lists which simulator features each level uses, as JSON. |

```sh
cargo build --release
target/release/game_check ../runs/1-1
```

## Using the library

```rust
use peggle_sim::{game::Game, level};
let base = level::parse(&std::fs::read("../game/levels/_base.dat")?)?;
let lvl = level::parse(&std::fs::read("../game/levels/level1.dat")?)?;
let mut g = Game::new(&lvl, &base, /*rng seed*/ 12, /*power*/ 13);  // 13 = Warren's Lucky Spin
g.set_stage_level(0, 0);
let r = g.shoot(angle, g.earliest_fire);   // aim angle in radians, fire tick
println!("{} pegs, {} points, won: {}", r.pegs, r.score, g.won);
```

`solver/src/levels.rs` shows how a level ID becomes a `Game`: the level file, the master's power and the stage.

## Modules

| Module | Contents |
|---|---|
| `phys` | Ball physics, collision, the board, peg hits and pops, powers that act in flight |
| `game` | Turns: shots, scoring, free balls, Lucky Spin, coin flip, Fever, the timing of the next shot |
| `level` | Level file parser |
| `mover`, `brick` | Moving objects and curved bricks |
| `score`, `colors`, `zen`, `tube`, `slot` | Scoring and style bonuses, peg colouring, Zen Shot search, the ball tube, the Lucky Spin wheel |
| `aim` | Mouse position to aim angle, as the game computes it |
| `rng` | The game's Mersenne Twister and the playback DLL's reseeding |
| `x87` | Helpers for x87 rounding (80-bit intermediates rounded where the game stores floats) |
| `oracle`, `check` | Read real-game state dumps from the playback DLL and compare them with the simulator |

Comments name the game's own functions and data by address (for example `FUN_0046e880`, `LogicMgr+0x108`). Use
them to find the matching code if you dig into a difference.
