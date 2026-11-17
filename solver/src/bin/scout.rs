//! Seed scouting by Fever lane: `scout LEVEL CHAR FROM TO [TOP]`. The seed decides which pegs are orange and
//! green; the best max-score runs keep a lane of pegs for the winning shot, which only pays when the last orange
//! sits at the lane's start and the other oranges are off it. Three stages:
//! 1. Lane library: for `SCOUT_LIB` (default 48) sample seeds, the board with that seed's oranges cleared; the
//!    first-contact sequences of a fan of shots, longest first (the geometry once the oranges are gone).
//! 2. Every seed in FROM..=TO against the library (no simulation): a lane is worth its pegs after its first
//!    orange of this seed, minus the seed's later oranges on it, plus a green after it (Warren's Lucky Spin
//!    can triple the Fever shot).
//! 3. The best `SCOUT_EXACT` (default 40) of those, simulated: for each orange L, the board with every other
//!    orange cleared and a fan of shots: the best winning shot's score (the Fever shot if L goes last).
//!    Seeds are printed best first by that, with L.
//! Upper bounds for the search to aim at, not plans: clearing the other oranges costs balls and pegs.
use peggle_sim::game::Game;
use peggle_sim::score::{GREEN, ORANGE};
use peggle_solver::{levels, search::*};
use rayon::prelude::*;

fn env(k: &str, d: usize) -> usize { std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d) }

/// the board with these oranges cleared (as if shot away without touching anything else)
fn clear(g: &Game, keep: Option<usize>) -> Game {
    let mut g = g.clone();
    let mut left = 0;
    for (i, o) in g.board.objs.iter_mut().enumerate() {
        if o.has_peginfo && o.peg_type == ORANGE && !o.hit {
            if Some(i) == keep { left += 1; } else { o.hit = true; o.collidable = false; }
        }
    }
    g.board.orange_total = left;
    g
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (st, ix) = levels::parse_level_id(&a[1]).expect("level");
    let ch: usize = a[2].parse().unwrap();
    let (from, to): (u32, u32) = (a[3].parse().unwrap(), a[4].parse().unwrap());
    let top: usize = a.get(5).map_or(20, |x| x.parse().unwrap());
    let pw = peggle_sim::oracle::CHAR_POWERS[ch];
    let angles = Angles::new();
    let game = |seed: u32| { let mut g = levels::new_game(st, ix, seed, pw); g.earliest_fire = rt::FIRST_CLICK; g };
    let delays = [0, 8, 16, 32, 64];

    // 1. lane library
    let fan = angles.coarse(1500);
    let jobs: Vec<(u32, usize, i32)> = (0..env("SCOUT_LIB", 48) as u32).flat_map(|k| {
        let seed = 1_000_003 + k * 7919;
        fan.iter().flat_map(move |&ai| delays.map(move |d| (seed, ai, d)))
    }).collect();
    let bases: std::collections::HashMap<u32, Game> = jobs.iter().map(|j| j.0).collect::<std::collections::BTreeSet<_>>().into_iter().map(|s| (s, clear(&game(s), None))).collect();
    let mut lanes: Vec<Vec<usize>> = jobs.par_iter().map(|&(s, ai, d)| {
        let mut h = bases[&s].clone();
        let c = h.earliest_fire + d;
        h.shoot(angles.list[ai].0, c);
        h.board.shot_hits.clone()
    }).collect();
    lanes.sort_by(|x, y| y.len().cmp(&x.len()));
    let mut seen = std::collections::HashSet::new();
    lanes.retain(|l| l.len() >= 6 && seen.insert(l.clone()));
    lanes.truncate(env("SCOUT_LANES", 2000));
    eprintln!("{}: lane library {} (longest {:?})", a[1], lanes.len(), lanes.iter().take(6).map(|l| l.len()).collect::<Vec<_>>());

    // 2. rank seeds against the library: the lane's pegs before its first orange can be absent in this seed's
    // geometry only if they are oranges (cleared earlier), so a lane counts from its first orange on
    let rank = |seed: u32| -> (i64, usize) {
        let s = levels::new_game(st, ix, seed, pw);
        let ty = |i: usize| s.board.objs[i].peg_type;
        let mut best = (i64::MIN, 0);
        for (li, l) in lanes.iter().enumerate() {
            let Some(p) = l.iter().position(|&i| ty(i) == ORANGE) else { continue };
            let after = &l[p + 1..];
            let later = after.iter().filter(|&&i| ty(i) == ORANGE).count() as i64;
            let greens = after.iter().any(|&i| ty(i) == GREEN) as i64;
            let val = (after.len() as i64 - later) * 10 - later * 60 + greens * 40;
            if val > best.0 { best = (val, li); }
        }
        best
    };
    let mut v: Vec<(i64, u32, usize)> = (from..=to).into_par_iter().map(|seed| { let r = rank(seed); (-r.0, seed, r.1) }).collect();
    v.sort();
    let n_exact = env("SCOUT_EXACT", 40).min(v.len());
    eprintln!("stage 2: best library values {:?}", v.iter().take(8).map(|x| (x.1, -x.0)).collect::<Vec<_>>());

    // 3. exact Fever shot per (seed, last orange)
    let fan3 = angles.coarse(800);
    let mut out: Vec<(i64, u32, usize, i32, i64)> = vec![];
    for x in v.iter().take(n_exact) {
        let g = game(x.1);
        let ors: Vec<usize> = g.board.objs.iter().enumerate().filter(|(_, o)| o.has_peginfo && o.peg_type == ORANGE).map(|(i, _)| i).collect();
        let jobs: Vec<(usize, usize, i32)> = ors.iter().flat_map(|&l| fan3.iter().flat_map(move |&ai| delays.map(move |d| (l, ai, d)))).collect();
        let bases: std::collections::HashMap<usize, Game> = ors.iter().map(|&l| (l, clear(&g, Some(l)))).collect();
        let (best, bj) = jobs.par_iter().map(|&(l, ai, d)| {
            let mut h = bases[&l].clone();
            let c = h.earliest_fire + d;
            let r = h.shoot(angles.list[ai].0, c);
            (if r.won { (r.score, l, r.pegs) } else { (0, l, 0) }, (l, ai, d))
        }).max().unwrap_or(((0, 0, 0), (0, 0, 0)));
        // SCOUT_TABU=DIR: the best Fever shot's pegs (orange L first) as DIR/SEED.tabu, for the solver's --tabu
        if let (Ok(dir), true) = (std::env::var("SCOUT_TABU"), best.0 > 0) {
            let mut h = bases[&bj.0].clone();
            let c = h.earliest_fire + bj.2;
            h.shoot(angles.list[bj.1].0, c);
            let _ = std::fs::create_dir_all(&dir);
            std::fs::write(format!("{dir}/{}.tabu", x.1), h.board.shot_hits.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(" ") + "\n").unwrap();
        }
        eprintln!("  seed {}: library value {}, Fever shot {} ({} pegs, last orange #{})", x.1, -x.0, best.0, best.2, best.1);
        out.push((best.0, x.1, best.1, best.2, -x.0));
    }
    out.sort_by(|a, b| b.0.cmp(&a.0));
    for o in out.iter().take(top) { println!("seed {}: Fever shot {} with orange #{} last ({} pegs); library value {}", o.1, o.0, o.2, o.3, o.4); }
}
