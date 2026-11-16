# Min-time (Any% IL)

Clear each level as fast as possible, timed the way speedrun.com times Peggle Deluxe individual-level runs: from
the input that loads the level to the Level Complete box. Fast forward (right mouse button) is held wherever the
game allows it. The 55 levels sum to **884.90 s**, against **1533.38 s** for the speedrun.com Any% IL records
when these runs were made; 55 of 55 levels beat their record. *TAS time* is the real recorded run's frame count
(10 ms frames) plus the level load.

## How these runs were made

The plans come from the same search `tas/pipeline.py` runs (its `CATEGORIES` table has the settings; some levels
were searched again with more seeds or wider beams): a cheap beam search over several masters and RNG seeds, then
wider searches on the best candidates. Every plan was then played in the
real game with `tas/play.sh` and checked shot by shot against the simulator (`game_check`). All 55 runs here won in
the real game and match the simulator exactly. The *Seed* is the RNG seed the playback DLL fixes at level start;
it decides the peg colours and the luck (Lucky Spins, coin flips), the way a TAS picks its luck.

Replay any level (Windows, Steam running):

```sh
tas/play.sh tas/runs/min-time/plans/S_L.txt $(cat tas/runs/min-time/plans/S_L.seed) runs/min-time-S_L
```

`S_L` is the 0-based stage and level (`0_0` = 1-1, `10_4` = 11-5). `plans/S_L.plan.txt` lists each shot: the window
pixel to aim at, the click tick and the predicted outcome.

## Levels

