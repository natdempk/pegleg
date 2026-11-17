//! Per-level beam search over human-reachable inputs.
//!
//! An input is (cursor pixel → aim angle via the game's mouse-aim routine, click tick). Every shot of
//! every beam node is expanded over a coarse grid of the reachable angle list × a set of click delays
//! after the earliest accepted click, the best children are refined on progressively finer angle
//! neighbourhoods (still inside the reachable list), and the next beam is the top `beam` distinct
//! children by the objective. All shot simulations run in parallel (rayon).
use peggle_sim::aim;
use peggle_sim::game::{Game, ShotResult};
use peggle_sim::phys::Gun;
use rayon::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Objective {
    /// clear the level as early as possible (tick the winning shot's ball is gone; ties: score)
    Speed,
    /// highest level total (fever bonus and balls left included)
    Score,
    /// single most impressive opening shot: most pegs hit (ties: score)
    Pegs,
    /// single opening shot with the highest score
    ShotScore,
    /// Ultra Extreme Fever (speedrun.com IL category): Speed, but the level must end with every peg cleared
    /// (the winning ball's fever bucket is worth 100000, BallHitPeg's all-cleared rule)
    Uef,
    /// showcase: fewest shots to clear the level (huge multi-peg shots), ties broken by the highest total
    Shots,
}

/// pegs (any colour) not hit yet
pub fn pegs_left(g: &Game) -> i32 {
    g.board.objs.iter().filter(|o| o.has_peginfo && o.peg_type != 0 && !o.hit).count() as i32
}

/// Score heuristic: Extreme Fever value of the non-orange pegs still on the board (see `Params::fever_frac`)
fn fever_est(p: &Params, g: &Game) -> f64 {
    if p.fever_frac <= 0.0 { return 0.0; }
    let n = g.board.objs.iter().filter(|o| o.has_peginfo && o.peg_type != 0 && o.peg_type != peggle_sim::score::ORANGE && !o.hit).count() as f64;
    let k = p.fever_frac * n;
    // Lucky Spin Score Tripler pending: the next shot (the fever shot, ideally) is tripled
    1000.0 * k * k * if g.trip_pend > 0 { p.trip_bonus } else { 1.0 }
}

#[derive(Clone, Debug)]
pub struct Params {
    pub objective: Objective,
    pub beam: usize,
    /// coarse angle samples per expansion
    pub coarse: usize,
    /// click delays (ticks after the earliest accepted click) tried for every angle
    pub delays: Vec<i32>,
    /// children refined per node
    pub refine_top: usize,
    /// refinement rounds (each one shrinks the neighbourhood by `refine_div`)
    pub refine_rounds: usize,
    pub refine_div: usize,
    /// candidates per side in each refinement neighbourhood
    pub refine_width: usize,
    pub max_shots: usize,
    /// every shot must return the ball (catch or free ball): no ball is ever lost
    pub keep_ball: bool,
    /// speed heuristic: frames (or ticks without fast forward) charged per orange left
    pub orange_cost: f64,
    /// UEF heuristic: frames charged per peg (any colour) left
    pub peg_cost: f64,
    /// score heuristic: points charged per orange left on an unwon node
    pub score_orange_cost: f64,
    /// score heuristic: expected share of the non-orange pegs left that the winning (Extreme Fever) shot
    /// hits at 100x; an unwon node is credited 1000 * (fever_frac * pegs)^2 (a shot's score grows with the
    /// square of its pegs). 0 = off (rank by the running total only)
    pub fever_frac: f64,
    /// score heuristic: factor on the fever estimate while a Score Tripler is pending for the next shot
    pub trip_bonus: f64,
    /// score objective: Extreme Fever probe. The best `probe_pool` distinct unwon children of each depth are
    /// valued by `fever_probe` (a fan of `probe_n` real shots at the earliest click with the board's orange
    /// count set to 1, so the first orange hit is the last one) instead of `fever_estimate`. 0 = off
    pub probe_n: usize,
    pub probe_pool: usize,
    /// weight on the probe value in the key (it is optimistic for nodes with several oranges left)
    pub probe_weight: f64,
    /// Warren (Lucky Spin): < 1 probes the fever shot tripled and weights nodes without a tripler armed by
    /// this (1 = off: probe as is)
    pub probe_untrip: f64,
    /// score objective with the probe: de-duplicate by pegs hit + balls and keep the beam (and probe pool)
    /// spread over the oranges-left counts
    pub strata: bool,
    /// score objective: value of a running catch streak (|Board catch_streak| after a caught shot that hit a peg),
    /// as this fraction of the styles it leads to: Crazy Mad Skillz (100000) every 10th catch in a row, Mad Skillz
    /// (25000) at the 5th. Also keeps nodes that differ only in their streak apart. 0 = off
    pub streak_value: f64,
    /// bucket timing: per node, the best `bucket_top` children that hit a peg but were not caught are
    /// re-fired at the delay (up to `bucket_max` frames) that puts the moving bucket under the ball's path (0 = off)
    pub bucket_top: usize,
    pub bucket_max: i32,
    /// exact Fever probe: with 2-3 oranges left, probe once per orange L with the others cleared and L kept (its lane
    /// as it stands, multipliers before L) instead of deleting them all and scoring 100x
    pub probe_exact: bool,
    /// fever probe click delays (frames after the earliest click)
    pub probe_delays: Vec<i32>,
    /// full signature: strata de-duplication also keeps apart the tripler state and the purple peg
    pub sig_full: bool,
    /// strata cells (oranges tier × tripler armed), tiers more than 2 behind the leading one dropped,
    /// and a node dominated by a kept one (same oranges left, standing pegs ⊆, total, balls, streak <=) skipped
    pub cells: bool,
    /// lane tabu, goal-first: pegs of the target Fever lane (a file of peg indices); hitting one before the win costs
    /// `tabu_cost` points in the key
    pub tabu: Vec<usize>,
    pub tabu_cost: f64,
    /// final-shot scan: every depth, the best `final_scan` nodes with one orange left get every reachable angle at
    /// the probe delays (the winning shot is a narrow target); 0 = off
    pub final_scan: usize,
    /// real-time model with the right button held (fast forward, see `rt`): clicks only at frame starts,
    /// cost = app frames. Off: cost = harness ticks, any click tick.
    pub ff: bool,
}

