//! peggle-solver: per-level search for human-reproducible Peggle Deluxe inputs (TAS scripts).
//!
//! usage: peggle-solver LEVEL [options]
//!   LEVEL                 1-1 .. 11-5 (speedrun.com numbering), S_L (0-based) or a level name
//!   --objective O         speed (default) | score | pegs | shotscore
//!   --seeds A-B | N,M     hook RNG seeds to try (default 5); the best plan over all seeds wins
//!   --beam N --coarse N --delays 0,10,25 --refine-top N --refine-rounds N --max-shots N
//!   --keep-ball           every shot must return the ball (bucket catch or free ball)
//!   --orange-cost T       speed heuristic weight (ticks per orange left)
//!   --power P             override the character power (needed for stage 11 with seeds other than 5)
//!   --chars all | 0,3,6   individual-level runs may pick the master: search these characters
//!                         (0 Bjorn, 1 Jimmy, 2 Kat, 3 Splork, 4 Claude, 5 Renfield, 6 Tula, 7 Warren,
//!                         8 Cinderbottom, 9 Hu); the script selects it (`char N`)
//!   --no-ff               old tick objective (no fast forward, any click tick)
//!   --first-click T       earliest first click (harness tick; default 103 with fast forward, else 310)
//!   --out DIR             write DIR/<S_L>.txt (harness script) and DIR/<S_L>.plan.txt
//!   --bench               only measure shot-simulation throughput on this level
use peggle_solver::{levels, search::*};
use std::sync::atomic::Ordering;
use std::time::Instant;

fn parse_seeds(s: &str) -> Vec<u32> {
    if let Some((a, b)) = s.split_once('-') { return (a.parse().unwrap()..=b.parse().unwrap()).collect(); }
    s.split(',').map(|x| x.parse().unwrap()).collect()
}

