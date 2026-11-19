# One shot

Every level won with its **first shot**: all 25 orange pegs cleared by a single ball. All 55 levels won in the
real game; 54 match the simulator exactly. On 9-5 the shot follows the predicted path and wins, but the final total
is 798,800 against the predicted 784,010. The 55 levels total **47,059,470** points.

How: the RNG seed decides which pegs are orange and green, but not where the ball goes. So the search simulates a
library of first shots once per level, then searches the 4,294,967,296 seeds for one whose 25 oranges
(after the master's power) all lie on one shot's path. Most levels use Tula's Flower Power or Splork's Space Blast
to light extra pegs during the shot; 3 use no power at all (Bjorn), 6-3 Renfield's Spooky Ball and 7-4 Jimmy's
Multiball. The hardest 11 use Claude's flippers: the plan includes a sequence of flipper presses that keeps the
ball up until every orange is lit (11-1 juggles one ball for over four minutes).

## How these runs were made

The plans come from `solver/target/release/oneshot` (see [solver/ONESHOT.md](../../../solver/ONESHOT.md)): a
shot library per level, a seed scan (on a GPU for the classes it supports) and the exact simulator on the best
candidates; of a level's exact wins the one that ends soonest was kept (on 11-4 a tie with a higher predicted
score turned up after the recording; the plan here is the one that was played). Every plan was then played in the
real game and checked against the simulator (`game_check`). The *Seed* is the RNG seed the playback DLL fixes at
level start; it decides the peg colours and the luck, the way a TAS picks its luck.

The plans without flips are timed like the other runs (fast forward held). The flipper plans have `flip TICK 1|0`
lines and play without fast forward (`tas/play.sh` does that by itself). *Time* is the real time from the first
board frame to the Level Complete box, recorded without fast forward.

Replay any level (Windows, Steam running):

```sh
tas/play.sh tas/runs/oneshot/plans/S_L.txt $(cat tas/runs/oneshot/plans/S_L.seed) runs/oneshot-S_L
```

`S_L` is the 0-based stage and level (`0_0` = 1-1, `10_4` = 11-5). `plans/S_L.plan.txt` gives the window pixel to
aim at, the click tick and the predicted outcome. To play and record all of them:

```sh
mkdir -p out/oneshot && cp -r tas/runs/oneshot/plans out/oneshot/best
tas/pipeline.py oneshot out/oneshot play       # → out/oneshot/results.tsv
tas/pipeline.py oneshot out/oneshot video      # → out/oneshot/tas.mp4 (real time, about 40 minutes)
```

## Levels

