# tas: making and recording runs

Glue that ties the solver, real-game playback and video together.

| File | What it does |
|---|---|
| `pipeline.py` | The whole flow for one category over many levels: screen, refine (or scout, for max-score), play and check, video. For one-shot plans (from `solver/target/release/oneshot`) only play and video |
| `play.sh` | Play one script in the real game and check it with `game_check` |
| `video.py`, `sound_mix.py`, `cursor_script.py` | Recording helpers: a 1080p video with a side panel (timer or score), or gameplay only for one-shots; soundtrack mixed from the game's sound log; cursor moves for the recording |
| `unpak.py` | Extracts the game's `main.pak` (used by `bootstrap.sh`) |
| `levels.json` | Level names |
| `runs/` | The finished runs: plans, results and write-ups |

## One level by hand

```sh
solver/target/release/peggle-solver 2-3 --chars 6,8 --seeds 1-8 --out plans/          # search
cat plans/1_2.plan.txt                                                                # read the plan
tas/play.sh plans/1_2.txt $(cat plans/1_2.seed) runs/2-3                              # play + check
```

`play.sh` ends with `game_check`'s verdict. `N/N shots ok (… real won at shot N)` means the real game did exactly
what the simulator predicted. The real final score is the `total` on the last `st5` line of `runs/2-3/harness.txt`.

## Without fast forward

Solver plans are timed with fast forward held (clicks on its 8-tick frame grid). Scripts with `flip` lines (the
one-shot flipper plans) play without fast forward, and so does `tas/play.sh --no-ff`: every tick is then a frame
start, and `play.sh` moves each click one tick later so the shot fires on the plan's tick.

## A whole category

```sh
tas/pipeline.py min-time out/min-time screen          # every level, cheap search over masters x seeds
tas/pipeline.py min-time out/min-time refine          # stronger searches on the best candidates → out/min-time/best
tas/pipeline.py min-time out/min-time play            # play each best plan, check it → out/min-time/results.tsv
tas/pipeline.py min-time out/min-time video           # record each level and stitch → out/min-time/tas.mp4
tas/pipeline.py max-score out/score all 2-4 6-5       # all phases, just two levels
```

Categories: `min-time`, `uef`, `max-score`, `showcase`, `oneshot`. Their settings (masters, seeds, beam sizes) are in the
`CATEGORIES` table at the top of `pipeline.py`. Override with `CHARS=7 SEEDS=1-32 TOP=4 SETS=light`.

Every phase is resumable: it skips levels whose output already exists, so you can stop it and run it again.
Searching all 55 levels takes hours to a day depending on the category and the CPU (max-score is the slowest).

For max-score the seed matters most, and `scout` finds good ones (the current max-score plans came from it):

```sh
tas/pipeline.py max-score out/score scout 2-4            # scout seeds 1-100000, Fever search on the best 2
SCOUT_SEEDS=1-20000 SCOUT_TOP=3 tas/pipeline.py max-score out/score scout 2-4
```

It ranks seeds with `solver/target/release/scout` (see `solver/README.md`), then runs the Fever search on the
best `SCOUT_TOP` seeds three ways (catch-streak value 0, 0.5, and 0.5 with `--review`) and copies the best plan
to `OUT/best` if it beats what is there. Then `play` as usual. About 20–40 minutes per level on 8 cores.
Run long searches in the background (`nohup ... &`) and check `out/<cat>/logs/`.

## Video

The `video` phase replays each plan at real speed with the cursor shown (`cursor_script.py`) and records the game
window. It mixes the soundtrack from the game's sound log (`sound_mix.py`; the playback DLL records every sound
the game plays, and the mix is aligned to the frames). It then builds a 1920x1080 video per level with a side
panel and joins them. Needs `ffmpeg` and Python `pillow` and `numpy`.

For `oneshot` (`MODE=oneshot`) the levels are recorded without fast forward and each clip is the game alone,
scaled 2x by nearest neighbour to 1600x1200 (`SCALE=N` for another factor), with no panel. Finished clips are
reused when more levels are added.

## Replaying a finished run

```sh
tas/play.sh tas/runs/max-score/plans/8_2.txt $(cat tas/runs/max-score/plans/8_2.seed) runs/9-3   # 5,049,800 points
```

A whole category (55 levels; max-score takes about an hour), checked level by level into `out/max-score/results.tsv`:

```sh
mkdir -p out/max-score && cp -r tas/runs/max-score/plans out/max-score/best
tas/pipeline.py max-score out/max-score play
```