fn main() {
    // keep the machine usable: by default leave two cores free (RAYON_NUM_THREADS overrides)
    if std::env::var_os("RAYON_NUM_THREADS").is_none() {
        let n = std::thread::available_parallelism().map_or(1, |n| n.get());
        let _ = rayon::ThreadPoolBuilder::new().num_threads(n.saturating_sub(2).max(1)).build_global();
    }
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 { eprintln!("usage: peggle-solver LEVEL [options] (see src/main.rs)"); std::process::exit(2); }
    let (st, ix) = levels::parse_level_id(&args[1]).expect("unknown level");
    let mut p = Params::default();
    let mut seeds = vec![5u32];
    let mut power = None;
    let mut chars: Vec<Option<usize>> = vec![None];
    let mut first_click: Option<i32> = None;
    let mut out: Option<String> = None;
    let mut bench = false;
    let mut top = 10;
    // --prefix SCRIPT --prefix-shots N: play the script's first N shots, then search the rest of the level
    let mut prefix: Option<String> = None;
    let mut prefix_shots = 0usize;
    let mut compact_plan = false;
    let mut i = 2;
    while i < args.len() {
        let v = args.get(i + 1).cloned().unwrap_or_default();
        match args[i].as_str() {
            "--objective" => { p.objective = match v.as_str() { "speed" => Objective::Speed, "score" => Objective::Score, "pegs" => Objective::Pegs, "shotscore" => Objective::ShotScore, "uef" => Objective::Uef, "shots" => Objective::Shots, _ => panic!("objective") }; i += 1; }
            "--seeds" => { seeds = parse_seeds(&v); i += 1; }
            "--beam" => { p.beam = v.parse().unwrap(); i += 1; }
            "--coarse" => { p.coarse = v.parse().unwrap(); i += 1; }
            "--delays" => { p.delays = v.split(',').map(|x| x.parse().unwrap()).collect(); i += 1; }
            "--refine-top" => { p.refine_top = v.parse().unwrap(); i += 1; }
            "--refine-rounds" => { p.refine_rounds = v.parse().unwrap(); i += 1; }
            "--max-shots" => { p.max_shots = v.parse().unwrap(); i += 1; }
            "--orange-cost" => { p.orange_cost = v.parse().unwrap(); i += 1; }
            "--score-orange-cost" => { p.score_orange_cost = v.parse().unwrap(); i += 1; }
            "--trip-bonus" => { p.trip_bonus = v.parse().unwrap(); i += 1; }
            "--probe" => { p.probe_n = v.parse().unwrap(); i += 1; }
            "--probe-pool" => { p.probe_pool = v.parse().unwrap(); i += 1; }
            "--probe-untrip" => { p.probe_untrip = v.parse().unwrap(); i += 1; }
            "--strata" => p.strata = true,
            "--streak-value" => { p.streak_value = v.parse().unwrap(); i += 1; }
            "--bucket-top" => { p.bucket_top = v.parse().unwrap(); i += 1; }
            "--bucket-max" => { p.bucket_max = v.parse().unwrap(); i += 1; }
            "--probe-exact" => p.probe_exact = true,
            "--probe-delays" => { p.probe_delays = v.split(',').map(|x| x.parse().unwrap()).collect(); i += 1; }
            "--sig-full" => p.sig_full = true,
            "--cells" => p.cells = true,
            "--tabu" => { p.tabu = std::fs::read_to_string(&v).unwrap().split_whitespace().map(|x| x.parse().unwrap()).collect(); i += 1; }
            "--tabu-cost" => { p.tabu_cost = v.parse().unwrap(); i += 1; }
            "--final-scan" => { p.final_scan = v.parse().unwrap(); i += 1; }
            "--compact" => compact_plan = true,
            // the score-search options below together (each flag above alone for A/B runs)
            "--review" => { p.bucket_top = 6; p.probe_exact = true; p.sig_full = true; p.cells = true; p.final_scan = 2; compact_plan = true; }
            "--probe-weight" => { p.probe_weight = v.parse().unwrap(); i += 1; }
            "--fever-frac" => { p.fever_frac = v.parse().unwrap(); i += 1; }
            "--peg-cost" => { p.peg_cost = v.parse().unwrap(); i += 1; }
            "--keep-ball" => p.keep_ball = true,
            "--power" => { power = Some(v.parse().unwrap()); i += 1; }
            "--chars" => { chars = if v == "all" { (0..10).map(Some).collect() } else { v.split(',').map(|x| Some(x.parse().unwrap())).collect() }; i += 1; }
            "--no-ff" => p.ff = false,
            "--first-click" => { first_click = Some(v.parse().unwrap()); i += 1; }
            "--out" => { out = Some(v); i += 1; }
            "--top" => { top = v.parse().unwrap(); i += 1; }
            "--bench" => bench = true,
            "--prefix" => { prefix = Some(v); i += 1; }
            "--prefix-shots" => { prefix_shots = v.parse().unwrap(); i += 1; }
            a => panic!("unknown option {a}"),
        }
        i += 1;
    }
    let name = levels::LEVELS[st][ix];
    let t_ang = Instant::now();
    let angles = Angles::new();
    eprintln!("{} {} ({}): {} reachable aim angles ({:?}), {} threads", st + 1, ix + 1, name, angles.list.len(), t_ang.elapsed(), rayon::current_num_threads());
    let stats = Stats { shots: Default::default(), frames: Default::default() };
    let t0 = Instant::now();

    if bench {
        let g = levels::new_game(st, ix, seeds[0], levels::power_for(st, ix, seeds[0], power));
        let mut q = p.clone(); q.refine_rounds = 0; q.delays = vec![0]; q.coarse = 4000;
        let n = Node::new(g, Default::default(), 0, 0, &p);
        let _ = best_single(&n.game, &q, &angles, &stats, 1);
        let dt = t0.elapsed().as_secs_f64();
        let (s, f) = (stats.shots.load(Ordering::Relaxed), stats.frames.load(Ordering::Relaxed));
        println!("{name}: {s} first shots in {dt:.2}s = {:.0} shots/s, {:.2}M ball-frames/s ({:.0} frames/shot)", s as f64 / dt, f as f64 / dt / 1e6, f as f64 / s as f64);
        return;
    }

    let single = matches!(p.objective, Objective::Pegs | Objective::ShotScore);
    let mut results: Vec<(u32, Option<usize>, Node)> = vec![];
    let first = first_click.unwrap_or(if p.ff { rt::FIRST_CLICK } else { peggle_sim::game::FIRST_FIRE });
    for &ch in &chars {
    for &seed in &seeds {
        let pw = match ch { Some(c) => peggle_sim::oracle::CHAR_POWERS[c], None => levels::power_for(st, ix, seed, power) };
        let mut g = levels::new_game(st, ix, seed, pw);
        g.earliest_fire = first;
        let ts = Instant::now();
        if single {
            let v = best_single(&g, &p, &angles, &stats, top);
            eprintln!("seed {seed}: {:?}", ts.elapsed());
            for n in v { results.push((seed, ch, n)); }
        } else {
            let o = if let Some(pf) = &prefix {
                let shots: Vec<(f32, i32)> = std::fs::read_to_string(pf).unwrap().lines().filter_map(|l| {
                    let w: Vec<&str> = l.split_whitespace().collect();
                    (w.len() == 3 && w[0] == "shotx").then(|| (f32::from_bits(u32::from_str_radix(w[1], 16).unwrap()), w[2].parse().unwrap()))
                }).take(prefix_shots).collect();
                let n = prefix_node(&g, &shots, &p, &angles);
                eprintln!("prefix: {} shots, total {}, {} oranges left, balls {}, frame grid at tick {}", n.steps.len(), n.game.total, n.game.oranges_left(), n.game.balls, n.grid0);
                eprintln!("seed {seed} char {ch:?} (power {pw}, {} oranges):", n.game.oranges_left());
                solve_node(n, &p, &angles, &stats, true)
            } else {
                eprintln!("seed {seed} char {ch:?} (power {pw}, {} oranges):", g.oranges_left());
                solve(&g, &p, &angles, &stats, true)
            };
            let best = o.best.map(|b| {
                if !compact_plan { return b; }
                match compact(&g, &b.steps.to_vec(), &p, &angles) {
                    Some(c) if c.game.total > b.game.total => {
                        eprintln!("compaction: {} -> {} shots, total {} -> {}", b.steps.len(), c.steps.len(), b.game.total, c.game.total);
                        c
                    }
                    _ => b,
                }
            });
            match best {
                Some(b) => {
                    let r = replay(&g, &b.steps.to_vec());
                    eprintln!("seed {seed} char {ch:?}: {} shots, {} frames (~{:.2} s + load), end tick {}, total {} ({:?}){}", b.steps.len(), b.frames, rt::secs(b.frames), b.steps.last().unwrap().res.end_tick, b.game.total, ts.elapsed(),
                              if let Err(e) = &r { format!(" REPLAY FAILED: {e}") } else { String::new() });
                    if r.is_ok() { results.push((seed, ch, b)); }
                }
                None => eprintln!("seed {seed}: no solution found ({:?})", ts.elapsed()),
            }
        }
    }
    }
    let dt = t0.elapsed().as_secs_f64();
    let (s, f) = (stats.shots.load(Ordering::Relaxed), stats.frames.load(Ordering::Relaxed));
    eprintln!("search: {s} shots simulated in {dt:.1}s ({:.0} shots/s, {:.2}M ball-frames/s)", s as f64 / dt, f as f64 / dt / 1e6);
    let key = |n: &Node| -> f64 {
        match p.objective {
            Objective::Speed | Objective::Uef => n.frames as f64 - n.game.total as f64 * 1e-9,
            Objective::Score => -(n.game.total as f64),
            Objective::Shots => n.steps.len() as f64 * 1e12 - n.game.total as f64,
            Objective::Pegs => -(n.steps.last().unwrap().res.pegs as f64 * 1e7 + n.steps.last().unwrap().res.score as f64),
            Objective::ShotScore => -(n.steps.last().unwrap().res.score as f64),
        }
    };
    results.sort_by(|a, b| key(&a.2).partial_cmp(&key(&b.2)).unwrap());
    if chars.len() > 1 && !single {
        println!("per character (best seed):");
        for c in &chars {
            if let Some((sd, _, n)) = results.iter().find(|r| r.1 == *c) {
                println!("  char {:?}: seed {sd}, {} shots, {} frames (~{:.2} s + load), score {}", c, n.steps.len(), n.frames, rt::secs(n.frames), n.game.total);
            } else { println!("  char {c:?}: no solution"); }
        }
    }
    let Some((seed, ch, best)) = results.first() else { println!("no solution"); std::process::exit(1) };
    if single {
        println!("top opening shots ({name}):");
        for (sd, _, n) in results.iter().take(top) {
            let s = n.steps.last().unwrap();
            println!("  seed {sd}: pixel ({}, {}) angle {:.6} click {}: {} pegs ({} orange), score {}, styles {:?}, catches {}, free balls {}{}",
                     s.pixel[0], s.pixel[1], s.angle, s.click, s.res.pegs, s.res.oranges, s.res.score, s.res.styles, s.res.catches, s.res.free_balls, if s.res.won { ", WINS" } else { "" });
        }
    }
    let mut script = String::new();
    if let Some(c) = ch { script += &format!("char {c}\n"); }
    script += &format!("level {st} {ix}\n");
    let mut plan = format!("# {name} ({}-{}) seed {seed} char {ch:?} objective {:?}{}\n# shot: window pixel (x, y) → angle, click tick | predicted outcome\n", st + 1, ix + 1, p.objective,
                           if p.ff { format!(", fast forward held: {} frames = {:.2} s from the first board frame to the Level Complete box", best.frames, rt::secs(best.frames)) } else { String::new() });
    for (k, s) in best.steps.to_vec().iter().enumerate() {
        script += &format!("shotx {:08x} {}\n", s.angle.to_bits(), s.click);
        plan += &format!("{}: pixel ({}, {}) angle {:.7} click {} | {} pegs, {} orange, score {}, end tick {}, styles {:?}, catches {}, free {}{}\n",
                         k + 1, s.pixel[0], s.pixel[1], s.angle, s.click, s.res.pegs, s.res.oranges, s.res.score, s.res.end_tick, s.res.styles, s.res.catches, s.res.free_balls,
                         if s.res.won { " WIN" } else { "" });
    }
    script += "waitms 3000\nquit\n";
    plan += &format!("# total {} after {} shots, balls left {}\n", best.game.total, best.steps.len(), best.game.balls);
    print!("{plan}");
    if let Some(dir) = out {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(format!("{dir}/{st}_{ix}.txt"), &script).unwrap();
        std::fs::write(format!("{dir}/{st}_{ix}.plan.txt"), &plan).unwrap();
        std::fs::write(format!("{dir}/{st}_{ix}.seed"), format!("{seed}\n")).unwrap();
        eprintln!("wrote {dir}/{st}_{ix}.txt (play it: tas/play.sh {dir}/{st}_{ix}.txt)");
    }
}
