//! The ball tube on the left of the board (InterfaceMgr+0x168: (pos, vel) per ball, index 0 at the
//! bottom). It matters for timing only: the next turn starts once the tube has settled (LogicMgr states
//! 6/7, FUN_00471600 / FUN_0044b690), and clicks are accepted 28 steps after the turn start's ball has
//! left the tube (LogicMgr+0x138: 1 at turn start, 2 when the ball is out, counted to 30 in state 1, then 0).
//! Updated once per game step after LogicMgr::Update (FUN_00402ad0 → FUN_004518b0 → FUN_00443cf0).

/// ball spacing (FUN_00436720: ModVal 0 + tube ball image height)
pub const H: i32 = 20;
const GRAV: f32 = 0.15; // InterfaceMgr.cpp1047
const BOUNCE: f32 = -0.35; // 1048
const PUSH: f32 = 0.75; // 1049 (× 0.5)
/// a ball leaves the tube when it rises above this
const OUT_Y: f32 = 130.0;

#[derive(Clone, Debug, Default)]
pub struct Tube {
    pub v: Vec<(f32, f32)>,
    pub t58: i32,
    pub t5c: i32,
    pub e8: i32,
    pub ec: i32,
    pub settled: bool,
}

impl Tube {
    /// FUN_004366a0: the bottom ball's rest position
    pub fn floor(&self) -> i32 {
        let mut f = 410 + self.t5c + 3 * self.ec;
        if self.t58 != 0 {
            if self.t58 < 21 { return f + self.t58 * 2; }
            if self.t58 < 30 { f += (30 - self.t58) * 2; }
        }
        f
    }

    /// FUN_00457870(n): n balls at rest
    pub fn reset(n: i32) -> Tube {
        let n = n.max(0);
        let k = n.min(15);
        let mut t = Tube { v: vec![(0.0, 0.0); n as usize], t58: 0, t5c: 0, e8: k, ec: k, settled: true };
        let mut p = t.floor();
        for b in t.v.iter_mut() { *b = (p as f32, 0.0); p -= H; }
        t
    }

    /// FUN_0045c860(k): k free balls dropped in from the top
    pub fn add_top(&mut self, k: i32) {
        self.settled = false;
        let mut p = 100;
        for _ in 0..k { self.v.push((p as f32, 0.0)); p -= H; }
    }

    /// FUN_0043c9b0 (turn start): the top ball starts moving out; returns true when the tube was empty
    /// (LogicMgr+0x138 = 2 right away)
    pub fn kick(&mut self) -> bool {
        if self.v.is_empty() { return true; }
        self.settled = false;
        self.t58 = 1;
        false
    }

    /// FUN_00443cf0, one game step; returns true when the moving-out ball left (LogicMgr+0x138 = 2)
    pub fn step(&mut self) -> bool {
        if self.v.is_empty() || self.settled {
            if self.t58 != 0 { self.t58 = 0; return true; }
            return false;
        }
        if self.t5c != 0 { self.t5c = (self.t5c - 3).max(0); }
        let n = self.v.len() as i32;
        let cnt = n.min(25);
        if self.t58 != 0 {
            self.t58 += 1;
            if self.t58 == 20 {
                for i in 0..cnt as usize { self.v[i].1 = if i as i32 == cnt - 1 { -15.0 } else { -1.5 }; }
            }
        }
        let floor = self.floor() as f32;
        let mut l14 = n.min(15);
        if self.t58 > 0x13 && l14 > 0 { l14 -= 1; }
        if l14 == 0 { self.e8 = 0; }
        let mut moving = false;
        let p0 = self.v[0].0 + self.v[0].1;
        self.v[0].0 = p0;
        if p0 <= floor {
            if p0 < floor { moving = true; self.v[0].1 += GRAV; }
        } else {
            self.v[0].0 = floor;
            self.v[0].1 = if self.v[0].1.abs() >= 1.0 { BOUNCE * self.v[0].1 } else { 0.0 };
            if self.e8 < 1 { self.e8 = 1; }
        }
        for i in 1..cnt as usize {
            let (pp, pv) = self.v[i - 1];
            let (cp, cv) = self.v[i];
            let gap = (pp - (cp + cv)) - H as f32;
            if gap >= 0.0 {
                if !moving && gap > 5.0 { moving = true; }
                let nv = GRAV + cv;
                self.v[i] = (nv + cp, nv);
            } else {
                self.v[i].0 = pp - H as f32;
                let mut f = (PUSH as f64 * 0.5 * (cv.abs() + pv.abs()) as f64) as f32;
                if f < 1.0 { f = 0.0; }
                if self.v[i].1 > 0.0 { self.v[i].1 = 0.0; }
                if self.v[i - 1].1 < 0.0 { self.v[i - 1].1 = 0.0; }
                self.v[i].1 -= f;
                self.v[i - 1].1 += f;
                if !moving && self.e8 < i as i32 + 1 { self.e8 = i as i32 + 1; }
            }
        }
        let e8 = self.e8;
        if l14 < e8 || (e8 > 0 && l14 - e8 > 3) { self.e8 = l14; }
        let mut out = false;
        if self.t58 >= 21 && (self.v[n as usize - 1].0 < OUT_Y || self.t58 >= 61) {
            self.t58 = 0;
            self.v.pop();
            if self.v.len() >= 25 { self.v[24] = (OUT_Y, 0.0); }
            out = true;
        }
        if self.t5c == 0 && self.t58 == 0 {
            self.settled = self.v.iter().take(25).all(|b| b.1 == 0.0);
        }
        if self.ec == self.e8 { return out; }
        if self.ec < self.e8 {
            self.ec += 1;
            if self.ec == self.e8 && l14 < self.ec { self.e8 = l14; }
        } else {
            self.ec -= 1;
        }
        self.settled = false;
        out
    }
}
