# Ultra Extreme Fever (IL)

Same timing as Any%, but every peg on the board must be hit before the winning ball lands in the fever bucket.
The 55 levels sum to **1841.19 s**, against **3086.03 s** for the speedrun.com Ultra Extreme Fever IL records
when these runs were made; 55 of 55 levels beat their record.

## How these runs were made

The plans come from the same search `tas/pipeline.py` runs (its `CATEGORIES` table has the settings; some levels
were searched again with more seeds or wider beams): a cheap beam search over several masters and RNG seeds, then
wider searches on the best candidates. Every plan was then played in the
real game with `tas/play.sh` and checked shot by shot against the simulator (`game_check`). All 55 runs here won in
the real game and match the simulator exactly. The *Seed* is the RNG seed the playback DLL fixes at level start;
it decides the peg colours and the luck (Lucky Spins, coin flips), the way a TAS picks its luck.

Replay any level (Windows, Steam running):

```sh
tas/play.sh tas/runs/uef/plans/S_L.txt $(cat tas/runs/uef/plans/S_L.seed) runs/uef-S_L
```

`S_L` is the 0-based stage and level (`0_0` = 1-1, `10_4` = 11-5). `plans/S_L.plan.txt` lists each shot: the window
pixel to aim at, the click tick and the predicted outcome.

## Levels

