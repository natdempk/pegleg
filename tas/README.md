# tas: making and recording runs

Glue that ties the solver, real-game playback and video together.

| File | What it does |
|---|---|
| `pipeline.py` | The whole flow for one category over many levels: screen, refine, play and check, video |
| `play.sh` | Play one script in the real game and check it with `game_check` |
| `video.py`, `sound_mix.py`, `cursor_script.py` | Recording helpers: a 1080p video with a side panel (timer or score), soundtrack mixed from the game's sound log, cursor moves for the recording |
| `unpak.py` | Extracts the game's `main.pak` (used by `bootstrap.sh`) |
| `levels.json` | Level names |
| `runs/` | The four finished runs: plans, results and write-ups |

## One level by hand

```sh
solver/target/release/peggle-solver 2-3 --chars 6,8 --seeds 1-8 --out plans/          # search
cat plans/1_2.plan.txt                                                                # read the plan
tas/play.sh plans/1_2.txt $(cat plans/1_2.seed) runs/2-3                              # play + check
```

`play.sh` ends with `game_check`'s verdict. `N/N shots ok (… real won at shot N)` means the real game did exactly
what the simulator predicted. The real final score is the `total` on the last `st5` line of `runs/2-3/harness.txt`.

## A whole category

```sh
tas/pipeline.py min-time out/min-time screen          # every level, cheap search over masters x seeds
tas/pipeline.py min-time out/min-time refine          # stronger searches on the best candidates → out/min-time/best
tas/pipeline.py min-time out/min-time play            # play each best plan, check it → out/min-time/results.tsv
tas/pipeline.py min-time out/min-time video           # record each level and stitch → out/min-time/tas.mp4
tas/pipeline.py max-score out/score all 2-4 6-5       # all phases, just two levels
```

Categories: `min-time`, `uef`, `max-score`, `showcase`. Their settings (masters, seeds, beam sizes) are in the
`CATEGORIES` table at the top of `pipeline.py`. Override with `CHARS=7 SEEDS=1-32 TOP=4 SETS=light`.

Every phase is resumable: it skips levels whose output already exists, so you can stop it and run it again.
Searching all 55 levels takes hours to a day depending on the category and the CPU (max-score is the slowest).
Run long searches in the background (`nohup ... &`) and check `out/<cat>/logs/`.

## Video

The `video` phase replays each plan at real speed with the cursor shown (`cursor_script.py`) and records the game
window. It mixes the soundtrack from the game's sound log (`sound_mix.py`; the playback DLL records every sound
the game plays, and the mix is aligned to the frames). It then builds a 1920x1080 video per level with a side
panel and joins them. Needs `ffmpeg` and Python `pillow` and `numpy`.

## Replaying a finished run

```sh
tas/play.sh tas/runs/max-score/plans/8_2.txt $(cat tas/runs/max-score/plans/8_2.seed) runs/9-3   # 5,049,800 points
```

A whole category (55 levels; max-score takes about an hour), checked level by level into `out/max-score/results.tsv`:

```sh
mkdir -p out/max-score && cp -r tas/runs/max-score/plans out/max-score/best
tas/pipeline.py max-score out/max-score play
```