impl Default for Params {
    fn default() -> Self {
        Params { objective: Objective::Speed, beam: 24, coarse: 720, delays: vec![0, 10, 25, 50], refine_top: 6,
                 refine_rounds: 4, refine_div: 8, refine_width: 8, max_shots: 10, keep_ball: false, orange_cost: 260.0, peg_cost: 60.0, score_orange_cost: 2000.0, fever_frac: 0.0, trip_bonus: 1.0, probe_n: 0, probe_pool: 0, probe_weight: 1.0, probe_untrip: 1.0, strata: false, streak_value: 0.0,
                 bucket_top: 0, bucket_max: 64, probe_exact: false, probe_delays: vec![0], sig_full: false, cells: false, tabu: vec![], tabu_cost: 0.0, final_scan: 0, ff: true }
    }
}

#[derive(Clone, Debug)]
pub struct Step {
    /// index into the reachable angle list
    pub ai: usize,
    pub angle: f32,
    /// window pixel that produces `angle` (800×600 client area)
    pub pixel: [i32; 2],
    pub click: i32,
    pub res: ShotResult,
}

/// A node's shots: a persistent list shared with its ancestors (a child adds one step in O(1) instead of
/// copying every step, each with its own Vecs).
#[derive(Clone, Default)]
pub struct Trail(Option<std::sync::Arc<(Step, Trail, usize)>>);

impl Trail {
    pub fn len(&self) -> usize { self.0.as_ref().map_or(0, |t| t.2) }
    pub fn is_empty(&self) -> bool { self.0.is_none() }
    pub fn last(&self) -> Option<&Step> { self.0.as_ref().map(|t| &t.0) }
    pub fn push(&self, s: Step) -> Trail { Trail(Some(std::sync::Arc::new((s, self.clone(), self.len() + 1)))) }
    /// the shots in order (first shot first)
    pub fn to_vec(&self) -> Vec<Step> {
        let mut v = Vec::with_capacity(self.len());
        let mut t = self;
        while let Some(n) = &t.0 { v.push(n.0.clone()); t = &n.1; }
        v.reverse();
        v
    }
}

#[derive(Clone)]
pub struct Node {
    pub game: Game,
    pub steps: Trail,
    /// real-time cost so far (app frames with fast forward, else harness ticks), up to `grid0`
    pub frames: i64,
    /// harness tick of the frame start the current aiming phase's frame grid is anchored to
    pub grid0: i32,
    /// score objective: best Extreme Fever shot found by `fever_probe` (None = not probed)
    pub probe: Option<f64>,
    /// ranking key (lower is better), cached: `Node::compute_key`
    pub key: f64,
}

/// Real-time model (speedrun timing). Board::Update (one call per 10 ms app frame) runs the physics
/// step once, or 8 times while the right button is held and the LogicMgr state is not 2 (ball in
/// flight; 4x there only with the "scott" cheat). Verified in the real game:
/// - with fast forward held from the level load, frame f starts at harness tick 8(f-1) until the first
///   shot; a click made between frames at the frame starting at tick s acts like the harness's click
///   at tick s-1, so reachable clicks are `grid0 + 8j - 1`;
/// - the launch (click + 9) falls in the click's second frame; from the next frame start every frame is
///   one tick until the ball is gone (end_tick), so a shot costs `res.frames - 5` frames from the click
///   frame, and the next frame grid starts at end_tick + 1 (+ `res.pop_tail` 1-tick frames while an
///   in-flight pop round finishes; verified on the 4 such shots in the verify runs);
/// - the near-win slow-mo (Board+0xc1: rate 100/1000 per update, countdown 100; started by the look-ahead
///   `Board::slowmo_peek`, on the winning shot and on near misses, `ShotResult::slowmo_at`) is cut
///   by a click (LogicMgr::MouseDown clears +0xf9 and sets the countdown to 1): the rate then ramps
///   +20/update to 1000, 46 frames for 24 ticks = +22 frames;
/// - after the winning ball is gone the fever sequence (state 3/6) runs ~640 ticks at 8x until state 5
///   shows the Level Complete box (the speedrun end): 80 frames on average.
pub mod rt {
    pub const FF: i32 = 8;
    pub const SHOT_FRAMES_OFF: i64 = 5;
    pub const SLOWMO_FRAMES: i64 = 22;
    /// mean over the 288 fast-forward verify runs (state 3/6 -> 5 at 8 ticks per frame, sd 13): its length
    /// follows the final shot's pops and score tally (LogicMgr state 3, FUN_00471030), not modelled yet
    pub const FEVER_FRAMES: i64 = 80;
    /// earliest first click (harness tick, level-relative) with the character dialog closed at once
    pub const FIRST_CLICK: i32 = 103;
    pub fn secs(frames: i64) -> f64 { frames as f64 / 100.0 }
}

pub struct Angles {
    pub list: Vec<(f32, [i32; 2])>,
}

impl Angles {
    pub fn new() -> Angles { Angles { list: aim::mouse_angles(&Gun::default()) } }
    /// `n` indices spread evenly over the shootable range (downward half, angles in (π, 2π) are the
    /// only ones that leave the gun downward; the clamp maps the rest onto 7°/173°).
    pub fn coarse(&self, n: usize) -> Vec<usize> {
        let lo = self.list.partition_point(|a| a.0 < std::f32::consts::PI);
        let hi = self.list.len();
        let span = hi - lo;
        let mut v: Vec<usize> = (0..n).map(|k| lo + k * span / n).collect();
        // the two clamp angles (7° / 173° cone edges) are reachable too: shooting along the walls
        for a in [0.12217305f32, 3.0194197] {
            let i = self.list.partition_point(|x| x.0 < a);
            if i < self.list.len() { v.push(i); }
        }
        v.sort(); v.dedup();
        v
    }
}

pub struct Stats {
    pub shots: AtomicU64,
    pub frames: AtomicU64,
}

impl Node {
    pub fn new(game: Game, steps: Trail, frames: i64, grid0: i32, p: &Params) -> Node {
        let mut n = Node { game, steps, frames, grid0, probe: None, key: 0.0 };
        n.key = n.compute_key(p);
        n
    }