| Level | Name | Master | Seed | Shots | TAS time (s) | Record (s) | Record master |
|---|---|---|---:|---:|---:|---:|---|
| 1-1 | Peggleland | Splork | 27 | 4 | 11.49 | 17.560 | Splork Sporkan |
| 1-2 | Slip and Slide | Splork | 2 | 4 | 16.43 | 24.900 | Tula Sunflower |
| 1-3 | Bjorn's Gazebo | Tula | 8 | 4 | 16.56 | 21.500 | Tula Sunflower |
| 1-4 | Das Bucket | Tula | 2 | 4 | 14.44 | 22.533 | Splork Sporkan |
| 1-5 | Snow Day | Tula | 3 | 5 | 14.90 | 22.667 | Tula Sunflower |
| 2-1 | Birdy's Crib | Splork | 8 | 5 | 15.69 | 26.967 | Splork Sporkan |
| 2-2 | Buffalo Wings | Tula | 1 | 5 | 17.95 | 24.667 | Tula Sunflower |
| 2-3 | Skate Park | Tula | 4 | 4 | 17.68 | 31.833 | Lord Cinderbottom |
| 2-4 | Spiral of Doom | Tula | 6 | 5 | 14.31 | 28.960 | Jimmy Lightning |
| 2-5 | Mr. Peepers | Tula | 4 | 4 | 13.74 | 26.667 | Lord Cinderbottom |
| 3-1 | Scarab Crunch | Tula | 8 | 4 | 16.70 | 29.033 | Tula Sunflower |
| 3-2 | Infinite Cheese | Tula | 6 | 5 | 14.44 | 31.533 | Tula Sunflower |
| 3-3 | Ra Deal | Tula | 3 | 4 | 15.84 | 28.367 | Tula Sunflower |
| 3-4 | Croco-gator Pit | Tula | 3 | 5 | 15.27 | 26.800 | Tula Sunflower |
| 3-5 | The Fever Level | Cinderbottom | 5 | 4 | 16.38 | 24.833 | Lord Cinderbottom |
| 4-1 | The Amoeban | Cinderbottom | 8 | 6 | 18.06 | 29.200 | Lord Cinderbottom |
| 4-2 | The Last Flower | Tula | 7 | 4 | 15.41 | 35.800 | Lord Cinderbottom |
| 4-3 | We Come In Peace | Splork | 1 | 4 | 14.29 | 25.633 | Splork Sporkan |
| 4-4 | Maid In Space | Tula | 2 | 4 | 11.71 | 30.667 | Splork Sporkan |
| 4-5 | Getting The Spare | Cinderbottom | 4 | 5 | 16.68 | 25.500 | Lord Cinderbottom |
| 5-1 | Pearl Clam | Tula | 7 | 5 | 16.58 | 30.100 | Tula Sunflower |
| 5-2 | Insane Aquarium | Tula | 4 | 5 | 14.32 | 26.533 | Tula Sunflower |
| 5-3 | Tasty Waves | Tula | 5 | 4 | 11.99 | 29.967 | Tula Sunflower |
| 5-4 | Our Favorite Eel | Cinderbottom | 1 | 5 | 16.46 | 26.760 | Claude Lobster |
| 5-5 | Love Story | Splork | 5 | 5 | 15.44 | 28.760 | Tula Sunflower |
| 6-1 | Waves | Tula | 4 | 3 | 14.54 | 22.433 | Warren Rabbit |
| 6-2 | Spiderweb | Cinderbottom | 2 | 5 | 22.14 | 35.933 | Tula Sunflower |
| 6-3 | Blockers | Cinderbottom | 1 | 5 | 17.85 | 30.067 | Tula Sunflower |
| 6-4 | Baseball | Tula | 3 | 5 | 16.17 | 29.120 | Tula Sunflower |
| 6-5 | Vermin | Cinderbottom | 4 | 4 | 18.59 | 28.720 | Lord Cinderbottom |
| 7-1 | Holland Oats | Tula | 4 | 5 | 14.44 | 34.767 | Lord Cinderbottom |
| 7-2 | I Heart Flowers | Tula | 1 | 5 | 13.86 | 27.000 | Lord Cinderbottom |
| 7-3 | Workin From Home | Tula | 3 | 4 | 20.24 | 32.133 | Lord Cinderbottom |
| 7-4 | Tula's Ride | Cinderbottom | 6 | 4 | 17.83 | 31.167 | Lord Cinderbottom |
| 7-5 | 70 and Sunny | Splork | 3 | 5 | 15.50 | 24.567 | Lord Cinderbottom |
| 8-1 | Win a Monkey | Tula | 1 | 4 | 15.20 | 24.600 | Splork Sporkan |
| 8-2 | Dog Pinball | Tula | 1 | 5 | 17.85 | 28.233 | Lord Cinderbottom |
| 8-3 | Spin Again | Tula | 2 | 5 | 14.53 | 33.633 | Splork Sporkan |
| 8-4 | Roll 'em | Cinderbottom | 6 | 4 | 13.84 | 26.067 | Lord Cinderbottom |
| 8-5 | Five of a Kind | Cinderbottom | 2 | 6 | 16.23 | 26.333 | Lord Cinderbottom |
| 9-1 | The Love Moat | Tula | 4 | 4 | 20.98 | 28.667 | Lord Cinderbottom |
| 9-2 | Doom With a View | Cinderbottom | 2 | 6 | 16.68 | 31.300 | Tula Sunflower |
| 9-3 | Rhombi | Splork | 4 | 5 | 13.59 | 21.333 | Lord Cinderbottom |
| 9-4 | 9 Luft Balloons | Splork | 2 | 5 | 15.44 | 27.700 | Lord Cinderbottom |
| 9-5 | Twisted Sisters | Tula | 4 | 6 | 17.88 | 25.333 | Lord Cinderbottom |
| 10-1 | Spin Cycle | Tula | 3 | 5 | 12.66 | 27.867 | Bjorn Unicorn |
| 10-2 | The Dude Abides | Splork | 4 | 4 | 18.33 | 28.367 | Lord Cinderbottom |
| 10-3 | When Pigs Fly | Tula | 7 | 6 | 15.67 | 28.900 | Bjorn Unicorn |
| 10-4 | Yang, Yin | Cinderbottom | 3 | 6 | 19.82 | 37.200 | Lord Cinderbottom |
| 10-5 | Zen Frog | Tula | 2 | 5 | 14.30 | 33.500 | Lord Cinderbottom |
| 11-1 | Paw Reader | Cinderbottom | 5 | 6 | 26.84 | 30.300 | Lord Cinderbottom |
| 11-2 | End of Time | Cinderbottom | 6 | 6 | 16.16 | 24.033 | Lord Cinderbottom |
| 11-3 | Billions & Billions | Tula | 5 | 4 | 16.62 | 34.700 | Tula Sunflower |
| 11-4 | Don't Panic | Tula | 3 | 5 | 13.77 | 23.167 | Tula Sunflower |
| 11-5 | Beyond Reason | Splork | 1 | 4 | 14.59 | 18.500 | Lord Cinderbottom |
| **All 55** | | | | **259** | **884.90** | **1533.38** | |
