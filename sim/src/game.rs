//! Self-contained adventure-level game state: everything the TAS harness used to read back from the
//! real game at each ready point (peg colours, power turn counters, balls, Lucky Spin carry-overs) is
//! modelled here, so a whole level can be played offline from (level, RNG seed, character) and the
//! resulting script replayed in the real game (oracle hook `PEGGLE_RNG_SEED`, `shotx ANGLE TICK`).
//!
//! Timeline: `tick` = harness tick = object updates since level load (the bucket counter). A shot is
//! fired by a click at tick T (the hook's `shotx A T`); Gun::Fire launches the ball 9 ticks later
//! (LogicMgr+0xf0 countdown), i.e. after the object update that brings the counters to T + 9.
//!
//! Turn logic (FUN_0046e880 → FUN_00461560):
//! - power turn counters c[p] = LogicMgr+0x1d4[(player + 2p)]: a green hit (FUN_0046edf0) adds
//!   SuperGuide 3 (+1 if 0), Flippers 3, Pyramid 5, Spooky 1, Zen 1 (+1 if 0), Fireball 1 (+1 if 0),
//!   Nudge/TimeBomb 1; each turn start decrements every positive counter and re-applies the power
//!   (FUN_004612e0: flippers on/off — and FUN_004582f0 every time it is on —, pyramid off, lid).
//! - Lucky Spin: magic hat (+0x60, +0x69) and score tripler (+0x58, +0x68) apply to the current shot
//!   and carry to the next one; extra ball +1.
//! - balls (+0x17c): −1 per shot, +1 per free-ball threshold (score.rs), per bucket catch, per extra
//!   ball.
//! - catch streak (+0x320): negated at turn start (a negative streak resets to 0).
//! - purple re-roll at turn start if the shot hit any peg (LogicMgr+0x12c > 0).
use crate::colors;
use crate::level::Level;
use crate::phys::{Ball, Board, Event, Gun, Hat};
use crate::rng::{self, Mt};
use crate::score::{ShotScore, ORANGE};
use crate::tube::Tube;

/// Click → Gun::Fire (LogicMgr+0xf0 = 8 countdown).
pub const FIRE_LAG: i32 = 9;
/// Zen Shot: the board is frozen during the Zen state; Gun::Fire comes 2 object updates after the click.
pub const ZEN_LAG: i32 = 1;
/// Earliest first fire tick (the level intro ends at a wall-clock-dependent tick, observed ≤ 300; the
/// hook needs the aim held 3 ticks before it clicks).
pub const FIRST_FIRE: i32 = 310;
/// After the ball is gone: peg pop phase = 47 + 5 per peg hit, then the turn start (LogicMgr state 7 → 1)
/// once the ball tube is at rest; clicks are taken 28 ticks after the turn's ball has left the tube
/// (`Game::tube_turn`, tube.rs; exact on every non-FF shot of the golden captures without a margin below).
pub const POP_BASE: i32 = 47;
pub const POP_PER_PEG: i32 = 5;
/// extra margins for phases not modelled step by step, set from the real ready points of every consistent
/// capture (2026-11-05: need = margin − (model earliest − real ready − 3)): a tripled shot's tally needs
/// 121..186 (404 shots; with 0..2 tally free balls, not a clean function of score or balls), and 204 on
/// score3 11-5 shot 21 (2026-11-14: 7 pegs, 3 oranges, 1 tally free ball); a Zen shot 0..1 (13 exact + 14 FF);
/// a Lucky Spin 0 (8 exact + 222 FF spins).
pub const TRIPLE_MARGIN: i32 = 204;
pub const ZEN_MARGIN: i32 = 1;
/// Extra turn-start wait per free ball the Score Tripler tally earns (`ShotResult::tally_balls`): the margin
/// over all captured tripled shots was 73..83 with no tally ball but down to -16 with one or two (score4 5-1
/// shot 57, 6-2 shot 41, 7-2 shot 11, 2026-11-15); 10 per ball makes every one safe, 12 leaves slack.
pub const TALLY_BALL_MARGIN: i32 = 12;
// The Zen AI search runs `zen::search_steps` (500) live object updates that advance the harness tick, then
// restores the board (counters included): a Zen shot costs that many harness ticks with no board time
// (stage 9 captures: GUN counter = click + 501, next shot's counter 500 behind the script). `Game::zen_search_ticks`.
pub const LUCKY_MARGIN: i32 = 0;
/// A shot with no peg hit and no score ends in a coin flip (FUN_00471030 → FUN_00471500): the next shot
/// is ready later; one capture with 14 flips, won and lost): ready − ball gone 179..317 vs ~111 without.
/// Margin need up to 231 (score3 1-5 shot 30, 2026-11-14; was 215).
pub const COIN_MARGIN: i32 = 231;
/// With fast forward held (8 ticks per frame outside ball flight) the real ready tick lands on the
/// frame grid, up to 7 past the exact one. `ff_pad` is added on top in FF plans.
pub const FF_PAD: i32 = 7;
/// Real frames a Lucky Spin freezes the game for: wheel updates + this (4/4 spins exact, warren_cal runs;
/// the board tick does not advance meanwhile and fast forward does not apply)
pub const SPIN_FIXED_FRAMES: i32 = 139;
/// a free ball earned at a hit drops into the tube this many ticks after the hit's Board counter + lag
pub const TUBE_HIT_OFS: i32 = 0;
static TUBE_TRACE: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| std::env::var_os("TUBE_TRACE").is_some());
/// earliest accepted click − the tick the turn's ball left the tube − 28
pub const READY_OFS: i32 = 4;
pub const ALL_POPPED_WAIT: i32 = 5;

