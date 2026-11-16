# solver: peggle-solver

Finds a TAS plan for one level: a sequence of shots (aim and click time) that reaches a goal, using the simulator
from `../sim`. It writes a script the playback DLL can play in the real game, and a readable plan.

```sh
cargo build --release
target/release/peggle-solver 1-1 --chars 3 --seeds 1-8 --out ../plans/
```

## How the search works

1. **Beam search over shots.** From each node (a board state after some shots) it tries a coarse grid of aim
   angles (`--coarse`) at a few click moments (`--delays`). It then refines the most promising angles more finely
   (`--refine-top`, `--refine-rounds`) and keeps the best `--beam` nodes for the next shot.
2. **Ranking by objective.** `speed` ranks nodes by real elapsed time: app frames with fast forward held, plus a
   cost per orange left. `uef` charges per peg left. `score` ranks by the total plus an estimate of what the
   Extreme Fever finish can still earn. `shots` ranks fewest shots first, then highest total.
3. **Seeds and masters.** Every (seed, master) pair is a separate search; the best plan over all of them is
   written. The seed is the RNG seed the playback DLL fixes at level start, so it decides the peg colours and
   the luck.
4. **Human inputs only.** Aims are whole window pixels, converted to angles exactly as the game does. Clicks land
   on frame boundaries, as a real mouse click would.

A beam search with a heuristic is not monotone: a wider search can do worse than a narrow one on the same seed.
Many cheap searches over seeds, then a few stronger ones on the best candidates, beats one huge search. That's
what `../tas/pipeline.py` automates.

## Options

| Option | Default | What it does |
|---|---|---|
| `LEVEL` | n/a | `1-1` … `11-5`, `S_L` (0-based: `0_0` = 1-1) or a level name |
| `--objective` | `speed` | `speed`, `uef` (clear every peg), `score`, `shots` (fewest, then score), `pegs` / `shotscore` (best single opening shot) |
| `--seeds 1-8` / `--seeds 3,7` | `5` | RNG seeds to try |
| `--chars 0,3,6` / `--chars all` | stage master | Masters to try: 0 Bjorn, 1 Jimmy, 2 Kat, 3 Splork, 4 Claude, 5 Renfield, 6 Tula, 7 Warren, 8 Cinderbottom, 9 Hu |
| `--beam N` | 24 | Nodes kept per shot depth |
| `--coarse N` | 720 | Coarse angles per expansion |
| `--delays a,b,…` | 0,10,25,50 | Click delays after the earliest possible click, in frames; they only matter on levels with moving parts |
| `--refine-top N`, `--refine-rounds N` | 6, 4 | Fine refinement around the best coarse angles |
| `--max-shots N` | 10 | Depth limit |
| `--keep-ball` | off | Every shot must catch the ball or earn a free ball |
| `--orange-cost F`, `--peg-cost F` | | Heuristic weights for `speed` and `uef` |
| `--fever-frac F`, `--probe N`, `--probe-pool N`, `--probe-untrip F`, `--strata` | off | `score` objective: value nodes by probing the Extreme Fever finish (shots fired with one orange left), optionally assuming a Score Tripler. Keep the beam spread over oranges-left counts. See `tas/pipeline.py` max-score for working values |
| `--power P` | | Override the master's power (stage 11's random master) |
| `--no-ff` | | Model no fast forward: cost in game ticks, click on any tick |
| `--out DIR` | | Write `DIR/S_L.txt` (script), `S_L.plan.txt` (readable plan), `S_L.seed` |
| `--bench` | | Only measure simulation speed on this level |

Threads: all cores but two by default; set `RAYON_NUM_THREADS` to change. Throughput is roughly 10k–40k simulated
shots per second on a modern 8-core CPU, depending on the level.

## Output

`S_L.txt` is the script the playback DLL reads:

```
char 3                 select master 3 (Splork) for individual-level play
level 0 0              start adventure stage 0, level 0 (1-1)
shotx 409054cb 103     aim at this exact float angle (hex bits) and click at game tick 103
shotx 40b66e21 1105
waitms 3000            wait 3 s of wall time (the Level Complete screen)
quit
```

`S_L.plan.txt` has a header (level, seed, master, objective, predicted frames or total), then one line per shot:
the window pixel to aim at, the angle, the click tick and the predicted outcome (pegs, oranges, score, styles,
free balls). Plans are deterministic: the same arguments always give the same plan, on any machine.
