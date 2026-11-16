# Max score

The highest final score on each level, as the game counts it at the Level Complete screen: peg points (scaled by
the number of oranges already cleared), style bonuses, free balls, the fever bucket, the Extreme Fever bonus and
10,000 per ball left. The 55 levels total **188,496,960** points; 53 of 55 levels beat the Cyberscore per-level
record (records fetched 2026-11-14; their sum is 98,899,110).

The strategy the search found: Warren (Lucky Spin) can land the Score Tripler, which triples a shot. The best plans
save a dense lane of pegs and the last orange for one tripled shot during Extreme Fever, where every peg after the
last orange scores 100x, and farm free balls along the way.

## How these runs were made

The plans come from the same search `tas/pipeline.py` runs (its `CATEGORIES` table has the settings; some levels
were searched again with more seeds or wider beams): a cheap beam search over several masters and RNG seeds, then
wider searches on the best candidates. Every plan was then played in the
real game with `tas/play.sh` and checked shot by shot against the simulator (`game_check`). All 55 runs here won in
the real game and match the simulator exactly. The *Seed* is the RNG seed the playback DLL fixes at level start;
it decides the peg colours and the luck (Lucky Spins, coin flips), the way a TAS picks its luck.

Replay any level (Windows, Steam running):

```sh
tas/play.sh tas/runs/max-score/plans/S_L.txt $(cat tas/runs/max-score/plans/S_L.seed) runs/max-score-S_L
```

`S_L` is the 0-based stage and level (`0_0` = 1-1, `10_4` = 11-5). `plans/S_L.plan.txt` lists each shot: the window
pixel to aim at, the click tick and the predicted outcome.

## Levels