    fn set_probe(&mut self, v: f64, p: &Params) { self.probe = Some(v); self.key = self.compute_key(p); }

    pub fn compute_key(&self, p: &Params) -> f64 {
        let g = &self.game;
        match p.objective {
            Objective::Shots => {
                let s = g.total as f64 + 10000.0 * g.balls as f64;
                if g.won { self.steps.len() as f64 * 1e12 - s - 1e15 }
                else { g.oranges_left() as f64 * 1e8 - s }
            }
            Objective::Uef => {
                if g.won { self.frames as f64 - 1e9 }
                else {
                    let wait = (earliest(p, g) + 1 - self.grid0).max(0) as f64 / if p.ff { rt::FF as f64 } else { 1.0 };
                    self.frames as f64 + wait + p.orange_cost * g.oranges_left() as f64 + p.peg_cost * pegs_left(g) as f64
                }
            }
            Objective::Speed => {
                if g.won { self.frames as f64 - 1e9 - g.total as f64 * 1e-9 }
                else {
                    let wait = (earliest(p, g) + 1 - self.grid0).max(0) as f64 / if p.ff { rt::FF as f64 } else { 1.0 };
                    self.frames as f64 + wait + p.orange_cost * g.oranges_left() as f64
                }
            }
            Objective::Score => {
                // balls left are worth 10000 each at the end; unwon nodes get a nominal fever bonus
                let mut s = g.total as f64 + 10000.0 * g.balls as f64 + p.streak_value * streak_est(g);
                if !g.won && p.tabu_cost > 0.0 { s -= p.tabu_cost * p.tabu.iter().filter(|&&i| g.board.objs[i].hit).count() as f64; }
                let fever = match self.probe { Some(v) => p.probe_weight * v, None => fever_est(p, g) };
                -(if g.won { s + 1e12 } else { s - p.score_orange_cost * g.oranges_left() as f64 + fever })
            }
            Objective::Pegs => { let r = &self.steps.last().unwrap().res; -(r.pegs as f64 * 1e7 + r.score as f64) }
            Objective::ShotScore => -(self.steps.last().unwrap().res.score as f64),
        }
    }

    /// score objective with strata: pegs hit + balls (the aiming time is not a different state worth keeping)
    fn sig_score(&self, p: &Params) -> (Vec<bool>, i32, i32, i32) {
        let g = &self.game;
        // tripler pending/armed and the purple peg's index (it moves every turn and is worth 500 x multiplier)
        let full = if p.sig_full {
            let purple = g.board.objs.iter().position(|o| o.has_peginfo && !o.hit && o.peg_type == peggle_sim::score::PURPLE).map_or(-1, |i| i as i32);
            (purple << 2) | ((g.trip_pend > 0) as i32) << 1 | g.trip_now as i32
        } else { 0 };
        (g.board.objs.iter().map(|o| o.hit).collect(), g.balls,
         if p.streak_value > 0.0 { g.board.catch_streak.abs() } else { 0 }, full)
    }

    /// `other` is at least as good in every respect the score search keeps (`Params::cells`): same oranges left
    /// and tripler state, every peg it has hit is hit here too (so its board still holds all of ours), and total,
    /// balls and streak no lower
    fn dominated_by(&self, other: &Node) -> bool {
        let (a, b) = (&self.game, &other.game);
        a.oranges_left() == b.oranges_left() && a.trip_now == b.trip_now && (a.trip_pend > 0) == (b.trip_pend > 0)
            && b.total >= a.total && b.balls >= a.balls && b.board.catch_streak.abs() >= a.board.catch_streak.abs()
            && b.board.objs.iter().zip(&a.board.objs).all(|(x, y)| !x.hit || y.hit)
    }

    /// state signature for de-duplication (pegs hit + timing bucket)
    fn sig(&self) -> (Vec<bool>, i32, i32) {
        (self.game.board.objs.iter().map(|o| o.hit).collect(), (self.frames / 4) as i32, self.game.balls)
    }
}

/// The game advanced to the click tick (shared by all angles fired at that tick).
fn at_click(g: &Game, click: i32) -> Game {
    let mut c = g.clone();
    let lag = if c.zen_armed() { peggle_sim::game::ZEN_LAG } else { peggle_sim::game::FIRE_LAG };
    c.advance_to(click + lag);
    c
}

/// First reachable click tick >= `t` on the node's frame grid (+ `d` frames).
/// earliest click the plan may use: the model's earliest fire, plus the fast-forward frame-grid pad
pub fn earliest(p: &Params, g: &Game) -> i32 { g.earliest_fire + if p.ff { g.ff_pad } else { 0 } }

pub fn grid_click(p: &Params, grid0: i32, t: i32, d: i32) -> i32 {
    if !p.ff { return t + d; }
    let f = rt::FF;
    let j = (t + 1 - grid0 + f - 1).div_euclid(f).max(0);
    grid0 + (j + d) * f - 1
}

/// Advance a node's real-time accounting over one shot.
pub fn account(p: &Params, frames: i64, grid0: i32, click: i32, res: &ShotResult, zen_ticks: i32, won: bool) -> (i64, i32) {
    let f = if p.ff { rt::FF } else { 1 };
    let mut fr = frames + ((click + 1 - grid0) / f) as i64;
    fr += if p.ff { res.frames as i64 - rt::SHOT_FRAMES_OFF + (zen_ticks / f) as i64 } else { (res.end_tick - click) as i64 };
    if p.ff { fr += res.spin_frames as i64; } // Lucky Spin: game frozen while the wheel turns
    if won && p.ff { fr += rt::SLOWMO_FRAMES + rt::FEVER_FRAMES; }
    // a near miss of the last orange (or, for Splork, a board-clearing green) starts the slow-mo too; the
    // click cuts it the same way (verified on 57 non-winning shots of the verify runs: +22 each)
    else if p.ff && res.slowmo_at.is_some() { fr += rt::SLOWMO_FRAMES; }
    // a pop round still running when the ball is gone keeps state 2 (one tick per frame) that long
    fr += res.pop_tail as i64;
    (fr, res.end_tick + 1 + res.pop_tail)
}