/// End-of-shot pop phase for `n` hit pegs (after POP_BASE, counted from the end of `pop_tail`): 5 per
/// peg up to 30 (exact in 330+ captured shots); beyond that the pop delay shrinks
/// (clamp(8 − remaining/10, 1, 5)): that sum is exact on ~100 big shots but one (4-3, a long
/// pop tail) needed 2 more, so 3 margin.
pub fn pop_ticks(n: i32) -> i32 {
    if n <= 30 { return 5 * n; }
    let mut t = 0;
    for rem in 0..n { t += (8 - rem / 10).clamp(1, 5); }
    t + 3
}

#[derive(Clone, Debug, Default)]
pub struct ShotResult {
    pub angle: f32,          // fired angle (Zen: the AI's pick)
    pub fire_tick: i32,
    pub frames: i32,         // ticks from launch until the ball is gone
    pub end_tick: i32,       // tick when the ball is gone
    pub pegs: i32,           // first hits this shot
    pub oranges: i32,
    pub score: i64,          // added to the total (styles included, tripler applied)
    pub shot_score: i64,     // LogicMgr+0x108
    pub styles: Vec<u8>,
    pub catches: i32,
    pub free_balls: i32,
    pub fever_hole: i32,
    /// real frames spent in Lucky Spin slot machines during this shot (board frozen)
    pub spin_frames: i32,
    pub won: bool,
    pub activations: Vec<u8>,
    pub lucky: Vec<u32>,
    /// ticks after the ball is gone that LogicMgr stays in state 2 because a pop round started in flight
    /// is still running (FUN_00472f60 waits for +0xc == 0; every pop re-arms it while pegs remain). One
    /// tick per app frame even with fast forward, and the next frame grid starts after it.
    pub pop_tail: i32,
    /// physics tick (from launch) at which the near-win slow-mo look-ahead fired (Board::slowmo_peek); a
    /// click cuts it (+22 frames under fast forward) and it fires at most once per shot
    pub slowmo_at: Option<i32>,
    /// a Zen Shot (the AI picked `angle`), a coin flip after the shot (no peg, no score, no catch), a tripled
    /// score (Score Tripler or Lucky Spin result 2): the phases still covered by margins (ZEN/COIN/TRIPLE_MARGIN)
    pub zen: bool,
    pub coin: bool,
    pub tripled: bool,
    /// free balls the tripled-score tally awards (`ShotScore::tally_triple`)
    pub tally_balls: i32,
}