| Level | Name | Master | Seed | Shots | Total score | Cyberscore record | |
|---|---|---|---:|---:|---:|---:|---|
| 1-1 | Peggleland | Warren | 12 | 40 | 3,556,990 | 1,868,400 | ahead |
| 1-2 | Slip and Slide | Warren | 2 | 55 | 2,488,620 | 1,739,030 | ahead |
| 1-3 | Bjorn's Gazebo | Warren | 3 | 32 | 3,155,450 | 2,000,460 | ahead |
| 1-4 | Das Bucket | Warren | 7 | 23 | 3,218,080 | 2,783,660 | ahead |
| 1-5 | Snow Day | Warren | 1 | 34 | 2,772,640 | 2,417,530 | ahead |
| 2-1 | Birdy's Crib | Warren | 2 | 29 | 2,936,800 | 1,816,390 | ahead |
| 2-2 | Buffalo Wings | Warren | 3 | 31 | 2,790,980 | 2,081,040 | ahead |
| 2-3 | Skate Park | Warren | 2 | 45 | 2,227,010 | 1,147,350 | ahead |
| 2-4 | Spiral of Doom | Warren | 3 | 52 | 3,594,320 | 4,805,370 | behind |
| 2-5 | Mr. Peepers | Warren | 2 | 32 | 4,775,020 | 1,481,480 | ahead |
| 3-1 | Scarab Crunch | Warren | 1 | 13 | 2,559,810 | 2,168,810 | ahead |
| 3-2 | Infinite Cheese | Warren | 2 | 39 | 2,853,330 | 1,137,430 | ahead |
| 3-3 | Ra Deal | Warren | 2 | 41 | 3,110,520 | 1,274,130 | ahead |
| 3-4 | Croco-gator Pit | Warren | 2 | 49 | 2,173,580 | 1,281,860 | ahead |
| 3-5 | The Fever Level | Warren | 2 | 25 | 5,167,110 | 1,551,690 | ahead |
| 4-1 | The Amoeban | Warren | 3 | 32 | 4,283,650 | 1,034,930 | ahead |
| 4-2 | The Last Flower | Warren | 1 | 20 | 4,458,760 | 1,489,850 | ahead |
| 4-3 | We Come In Peace | Warren | 2 | 15 | 2,505,450 | 1,189,500 | ahead |
| 4-4 | Maid In Space | Warren | 1 | 32 | 3,396,840 | 1,047,460 | ahead |
| 4-5 | Getting The Spare | Warren | 1 | 23 | 4,516,180 | 1,087,010 | ahead |
| 5-1 | Pearl Clam | Warren | 1 | 17 | 1,900,820 | 1,353,360 | ahead |
| 5-2 | Insane Aquarium | Warren | 3 | 13 | 3,070,840 | 1,659,080 | ahead |
| 5-3 | Tasty Waves | Warren | 2 | 60 | 2,986,260 | 1,356,460 | ahead |
| 5-4 | Our Favorite Eel | Warren | 1 | 8 | 2,759,970 | 1,372,750 | ahead |
| 5-5 | Love Story | Warren | 3 | 9 | 2,351,180 | 1,849,210 | ahead |
| 6-1 | Waves | Warren | 3 | 58 | 2,990,700 | 1,519,870 | ahead |
| 6-2 | Spiderweb | Warren | 1 | 5 | 2,675,860 | 1,605,640 | ahead |
| 6-3 | Blockers | Warren | 3 | 17 | 1,896,310 | 1,858,390 | ahead |
| 6-4 | Baseball | Warren | 3 | 58 | 2,537,670 | 2,076,660 | ahead |
| 6-5 | Vermin | Warren | 10 | 50 | 3,270,200 | 3,536,590 | behind |
| 7-1 | Holland Oats | Warren | 2 | 16 | 4,046,930 | 1,658,760 | ahead |
| 7-2 | I Heart Flowers | Warren | 1 | 42 | 1,748,730 | 1,541,730 | ahead |
| 7-3 | Workin From Home | Warren | 3 | 14 | 3,492,290 | 1,469,220 | ahead |
| 7-4 | Tula's Ride | Warren | 2 | 36 | 3,958,340 | 1,179,130 | ahead |
| 7-5 | 70 and Sunny | Warren | 1 | 12 | 1,382,110 | 1,184,290 | ahead |
| 8-1 | Win a Monkey | Warren | 3 | 7 | 4,248,280 | 1,227,130 | ahead |
| 8-2 | Dog Pinball | Warren | 1 | 12 | 2,774,960 | 1,478,730 | ahead |
| 8-3 | Spin Again | Warren | 1 | 32 | 2,458,230 | 1,097,590 | ahead |
| 8-4 | Roll 'em | Warren | 13 | 32 | 3,538,020 | 2,116,580 | ahead |
| 8-5 | Five of a Kind | Warren | 2 | 39 | 1,796,540 | 1,291,680 | ahead |
| 9-1 | The Love Moat | Warren | 2 | 32 | 4,674,610 | 3,215,320 | ahead |
| 9-2 | Doom With a View | Warren | 2 | 10 | 4,465,380 | 2,106,170 | ahead |
| 9-3 | Rhombi | Warren | 9 | 28 | 5,049,800 | 2,129,460 | ahead |
| 9-4 | 9 Luft Balloons | Warren | 1 | 8 | 3,593,850 | 1,302,220 | ahead |
| 9-5 | Twisted Sisters | Warren | 9 | 55 | 2,984,190 | 2,277,270 | ahead |
| 10-1 | Spin Cycle | Warren | 2 | 47 | 3,743,860 | 1,187,610 | ahead |
| 10-2 | The Dude Abides | Warren | 2 | 40 | 8,820,590 | 1,048,520 | ahead |
| 10-3 | When Pigs Fly | Warren | 1 | 24 | 2,311,780 | 1,133,380 | ahead |
| 10-4 | Yang, Yin | Warren | 1 | 34 | 1,778,790 | 1,404,720 | ahead |
| 10-5 | Zen Frog | Warren | 3 | 45 | 4,492,740 | 1,608,730 | ahead |
| 11-1 | Paw Reader | Warren | 1 | 16 | 3,458,270 | 1,763,720 | ahead |
| 11-2 | End of Time | Warren | 15 | 8 | 2,122,390 | 1,905,820 | ahead |
| 11-3 | Billions & Billions | Warren | 3 | 58 | 2,676,080 | 1,379,590 | ahead |
| 11-4 | Don't Panic | Warren | 2 | 5 | 2,260,130 | 1,028,580 | ahead |
| 11-5 | Beyond Reason | Warren | 1 | 43 | 13,639,120 | 7,571,770 | ahead |
| **All 55** | | | | **1652** | **188,496,960** | **98,899,110** | |