fn try_shot(base: &Game, click: i32, ai: usize, angles: &Angles, st: &Stats) -> (Game, Step) {
    let mut g = base.clone();
    let (angle, pixel) = angles.list[ai];
    let res = g.shoot(angle, click);
    st.shots.fetch_add(1, Ordering::Relaxed);
    st.frames.fetch_add(res.frames as u64, Ordering::Relaxed);
    (g, Step { ai, angle, pixel, click, res })
}

fn ok_child(p: &Params, g: &Game, s: &Step) -> bool {
    if g.lost { return false; }
    if p.keep_ball && !g.won && s.res.catches + s.res.free_balls == 0 { return false; }
    // UEF: ending the level with pegs left is a dead end
    if p.objective == Objective::Uef && g.won && (pegs_left(g) > 0 || s.res.fever_hole != 100000) { return false; }
    true
}

/// The styles a running catch streak is heading for (see `Params::streak_value`): the share of the next Crazy Mad
/// Skillz (100000 at every 10th catch in a row) already built, plus Mad Skillz (25000 at the 5th) before it.
/// A streak survives a shot only if the ball hits a peg and is caught (FUN_0046e880 / FUN_0046bbc0).
fn streak_est(g: &Game) -> f64 {
    // between shots the game keeps a running streak negated (turn start); the next caught shot continues from |k|
    let k = g.board.catch_streak.abs();
    if k == 0 { return 0.0; }
    (k % 10) as f64 * 10000.0 + if k < 5 { k as f64 * 5000.0 } else { 0.0 }
}

/// Multiplier tier of an oranges-left count (score.rs `multiplier`): 1-3 left (10x, where free-ball chains of
/// blues pay), 4-6, 7-10, 11-15, 16+
fn tier(oranges_left: i32) -> i32 { peggle_sim::score::multiplier(oranges_left, true) }

/// Keep `n` of `v` (sorted best first), spread over the groups of `f`: round-robin over the groups, each in its
/// own order, so every stage of progress keeps its best nodes.
fn stratify<F: Fn(&Node) -> i32>(v: Vec<Node>, n: usize, f: F) -> Vec<Node> {
    let mut groups: std::collections::BTreeMap<i32, std::collections::VecDeque<Node>> = Default::default();
    for x in v { groups.entry(f(&x)).or_default().push_back(x); }
    let mut out = vec![];
    while out.len() < n && groups.values().any(|g| !g.is_empty()) {
        for g in groups.values_mut() { if out.len() < n { if let Some(x) = g.pop_front() { out.push(x); } } }
    }
    out.sort_by_key(|x| f(x));
    out
}

/// `stratify` by oranges tier (`Params::cells`: × tripler armed, tiers more than 2 behind the most advanced one
/// present dropped, dominated nodes skipped)
fn strat(v: Vec<Node>, n: usize, p: &Params) -> Vec<Node> {
    if !p.cells { return stratify(v, n, |x| tier(x.game.oranges_left())); }
    const RANK: [i32; 5] = [1, 2, 3, 5, 10];
    let rank = |x: &Node| RANK.iter().position(|&t| t == tier(x.game.oranges_left())).unwrap() as i32;
    let lead = v.iter().map(rank).max().unwrap_or(0);
    let cell = |x: &Node| rank(x) * 2 + (x.game.trip_now || x.game.trip_pend > 0) as i32;
    let mut groups: std::collections::BTreeMap<i32, std::collections::VecDeque<Node>> = Default::default();
    for x in v { if rank(&x) >= lead - 2 { groups.entry(cell(&x)).or_default().push_back(x); } }
    let mut out: Vec<Node> = vec![];
    while out.len() < n && groups.values().any(|g| !g.is_empty()) {
        for g in groups.values_mut() {
            while out.len() < n {
                let Some(x) = g.pop_front() else { break };
                if out.iter().any(|o| x.dominated_by(o)) { continue; }
                out.push(x);
                break;
            }
        }
    }
    out.sort_by_key(|x| cell(x));
    out
}

/// Score objective: value of the node's best Extreme Fever shot: a fan of `probe_n` shots at the earliest click.
/// Exact for nodes with one orange left; with more, the oranges are removed on a copy (see below). The shot carries the
/// node's Lucky Spin state (a pending Score Tripler triples it), purple, powers and moving board.
pub fn fever_probe(node: &Node, p: &Params, angles: &Angles, st: &Stats) -> f64 {
    probe_all(std::slice::from_ref(node), p, angles, st)[0]
}

/// The probe's shot bases for one node: (game at the click, click, total before, weight), one per probe delay
/// (and with `probe_exact` and 2-3 oranges left, per orange L kept as the last one).
fn probe_bases(node: &Node, p: &Params) -> Vec<(Game, i32, i64, f64)> {
    let ors: Vec<usize> = node.game.board.objs.iter().enumerate()
        .filter(|(_, o)| o.has_peginfo && o.peg_type == peggle_sim::score::ORANGE && !o.hit).map(|(i, _)| i).collect();
    let n_or = node.game.board.orange_total;
    // which orange stays (None: all removed, every hit at 100x)
    let keeps: Vec<Option<usize>> = if n_or <= 1 { vec![None] }
        else if p.probe_exact && n_or <= 3 && ors.len() as i32 == n_or { ors.iter().map(|&i| Some(i)).collect() }
        else { vec![None] };
    let mut out = vec![];
    for keep in keeps {
        let mut g = node.game.clone();
        // several oranges left: they must all be cleared before the fever shot (often breaking up its lane), so value
        // the best lane through the non-orange pegs: remove the oranges and score every hit at 100x (as if the
        // last orange sat at the lane's start); `probe_exact`: keep orange L (the others cleared) as the last one
        if n_or > 1 {
            for (i, o) in g.board.objs.iter_mut().enumerate() {
                if o.has_peginfo && o.peg_type == peggle_sim::score::ORANGE && !o.hit && Some(i) != keep { o.hit = true; o.collidable = false; }
            }
            g.board.orange_total = keep.is_some() as i32;
        }
        // Warren: value the fever shot as tripled (the strategy the records use); a node without a Score Tripler
        // armed for the next shot gets `probe_untrip` of that (the chance of arming one in time)
        let force = p.probe_untrip < 1.0 && g.board.power == 13 && !g.trip_now;
        if force { g.trip_now = true; }
        let w = if force { p.probe_untrip } else { 1.0 };
        let t0 = g.total;
        for &d in &p.probe_delays {
            let click = grid_click(p, node.grid0, earliest(p, &g), d);
            out.push((at_click(&g, click), click, t0, w));
        }
    }
    out
}

