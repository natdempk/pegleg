//! Check one captured real-game run against the sim (the core of `game_check` and of the golden test).
//!
//! A run is the harness's files: `script.txt` (level + planned shots), `oracle.bin` (the hook's records; only
//! 'LVL '/'RDY '/'LGC '/'PEG '/'BRD ' are used: the state dump at every ready point), `peggle_oracle_info.txt`
//! (RNG seed, fast forward) and `harness.txt` (real fire ticks, the final total of the Level Complete state).
//! The sim replays the shots from the level start and is compared at every ready point: every peg (colour, hit,
//! collidable), balls, total, oranges, power counters, Lucky Spin flags, catch streak and the bucket phase;
//! the model's earliest next click must not be before the real one (ready + 3, plus the fast-forward frame
//! pad); and when the level was won the sim's total must equal the real final total.
use crate::game::{Game, ShotResult, FIRE_LAG};
use crate::level::{self, Level};
use crate::oracle::{self, Ready};

/// The files of one run, as text / bytes.
#[derive(Default, Clone)]
pub struct RunFiles {
    pub script: String,
    pub oracle: Vec<u8>,
    pub info: String,
    pub harness: String,
}

impl RunFiles {
    pub fn read(dir: &std::path::Path) -> Option<RunFiles> {
        Some(RunFiles {
            script: std::fs::read_to_string(dir.join("script.txt")).ok()?,
            oracle: std::fs::read(dir.join("oracle.bin")).ok()?,
            info: std::fs::read_to_string(dir.join("peggle_oracle_info.txt")).unwrap_or_default(),
            harness: std::fs::read_to_string(dir.join("harness.txt")).unwrap_or_default(),
        })
    }
}

#[derive(Debug, Default)]
pub struct Report {
    pub level: String,
    pub seed: u32,
    pub power: u8,
    pub shots: usize,
    pub shots_ok: usize,
    /// the real level end: (shot, won)
    pub real_end: Option<(usize, bool)>,
    /// shots fired before the model's earliest click (fine when real; counted for information)
    pub early: usize,
    /// every difference found; empty = the run is fully consistent
    pub errors: Vec<String>,
    /// per ready point: model earliest click (with the FF pad) − real (ready + 3); must be >= 0
    pub slack: Vec<i32>,
    pub results: Vec<ShotResult>,
    pub sim_total: i64,
    pub real_total: Option<i64>,
}

impl Report {
    pub fn ok(&self) -> bool { self.errors.is_empty() }
}

/// Real fire (click) ticks from harness.txt ("fire N angle ... tick T"; frame-start fires log click + 1).
pub fn real_fire_ticks(harness: &str) -> Vec<i32> {
    harness.lines().filter_map(|l| {
        let w: Vec<&str> = l.split_whitespace().collect();
        let i = w.iter().position(|&x| x == "fire")?;
        if w.get(i + 2) != Some(&"angle") { return None; }
        let j = w.iter().position(|&x| x == "tick")?;
        let off = if l.contains("frame start") { 1 } else { 0 };
        w.get(j + 1)?.parse::<i32>().ok().map(|t| t - off)
    }).collect()
}

/// The final total the Level Complete state showed (last 'st5 ... total N' line), if logged.
pub fn real_final_total(harness: &str) -> Option<i64> {
    harness.lines().rev().find(|l| l.contains(" st5 ")).and_then(|l| l.trim_end().rsplit(' ').next()?.parse().ok())
}