#[derive(Clone)]
pub struct Game {
    pub board: Board,
    pub gun: Gun,
    pub tick: i32,
    pub seed: u32,
    pub balls: i32,
    pub total: i64,
    pub shots: u32,
    pub ctr: [i32; 14],
    pub trip_pend: i32,
    pub hat_pend: i32,
    pub trip_now: bool,
    pub hat_now: bool,
    pub purple: Option<usize>,
    /// earliest tick at which the next click is accepted (see POP_BASE / tube_turn)
    pub earliest_fire: i32,
    pub won: bool,
    pub lost: bool,
    /// LogicMgr+0x15c: highest unlocked character index (Lucky Spin random-power list)
    pub unlocked: i32,
    /// debug: when Some, every shot frame pushes ball 0's (pos, vel) before the update
    pub trace: Option<Vec<([f32; 2], [f32; 2])>>,
    /// extra ticks after `earliest_fire` for a fast-forward click (FF_PAD..)
    pub ff_pad: i32,
    /// debug: every live ball's (pos, vel) per frame, before the update
    pub trace_all: Option<Vec<Vec<([f32; 2], [f32; 2])>>>,
    /// the ball tube (timing of the next turn start and of the ready point, see tube.rs)
    pub tube: Tube,
    /// Profile tip bits (Profile+0x64: bit 0 catch, 1 score free ball, 2 coin flip; a fresh profile, as in
    /// every oracle run, has none) and this level's LogicMgr+0x19/+0x1a/+0x1b flags (a catch / a score free
    /// ball / a coin flip happened, same bits). See `tip_check`.
    pub tip_bits: u8,
    pub tip_flags: u8,
    /// Beginner aim assist (FUN_00467ca0, at the click when no Zen Shot is armed): in adventure mode with a
    /// fresh profile (+0x48 < 1) it runs when the balls left (LogicMgr+0x17c, incl. the one being shot) are at
    /// most this; an AI pass (FUN_00467b80(1)) that advances the harness `zen::search_steps` object updates
    /// with the board frozen and keeps the aim. 6 on stage 0 level 0 (app+0x764/+0x768 both 0), else 0 here
    /// (other levels: 0..3 from LogicMgr+0x160, never seen to trigger). Set by `set_stage_level`.
    /// The aim is kept because the playback reports the aim guide's first hit at step 0 (Gun+0x198 = 0, see
    /// `hook_collide` in play/hook.c): the assist's ball filter then rejects every ball and the gun stays put.
    pub assist_balls: i32,
}

/// Next turn's tip check (FUN_0046e880 → FUN_0046a370(-1)): the first of tips 0 (catch), 1 (score free
/// ball), 2 (coin flip) whose event happened this level and that the profile has not shown yet is shown
/// and its profile bit set (FUN_004038c0); at most one per turn. (Later tips do not affect play.) Returns the
/// new profile bits.
fn tip_check(flags: u8, bits: u8) -> u8 {
    for b in 0..3 {
        if flags & (1 << b) != 0 && bits & (1 << b) == 0 { return bits | 1 << b; }
    }
    bits
}

impl Game {
    /// Adventure stage/level (0-based) of the level being played: see `assist_balls`.
    pub fn set_stage_level(&mut self, stage: usize, level: usize) {
        self.assist_balls = if stage == 0 && level == 0 { 6 } else { 0 };
    }

