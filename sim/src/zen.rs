//! Zen Shot AI search (AIMgr). LogicMgr state 10 (FUN_0046d0c0) calls FUN_00467b80, which sets the AI
//! mode DAT_00650910 = 10, a 10000-step budget (AIMgr+0x54), then:
//! - FUN_00465740 → FUN_004637e0 (AIMgr mode +0x6c = 1): 241 AIBalls (FUN_00454620) at angles
//!   `norm((float)i * 0.001 + aim)` for i = -120..=120 (ModVal AIMgr.cpp18 = 120, cpp19 = 0.001), each
//!   placed and launched like a real shot (FUN_004bc2d0 rotation, speed Gun+0x160). Step budget
//!   AIMgr+0x50 = 500 (cpp16; 1000 with a fireball counter or Space Blast greens).
//! - FUN_004604c0: 500 × FUN_00460380 = one live physics tick of the board objects and every AIBall
//!   (object updates, FUN_0045add0(1), moves of balls not stopped); the board is saved and restored
//!   around the search. AIBalls have no ball-ball pairs (+0x163 = 0) and never leave the board.
//!   Every contact calls the AIBall's handler FUN_00463080 (`AiBall::contact`); then the selection
//!   pass (`select`) picks the best ball and its angle becomes the aim (LogicMgr+0xe0, swapped into
//!   +0xe4 by FUN_00467b80; the shot fires at it 50 frames later).
//! In Zen mode (DAT_00650910 = 10) the search uses no random numbers.

use crate::phys::{Ball, Board, Event, Geom, Gun};
use crate::score::{multiplier, GREEN, ORANGE, PURPLE};

pub const ZEN_HALF: i32 = 120;
pub const ZEN_DELTA: f32 = 0.001;
pub const ZEN_STEPS: i32 = 500;

/// LogicMgr state the search reads (indices are for the current player).
#[derive(Clone, Debug, Default)]
pub struct ZenCtx {
    /// LogicMgr+0xfb (no 100x multiplier at 0 oranges left)
    pub fever_off: bool,
    /// LogicMgr+0x124
    pub l124: bool,
    /// LogicMgr+0x18
    pub l18: bool,
    /// DAT_0065090c = LogicMgr+0x1c4[player]: the stage power
    pub power: u8,
    /// AIMgr+0x68: LogicMgr+0x1dc[player] > 0 (Super Guide turns)
    pub guide: bool,
    /// AIMgr+0x69: LogicMgr+0x234[player] > 0 (fireball turns)
    pub fireball: bool,
    /// LogicMgr+0x22c[player] > 0 (power 11 turns: the search runs 1000 steps)
    pub ctr11: bool,
    /// LogicMgr+0x360: oranges left (AIBall+0x194 start)
    pub oranges_left: i32,
    /// LogicMgr+0x244[player]
    pub l244: bool,
    /// DAT_00650901: flippers or pyramid currently active (ctr[2] > 0 || ctr[4] > 0)
    pub power_active: bool,
}

impl ZenCtx {
    /// From a LogicMgr snapshot (0x4b0 bytes, oracle 'LGC ' records).
    pub fn from_lgc(lgc: &[u8]) -> ZenCtx {
        let li = |o: usize| i32::from_le_bytes(lgc[o..o + 4].try_into().unwrap());
        let p = li(0x128) as usize;
        ZenCtx { fever_off: lgc[0xfb] != 0, l124: lgc[0x124] != 0, l18: lgc[0x18] != 0, power: li(0x1c4 + 4 * p) as u8,
                 guide: li(0x1dc + 4 * p) > 0, fireball: li(0x234 + 4 * p) > 0, ctr11: li(0x22c + 4 * p) > 0, oranges_left: li(0x360), l244: lgc[0x244 + p] != 0,
                 power_active: li(0x1e4 + 4 * p) > 0 || li(0x1f4 + 4 * p) > 0 }
    }
    /// Zen Shot armed for the next shot: LogicMgr+0x20c[player] > 0.
    pub fn armed(lgc: &[u8]) -> bool {
        lgc.len() == 0x4b0 && {
            let li = |o: usize| i32::from_le_bytes(lgc[o..o + 4].try_into().unwrap());
            li(0x20c + 4 * li(0x128) as usize) > 0
        }
    }
}

