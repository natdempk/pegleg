# The one-shot search

`oneshot` (`src/bin/oneshot.rs`) looks for a seed and an aim that win a level with its **first shot**: all 25
orange pegs cleared by one ball. It found one for every Adventure level (`../tas/runs/oneshot`).

```sh
cargo build --release          # builds target/release/oneshot with peggle-solver
```

## Why it is possible

The playback DLL's seed decides the colours (which pegs are orange, purple and green) and the luck rolls. It does
not move the ball: a given aim and click time hits the same pegs in the same order under every seed, until the
last orange (when Extreme Fever starts). So the physics only has to be simulated once per level, and testing a
seed is a set question: does one of the simulated shots hit all 25 of this seed's oranges?

Oranges land almost uniformly on the ~100 pegs that can be orange, so a shot that hits *h* of them covers all 25
for a fraction C(*h*, 25) / C(100, 25) of the seeds. With 2^32 seeds that takes *h* around 49 or more. A plain
opening shot hits 30–45 pegs (about 1e-15 per seed), so most levels need a master whose power lights or keeps
up more pegs during the shot.

## The funnel

1. **Shot library** (`lib`, once per level): a dense fan of aims × click delays shot on an all-blue board. The
   biggest hit sets are kept (`DIR/lib/LV.lib`).
2. **Seed scan** (`scan`): per seed, the colours exactly as the playback DLL makes them, then the best
   |hits ∩ oranges| over the library, with bit sets and integers only. Pass 1 only walks the shots that hit the
   seed's rarest oranges, so it skips over 99% of the library and stays exact. Seeds whose best is 24 or 25 are
   written as candidates.
3. **Exact verify** (`verify`): every candidate is shot in the full simulator with the real colours and power.
   Exact wins get a plan in `DIR/wins/`, and the fastest win per level (earliest end tick, then highest total)
   is copied to `DIR/best/S_L.{txt,plan.txt,seed}`, the layout `tas/pipeline.py` reads.
4. **Real game**: `tas/pipeline.py oneshot DIR play` plays `DIR/best` and checks each run with `game_check`.

Seeds are scanned in 256 chunks of 2^24 per (level, class). A finished chunk is one line in
`DIR/ledger/HOST.tsv` (level, class, library hash, chunk, time, a histogram of every seed's best orange count, the
best seeds). Each machine writes only its own ledger and candidate files (`DIR/cands/HOST.tsv`), so several
machines can share one search by copying `lib/`, `ledger/`, `cands/` and `verified.tsv` between them. Workers
on one machine skip each other's chunks through `DIR/claims/`. A changed library changes its hash, and its chunks
start over.

## Classes

A class is a master and how the scan models its power on the first shot. All classes are checked by `verify` in
the exact simulator, so an approximate filter can miss wins but never report a false one.

| Class | Master | The power during the shot | How the scan models it |
|---|---|---|---|
| `free` | Bjorn (Hu and Cinderbottom alike) | none (their powers act on later shots) | the hit set; exact |
| `tula` | Tula | Flower Power: the green lights the nearest ⌈20%⌉ of the oranges left | the shot's ordered hits + nearest-peg lists per peg (per tick on boards with moving pegs) |
| `splork` | Splork | Space Blast: lights every peg within 100 px of the green; lit greens blast too | the ordered hits + blast sets per peg (per tick when pegs move) |
| `multi` | Jimmy | Multiball: a second ball spawns at the green | a continuation library: one re-simulated shot per (library shot, first green) |
| `spooky` | Renfield | Spooky Ball: the ball respawns at the top; its 9 trial launches use the seed's luck | exact per seed (`oneshot/spooky.rs`): the respawn walk with memoized trials, including a second respawn from the other green. CPU only |
| `pyramid`, `claude`, `hat` | Kat, Claude, Warren | the pyramid / flippers switch on (never pressed) / Magic Hat from the Lucky Spin | continuation libraries |
| `flip` | Claude | flippers pressed by the player | see below. CPU only |

Powers that need a green peg depend on the seed's greens too, which the scan computes along with the oranges.

### Flippers (class `flip`)

After Claude's first green the flippers switch on, and the player can flip them with the mouse button for as
long as the ball stays up. The path then depends only on the shot, the first green and the flip schedule, not
on the seed. `flipcov` runs a beam search over flip schedules per template (library shot, first green) for the
largest coverage, and keeps each good schedule as a continuation entry (`DIR/lib/LV.flip.cont` and
`LV.flip.flips`). The scan then treats the entries like Multiball's. The game's flipper_clear (when the flippers
first switch on, it hides the corner pegs and moves their oranges to random pegs) is modelled exactly per seed.

Plans with flips are scripts with `flip TICK 1|0` lines (press / release at a harness tick) before the `shotx`.
They are timed without fast forward; `tas/play.sh` plays them that way automatically.