    /// Level start (FUN_0045dda0): colours with the hook's reseeds n = 0 (oranges), 1 (purple),
    /// 2 (greens). `power` = the character's power (oracle::CHAR_POWERS).
    pub fn new(level: &Level, base: &Level, seed: u32, power: u8) -> Game {
        let mut board = Board::from_levels(level, Some(base));
        board.rng_seed = seed;
        board.power = power;
        board.btick = 2; // Board+0x1c4 + 2 == harness tick (tas_play sync)
        let mut g = Game { board, gun: Gun::default(), tick: 0, seed, balls: 10, total: 0, shots: 0, ctr: [0; 14],
                           trip_pend: 0, hat_pend: 0, trip_now: false, hat_now: false, purple: None,
                           earliest_fire: FIRST_FIRE, won: false, lost: false, unlocked: 9, trace: None, ff_pad: 0, trace_all: None,
                           tube: Tube::reset(10), tip_bits: 0, tip_flags: 0, assist_balls: 0 };
        // first turn: the first ball leaves the tube long before FIRST_FIRE
        g.tube.kick();
        for _ in 0..400 { g.tube.step(); if g.tube.settled { break; } }
        let mut r = g.next_rng();
        g.board.orange_total = colors::assign_oranges(&mut g.board, &mut r, 0);
        let mut r = g.next_rng();
        colors::purple_reroll(&mut g.board, &mut r, &mut g.purple);
        let mut r = g.next_rng();
        colors::assign_greens(&mut g.board, &mut r);
        g.board.lucky_list = crate::oracle::CHAR_POWERS[..(g.unlocked + 1).clamp(0, 10) as usize].iter().copied().filter(|&p| p != 13).collect();
        if g.board.lucky_list.is_empty() { g.board.lucky_list.push(1); }
        g
    }

    fn next_rng(&mut self) -> Mt { let m = colors::hook_rng(self.seed, self.board.rng_n); self.board.rng_n += 1; m }

    pub fn oranges_left(&self) -> i32 { self.board.orange_total }
    pub fn over(&self) -> bool { self.won || self.lost }
    pub fn zen_armed(&self) -> bool { self.ctr[7] > 0 }
    pub fn fireball(&self) -> bool { self.ctr[12] > 0 }

    /// Advance the board's objects to tick `t` (aiming time).
    pub fn advance_to(&mut self, t: i32) {
        while self.tick < t { self.board.tick_objects(); self.tick += 1; }
    }

    /// Board ticks lag harness ticks by this much (+2 at level start, + the search steps per Zen shot).
    pub fn board_lag(&self) -> i32 { self.tick - self.board.btick
    }

    fn zen_ctx(&self) -> crate::zen::ZenCtx {
        crate::zen::ZenCtx { fever_off: false, l124: false, l18: false, power: self.board.power, guide: self.ctr[1] > 0,
                             fireball: self.ctr[12] > 0, ctr11: self.ctr[11] > 0, oranges_left: self.board.orange_total, l244: false }
    }

    /// Harness ticks the next shot's Zen search (or beginner aim assist, `assist_balls`) costs (board frozen):
    /// `zen::search_steps`, 0 if neither runs.
    pub fn zen_search_ticks(&self) -> i32 {
        if self.zen_armed() || self.balls <= self.assist_balls { crate::zen::search_steps(&self.board, &self.zen_ctx()) } else { 0 }
    }