static NO_MOVERS: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| std::env::var_os("ZEN_NO_MOVERS").is_some());
static TRACE: std::sync::LazyLock<Option<f32>> = std::sync::LazyLock::new(|| std::env::var("ZEN_TRACE").ok().and_then(|v| v.parse::<f32>().ok()));
/// ZEN_CLONE=1: the reference one-board-copy-per-ball search; ZEN_CHECK=1: run both and assert equal
static MODE: std::sync::LazyLock<(bool, bool)> = std::sync::LazyLock::new(|| (std::env::var_os("ZEN_CLONE").is_some(), std::env::var_os("ZEN_CHECK").is_some()));

/// AIBall search state (fields from +0x190).
#[derive(Clone, Debug, PartialEq)]
pub struct AiBall {
    pub angle: f32,     // +0x190
    pub rem: i32,       // +0x194 oranges left
    pub score: i32,     // +0x198
    /// +0x198 before the selection pass
    pub raw_score: i32,
    pub base: i32,      // +0x19c running sum of peg values
    pub oranges: i32,   // +0x1a0 oranges hit
    pub purple: i32,    // +0x1a4
    pub contact: [i32; 2], // +0x1a8 steps of the first two contacts
    pub peg: [i32; 3],  // +0x1b0 steps of the first three new pegs
    pub orange: [i32; 2], // +0x1bc steps of the first two oranges
    pub green: i32,     // +0x1c4
    pub tele: i32,      // +0x1d4
    pub hit: Vec<usize>, // +0x1d8 set (count +0x1e0)
    pub f1f8: i32,
    pub f1fc: i32,
    pub f200: i32,
    pub lid: bool,      // +0x1f5
    pub f205: bool,
    pub stopped: bool,  // +0x1f4
    nc: usize,
    np: usize,
    no: usize,
}

impl AiBall {
    pub fn new(angle: f32, rem: i32) -> AiBall {
        AiBall { angle, rem, score: 0, raw_score: 0, base: 0, oranges: 0, purple: 0, contact: [-1; 2], peg: [-1; 3], orange: [-1; 2],
                 green: -1, tele: -1, hit: vec![], f1f8: -1, f1fc: -1, f200: 100, lid: false, f205: false, stopped: false,
                 nc: 0, np: 0, no: 0 }
    }

