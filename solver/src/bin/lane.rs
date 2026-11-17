//! Fever-lane analysis for one level: `lane LEVEL SEED CHAR [SCRIPT [N]]`
//! - the board's pegs by type (round pegs vs bricks);
//! - the best single shots on the starting board with every orange removed (the longest lanes: what an Extreme
//!   Fever shot could hit if the rest were cleared without touching them), with their peg sets;
//! - with SCRIPT: replays its first N shots (default all but the last) and reports the position before the
//!   Fever shot: pegs left, the script's next shot's hits, the best lane from there, and how much of the
//!   starting board's best lane is still standing.
use peggle_sim::phys::Geom;
use peggle_sim::score::{BLUE, GREEN, ORANGE, PURPLE};
use peggle_solver::{levels, search::*};
use rayon::prelude::*;

fn standing(g: &peggle_sim::game::Game) -> Vec<usize> {
    g.board.objs.iter().enumerate().filter(|(_, o)| o.has_peginfo && !o.hit).map(|(i, _)| i).collect()
}

/// the board with every unhit orange removed (as the score probe does)
fn no_oranges(g: &peggle_sim::game::Game) -> peggle_sim::game::Game {
    let mut g = g.clone();
    for o in g.board.objs.iter_mut() {
        if o.has_peginfo && o.peg_type == ORANGE && !o.hit { o.hit = true; o.collidable = false; }
    }
    g.board.orange_total = 0;
    g
}

/// every coarse angle × a few click delays: (pegs hit, angle index, delay, hit set), best first
fn lanes(g: &peggle_sim::game::Game, angles: &Angles, n: usize) -> Vec<(i32, usize, i32, Vec<usize>)> {
    let before = standing(g);
    let fan = angles.coarse(n);
    let jobs: Vec<(usize, i32)> = fan.iter().flat_map(|&a| [0, 8, 16, 32, 64].map(|d| (a, d))).collect();
    let mut v: Vec<(i32, usize, i32, Vec<usize>)> = jobs.par_iter().map(|&(ai, d)| {
        let mut h = g.clone();
        let r = h.shoot(angles.list[ai].0, g.earliest_fire + d);
        let hit: Vec<usize> = before.iter().copied().filter(|&i| h.board.objs[i].hit).collect();
        (r.pegs, ai, d, hit)
    }).collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v
}

fn kind(o: &peggle_sim::phys::Obj) -> &'static str {
    let t = match o.peg_type { BLUE => "blue", ORANGE => "orange", PURPLE => "purple", GREEN => "green", _ => "?" };
    match o.geom { Geom::Peg { .. } => t, _ => match o.peg_type { BLUE => "blue brick", ORANGE => "orange brick", PURPLE => "purple brick", GREEN => "green brick", _ => "? brick" } }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (st, ix) = levels::parse_level_id(&a[1]).expect("level");
    let seed: u32 = a[2].parse().unwrap();
    let ch: usize = a[3].parse().unwrap();
    let angles = Angles::new();
    let mut g = levels::new_game(st, ix, seed, peggle_sim::oracle::CHAR_POWERS[ch]);
    g.earliest_fire = rt::FIRST_CLICK;
    let mut counts = std::collections::BTreeMap::new();
    for o in g.board.objs.iter().filter(|o| o.has_peginfo) { *counts.entry(kind(o)).or_insert(0) += 1; }
    println!("{} seed {seed}: {} pegs {:?}", a[1], standing(&g).len(), counts);
    let l0 = lanes(&no_oranges(&g), &angles, 2000);
    println!("best lanes on the starting board (oranges removed):");
    for l in l0.iter().take(5) { println!("  {} pegs (angle #{}, +{} ticks)", l.0, l.1, l.2); }
    let best0: std::collections::HashSet<usize> = l0[0].3.iter().copied().collect();
    let mut bk = std::collections::BTreeMap::new();
    for &i in &best0 { *bk.entry(kind(&g.board.objs[i])).or_insert(0) += 1; }
    println!("  best lane's pegs by kind: {bk:?}");
    let ors: Vec<usize> = standing(&g).into_iter().filter(|&i| g.board.objs[i].peg_type == ORANGE).collect();
    let pos = |i: usize| match g.board.objs[i].geom { Geom::Peg { pos, .. } => format!("({:.0},{:.0})", pos[0], pos[1]), _ => "brick".into() };
    println!("  oranges: {}", ors.iter().map(|&i| format!("#{i} {} {}", kind(&g.board.objs[i]), pos(i))).collect::<Vec<_>>().join(", "));

    let dump = |path: &str, g: &peggle_sim::game::Game, sets: &[(&str, Vec<usize>)]| {
        let objs: Vec<String> = g.board.objs.iter().enumerate().filter(|(_, o)| o.has_peginfo).map(|(i, o)| {
            let tags: Vec<String> = sets.iter().filter(|s| s.1.contains(&i)).map(|s| format!("\"{}\"", s.0)).collect();
            format!("{{\"i\":{i},\"kind\":\"{}\",\"bbox\":{:?},\"hit\":{},\"tags\":[{}]}}", kind(o), o.bbox, o.hit, tags.join(","))
        }).collect();
        std::fs::write(path, format!("[{}]", objs.join(",\n"))).unwrap();
    };
    if let Ok(p) = std::env::var("LANE_JSON") { dump(&format!("{p}.start.json"), &g, &[("lane0", l0[0].3.clone()), ("lane1", l0[1].3.clone())]); }
    let Some(sc) = a.get(4) else { return };
    let shots: Vec<(f32, i32)> = std::fs::read_to_string(sc).unwrap().lines().filter_map(|l| {
        let w: Vec<&str> = l.split_whitespace().collect();
        (w.len() == 3 && w[0] == "shotx").then(|| (f32::from_bits(u32::from_str_radix(w[1], 16).unwrap()), w[2].parse().unwrap()))
    }).collect();
    let n: usize = a.get(5).map(|x| x.parse().unwrap()).unwrap_or(shots.len() - 1);
    let mut lane_lost_at = vec![];
    for (k, &(ang, click)) in shots[..n].iter().enumerate() {
        let before: Vec<usize> = standing(&g);
        let r = g.shoot(ang, click);
        let lost = before.iter().filter(|&&i| g.board.objs[i].hit && best0.contains(&i)).count();
        if lost > 0 { lane_lost_at.push(format!("shot {} -{lost}", k + 1)); }
        let _ = r;
    }
    let left = standing(&g);
    println!("after {n} shots: total {}, {} pegs left, {} oranges left, balls {}, tripler armed {}", g.total, left.len(), g.oranges_left(), g.balls, g.trip_now);
    println!("  starting best lane still standing: {}/{} (lost: {})", left.iter().filter(|i| best0.contains(i)).count(), best0.len(), lane_lost_at.join(", "));
    if let Some(&(ang, click)) = shots.get(n) {
        let mut h = g.clone();
        let r = h.shoot(ang, click);
        let hit: Vec<usize> = left.iter().copied().filter(|&i| h.board.objs[i].hit).collect();
        if let Ok(p) = std::env::var("LANE_JSON") { dump(&format!("{p}.final.json"), &g, &[("lane0", l0[0].3.clone()), ("final", hit.clone())]); }
        println!("  the script's next shot: {} pegs ({} orange), score {}, of the starting best lane {}", r.pegs, r.oranges, r.score, hit.iter().filter(|i| best0.contains(i)).count());
    }
    let l1 = lanes(&no_oranges(&g), &angles, 2000);
    println!("  best lanes from here (oranges removed): {:?}", l1.iter().take(5).map(|l| l.0).collect::<Vec<_>>());
}