    /// Fire at `angle` with a click at tick `fire_tick` (>= earliest_fire). For a Zen shot `angle` is
    /// the aim and the game's AI picks the fired angle.
    pub fn shoot(&mut self, angle: f32, fire_tick: i32) -> ShotResult {
        assert!(!self.over());
        let zen = self.zen_armed();
        let lag = if zen { ZEN_LAG } else { FIRE_LAG };
        // the board may already be advanced to the launch (solver: shared pre-advance per click tick)
        assert!(fire_tick + lag >= self.tick, "fire tick {fire_tick} before now {}", self.tick);
        self.advance_to(fire_tick + lag);
        let angle = if zen { crate::zen::zen_pick(&self.board, &self.gun, angle, &self.zen_ctx(), None) } else { angle };
        if zen { self.tick += crate::zen::search_steps(&self.board, &self.zen_ctx()); } // board frozen
        else if self.balls <= self.assist_balls { self.tick += crate::zen::search_steps(&self.board, &self.zen_ctx()); }
        let fireball = self.fireball();
        let bd = &mut self.board;
        let mut ball: Ball = self.gun.launch(angle, if fireball { 10.0 } else { 6.0 });
        ball.fireball = fireball;
        if self.hat_now { ball.hat = Some(Hat { anchor: ball.pos, pos: [0.0; 2], ctr: 100 }); }
        ball.spooky = bd.spooky_left > 0;
        bd.triple = self.trip_now;
        bd.begin_shot();
        {   // LogicMgr+0x2c/+0x30 rolled at fire (hook keyed site 0x5B03, key = launch btick - 1)
            let mut m = rng::keyed(bd.rng_seed, rng::SITE_SHOT, bd.btick - 1);
            let r1 = m.rand() as u32; let r2 = m.rand() as u32;
            bd.lucky_ctr = Some([r1, r2]);
            if std::env::var_os("SIM_DEBUG").is_some() { eprintln!("shot ctr roll key {} -> {r1} {r2} (fire tick {fire_tick})", bd.btick - 1); }
        }
        let mut ev = vec![];
        let mut balls = vec![ball];
        let mut frames = 0;
        let mut catches = 0;
        // balls dropped into the tube during the shot: (harness tick, count)
        let mut tube_adds: Vec<(i32, i32)> = vec![];
        let flight_start = self.tick;
        let (mut slowmo_at, mut targets, mut tgt_hits) = (None, vec![], None);
        for n in 0..30000 {
            ev.clear();
            if let Some(t) = self.trace.as_mut() { t.push((balls[0].pos, balls[0].vel)); }
            if let Some(t) = self.trace_all.as_mut() { t.push(balls.iter().filter(|b| !b.gone).map(|b| (b.pos, b.vel)).collect()); }
            let alive = bd.frame_multi(&mut balls, &mut ev);
            frames = n + 1;
            if slowmo_at.is_none() && alive && balls.iter().filter(|b| !b.gone).count() < 4 {
                // targets change only with this shot's first hits (oranges left, hit flags), or with moving pegs
                // (Splork's 100 px test)
                if tgt_hits != Some(bd.shot_hits.len()) || (bd.power == 5 && !bd.movers.is_empty()) {
                    bd.slowmo_targets(&mut targets);
                    tgt_hits = Some(bd.shot_hits.len());
                }
                if !targets.is_empty() && (0..balls.len()).any(|k| !balls[k].gone && bd.slowmo_peek(&balls[k], &targets)) { slowmo_at = Some(n + 1); }
                if slowmo_at.is_some() && std::env::var_os("SIM_DEBUG").is_some() {
                    eprintln!("slowmo at {} ball {:?} v {:?} targets {:?} left {} hits {:?}", n + 1, balls[0].pos, balls[0].vel, targets.iter().map(|&i| (bd.objs[i].src_index, bd.objs[i].bbox)).collect::<Vec<_>>(), bd.oranges_left(),
                        bd.shot_hits.iter().zip(&bd.shot_hit_bticks).filter(|p| bd.objs[*p.0].peg_type == ORANGE).map(|(&i, &t)| (bd.objs[i].src_index, t)).collect::<Vec<_>>());
                }
            }
            for e in &ev { if let Event::Captured(i) = e { if bd.objs[*i].freeball { catches += 1; tube_adds.push((self.tick + n + 1, 1)); } } }
            if !alive || balls.iter().all(|b| b.gone) { break; }
        }
        self.tick += frames;
        let pop_tail = {
            let mut tail = 0;
            if bd.pop_delay > 0 && !bd.hit_list.is_empty() {
                let (mut d, mut rem) = (bd.pop_delay, bd.hit_list.len() as i32);
                loop { tail += d; rem -= 1; if rem <= 0 { break; } d = (8 - rem / 10).clamp(1, 5); }
            }
            tail
        };
        // scoring (BallHitPeg first hits, in order)
        let mut ss = ShotScore::default();
        let mut rem = bd.orange_total;
        let blag = self.tick - bd.btick; // harness tick − Board+0x1c4 counter
        for (j, &i) in bd.shot_hits.iter().enumerate() {
            let fb = ss.free_balls;
            ss.hit(bd.objs[i].peg_type, &mut rem);
            // free balls from the hit's score (FUN_0046e740 → FUN_0045c860) drop in at the hit
            if ss.free_balls > fb { tube_adds.push((bd.shot_hit_bticks.get(j).map_or(self.tick, |&b| b + blag + TUBE_HIT_OFS).max(flight_start), ss.free_balls - fb)); }
        }
        let won = rem == 0;
        let factor = bd.score_factor();
        // tripler tally balls join the tube during the tally (TRIPLE_MARGIN covers it)
        let tally_balls = if factor == 3 { ss.tally_triple(rem) } else { 0 };
        if bd.fever_hole > 0 { ss.fever_end(bd.fever_hole, self.balls); }
        let style_sum: i64 = bd.styles.iter().map(|s| s.1 as i64).sum();
        let oranges = bd.shot_hits.iter().filter(|&&i| bd.objs[i].peg_type == ORANGE).count() as i32;
        let lucky = bd.lucky_results.clone();
        let extra = lucky.iter().filter(|&&r| r == 3).count() as i32;
        let mut res = ShotResult {
            angle, fire_tick, frames, end_tick: self.tick, pegs: ss.pegs, oranges, score: ss.score * factor + style_sum,
            shot_score: ss.score, styles: bd.styles.iter().map(|s| s.0).collect(), catches, free_balls: ss.free_balls,
            fever_hole: bd.fever_hole, spin_frames: bd.lucky_steps.iter().map(|n| n + SPIN_FIXED_FRAMES).sum(), won, activations: bd.activations.clone(), lucky: lucky.clone(), pop_tail, slowmo_at,
            zen, coin: false, tripled: factor == 3, tally_balls,
        };
        self.shots += 1;
        self.total += res.score;
        self.balls += -1 + ss.free_balls + catches + extra;
        // FUN_00471030: shot score (+0x108) < 1 and no peg hit (+0x12c == 0) → coin flip; FUN_004373f0: won
        // when LogicMgr+0x30 % 100 <= 60 (80 with no balls left, +0x17c); a win gives the ball back
        // (not after a bucket catch: the 0-peg bucket shots of the score runs get no flip)
        let coin = res.pegs == 0 && ss.score == 0 && catches == 0 && !won;
        res.coin = coin;
        let mut coin_won = false;
        if coin {
            let r = bd.lucky_ctr.map_or(i32::MAX, |c| (c[1] as i32).wrapping_rem(100));
            // FUN_004373f0: with tips on and the coin-flip tip (profile bit 2) not yet shown, the flip is won
            if self.tip_bits & 4 == 0 || r <= if self.balls < 1 { 80 } else { 60 } { self.balls += 1; coin_won = true; }
        }
        if catches > 0 { self.tip_flags |= 1; }
        if ss.free_balls > 0 { self.tip_flags |= 2; }
        if coin { self.tip_flags |= 4; }
        self.tip_bits = tip_check(self.tip_flags, self.tip_bits);
        bd.orange_total = rem;
        // shot end: hit pegs are removed
        // (power hits — Space Blast, Flower Power — go through BallHitPeg without a contact: mark them too)
        for &i in &bd.shot_hits { bd.objs[i].hit = true; }
        for o in bd.objs.iter_mut() { if o.has_peginfo && o.hit { o.collidable = false; o.remove_timer = 0; } }
        // power counters (FUN_0046edf0 increments, in activation order)
        for &p in &res.activations {
            let c = &mut self.ctr[p as usize];
            *c += match p { 1 => 3 + (*c == 0) as i32, 2 => 3, 4 => 5, 7 => 1 + (*c == 0) as i32, 12 => 1 + (*c == 0) as i32, 10 | 11 => 1, _ => 0 };
        }
        self.ctr[6] = self.board.spooky_left;
        for &r in &lucky {
            match r {
                1 => { self.hat_pend += 1 + self.hat_now as i32; self.hat_now = true; }
                2 => { self.trip_pend += 1 + self.trip_now as i32; self.trip_now = true; }
                _ => {}
            }
        }
        if won { self.won = true; return res; }
        if self.balls <= 0 { self.lost = true; return res; }
        let tripled = factor == 3;
        self.turn_start(res.pegs > 0);
        // phases not modelled step by step delay the turn start by a margin; their balls (Lucky Spin extra
        // ball, won coin flip) join the tube at rest
        let wait = LUCKY_MARGIN * lucky.len() as i32 + (if tripled { TRIPLE_MARGIN } else { 0 }) + (if zen { ZEN_MARGIN } else { 0 })
            + (if coin { COIN_MARGIN } else { 0 });
        let wait = wait + TALLY_BALL_MARGIN * tally_balls;
        let unmodelled = extra + coin_won as i32 + tally_balls;
        // every hit peg already popped during the flight: the turn starts 5 ticks later (5/5 captured shots)
        let all_popped = res.pegs > 0 && self.board.hit_list.is_empty();
        let k0 = self.tick + res.pop_tail + POP_BASE - 1 + pop_ticks(res.pegs) + wait + if all_popped { ALL_POPPED_WAIT } else { 0 };
        let out = self.tube_turn(flight_start, k0, tube_adds, unmodelled);
        self.earliest_fire = out + 28 + READY_OFS;
        self.ff_pad = FF_PAD;
        res
    }