    /// FUN_00463080 (DAT_00650910 = 10 paths): `step` = AIMgr+0x58 (DAT_00650904).
    /// Returns `Some(peg_index)` if Multiball (power 3) should spawn a child AIBall at that peg.
    fn contact(&mut self, bd: &Board, i: usize, hole: bool, ctx: &ZenCtx, step: i32) -> Option<usize> {
        if self.nc < 2 { self.contact[self.nc] = step; self.nc += 1; }
        let o = &bd.objs[i];
        match o.geom {
            Geom::Poly { .. } if o.cover => {
                // FUN_00463080 lines 1024-1033: "freeballcover"
                if !self.lid {
                    self.lid = true;
                    if ctx.fireball { self.score += 5000; }
                } else {
                    self.stopped = true;
                }
            }
            Geom::Poly { .. } if o.freeball => {
                // "freeball" bucket parts (excluding "freeballcover")
                if self.rem == 1 { self.score -= 15000; } // ModVal AIMgr.cpp2
                if self.f205 { self.stopped = true; }
            }
            Geom::Poly { .. } if o.flipper => self.stopped = true,
            _ => {}
        }
        if hole {
            if let Geom::Hole { teleport: None, .. } = o.geom { self.stopped = true; }
            else if self.tele == -1 { self.tele = step; }
            if o.freeball && !ctx.l124 && self.score >= 1 && !self.f205 { self.score += 2500; }
            return None;
        }
        if !o.has_peginfo || o.peg_type == 0 { return None; }
        // bricks (PegInfo+0x16): FUN_00463080 keeps a per-brick counter in the same set (0 on insert; < 1 → −1, −2,
        // then −3 → the step) and scores only while it is −1, i.e. on the first contact: the same as the hit set
        // +0x1b0: first three PegInfo contacts (repeats included), recorded before the set insert
        if self.np < 3 { self.peg[self.np] = step; self.np += 1; }
        if self.hit.contains(&i) { return None; }
        self.hit.push(i);
        let t = o.peg_type;
        if t == ORANGE && self.no < 2 { self.orange[self.no] = step; self.no += 1; }
        if t == GREEN && self.green == -1 { self.green = step; }
        let mut v = match t { ORANGE => 100, PURPLE => 500, GREEN => 50, _ => 10 };
        if ctx.l244 && t == GREEN {
            // stage-power green values (switch on DAT_0065090c)
            v = match ctx.power { 1 | 6 | 8 | 12 | 2 | 4 | 7 => 300, 13 => 5000, _ => 100 }; // case 7: 10 when DAT_00650910 > 10
            if (ctx.power == 2 || ctx.power == 4) && !ctx.power_active { self.f205 = true; }
            if ctx.power != 13 { v *= multiplier(self.rem, ctx.fever_off) as i32; }
        } else {
            // l244 (LogicMgr+0x244[player]: the Duel computer player, FUN_0040f400) scores non-greens through
            // ModVals AIMgr.cpp5..13 instead; never set in single-player games, so not modelled
            v *= multiplier(self.rem, ctx.fever_off) as i32;
        }
        if v > 10000 && t == PURPLE { v = 10000; }
        let mut spawn_multi = None;
        if t == GREEN {
            // FUN_00463080 lines 1209-1223: green-peg power simulation in AI mode
            match ctx.power {
                3 => { spawn_multi = Some(i); } // FUN_00462e60
                5 => { // FUN_0043fb40 Space Blast
                    if let Some(gp) = bd.obj_pos(bd.objs[i].uid) {
                        for j in 0..bd.objs.len() {
                            let oj = &bd.objs[j];
                            if !oj.has_peginfo || oj.hit || !oj.visible || !oj.collidable || self.hit.contains(&j) { continue; }
                            if let Some(p) = bd.obj_pos(oj.uid) {
                                let dx = (p[0] as f64 - gp[0] as f64) as f32;
                                let dy = (p[1] as f64 - gp[1] as f64) as f32;
                                let d2 = ((dx as f64 * dx as f64) as f32 as f64 + (dy as f64 * dy as f64) as f32 as f64) as f32;
                                if d2 <= 10000.0 { self.contact(bd, j, false, ctx, step); }
                            }
                        }
                    }
                }
                9 => { // FUN_0045a7f0 Flower Power
                    if let Some(gp) = bd.obj_pos(bd.objs[i].uid) {
                        let mut c: Vec<(f32, usize)> = vec![];
                        for j in 0..bd.objs.len() {
                            let oj = &bd.objs[j];
                            if !oj.has_peginfo || oj.hit || !oj.visible || !oj.collidable || oj.peg_type != ORANGE || self.hit.contains(&j) { continue; }
                            if let Some(p) = bd.obj_pos(oj.uid) {
                                let dx = (p[0] as f64 - gp[0] as f64) as f32;
                                let dy = (p[1] as f64 - gp[1] as f64) as f32;
                                let d2 = ((dx as f64 * dx as f64) as f32 as f64 + (dy as f64 * dy as f64) as f32 as f64) as f32;
                                c.push((d2, j));
                            }
                        }
                        c.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                        let n = ((ctx.oranges_left as f32 as f64 * 0.20000000298023224) as f32 as f64).ceil() as usize;
                        for &(_, j) in c.iter().take(n) { self.contact(bd, j, false, ctx, step); }
                    }
                }
                11 => { self.f1fc = 0; }
                _ => {}
            }
        } else if t == PURPLE {
            self.purple = v;
        }
        self.base += v;
        let inc = (self.hit.len() as i32 - 1) * v + self.base;
        self.score += inc;
        if t == ORANGE { self.oranges += 1; self.rem -= 1; }
        spawn_multi
    }
}