/// `fever_probe` for many nodes at once: every node's fan in one parallel batch, then every refinement in one
/// (few large batches keep all threads busy; results are per node, so identical to probing one by one).
pub fn probe_all(nodes: &[Node], p: &Params, angles: &Angles, st: &Stats) -> Vec<f64> {
    let per: Vec<Vec<(Game, i32, i64, f64)>> = nodes.par_iter().map(|n| probe_bases(n, p)).collect();
    let owner: Vec<usize> = per.iter().enumerate().flat_map(|(ni, v)| std::iter::repeat(ni).take(v.len())).collect();
    let bases: Vec<(Game, i32, i64, f64)> = per.into_iter().flatten().collect();
    let val = |bi: usize, ai: usize| -> f64 {
        let (b, click, t0, _) = &bases[bi];
        let (g2, _) = try_shot(b, *click, ai, angles, st);
        if g2.won { (g2.total - t0) as f64 } else { 0.0 }
    };
    let fan = angles.coarse(p.probe_n);
    let jobs: Vec<(usize, usize)> = (0..bases.len()).flat_map(|bi| fan.iter().map(move |&ai| (bi, ai))).collect();
    let vals: Vec<f64> = jobs.par_iter().map(|&(bi, ai)| val(bi, ai)).collect();
    // refine around each base's best few (a lane entry can be narrower than the fan's spacing)
    let span = (angles.list.len() / 2 / p.probe_n.max(1)).max(1);
    let step = (span / 8).max(1);
    let mut first = vec![0.0f64; bases.len()];
    let mut rjobs: Vec<(usize, usize)> = vec![];
    for bi in 0..bases.len() {
        let mut res: Vec<(f64, usize)> = fan.iter().enumerate().map(|(k, &ai)| (vals[bi * fan.len() + k], ai)).collect();
        res.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        first[bi] = res.first().map_or(0.0, |r| r.0);
        for r in res.iter().take(4).filter(|r| r.0 > 0.0) {
            for j in 1..8usize { for ai in [r.1.saturating_sub(j * step), (r.1 + j * step).min(angles.list.len() - 1)] { rjobs.push((bi, ai)); } }
        }
    }
    let rvals: Vec<f64> = rjobs.par_iter().map(|&(bi, ai)| val(bi, ai)).collect();
    let mut best = first;
    for (&(bi, _), &v) in rjobs.iter().zip(&rvals) { best[bi] = best[bi].max(v); }
    let mut out = vec![0.0f64; nodes.len()];
    for (bi, (&v, b)) in best.iter().zip(&bases).enumerate() { out[owner[bi]] = out[owner[bi]].max(v * b.3); }
    out
}

/// Bucket timing (`Params::bucket_top`): the moving bucket's centre x per harness tick from `g`'s tick on (`n`
/// ticks, the board left alone), and its catch half-width and top y. None: no moving bucket.
fn bucket_path(g: &Game, n: usize) -> Option<(Vec<f32>, f32, f32)> {
    let (bi, _) = g.board.sync_refs();
    let bi = bi?;
    let peggle_sim::phys::Geom::Hole { pos, w, h, .. } = g.board.objs[bi].geom else { return None };
    let mut h2 = g.clone();
    let mut xs = Vec::with_capacity(n);
    for _ in 0..n {
        let peggle_sim::phys::Geom::Hole { pos, .. } = h2.board.objs[bi].geom else { unreachable!() };
        xs.push(pos[0]);
        h2.board.tick_objects(); h2.tick += 1;
    }
    Some((xs, w as f32 * 0.5, pos[1] - h as f32 * 0.5))
}