    /// The ball tube from the shot's launch to the next ready point: balls dropped in during the shot
    /// (`adds`, harness ticks), the turn start (FUN_0044b5b0, LogicMgr states 6/7) at the first tick ≥ `k0`
    /// with the tube at rest, then the moving-out ball. Returns the tick the ball leaves the tube
    /// (LogicMgr+0x138 = 2); clicks are accepted 28 ticks later.
    fn tube_turn(&mut self, from: i32, k0: i32, mut adds: Vec<(i32, i32)>, unmodelled: i32) -> i32 {
        let tube = &mut self.tube;
        adds.sort_unstable();
        let mut t = adds.first().map_or(k0, |a| a.0.min(k0)).max(from);
        let mut ai = 0;
        // logic first (turn start once at rest), then the drops of that tick, then the tube update
        while !(t >= k0 && tube.settled) && t < k0 + 5000 {
            while ai < adds.len() && adds[ai].0 <= t { tube.add_top(adds[ai].1); ai += 1; }
            tube.step();
            if *TUBE_TRACE { eprintln!("TUBE {t} {:?}", tube); }
            t += 1;
        }
        for &(_, k) in &adds[ai..] { tube.add_top(k); }
        if unmodelled > 0 { *tube = Tube::reset(tube.v.len() as i32 + unmodelled); }
        if std::env::var_os("SIM_DEBUG").is_some() || (tube.v.len() as i32 != self.balls && std::env::var_os("TUBE_WARN").is_some()) {
            eprintln!("tube: adds {adds:?} k0 {k0} kick {t} balls {} tube {}", self.balls, tube.v.len());
        }
        let mut out = t;
        if !tube.kick() {
            loop { if tube.step() || out > t + 100 { break; } out += 1; }
        }
        for _ in 0..200 { if tube.settled { break; } tube.step(); }
        out
    }