/// FUN_004bc3b0 + FUN_004327d0: wrap to [0, 2pi) and clamp to the gun range (AIMgr+0x78/+0x7c, from
/// 90 -/+ ModVal ThunderCommon.cpp123 = 83 degrees).
pub fn norm_angle(a: f32) -> f32 {
    const TWO_PI: f64 = 6.2831854820251465;
    const PI_F: f64 = 3.1415927410125732;
    let lo = ((7.0 * PI_F) / 180.0) as f32;
    let hi = ((173.0 * PI_F) / 180.0) as f32;
    let mut f = a as f64;
    if !(0.0 <= f && f < TWO_PI) {
        f = if f < 0.0 { (TWO_PI as f32 - (-f % TWO_PI) as f32) as f64 } else { ((f % TWO_PI) as f32) as f64 };
    }
    let mut f = f as f32;
    if f as f64 <= 1.5707963705062866 || f >= 4.71238899230957 {
        if f as f64 > 1.5707963705062866 { f = (f as f64 - TWO_PI) as f32; }
        if lo < f { f = lo; }
    } else if f < hi {
        return hi;
    }
    f
}

pub fn candidate_angles(aim: f32) -> Vec<f32> {
    (-ZEN_HALF..=ZEN_HALF).map(|i| norm_angle((i as f64 * ZEN_DELTA as f64 + aim as f64) as f32)).collect()
}

fn launch_ai_ball(gun: &Gun, angle: f32, fireball: bool) -> Ball {
    let mut b = gun.launch(angle, if fireball { 10.0 } else { 6.0 });
    b.fireball = fireball;
    b
}

fn spawn_ai_multi(bd: &Board, parent: &Ball, peg: usize) -> Option<Ball> {
    let pp = bd.obj_pos(bd.objs[peg].uid)?;
    let s = (pp[0] as f64 - parent.pos[0] as f64) as f32;
    let odd = bd.lucky_ctr.map_or(false, |c| (c[0] as i32) & 1 != 0);
    let vx = if s > 0.0 || (s == 0.0 && odd) { 0.5 } else { -0.5 };
    let mut nb = Ball { r: 6.0, ..Default::default() };
    nb.set_pos(pp);
    nb.prev = pp;
    nb.vel = [vx, -3.0];
    nb.id = 1;
    nb.ai_hits = parent.ai_hits.clone();
    Some(nb)
}

/// Simulate one AIBall for `steps` AIMgr steps from a copy of the board.
pub fn run_ball(board: &Board, gun: &Gun, angle: f32, ctx: &ZenCtx, steps: i32) -> AiBall {
    let mut bd = board.clone();
    bd.ai = true;
    bd.ai_movetoggle = true;
    bd.begin_shot();
    let mut ai = AiBall::new(angle, ctx.oranges_left);
    let mut balls: Vec<Ball> = vec![launch_ai_ball(gun, angle, ctx.fireball)];
    let mut ev = vec![];
    let tick_movers = !*NO_MOVERS;
    for step in 1..=steps {
        if tick_movers { bd.tick_objects(); }
        ev.clear();
        bd.step_multi(&mut balls, &mut ev);
        let mut spawn = None;
        for e in &ev {
            match *e {
                Event::Hit(i) => {
                    if let Some(p) = ai.contact(&bd, i, false, ctx, step) { spawn = Some(p); }
                    if ctx.fireball || ctx.power == 3 {
                        for b in balls.iter_mut() { if !b.ai_hits.contains(&i) { b.ai_hits.push(i); } }
                    }
                }
                // the hole callback runs twice per capture (observed: the freeball-hole +2500 is added twice)
                Event::Captured(i) | Event::Teleport(i) => for _ in 0..2 { ai.contact(&bd, i, true, ctx, step); },
                Event::Lost => {}
            }
        }
        if let Some(p) = spawn {
            if balls.len() == 1 {
                if let Some(nb) = spawn_ai_multi(&bd, &balls[0], p) { balls.push(nb); }
            }
        }
        if let Some(t) = *TRACE {
            if (t - angle).abs() < 1e-6 { let b = &balls[0]; eprintln!("ZT {step} {} {} {} {} {:?}", b.pos[0], b.pos[1], b.vel[0], b.vel[1], ev); }
        }
        if ai.stopped || balls.iter().all(|b| b.gone) { break; }
    }
    ai.raw_score = ai.score;
    ai
}