/// Check a run. `level_file(name)` returns the level's .dat contents; `real_fire` replays the real fire ticks
/// (physics checked apart from the timing model) instead of the script's planned ones.
pub fn check(f: &RunFiles, base: &Level, level_file: &dyn Fn(&str) -> Option<Vec<u8>>, real_fire: bool, verbose: bool) -> Option<Report> {
    let ff = f.info.contains("fast forward held");
    let seed: u32 = f.info.lines().find_map(|l| l.strip_prefix("tas: rng seed ")).and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    let mut shots: Vec<(f32, i32)> = vec![];
    for l in f.script.lines() {
        let w: Vec<&str> = l.split_whitespace().collect();
        if let ["shotx", a, t] = w.as_slice() {
            shots.push((f32::from_bits(u32::from_str_radix(a, 16).ok()?), t.parse::<i32>().ok()?));
        }
    }
    if real_fire {
        for (s, t) in shots.iter_mut().zip(real_fire_ticks(&f.harness)) { s.1 = t; }
    }
    let (name, readies) = oracle::read_readies(&f.oracle);
    let (name, r0) = (name?, readies.first()?);
    let lvl = level::parse(&level_file(&name.to_lowercase())?).ok()?;
    let power = r0.li(0x1c4) as u8; // the run's character
    let mut g = Game::new(&lvl, base, seed, power);
    if name == "Level1" { g.set_stage_level(0, 0); }
    let mut rep = Report { level: name, seed, power, shots: shots.len(), ..Default::default() };
    let cmp_pegs = |g: &Game, r: &Ready, tag: &str, errs: &mut Vec<String>| {
        let mut bad = vec![];
        for (p, t, hit, col, ix) in &r.pegs {
            match oracle::find_peg(&g.board, *ix, *p) {
                Some(i) => {
                    let o = &g.board.objs[i];
                    // colour of hit pegs is irrelevant after removal; compare live pegs fully
                    if (!*hit && o.peg_type != *t) || o.hit != *hit || o.collidable != *col {
                        bad.push(format!("#{} sim t{} h{} c{} real t{} h{} c{}", ix, o.peg_type, o.hit as u8, o.collidable as u8, t, *hit as u8, *col as u8));
                    }
                }
                None => bad.push(format!("#{ix} missing")),
            }
        }
        if !bad.is_empty() { errs.push(format!("{tag}: {} peg diffs: {}", bad.len(), bad.iter().take(if verbose { 99 } else { 6 }).cloned().collect::<Vec<_>>().join("; "))); }
    };
    cmp_pegs(&g, r0, "start", &mut rep.errors);
    for (k, &(a, t)) in shots.iter().enumerate() {
        let before = rep.errors.len();
        if t < g.earliest_fire && k > 0 { rep.early += 1; }
        // the sim's previous shot still running at the real fire: it already diverged, report and stop
        if t + FIRE_LAG < g.tick {
            rep.errors.push(format!("shot {}: real fire at tick {t} while the sim's previous shot ran to {}", k + 1, g.tick));
            break;
        }
        let res = g.shoot(a, t);
        if verbose { eprintln!("shot {}: {:?}", k + 1, res); }  // "shot N: ShotResult {..}" (parsed by scripts)
        rep.results.push(res);
        let Some(r) = readies.get(k + 1) else { break };
        if r.level_over || g.over() {
            // real won = no orange left (LogicMgr+0x360) at the level end
            let real_won = r.level_over && r.li(0x360) == 0;
            if r.level_over { rep.real_end = Some((k + 1, real_won)); }
            if r.level_over != g.over() || real_won != g.won {
                rep.errors.push(format!("shot {}: level over real {} (won {real_won}) sim won {} lost {}", k + 1, r.level_over, g.won, g.lost));
            }
            if rep.errors.len() == before { rep.shots_ok += 1; }
            break;
        }
        let tag = format!("after shot {}", k + 1);
        cmp_pegs(&g, r, &tag, &mut rep.errors);
        let mut d = vec![];
        let mut chk = |n: &str, sim: i64, real: i64| if sim != real { d.push(format!("{n} sim {sim} real {real}")); };
        chk("balls", g.balls as i64, r.li(0x17c) as i64);
        chk("total", g.total, r.li(0x174) as i64);
        chk("oranges", g.board.orange_total as i64, r.li(0x360) as i64);
        for p in 1..14 { chk(&format!("ctr{p}"), g.ctr[p] as i64, r.li(0x1d4 + 8 * p) as i64); }
        chk("trip_pend", g.trip_pend as i64, r.li(0x58) as i64);
        chk("hat_pend", g.hat_pend as i64, r.li(0x60) as i64);
        chk("trip_now", g.trip_now as i64, r.lgc.get(0x68).copied().unwrap_or(0) as i64);
        chk("hat_now", g.hat_now as i64, r.lgc.get(0x69).copied().unwrap_or(0) as i64);
        chk("streak", g.board.catch_streak as i64, r.li(0x320) as i64);
        chk("bucket lag", (g.board.btick - 2 - g.tick) as i64, (r.bucket - r.tick) as i64); // object counter − harness tick
        if verbose { eprintln!("  ready {}: sim tick {} btick {} | real tick {} bucket {} btick {} | earliest {}", k + 1, g.tick, g.board.btick, r.tick, r.bucket, r.btick, g.earliest_fire); }
        if !d.is_empty() { rep.errors.push(format!("{tag}: {}", d.join(", "))); }
        // fast forward: the real ready tick is on the 8-tick frame grid; FF plans click `ff_pad` later
        let e = g.earliest_fire + if ff { g.ff_pad } else { 0 };
        if shots.get(k + 1).is_some() {
            let s = e - (r.tick + 3);
            rep.slack.push(s);
            if s < 0 { rep.errors.push(format!("{tag}: UNSAFE model earliest fire {e} before real ready+3 {}", r.tick + 3)); }
        }
        if rep.errors.len() == before { rep.shots_ok += 1; }
    }
    rep.sim_total = g.total;
    rep.real_total = real_final_total(&f.harness);
    if let (true, Some(rt)) = (g.won, rep.real_total) {
        if rt != g.total { rep.errors.push(format!("final total sim {} real {rt}", g.total)); }
    }
    Some(rep)
}