    /// FUN_0046e880 (non-duel parts that matter for the board).
    fn turn_start(&mut self, hit_any: bool) {
        let bd = &mut self.board;
        bd.catch_streak = if bd.catch_streak >= 0 { -bd.catch_streak } else { 0 };
        self.trip_now = false;
        self.hat_now = false;
        // FUN_00461560
        for p in 0..14 {
            if self.ctr[p] > 0 { self.ctr[p] -= 1; }
            let on = self.ctr[p] != 0;
            match p {
                2 => {
                    if on {
                        let n = self.board.rng_n; // FUN_004582f0 via the hooked call site 0x4613bb
                        self.board.rng_n += 1;
                        if !self.board.flippers_init { self.board.flippers_init = true; self.board.flipper_clear(n); }
                    }
                    self.board.set_flippers(on);
                }
                4 => self.board.set_pyramid(on),
                _ => {}
            }
        }
        self.board.spooky_left = self.ctr[6];
        self.board.set_lid(self.ctr[12] > 0 || self.ctr[6] > 0);
        self.board.amp_target = if self.ctr[2] != 0 { 0x82 } else if self.ctr[4] != 0 { 0xc8 } else { 0x104 };
        if self.trip_pend > 0 { self.trip_now = true; self.trip_pend -= 1; }
        if self.hat_pend > 0 { self.hat_now = true; self.hat_pend -= 1; }
        if hit_any {
            let mut r = self.next_rng();
            colors::purple_reroll(&mut self.board, &mut r, &mut self.purple);
        }
    }
}