/// Expand every beam node: coarse grid × delays, then refinement around each node's best children. All nodes'
/// shots of a phase go into one parallel batch; each node's children keep the order a per-node expansion
/// gives (coarse jobs, then each round's refinement jobs), so ranking ties resolve as before.
fn expand_all(beam: &[Node], p: &Params, angles: &Angles, st: &Stats) -> Vec<Node> {
    use std::collections::HashMap;
    // per (node, delay frames): (click, game advanced to the click), built as delays are needed
    let mut bases: HashMap<(usize, i32), (i32, Game)> = HashMap::new();
    let add_bases = |bases: &mut HashMap<(usize, i32), (i32, Game)>, need: &[(usize, i32)]| {
        let mut need: Vec<(usize, i32)> = need.iter().copied().filter(|k| !bases.contains_key(k)).collect();
        need.sort(); need.dedup();
        let built: Vec<((usize, i32), (i32, Game))> = need.into_par_iter().map(|(ni, d)| {
            let n = &beam[ni];
            let c = grid_click(p, n.grid0, earliest(p, &n.game), d);
            ((ni, d), (c, at_click(&n.game, c)))
        }).collect();
        bases.extend(built);
    };
    let init: Vec<(usize, i32)> = (0..beam.len()).flat_map(|ni| p.delays.iter().map(move |&d| (ni, d))).collect();
    add_bases(&mut bases, &init);
    let zen: Vec<i32> = beam.iter().map(|n| n.game.zen_search_ticks()).collect();
    let mk = |bases: &HashMap<(usize, i32), (i32, Game)>, ni: usize, d: i32, ai: usize| -> Option<Node> {
        let node = &beam[ni];
        let (click, ref b) = bases[&(ni, d)];
        let (g, s) = try_shot(b, click, ai, angles, st);
        if !ok_child(p, &g, &s) { return None; }
        let (frames, grid0) = account(p, node.frames, node.grid0, click, &s.res, zen[ni], g.won);
        Some(Node::new(g, node.steps.push(s), frames, grid0, p))
    };
    let run = |bases: &HashMap<(usize, i32), (i32, Game)>, jobs: &[(usize, i32, usize)], kids: &mut Vec<Vec<Node>>| {
        let out: Vec<(usize, Option<Node>)> = jobs.par_iter().map(|&(ni, d, ai)| (ni, mk(bases, ni, d, ai))).collect();
        for (ni, k) in out { if let Some(k) = k { kids[ni].push(k); } }
    };
    let coarse = angles.coarse(p.coarse);
    let mut kids: Vec<Vec<Node>> = vec![vec![]; beam.len()];
    let mut jobs: Vec<(usize, i32, usize)> = Vec::with_capacity(beam.len() * p.delays.len() * coarse.len());
    for ni in 0..beam.len() { for &d in &p.delays { for &ai in &coarse { jobs.push((ni, d, ai)); } } }
    run(&bases, &jobs, &mut kids);
    let mut seen: Vec<std::collections::HashSet<(i32, usize)>> = vec![Default::default(); beam.len()];
    for &(ni, d, ai) in &jobs { seen[ni].insert((d, ai)); }
    // the delay a child was fired at (refinement keeps it)
    let delay_of = |bases: &HashMap<(usize, i32), (i32, Game)>, ni: usize, click: i32| -> i32 {
        *bases.iter().filter(|(k, v)| k.0 == ni && v.0 == click).map(|(k, _)| &k.1).min().unwrap()
    };
    if p.bucket_top > 0 && p.objective == Objective::Score { bucket_retime(beam, p, angles, st, &mut bases, &add_bases, &run, &delay_of, &mut kids, &mut seen); }
    // refinement: neighbourhoods around each node's best children, shrinking each round
    let mut span = (angles.list.len() / 2 / p.coarse.max(1)).max(1);
    for _ in 0..p.refine_rounds {
        let step = (span / p.refine_width.max(1)).max(1);
        let mut jobs = vec![];
        for ni in 0..beam.len() {
            let ks = &mut kids[ni];
            ks.sort_by(|a, b| a.key.partial_cmp(&b.key).unwrap());
            for k in ks.iter().take(p.refine_top) {
                let s = k.steps.last().unwrap();
                let d = delay_of(&bases, ni, s.click);
                for j in 1..=p.refine_width {
                    for ai in [s.ai.saturating_sub(j * step), (s.ai + j * step).min(angles.list.len() - 1)] {
                        if seen[ni].insert((d, ai)) { jobs.push((ni, d, ai)); }
                    }
                }
            }
        }
        run(&bases, &jobs, &mut kids);
        span = (span / p.refine_div.max(1)).max(1);
        if span == 1 && step == 1 { break; }
    }
    kids.into_iter().flatten().collect()
}

type Bases = std::collections::HashMap<(usize, i32), (i32, Game)>;

/// Bucket timing: each node's best `bucket_top` children that hit a peg without a catch are traced once; the
/// first delay (frames, up to `bucket_max`) that puts the bucket's catch hole under the ball when it reaches the
/// bucket's height (same path, everything later by the delay) is fired too (exact simulation: on a moving
/// board the path can change and miss).
#[allow(clippy::too_many_arguments)]
fn bucket_retime(beam: &[Node], p: &Params, angles: &Angles, st: &Stats, bases: &mut Bases,
                 add_bases: &dyn Fn(&mut Bases, &[(usize, i32)]), run: &dyn Fn(&Bases, &[(usize, i32, usize)], &mut Vec<Vec<Node>>),
                 delay_of: &dyn Fn(&Bases, usize, i32) -> i32, kids: &mut [Vec<Node>], seen: &mut [std::collections::HashSet<(i32, usize)>]) {
    let f = if p.ff { rt::FF } else { 1 };
    // candidates: (node, delay, angle index)
    let mut cand: Vec<(usize, i32, usize)> = vec![];
    for ni in 0..beam.len() {
        let mut ks: Vec<&Node> = kids[ni].iter().filter(|k| { let r = &k.steps.last().unwrap().res; !k.game.won && r.catches == 0 && r.pegs > 0 }).collect();
        ks.sort_by(|a, b| a.key.partial_cmp(&b.key).unwrap());
        for k in ks.into_iter().take(p.bucket_top) { let s = k.steps.last().unwrap(); cand.push((ni, delay_of(bases, ni, s.click), s.ai)); }
    }
    if cand.is_empty() { return; }
    let d0 = p.delays[0];
    let horizon = (p.bucket_max * f) as usize + 3000;
    let mut nodes: Vec<usize> = cand.iter().map(|c| c.0).collect();
    nodes.sort(); nodes.dedup();
    let paths: std::collections::HashMap<usize, Option<(Vec<f32>, f32, f32, i32)>> = nodes.par_iter().map(|&ni| {
        let (_, g) = &bases[&(ni, d0)];
        (ni, bucket_path(g, horizon).map(|(xs, hw, top)| (xs, hw, top, g.tick)))
    }).collect();
    // trace each candidate: harness tick and x when the ball first reaches the bucket's top
    let picks: Vec<Option<(usize, i32, usize)>> = cand.par_iter().map(|&(ni, d, ai)| {
        let Some((xs, hw, top, t0)) = &paths[&ni] else { return None };
        let (click, b) = &bases[&(ni, d)];
        let mut g = b.clone();
        g.trace = Some(vec![]);
        let res = g.shoot(angles.list[ai].0, *click);
        st.shots.fetch_add(1, Ordering::Relaxed);
        let flight0 = res.end_tick - res.frames;
        let tr = g.trace.as_ref().unwrap();
        let k = tr.iter().position(|t| t.0[1] >= *top)?;
        let (x, at) = (tr[k].0[0], flight0 + k as i32);
        let c0 = bases[&(ni, d)].0;
        for d2 in 0..=p.bucket_max {
            if d2 == d { continue; }
            let c2 = grid_click(p, beam[ni].grid0, earliest(p, &beam[ni].game), d2);
            let t = at + (c2 - c0) - t0;
            if t < 0 || t as usize >= xs.len() { continue; }
            if (xs[t as usize] - x).abs() < hw - 12.0 { return Some((ni, d2, ai)); }
        }
        None
    }).collect();
    let jobs: Vec<(usize, i32, usize)> = picks.into_iter().flatten().filter(|&(ni, d, ai)| seen[ni].insert((d, ai))).collect();
    let need: Vec<(usize, i32)> = jobs.iter().map(|j| (j.0, j.1)).collect();
    add_bases(bases, &need);
    let mut v: Vec<Vec<Node>> = vec![vec![]; beam.len()];
    run(bases, &jobs, &mut v);
    for (ni, ks) in v.into_iter().enumerate() { kids[ni].extend(ks); }
}