| Level | Name | Master | Seed | Shots | TAS time (s) | Record (s) | Record holder |
|---|---|---|---:|---:|---:|---:|---|
| 1-1 | Peggleland | Splork | 3 | 8 | 22.31 | 37.367 | polypies73 |
| 1-2 | Slip and Slide | Splork | 2 | 9 | 29.19 | 50.500 | britsha |
| 1-3 | Bjorn's Gazebo | Cinderbottom | 4 | 11 | 32.62 | 44.900 | britsha |
| 1-4 | Das Bucket | Cinderbottom | 3 | 6 | 28.87 | 31.367 | Brassbeat |
| 1-5 | Snow Day | Splork | 2 | 11 | 29.94 | 54.367 | Brassbeat |
| 2-1 | Birdy's Crib | Splork | 2 | 11 | 32.11 | 50.300 | Brassbeat |
| 2-2 | Buffalo Wings | Jimmy | 4 | 13 | 33.35 | 56.033 | britsha |
| 2-3 | Skate Park | Cinderbottom | 2 | 11 | 35.90 | 64.517 | crazytonedog |
| 2-4 | Spiral of Doom | Splork | 1 | 13 | 32.82 | 53.033 | Brassbeat |
| 2-5 | Mr. Peepers | Cinderbottom | 3 | 9 | 28.64 | 61.183 | crazytonedog |
| 3-1 | Scarab Crunch | Splork | 1 | 10 | 36.04 | 56.000 | Brassbeat |
| 3-2 | Infinite Cheese | Cinderbottom | 1 | 13 | 35.30 | 57.267 | Brassbeat |
| 3-3 | Ra Deal | Splork | 3 | 11 | 33.77 | 56.833 | Brassbeat |
| 3-4 | Croco-gator Pit | Cinderbottom | 4 | 11 | 36.26 | 62.767 | britsha |
| 3-5 | The Fever Level | Splork | 3 | 12 | 37.61 | 48.400 | Brassbeat |
| 4-1 | The Amoeban | Splork | 4 | 12 | 32.94 | 52.067 | Brassbeat |
| 4-2 | The Last Flower | Cinderbottom | 3 | 11 | 31.51 | 69.267 | britsha |
| 4-3 | We Come In Peace | Splork | 1 | 10 | 26.38 | 48.967 | Brassbeat |
| 4-4 | Maid In Space | Splork | 1 | 10 | 27.59 | 57.567 | Brassbeat |
| 4-5 | Getting The Spare | Cinderbottom | 1 | 9 | 31.81 | 60.800 | britsha |
| 5-1 | Pearl Clam | Splork | 1 | 14 | 34.19 | 62.633 | Brassbeat |
| 5-2 | Insane Aquarium | Tula | 3 | 13 | 34.95 | 54.067 | Brassbeat |
| 5-3 | Tasty Waves | Splork | 3 | 10 | 33.77 | 59.233 | Brassbeat |
| 5-4 | Our Favorite Eel | Splork | 4 | 11 | 34.61 | 50.233 | Brassbeat |
| 5-5 | Love Story | Cinderbottom | 3 | 10 | 35.53 | 64.033 | britsha |
| 6-1 | Waves | Cinderbottom | 4 | 9 | 28.79 | 42.467 | Brassbeat |
| 6-2 | Spiderweb | Jimmy | 3 | 14 | 43.59 | 67.933 | britsha |
| 6-3 | Blockers | Tula | 3 | 11 | 36.05 | 55.600 | Brassbeat |
| 6-4 | Baseball | Cinderbottom | 3 | 11 | 32.28 | 61.200 | britsha |
| 6-5 | Vermin | Splork | 1 | 14 | 37.64 | 58.500 | Brassbeat |
| 7-1 | Holland Oats | Tula | 1 | 12 | 36.83 | 78.267 | britsha |
| 7-2 | I Heart Flowers | Cinderbottom | 1 | 12 | 32.41 | 55.700 | Brassbeat |
| 7-3 | Workin From Home | Cinderbottom | 2 | 9 | 32.36 | 65.633 | Brassbeat |
| 7-4 | Tula's Ride | Cinderbottom | 3 | 7 | 29.09 | 44.667 | Brassbeat |
| 7-5 | 70 and Sunny | Cinderbottom | 3 | 12 | 30.75 | 52.267 | britsha |
| 8-1 | Win a Monkey | Splork | 4 | 14 | 36.08 | 56.067 | britsha |
| 8-2 | Dog Pinball | Cinderbottom | 1 | 9 | 30.67 | 48.567 | Brassbeat |
| 8-3 | Spin Again | Splork | 4 | 9 | 27.31 | 65.133 | Brassbeat |
| 8-4 | Roll 'em | Cinderbottom | 2 | 10 | 28.63 | 51.833 | Brassbeat |
| 8-5 | Five of a Kind | Cinderbottom | 2 | 11 | 30.34 | 49.733 | Brassbeat |
| 9-1 | The Love Moat | Cinderbottom | 3 | 13 | 49.24 | 64.733 | Brassbeat |
| 9-2 | Doom With a View | Cinderbottom | 2 | 11 | 29.97 | 54.467 | Brassbeat |
| 9-3 | Rhombi | Splork | 4 | 11 | 29.62 | 45.300 | narcolep7z |
| 9-4 | 9 Luft Balloons | Splork | 4 | 10 | 29.19 | 45.200 | Brassbeat |
| 9-5 | Twisted Sisters | Splork | 4 | 12 | 33.71 | 58.233 | Brassbeat |
| 10-1 | Spin Cycle | Splork | 2 | 10 | 30.52 | 59.933 | Brassbeat |
| 10-2 | The Dude Abides | Cinderbottom | 3 | 11 | 34.38 | 57.333 | britsha |
| 10-3 | When Pigs Fly | Cinderbottom | 1 | 8 | 34.26 | 57.033 | britsha |
| 10-4 | Yang, Yin | Tula | 2 | 15 | 54.63 | 69.367 | Brassbeat |
| 10-5 | Zen Frog | Cinderbottom | 3 | 12 | 37.90 | 74.300 | narcolep7z |
| 11-1 | Paw Reader | Cinderbottom | 1 | 12 | 38.65 | 56.333 | Brassbeat |
| 11-2 | End of Time | Splork | 2 | 10 | 33.75 | 38.833 | narcolep7z |
| 11-3 | Billions & Billions | Splork | 4 | 10 | 35.12 | 88.900 | britsha |
| 11-4 | Don't Panic | Splork | 2 | 10 | 29.75 | 50.033 | Brassbeat |
| 11-5 | Beyond Reason | Cinderbottom | 1 | 9 | 39.67 | 48.767 | Brassbeat |
| **All 55** | | | | **597** | **1841.19** | **3086.03** | |