| Level | Name | Master | Seed | Flips | Total score | Time (s) |
|---|---|---|---:|---:|---:|---:|
| 1-1 | Peggleland | Splork | 4280280876 |  | 1,025,080 | 19.74 |
| 1-2 | Slip and Slide | Tula | 4286703245 |  | 617,560 | 20.12 |
| 1-3 | Bjorn's Gazebo | Tula | 3788280221 |  | 664,310 | 23.77 |
| 1-4 | Das Bucket | Splork | 4278391995 |  | 903,150 | 19.26 |
| 1-5 | Snow Day | Tula | 4065948471 |  | 681,630 | 27.41 |
| 2-1 | Birdy's Crib | Splork | 3840404064 |  | 752,460 | 20.51 |
| 2-2 | Buffalo Wings | Tula | 4293741716 |  | 536,560 | 20.70 |
| 2-3 | Skate Park | Tula | 4279719388 |  | 761,520 | 33.11 |
| 2-4 | Spiral of Doom | Tula | 3627043448 |  | 756,020 | 30.66 |
| 2-5 | Mr. Peepers | Claude | 40296831 | 10 | 919,450 | 65.79 |
| 3-1 | Scarab Crunch | Tula | 2556186015 |  | 550,580 | 23.03 |
| 3-2 | Infinite Cheese | Tula | 4279365445 |  | 692,320 | 21.24 |
| 3-3 | Ra Deal | Tula | 4288263068 |  | 665,760 | 24.43 |
| 3-4 | Croco-gator Pit | Tula | 4283191368 |  | 550,000 | 21.61 |
| 3-5 | The Fever Level | Tula | 4261686533 |  | 891,610 | 20.64 |
| 4-1 | The Amoeban | Tula | 4278800078 |  | 620,880 | 25.49 |
| 4-2 | The Last Flower | Bjorn | 4287016897 |  | 1,129,840 | 55.02 |
| 4-3 | We Come In Peace | Splork | 4278686614 |  | 700,340 | 19.19 |
| 4-4 | Maid In Space | Splork | 4081562376 |  | 757,000 | 27.17 |
| 4-5 | Getting The Spare | Claude | 16310671 | 11 | 958,040 | 71.58 |
| 5-1 | Pearl Clam | Tula | 4169290034 |  | 622,790 | 20.45 |
| 5-2 | Insane Aquarium | Tula | 2360073963 |  | 801,950 | 25.88 |
| 5-3 | Tasty Waves | Tula | 2901433790 |  | 705,410 | 21.72 |
| 5-4 | Our Favorite Eel | Tula | 3701528228 |  | 665,640 | 21.54 |
| 5-5 | Love Story | Splork | 3821406628 |  | 806,600 | 18.14 |
| 6-1 | Waves | Bjorn | 4281037417 |  | 887,950 | 24.28 |
| 6-2 | Spiderweb | Claude | 4637413 | 15 | 2,764,400 | 106.41 |
| 6-3 | Blockers | Renfield | 4883902 |  | 1,059,060 | 37.21 |
| 6-4 | Baseball | Splork | 4174923688 |  | 699,040 | 25.80 |
| 6-5 | Vermin | Claude | 33830392 | 6 | 1,200,800 | 61.73 |
| 7-1 | Holland Oats | Tula | 4159961728 |  | 604,900 | 31.63 |
| 7-2 | I Heart Flowers | Claude | 2938668 | 6 | 858,720 | 54.07 |
| 7-3 | Workin From Home | Tula | 4279066043 |  | 641,880 | 26.77 |
| 7-4 | Tula's Ride | Jimmy | 2112757656 |  | 669,120 | 35.54 |
| 7-5 | 70 and Sunny | Claude | 34470077 | 4 | 709,640 | 49.07 |
| 8-1 | Win a Monkey | Tula | 4291200906 |  | 794,610 | 21.47 |
| 8-2 | Dog Pinball | Splork | 997954403 |  | 607,290 | 19.18 |
| 8-3 | Spin Again | Splork | 3957460673 |  | 701,230 | 17.73 |
| 8-4 | Roll 'em | Splork | 4164334928 |  | 821,360 | 26.00 |
| 8-5 | Five of a Kind | Claude | 105486404 | 16 | 1,016,020 | 86.60 |
| 9-1 | The Love Moat | Splork | 4173344129 |  | 1,041,650 | 26.56 |
| 9-2 | Doom With a View | Tula | 2520607599 |  | 623,730 | 23.98 |
| 9-3 | Rhombi | Splork | 116730836 |  | 868,580 | 18.93 |
| 9-4 | 9 Luft Balloons | Splork | 3176139343 |  | 862,830 | 18.07 |
| 9-5 | Twisted Sisters | Splork | 4243844348 |  | 798,800 (predicted 784,010) | 25.51 |
| 10-1 | Spin Cycle | Tula | 4282093409 |  | 661,240 | 28.72 |
| 10-2 | The Dude Abides | Claude | 173999149 | 19 | 2,123,040 | 122.81 |
| 10-3 | When Pigs Fly | Tula | 4072096390 |  | 493,340 | 17.97 |
| 10-4 | Yang, Yin | Claude | 253253792 | 6 | 933,680 | 63.71 |
| 10-5 | Zen Frog | Splork | 4280379148 |  | 1,141,000 | 26.81 |
| 11-1 | Paw Reader | Claude | 18600002 | 70 | 1,202,500 | 256.42 |
| 11-2 | End of Time | Claude | 159102524 | 26 | 986,490 | 107.14 |
| 11-3 | Billions & Billions | Tula | 3417566394 |  | 530,810 | 27.77 |
| 11-4 | Don't Panic | Splork | 3343935992 |  | 778,760 | 24.45 |
| 11-5 | Beyond Reason | Bjorn | 4278191238 |  | 1,240,500 | 35.27 |
| **All 55** | | | | | **47,059,470** | **2145.81** |