/// All AIBalls of one search in lockstep on one shared board, like AIMgr: the board objects tick once per
/// step, then each running ball steps. Falls back to per-ball clones when the board has `movetoggle`
/// objects (FUN_004637e0 clones movetoggle objects per AIBall).
pub fn run_balls(board: &Board, gun: &Gun, angles: &[f32], ctx: &ZenCtx, steps: i32) -> Vec<AiBall> {
    let (clone, check) = *MODE;
    let has_toggle = board.objs.iter().any(|o| o.movetoggle);
    if (clone && !check) || has_toggle { return angles.iter().map(|&a| run_ball(board, gun, a, ctx, steps)).collect(); }
    let mut bd = board.clone();
    bd.ai = true;
    bd.begin_shot();
    let mut ais: Vec<AiBall> = angles.iter().map(|&a| AiBall::new(a, ctx.oranges_left)).collect();
    let mut balls: Vec<Vec<Ball>> = angles.iter().map(|&a| vec![launch_ai_ball(gun, a, ctx.fireball)]).collect();
    let mut live: Vec<usize> = (0..angles.len()).collect();
    let mut ev = vec![];
    for step in 1..=steps {
        if live.is_empty() { break; }
        if !*NO_MOVERS { bd.tick_objects(); }
        live.retain(|&k| {
            ev.clear();
            bd.step_multi(&mut balls[k], &mut ev);
            let ai = &mut ais[k];
            let mut spawn = None;
            for e in &ev {
                match *e {
                    Event::Hit(i) => {
                        if let Some(p) = ai.contact(&bd, i, false, ctx, step) { spawn = Some(p); }
                        if ctx.fireball || ctx.power == 3 {
                            for b in balls[k].iter_mut() { if !b.ai_hits.contains(&i) { b.ai_hits.push(i); } }
                        }
                    }
                    Event::Captured(i) | Event::Teleport(i) => for _ in 0..2 { ai.contact(&bd, i, true, ctx, step); },
                    Event::Lost => {}
                }
            }
            if let Some(p) = spawn {
                if balls[k].len() == 1 {
                    if let Some(nb) = spawn_ai_multi(&bd, &balls[k][0], p) { balls[k].push(nb); }
                }
            }
            !(ai.stopped || balls[k].iter().all(|b| b.gone))
        });
    }
    for ai in ais.iter_mut() { ai.raw_score = ai.score; }
    if check {
        for (k, &a) in angles.iter().enumerate() {
            let r = run_ball(board, gun, a, ctx, steps);
            assert_eq!(r, ais[k], "ZEN_CHECK: lockstep AIBall differs at angle {a}");
        }
        eprintln!("ZEN_CHECK ok {} balls", angles.len());
    }
    ais
}

