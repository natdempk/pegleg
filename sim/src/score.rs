//! Peg scoring (LogicMgr::BallHitPeg FUN_0046f480, first hit of each peg).
//!
//! Peg types (PegInfo+0x10): 1 blue, 2 orange, 3 purple, 4 green.
//! Per first hit: m = multiplier(oranges remaining *before* this hit) (FUN_004370b0, stored to
//! LogicMgr+0x114); base = 10/100/500/10 (purple capped to 100 when m >= 100); v = m*base;
//! sum (+0x10c) += v; shot score (+0x108) += n*v + sum where n (+0x12c) = pegs already hit this
//! shot; then n += 1 and an orange decrements the remaining count (+0x360). The shot score is added
//! to the total (+0x174) at shot end.
//!
//! Free balls (FUN_0046e740): every score add that takes the shot score across 25000 / 75000 /
//! 125000 earns a ball, unless no oranges are left (FUN_0043d580, +0x360 == 0 — the orange count is
//! decremented in FUN_00448e60 *before* the peg's score is added).
//! Score Tripler: the meter counts the plain shot score in flight; the ×3 comes in the tally after the
//! shot, whose add crosses the remaining thresholds only while oranges remain, so a winning tripled
//! shot gets no tally balls (13/13 such real level totals; non-final shots are the same either way).
//! Level end (Extreme Fever): the fever hole adds its value (Hole+0xf0) via FUN_0046e740, then
//! FUN_0046ca60/FUN_0046c400 drain the Ball-o-Tron adding 10000 per ball left (5000 if +0x124).
//! Style-shot bonuses are tracked in `Board::styles` (`phys.rs`) and added to `ShotResult::score`.

pub const BLUE: u8 = 1;
pub const ORANGE: u8 = 2;
pub const PURPLE: u8 = 3;
pub const GREEN: u8 = 4;

/// FUN_004370b0 (`fever_off` = LogicMgr+0xfb set: no 100x at 0 remaining).
pub fn multiplier(remaining: i32, fever_off: bool) -> i32 {
    if remaining < 1 && !fever_off { 100 }
    else if remaining < 4 { 10 }
    else if remaining < 7 { 5 }
    else if remaining < 11 { 3 }
    else if remaining < 16 { 2 }
    else { 1 }
}

pub fn base_value(peg_type: u8) -> i32 {
    match peg_type { ORANGE => 100, PURPLE => 500, _ => 10 }
}

#[derive(Clone, Debug, Default)]
pub struct ShotScore {
    pub score: i64,   // +0x108
    pub sum: i64,     // +0x10c
    pub pegs: i32,    // +0x12c
    pub oranges: i32, // +0x4c oranges hit this shot
    pub free_balls: i32, // balls earned from the 25k/75k/125k thresholds this shot
    /// free-ball meter: the shot score as the game accumulates it
    pub meter: i64,
}

pub const FREE_BALL_THRESHOLDS: [i64; 3] = [25000, 75000, 125000];

impl ShotScore {
    /// Apply one first-hit; `remaining` = oranges left (+0x360), updated in place.
    pub fn hit(&mut self, peg_type: u8, remaining: &mut i32) {
        let m = multiplier(*remaining, false) as i64;
        let mut base = base_value(peg_type) as i64;
        if m >= 100 && base >= 500 { base = 100; }
        let v = m * base;
        self.sum += v;
        self.pegs += 1;
        if peg_type == ORANGE {
            self.oranges += 1;
            *remaining -= 1;
        }
        self.add((self.pegs - 1) as i64 * v + self.sum, *remaining);
    }

    /// FUN_0046e740: add to the shot score; free ball per threshold crossed while oranges remain.
    pub fn add(&mut self, v: i64, remaining: i32) {
        self.score += v;
        let old = self.meter;
        self.meter += v;
        if remaining != 0 {
            self.free_balls += FREE_BALL_THRESHOLDS.iter().filter(|&&t| old < t && t <= self.meter).count() as i32;
        }
    }

    /// Score Tripler tally at shot end: the meter goes to 3× the shot score; returns the balls it earns
    /// (none once every orange is hit).
    pub fn tally_triple(&mut self, remaining: i32) -> i32 {
        let (old, new) = (self.meter, self.meter * 3);
        self.meter = new;
        if remaining == 0 { return 0; }
        let k = FREE_BALL_THRESHOLDS.iter().filter(|&&t| old < t && t <= new).count() as i32;
        self.free_balls += k;
        k
    }

    /// Extreme Fever end: the ball fell in a fever hole worth `hole_value`; `balls_at_fire` is
    /// LogicMgr+0x17c[player] at fire (includes the ball in play).
    pub fn fever_end(&mut self, hole_value: i32, balls_at_fire: i32) {
        self.add(hole_value as i64, 0);
        self.score += 10000 * (balls_at_fire - 1 + self.free_balls).max(0) as i64;
    }
}