/// Dense final shot (`Params::final_scan`): from each node, a fan of `FINAL_FAN` angles at the probe delays, then
/// neighbourhoods around the best winners refined down to single reachable angles; the best winning child.
const FINAL_FAN: usize = 4000;
fn final_scan(nodes: &[&Node], p: &Params, angles: &Angles, st: &Stats) -> Option<Node> {
    if nodes.is_empty() { return None; }
    let bases: Vec<(usize, Game, i32)> = nodes.iter().enumerate().flat_map(|(ni, n)| p.probe_delays.iter().map(move |&d| (ni, d, n)))
        .collect::<Vec<_>>().into_par_iter().map(|(ni, d, n)| {
            let c = grid_click(p, n.grid0, earliest(p, &n.game), d);
            (ni, at_click(&n.game, c), c)
        }).collect();
    let shoot = |jobs: &[(usize, usize)]| -> Vec<(usize, usize, Node)> {
        jobs.par_iter().filter_map(|&(bi, ai)| {
            let (ni, b, click) = &bases[bi];
            let node = nodes[*ni];
            let (g, s) = try_shot(b, *click, ai, angles, st);
            if !g.won || !ok_child(p, &g, &s) { return None; }
            let (frames, grid0) = account(p, node.frames, node.grid0, *click, &s.res, node.game.zen_search_ticks(), true);
            Some((bi, ai, Node::new(g, node.steps.push(s), frames, grid0, p)))
        }).collect()
    };
    let fan = angles.coarse(FINAL_FAN);
    let jobs: Vec<(usize, usize)> = (0..bases.len()).flat_map(|bi| fan.iter().map(move |&ai| (bi, ai))).collect();
    let mut seen: std::collections::HashSet<(usize, usize)> = jobs.iter().copied().collect();
    let mut wins = shoot(&jobs);
    let mut span = (angles.list.len() / 2 / FINAL_FAN).max(1);
    while span > 1 {
        let step = (span / 8).max(1);
        wins.sort_by(|a, b| a.2.key.partial_cmp(&b.2.key).unwrap().then((a.0, a.1).cmp(&(b.0, b.1))));
        let mut jobs = vec![];
        for w in wins.iter().take(6) {
            for j in 1..8usize { for ai in [w.1.saturating_sub(j * step), (w.1 + j * step).min(angles.list.len() - 1)] { if seen.insert((w.0, ai)) { jobs.push((w.0, ai)); } } }
        }
        wins.extend(shoot(&jobs));
        span = step;
    }
    wins.into_iter().min_by(|a, b| a.2.key.partial_cmp(&b.2.key).unwrap().then((a.0, a.1).cmp(&(b.0, b.1)))).map(|w| w.2)
}

/// Plan compaction: drop a shot that hit nothing and was not caught (it only cost a ball), keep each later
/// shot's delay on its own frame grid (frames after its earliest click), and keep the shorter plan if it
/// still wins with a higher total. Repeats until no shot can go.
pub fn compact(root: &Game, steps: &[Step], p: &Params, angles: &Angles) -> Option<Node> {
    let delays = |steps: &[Step]| -> Option<Vec<i32>> {
        let mut n = Node::new(root.clone(), Trail::default(), 0, 0, p);
        let mut v = vec![];
        for s in steps {
            let c0 = grid_click(p, n.grid0, earliest(p, &n.game), 0);
            let f = if p.ff { rt::FF } else { 1 };
            if (s.click - c0) % f != 0 || s.click < c0 { return None; }
            v.push((s.click - c0) / f);
            n = prefix_node_step(n, s.angle, s.click, p, angles);
        }
        Some(v)
    };
    let play = |shots: &[(f32, i32)]| -> Option<Node> {
        let mut n = Node::new(root.clone(), Trail::default(), 0, 0, p);
        for &(a, d) in shots {
            if n.game.over() { return None; }
            let c = grid_click(p, n.grid0, earliest(p, &n.game), d);
            n = prefix_node_step(n, a, c, p, angles);
        }
        n.game.won.then_some(n)
    };
    let mut cur: Vec<(f32, i32)> = steps.iter().map(|s| s.angle).zip(delays(steps)?).collect();
    let mut best = play(&cur)?;
    loop {
        let bs = best.steps.to_vec();
        let mut improved = false;
        for k in (0..bs.len()).rev() {
            let r = &bs[k].res;
            if r.won || r.pegs > 0 || r.catches > 0 || r.free_balls > 0 { continue; }
            let mut t = cur.clone();
            t.remove(k);
            if let Some(n) = play(&t) { if n.game.total > best.game.total { cur = t; best = n; improved = true; break; } }
        }
        if !improved { break; }
    }
    Some(best)
}

fn prefix_node_step(n: Node, angle: f32, click: i32, p: &Params, angles: &Angles) -> Node {
    let ai = angles.list.iter().position(|a| a.0.to_bits() == angle.to_bits()).expect("angle not reachable");
    let mut g = n.game.clone();
    let zen = g.zen_search_ticks();
    let res = g.shoot(angle, click);
    let (frames, grid0) = account(p, n.frames, n.grid0, click, &res, zen, g.won);
    Node::new(g, n.steps.push(Step { ai, angle, pixel: angles.list[ai].1, click, res }), frames, grid0, p)
}

pub struct Outcome {
    pub best: Option<Node>,
    /// best node per depth (for reporting)
    pub frontier_best: Vec<(usize, f64, i32, i32)>,
}

pub fn solve(root: &Game, p: &Params, angles: &Angles, st: &Stats, verbose: bool) -> Outcome {
    solve_from(root, Trail::default(), p, angles, st, verbose)
}