/// FUN_004604c0 selection (DAT_00650910 = 10, mode 1). Returns the index of the chosen ball.
pub fn select(balls: &mut [AiBall], ctx: &ZenCtx) -> Option<usize> {
    let mut best: Option<usize> = None;
    let mut best_score = 0i32;
    for k in 0..balls.len() {
        let (bo, bs) = best.map_or((0, 0), |j| (balls[j].oranges, balls[j].score));
        let b = &mut balls[k];
        let a = b.peg[0];
        let c = b.orange[0];
        let mut l11 = a == c;
        let mut _l12 = !(b.peg[1] < c || a + 25 <= c);
        let l16 = !(b.green < 0 || b.peg[1] < b.green || a + 20 <= b.green);
        if ctx.fireball && c < b.contact[0] + 200 && -1 < c { _l12 = true; l11 = true; b.oranges = 1; }
        if ctx.l18 || (ctx.power == 8 && l16) { _l12 = true; l11 = true; b.oranges = 1; }
        if ctx.guide && !l11 { l11 = !(b.peg[1] < b.orange[0]) && !(a + 50 <= b.orange[0]); }
        let _ = l11;
        if b.score < b.f1f8 {
            b.score = b.f1f8;
            b.rem = b.f200;
            if b.oranges < 1 { b.oranges = 1; }
        } else {
            b.f1fc = -1;
        }
        if b.rem == 0 && b.oranges > 0 && !ctx.fever_off {
            b.score += 25000; // (100000 with LogicMgr+0x124 near the score target: not modelled)
        } else if !ctx.l124 && best.is_some() && b.oranges < bo {
            let keep = b.score > 7999 && (1.5f64 * bs as f64) <= (b.score as f32) as f64; // ModVal AIMgr.cpp24
            if !keep && b.score < 25000 { b.score = 0; }
        }
        if ctx.l124 && b.oranges == 0 { b.score = 0; }
        if best_score < b.score { best_score = b.score; best = Some(k); }
    }
    best
}

/// AIMgr search length (FUN_00467b80 setup → AIMgr+0x50): 500, or 1000 with power 11 turns left
/// (LogicMgr+0x22c[player]) or stage power 11 with a green peg in the peg list (type 4). Each step is a live
/// object update, so this is also the Zen shot's harness-tick cost.
pub fn search_steps(board: &Board, ctx: &ZenCtx) -> i32 {
    let green = || board.objs.iter().any(|o| o.has_peginfo && o.collidable && o.peg_type == GREEN);
    if ctx.ctr11 || (ctx.power == 11 && green()) { 1000 } else { ZEN_STEPS }
}

/// The whole search: returns (chosen angle, all balls). With no scoring ball the aim is kept.
pub fn zen_search(board: &Board, gun: &Gun, aim: f32, ctx: &ZenCtx) -> (f32, Vec<AiBall>) {
    let angles = candidate_angles(aim);
    let steps = search_steps(board, ctx);
    let mut balls: Vec<AiBall> = std::thread::scope(|s| {
        let chunks: Vec<_> = angles.chunks(31).map(|ch| s.spawn(move || run_balls(board, gun, ch, ctx, steps))).collect();
        chunks.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });
    let pick = select(&mut balls, ctx).map_or(aim, |k| balls[k].angle);
    (pick, balls)
}

/// Single-threaded search returning only the chosen angle (for callers that parallelise themselves).
/// `memo` caches AIBall runs by angle bits (valid for one board state).
pub fn zen_pick(board: &Board, gun: &Gun, aim: f32, ctx: &ZenCtx, memo: Option<&std::sync::Mutex<std::collections::HashMap<u32, AiBall>>>) -> f32 {
    let steps = search_steps(board, ctx);
    let angles = candidate_angles(aim);
    let mut balls: Vec<Option<AiBall>> = angles.iter().map(|a| memo.and_then(|m| m.lock().unwrap().get(&a.to_bits()).cloned())).collect();
    let miss: Vec<f32> = angles.iter().zip(&balls).filter(|(_, b)| b.is_none()).map(|(&a, _)| a).collect();
    let mut run = run_balls(board, gun, &miss, ctx, steps).into_iter();
    for b in balls.iter_mut().filter(|b| b.is_none()) {
        let r = run.next().unwrap();
        if let Some(m) = memo { m.lock().unwrap().insert(r.angle.to_bits(), r.clone()); }
        *b = Some(r);
    }
    let mut balls: Vec<AiBall> = balls.into_iter().map(Option::unwrap).collect();
    select(&mut balls, ctx).map_or(aim, |k| balls[k].angle)
}
