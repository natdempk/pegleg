# Max score

The highest final score on each level, as the game counts it at the Level Complete screen: peg points (scaled by
the number of oranges already cleared), style bonuses, free balls, the fever bucket, the Extreme Fever bonus and
10,000 per ball left. The 55 levels total **235,502,410** points; all 55 levels beat the Cyberscore per-level
record (records fetched 2026-11-14; their sum is 98,899,110).

The strategy the search found: Warren (Lucky Spin) can land the Score Tripler, which triples a shot. The best plans
save a dense lane of pegs and the last orange for one tripled shot during Extreme Fever, where every peg after the
last orange scores 100x, and farm free balls along the way. The seed matters as much as the aim: the best seeds
put the last orange at the start of a long lane and keep the other oranges off it.

## How these runs were made

The plans come from the same searches `tas/pipeline.py` runs (its `CATEGORIES` table has the settings). Most levels'
best plans come from its `scout` phase: `scout` ranks 100,000 seeds by their Extreme Fever lane, then the Fever
search runs on the best seeds with the score-search options (`--streak-value`, `--review`; see `solver/README.md`).
The rest come from the cheap beam search over several masters and RNG seeds, then wider searches on the best
candidates. Every plan was then played in the
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
| 1-1 | Peggleland | Warren | 80483 | 5 | 3,979,440 | 1,868,400 | ahead |
| 1-2 | Slip and Slide | Warren | 56500 | 18 | 4,128,830 | 1,739,030 | ahead |
| 1-3 | Bjorn's Gazebo | Warren | 72058 | 32 | 3,261,430 | 2,000,460 | ahead |
| 1-4 | Das Bucket | Warren | 7 | 23 | 3,218,080 | 2,783,660 | ahead |
| 1-5 | Snow Day | Warren | 10 | 29 | 3,521,370 | 2,417,530 | ahead |
| 2-1 | Birdy's Crib | Warren | 2 | 29 | 2,936,800 | 1,816,390 | ahead |
| 2-2 | Buffalo Wings | Warren | 26033 | 26 | 3,659,680 | 2,081,040 | ahead |
| 2-3 | Skate Park | Warren | 35668 | 45 | 2,790,020 | 1,147,350 | ahead |
| 2-4 | Spiral of Doom | Warren | 563404 | 54 | 6,996,970 | 4,805,370 | ahead |
| 2-5 | Mr. Peepers | Warren | 2 | 32 | 4,775,020 | 1,481,480 | ahead |
| 3-1 | Scarab Crunch | Warren | 41007 | 54 | 3,400,020 | 2,168,810 | ahead |
| 3-2 | Infinite Cheese | Warren | 4670 | 19 | 5,142,060 | 1,137,430 | ahead |
| 3-3 | Ra Deal | Warren | 92049 | 28 | 4,099,600 | 1,274,130 | ahead |
| 3-4 | Croco-gator Pit | Warren | 12310 | 25 | 3,494,490 | 1,281,860 | ahead |
| 3-5 | The Fever Level | Warren | 68667 | 54 | 5,582,610 | 1,551,690 | ahead |
| 4-1 | The Amoeban | Warren | 34410 | 32 | 5,081,220 | 1,034,930 | ahead |
| 4-2 | The Last Flower | Warren | 38050 | 34 | 7,065,170 | 1,489,850 | ahead |
| 4-3 | We Come In Peace | Warren | 17071 | 59 | 4,223,410 | 1,189,500 | ahead |
| 4-4 | Maid In Space | Warren | 11930 | 32 | 4,551,510 | 1,047,460 | ahead |
| 4-5 | Getting The Spare | Warren | 5792 | 26 | 5,321,910 | 1,087,010 | ahead |
| 5-1 | Pearl Clam | Warren | 18857 | 35 | 3,121,620 | 1,353,360 | ahead |
| 5-2 | Insane Aquarium | Warren | 52630 | 9 | 3,577,510 | 1,659,080 | ahead |
| 5-3 | Tasty Waves | Warren | 504 | 8 | 3,642,090 | 1,356,460 | ahead |
| 5-4 | Our Favorite Eel | Warren | 45934 | 18 | 3,152,460 | 1,372,750 | ahead |
| 5-5 | Love Story | Warren | 2982 | 15 | 2,797,500 | 1,849,210 | ahead |
| 6-1 | Waves | Warren | 3081 | 51 | 3,535,500 | 1,519,870 | ahead |
| 6-2 | Spiderweb | Warren | 1065 | 17 | 4,170,870 | 1,605,640 | ahead |
| 6-3 | Blockers | Warren | 22322 | 45 | 3,143,040 | 1,858,390 | ahead |
| 6-4 | Baseball | Warren | 20883 | 37 | 3,641,130 | 2,076,660 | ahead |
| 6-5 | Vermin | Warren | 87 | 25 | 6,230,830 | 3,536,590 | ahead |
| 7-1 | Holland Oats | Warren | 2 | 16 | 4,046,930 | 1,658,760 | ahead |
| 7-2 | I Heart Flowers | Warren | 23849 | 12 | 2,843,630 | 1,541,730 | ahead |
| 7-3 | Workin From Home | Warren | 66055 | 35 | 4,171,910 | 1,469,220 | ahead |
| 7-4 | Tula's Ride | Warren | 2 | 36 | 3,958,340 | 1,179,130 | ahead |
| 7-5 | 70 and Sunny | Warren | 3041 | 32 | 2,178,680 | 1,184,290 | ahead |
| 8-1 | Win a Monkey | Warren | 101 | 57 | 5,064,650 | 1,227,130 | ahead |
| 8-2 | Dog Pinball | Warren | 1458 | 9 | 5,058,330 | 1,478,730 | ahead |
| 8-3 | Spin Again | Warren | 16189 | 48 | 3,321,740 | 1,097,590 | ahead |
| 8-4 | Roll 'em | Warren | 50718 | 40 | 4,623,070 | 2,116,580 | ahead |
| 8-5 | Five of a Kind | Warren | 84505 | 43 | 2,338,530 | 1,291,680 | ahead |
| 9-1 | The Love Moat | Warren | 2 | 32 | 4,674,610 | 3,215,320 | ahead |
| 9-2 | Doom With a View | Warren | 2 | 10 | 4,465,380 | 2,106,170 | ahead |
| 9-3 | Rhombi | Warren | 9 | 28 | 5,049,800 | 2,129,460 | ahead |
| 9-4 | 9 Luft Balloons | Warren | 55302 | 58 | 3,905,590 | 1,302,220 | ahead |
| 9-5 | Twisted Sisters | Warren | 9 | 55 | 2,984,190 | 2,277,270 | ahead |
| 10-1 | Spin Cycle | Warren | 80440 | 5 | 3,973,930 | 1,187,610 | ahead |
| 10-2 | The Dude Abides | Warren | 3961 | 47 | 9,982,760 | 1,048,520 | ahead |
| 10-3 | When Pigs Fly | Warren | 75432 | 37 | 2,414,440 | 1,133,380 | ahead |
| 10-4 | Yang, Yin | Warren | 2467 | 36 | 2,927,200 | 1,404,720 | ahead |
| 10-5 | Zen Frog | Warren | 87284 | 49 | 4,728,560 | 1,608,730 | ahead |
| 11-1 | Paw Reader | Warren | 1 | 16 | 3,458,270 | 1,763,720 | ahead |
| 11-2 | End of Time | Warren | 15 | 8 | 2,122,390 | 1,905,820 | ahead |
| 11-3 | Billions & Billions | Warren | 3 | 58 | 2,676,080 | 1,379,590 | ahead |
| 11-4 | Don't Panic | Warren | 24377 | 55 | 2,746,230 | 1,028,580 | ahead |
| 11-5 | Beyond Reason | Warren | 90169 | 16 | 17,544,980 | 7,571,770 | ahead |
| **All 55** | | | | **1754** | **235,502,410** | **98,899,110** | |