/// `solve` continuing after `start` (shots already played to reach `root`, e.g. a fixed plan prefix)
pub fn solve_from(root: &Game, start: Trail, p: &Params, angles: &Angles, st: &Stats, verbose: bool) -> Outcome {
    solve_node(Node::new(root.clone(), start, 0, 0, p), p, angles, st, verbose)
}

/// `solve` from a node (its frames and fast-forward frame grid included: see `prefix_node`)
pub fn solve_node(root: Node, p: &Params, angles: &Angles, st: &Stats, verbose: bool) -> Outcome {
    let mut beam = vec![root];
    let mut best: Option<Node> = None;
    let mut fb = vec![];
    for depth in 0..p.max_shots {
        let t0 = std::time::Instant::now();
        let mut kids: Vec<Node> = expand_all(&beam, p, angles, st);
        kids.sort_by(|a, b| a.key.partial_cmp(&b.key).unwrap());
        for k in kids.iter().filter(|k| k.game.won) {
            if best.as_ref().is_none_or(|b| k.key < b.key) { best = Some(k.clone()); }
            break;
        }
        // next beam: unwon, distinct states
        let probing = p.objective == Objective::Score && p.probe_n > 0;
        let cap = if probing { p.probe_pool.max(p.beam) } else { p.beam };
        let mut next = vec![];
        let mut sigs = std::collections::HashSet::new();
        let kids = if probing && p.strata {
            // pool: the best distinct children of every orange count (not just of the most advanced ones)
            let mut d = vec![];
            let mut seen = std::collections::HashSet::new();
            for k in kids.into_iter() { if !k.game.won && seen.insert(k.sig_score(p)) { d.push(k); } }
            strat(d, cap, p)
        } else { kids };
        for k in kids.into_iter() {
            if k.game.won { continue; }
            if !(probing && p.strata) && !sigs.insert(k.sig()) { continue; }
            // speed: a node that cannot beat the best finish even with a perfect next shot is dropped
            if let (Objective::Speed | Objective::Uef, Some(b)) = (p.objective, &best) {
                if k.frames >= b.frames { continue; }
            }
            next.push(k);
            if next.len() >= cap { break; }
        }
        if probing {
            let pv = probe_all(&next, p, angles, st);
            for (n, v) in next.iter_mut().zip(pv) { n.set_probe(v, p); }
            next.sort_by(|a, b| a.key.partial_cmp(&b.key).unwrap());
            if p.strata { next = strat(next, p.beam, p); } else { next.truncate(p.beam); }
        }
        if p.final_scan > 0 && p.objective == Objective::Score {
            let top: Vec<&Node> = next.iter().filter(|n| n.game.oranges_left() == 1).take(p.final_scan).collect();
            if let Some(w) = final_scan(&top, p, angles, st) {
                if verbose { eprintln!("  final scan: win {} after {} shots", w.game.total, w.steps.len()); }
                if best.as_ref().is_none_or(|b| w.key < b.key) { best = Some(w); }
            }
        }
        if let Some(n) = next.first() {
            fb.push((depth + 1, n.key, n.game.oranges_left(), n.game.earliest_fire));
        }
        if verbose {
            let b = best.as_ref().map(|b| format!("best: {} shots, {} frames, end tick {}, score {}", b.steps.len(), b.frames, b.steps.last().unwrap().res.end_tick, b.game.total)).unwrap_or("no win yet".into());
            let pr = if next.iter().any(|n| n.probe.is_some()) { format!("; probes {:?}", next.iter().take(5).map(|n| (n.probe.unwrap_or(0.0) / 1000.0) as i64).collect::<Vec<_>>()) } else { String::new() };
            eprintln!("  depth {}: {} nodes kept ({:?}); frontier oranges left {:?}{pr}; {b}; {} shots simulated",
                      depth + 1, next.len(), t0.elapsed(), next.iter().take(5).map(|n| n.game.oranges_left()).collect::<Vec<_>>(), st.shots.load(Ordering::Relaxed));
        }
        if next.is_empty() { break; }
        // fewest shots: the first depth with a win is optimal for this beam
        if p.objective == Objective::Shots && best.is_some() { break; }
        beam = next;
    }
    Outcome { best, frontier_best: fb }
}

/// The node after playing `shots` (angle, click) from `root`, with the frame accounting and the fast-forward
/// click grid the search itself would have (a later shot's click must sit on that grid to land in the real game).
pub fn prefix_node(root: &Game, shots: &[(f32, i32)], p: &Params, angles: &Angles) -> Node {
    let mut n = Node::new(root.clone(), Trail::default(), 0, 0, p);
    for &(angle, click) in shots { n = prefix_node_step(n, angle, click, p, angles); }
    n
}

/// Replay a plan from scratch (as the game would) and check it reproduces.
pub fn replay(root: &Game, steps: &[Step]) -> Result<Game, String> {
    let mut g = root.clone();
    for (k, s) in steps.iter().enumerate() {
        if s.click < g.earliest_fire { return Err(format!("shot {}: click {} before earliest {}", k + 1, s.click, g.earliest_fire)); }
        let r = g.shoot(s.angle, s.click);
        if r.pegs != s.res.pegs || r.score != s.res.score || r.end_tick != s.res.end_tick {
            return Err(format!("shot {}: replay differs (pegs {} vs {}, score {} vs {})", k + 1, r.pegs, s.res.pegs, r.score, s.res.score));
        }
    }
    Ok(g)
}

/// Single-shot objectives (Pegs / ShotScore): the top `n` opening shots.
pub fn best_single(root: &Game, p: &Params, angles: &Angles, st: &Stats, n: usize) -> Vec<Node> {
    let node = Node::new(root.clone(), Trail::default(), 0, 0, p);
    let mut kids = expand_all(std::slice::from_ref(&node), p, angles, st);
    kids.sort_by(|a, b| a.key.partial_cmp(&b.key).unwrap());
    let mut out: Vec<Node> = vec![];
    for k in kids {
        // distinct outcomes only
        if out.iter().any(|o| o.steps.last().unwrap().res.pegs == k.steps.last().unwrap().res.pegs && o.steps.last().unwrap().res.score == k.steps.last().unwrap().res.score) { continue; }
        out.push(k);
        if out.len() >= n { break; }
    }
    out
}
