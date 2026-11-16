# Showcase (fewest shots)

The entertainment run: each level is cleared in as few shots as possible, and among those the highest score wins.
All 55 levels take **162 shots** for **61,457,420** points.

## How these runs were made

The plans come from the same search `tas/pipeline.py` runs (its `CATEGORIES` table has the settings; some levels
were searched again with more seeds or wider beams): a cheap beam search over several masters and RNG seeds, then
wider searches on the best candidates. Every plan was then played in the
real game with `tas/play.sh` and checked shot by shot against the simulator (`game_check`). All 55 runs here won in
the real game and match the simulator exactly. The *Seed* is the RNG seed the playback DLL fixes at level start;
it decides the peg colours and the luck (Lucky Spins, coin flips), the way a TAS picks its luck.

Replay any level (Windows, Steam running):

```sh
tas/play.sh tas/runs/showcase/plans/S_L.txt $(cat tas/runs/showcase/plans/S_L.seed) runs/showcase-S_L
```

`S_L` is the 0-based stage and level (`0_0` = 1-1, `10_4` = 11-5). `plans/S_L.plan.txt` lists each shot: the window
pixel to aim at, the click tick and the predicted outcome.

## Levels

| Level | Name | Master | Seed | Shots | Total score |
|---|---|---|---:|---:|---:|
| 1-1 | Peggleland | Splork | 2 | 2 | 1,205,400 |
| 1-2 | Slip and Slide | Warren | 2 | 2 | 809,250 |
| 1-3 | Bjorn's Gazebo | Warren | 1 | 3 | 1,596,780 |
| 1-4 | Das Bucket | Renfield | 3 | 2 | 1,136,140 |
| 1-5 | Snow Day | Renfield | 1 | 3 | 903,900 |
| 2-1 | Birdy's Crib | Renfield | 3 | 3 | 1,165,750 |
| 2-2 | Buffalo Wings | Renfield | 3 | 3 | 713,790 |
| 2-3 | Skate Park | Tula | 4 | 3 | 1,083,280 |
| 2-4 | Spiral of Doom | Renfield | 3 | 3 | 766,810 |
| 2-5 | Mr. Peepers | Warren | 1 | 3 | 996,450 |
| 3-1 | Scarab Crunch | Renfield | 2 | 3 | 766,020 |
| 3-2 | Infinite Cheese | Renfield | 1 | 2 | 652,550 |
| 3-3 | Ra Deal | Warren | 3 | 3 | 1,771,760 |
| 3-4 | Croco-gator Pit | Renfield | 3 | 3 | 861,160 |
| 3-5 | The Fever Level | Warren | 3 | 3 | 1,636,250 |
| 4-1 | The Amoeban | Cinderbottom | 2 | 3 | 1,054,340 |
| 4-2 | The Last Flower | Tula | 2 | 2 | 851,420 |
| 4-3 | We Come In Peace | Warren | 1 | 3 | 2,420,950 |
| 4-4 | Maid In Space | Warren | 1 | 3 | 1,147,760 |
| 4-5 | Getting The Spare | Warren | 1 | 3 | 1,120,200 |
| 5-1 | Pearl Clam | Renfield | 1 | 3 | 977,890 |
| 5-2 | Insane Aquarium | Warren | 3 | 3 | 1,571,340 |
| 5-3 | Tasty Waves | Renfield | 4 | 3 | 898,530 |
| 5-4 | Our Favorite Eel | Warren | 2 | 3 | 847,290 |
| 5-5 | Love Story | Renfield | 4 | 3 | 794,530 |
| 6-1 | Waves | Jimmy | 2 | 2 | 774,770 |
| 6-2 | Spiderweb | Warren | 4 | 3 | 799,960 |
| 6-3 | Blockers | Renfield | 2 | 3 | 714,120 |
| 6-4 | Baseball | Warren | 1 | 3 | 873,750 |
| 6-5 | Vermin | Warren | 3 | 3 | 2,128,310 |
| 7-1 | Holland Oats | Jimmy | 4 | 3 | 943,780 |
| 7-2 | I Heart Flowers | Renfield | 4 | 3 | 639,070 |
| 7-3 | Workin From Home | Warren | 3 | 3 | 1,402,240 |
| 7-4 | Tula's Ride | Warren | 3 | 3 | 935,520 |
| 7-5 | 70 and Sunny | Renfield | 2 | 3 | 796,630 |
| 8-1 | Win a Monkey | Tula | 1 | 3 | 1,062,050 |
| 8-2 | Dog Pinball | Renfield | 4 | 3 | 721,540 |
| 8-3 | Spin Again | Warren | 4 | 3 | 957,000 |
| 8-4 | Roll 'em | Warren | 4 | 3 | 1,789,780 |
| 8-5 | Five of a Kind | Tula | 1 | 3 | 685,860 |
| 9-1 | The Love Moat | Renfield | 3 | 3 | 1,035,100 |
| 9-2 | Doom With a View | Jimmy | 1 | 3 | 732,050 |
| 9-3 | Rhombi | Cinderbottom | 4 | 3 | 1,141,540 |
| 9-4 | 9 Luft Balloons | Renfield | 1 | 3 | 1,140,250 |
| 9-5 | Twisted Sisters | Warren | 4 | 4 | 734,290 |
| 10-1 | Spin Cycle | Warren | 1 | 3 | 1,922,600 |
| 10-2 | The Dude Abides | Tula | 4 | 3 | 2,139,090 |
| 10-3 | When Pigs Fly | Hu | 1 | 3 | 688,600 |
| 10-4 | Yang, Yin | Tula | 3 | 4 | 1,032,800 |
| 10-5 | Zen Frog | Warren | 1 | 3 | 2,436,290 |
| 11-1 | Paw Reader | Renfield | 2 | 4 | 472,840 |
| 11-2 | End of Time | Renfield | 2 | 4 | 845,500 |
| 11-3 | Billions & Billions | Renfield | 4 | 3 | 705,930 |
| 11-4 | Don't Panic | Jimmy | 3 | 3 | 921,740 |
| 11-5 | Beyond Reason | Warren | 3 | 2 | 2,534,880 |
| **All 55** | | | | **162** | **61,457,420** |