The fastest flip wins: along a schedule the pegs light up at seed-independent ticks, so a seed's win tick is
known without simulating. `fliptimes` estimates the expected wins over 2^32 seeds by win tick, and
`fastscan --t T` finds the seeds that win by tick T. Pick T where the estimate gives a few hundred wins.

## Commands

All take a work directory `DIR` (created on first use) and levels as `1-1` … `11-5`.

| Command | What it does |
|---|---|
| `lib DIR LV... [--angles 100000] [--delays 8] [--dmax D --dstride S] [--keep 30000] [--keep-power N]` | Build the shot library (`all` for every level). 200,000 angles × 8 delays takes about 10 minutes per level on 4 threads |
| `scan DIR LV CLASS [--chunks A-B] [--desc] [--max N] [--host H] [--gpu BIN [--batch 8]]` | Scan unfinished chunks; `--desc` from the top of the seed space; `--gpu` uses the CUDA kernel for the classes it supports |
| `verify DIR [LV...]` | Exact sim of all new candidates → `verified.tsv`, `wins/`, `best/` |
| `status DIR` | Per (level, class): chunks done, best count, near misses, exact wins |
| `next DIR [--classes free,tula,splork] [--desc] [--gpu]` | The queue: one chunk of every (level, class) first, then the pair whose near-miss histogram predicts the most wins per chunk, over the levels not won yet. Prints `LEVEL CLASS CHUNK EST` |
| `cont DIR LV CLASS` | Build a continuation library (`scan` also builds a missing one) |
| `check DIR LV CLASS [N]` | The scan's count vs the exact sim on N random seeds |
| `bench DIR LV CLASS [--n N]`, `probe`, `gpucheck DIR LV CLASS BIN` | Scan speed; how far a power reaches; the GPU kernel vs the CPU scan on one range (must print IDENTICAL) |
| `flipcov DIR LV [--tmpl 30] [--beam 32] [--horizon H]` | Flip schedules for class `flip` (resumable). `--horizon H` favours coverage by harness tick H, for faster wins |
| `fliptimes DIR LV`, `fastscan DIR LV --t T [--chunks A-B]`, `fastcheck DIR LV` | Fastest flip wins: expected wins by tick; seeds winning by tick T (candidates for `verify`); the predicted win tick vs the simulator |
| `flipcheck`, `flipdbg`, `flipinfo`, `flipplan`, `spooky DIR check LV` | Checks and diagnostics for the flip and Spooky Ball models |

A typical level:

```sh
B=solver/target/release/oneshot
$B lib out/oneshot 4-1 --angles 200000 --delays 8
$B scan out/oneshot 4-1 tula --chunks 0-15          # 16 of 256 chunks
$B verify out/oneshot 4-1 && $B status out/oneshot
tas/pipeline.py oneshot out/oneshot play 4-1        # Windows, Steam running
```

A Claude flip level:

```sh
$B flipcov out/oneshot 7-5 --horizon 4000
$B fliptimes out/oneshot 7-5                        # pick T
$B fastscan out/oneshot 7-5 --t 3500 --chunks 0-3
$B verify out/oneshot 7-5
```

For long runs, `solver/oneshot_worker.sh DIR TAG asc|desc LEVEL...` builds the missing libraries and then scans what
`next` says until `DIR/STOP` exists (`DIR/classes` sets the classes, `ONESHOT_GPU=BIN` scans on a GPU). Run one
per machine with its own TAG; `desc` on a second machine takes chunks from the other end.

## Speed

On a CPU the free class scans about 0.8M seeds per second on 4 threads (a 2^24-seed chunk in under a minute). The
power classes are slower (down to ~10k seeds per second for Tula / Splork on boards with moving pegs), and Spooky
Ball runs at about 10k per second on 6 threads. One chunk of the right class was enough for many early
levels; the hard ones took many chunks. Set `RAYON_NUM_THREADS` to share the machine.

### GPU (CUDA)

`gpu/oneshot_gpu.cu` is an exact port of the scan for `free`, `multi`, and `tula` / `splork` (including boards
with moving pegs): one thread per seed, the same candidates and ledger lines as the CPU. On an RTX 3080 it scans
1.6–7.6M seeds per second (0.1–0.4M with moving pegs). Build it on a machine with the CUDA toolkit:

```sh
cd solver/gpu && nvcc -O3 -arch=sm_86 -fmad=false -o oneshot_gpu oneshot_gpu.cu
```

`-arch` is your GPU's compute capability (`sm_86` for RTX 30xx, `sm_89` for 40xx). `-fmad=false` is required:
fused multiply-adds would round the green-distance test differently from the CPU. Check a new build with
`oneshot gpucheck DIR LV CLASS solver/gpu/oneshot_gpu`, then scan with `--gpu solver/gpu/oneshot_gpu`.
