//! Movers (FUN_00478ea0 / phase FUN_004764f0), with exact f32 store points.
use crate::level::Mover as MoverDef;
use crate::phys::{cos_r, rotate_about, sin_r};
use crate::x87::{X, x};

const TWO_PI: f64 = 6.2831854820251465; // @5d8fe0 (f32 2π as double)
const HALF_PI: f64 = 1.5707963705062866; // @5d4f70
const PI_F: f32 = 3.1415927; // @5f0e64

#[derive(Clone, Debug)]
pub struct Mover {
    pub def: MoverDef,
    pub pos: [f32; 2],    // 0x54/0x58 output position
    pub center: [f32; 2], // 0x5c/0x60
    pub angle: f32,       // 0x64 output angle
    /// 0x48 sub-mover (index into Board::movers) and 0x4c/0x50 its output position last seen
    pub sub: Option<(usize, [f32; 2])>,
    /// bumped whenever `def` or `center` is changed after load (fever, bucket easing, oracle state):
    /// a `MoverTable` is valid only for the generation it was built from
    pub version: u32,
}

/// _ftol2: truncate toward zero.
fn ftol(v: X) -> i32 { v.0 as i32 }

impl Mover {
    pub fn new(def: &MoverDef) -> Mover {
        Mover { def: def.clone(), pos: [def.px, def.py], center: [def.cx, def.cy], angle: def.f64, sub: None, version: 0 }
    }

    /// FUN_00476460: ticks from object counter `frame` until the mover reaches `phase` (0 if it is
    /// exactly there; FUN_00476400 positive modulo).
    pub fn ticks_to_phase(&self, frame: i32, phase: f32) -> i32 {
        let m = &self.def;
        let period = m.time as i32;
        let p2c = m.u2c as i32;
        let (mut t, modulus);
        if p2c > 0 {
            let total = period + m.u34 as i32 + p2c;
            if period < 1 || total < 1 { return 0; }
            t = ftol(X::d(period as f64) * phase);
            if (m.b30 as f64) < phase as f64 { t += p2c; }
            if (m.b38 as f64) < phase as f64 { t += m.u34 as i32; }
            modulus = total;
        } else {
            if period < 1 { return 0; }
            t = ftol(X::d(period as f64) * phase);
            modulus = period;
        }
        let v = t - m.s14 - frame;
        if v >= 0 { v % modulus } else { modulus - (-v) % modulus }
    }

    /// FUN_004764f0: phase in [0,1) for object frame counter `frame`.
    pub fn phase(&self, frame: i32) -> f32 {
        let m = &self.def;
        let period = m.time as i32;
        let p2c = m.u2c as i32;
        let mut t = frame + m.s14;
        if p2c > 0 {
            let p34 = m.u34 as i32;
            let total = p34 + period + p2c;
            if total > 0 {
                let p1 = (period * m.b30) / 100;
                let ecx = p2c + p1;
                let ebx = ((m.b38 - m.b30) * period) / 100 + ecx;
                let v = ftol(x(m.f40) * X::d(total as f64)) + t;
                let rem = v % total;
                t = if rem <= p1 {
                    rem
                } else if rem <= p2c + p1 {
                    p1
                } else if rem <= ebx {
                    rem - p2c
                } else if rem <= p34 + ebx {
                    ebx - p2c
                } else {
                    rem - p34 - p2c
                };
            }
        }
        let v = ftol((x(m.f20) * X::d(period as f64)) + X::d(t as f64));
        let rem = v % period;
        (X::d(rem as f64) / X::d(period as f64)).r()
    }

    /// FUN_00478ea0 head: with a sub-mover, shift pos and centre by the sub-mover's output motion
    /// since the last eval (FUN_00476420). `sub_pos` = the sub-mover's current 0x54/0x58.
    pub fn follow_sub(&mut self, sub_pos: [f32; 2]) {
        let Some((_, last)) = self.sub.as_mut() else { return };
        let d = [(x(sub_pos[0]) - last[0]).r(), (x(sub_pos[1]) - last[1]).r()];
        self.pos = [(x(d[0]) + self.pos[0]).r(), (x(d[1]) + self.pos[1]).r()];
        self.center = [(x(self.center[0]) + d[0]).r(), (x(d[1]) + self.center[1]).r()];
        *last = sub_pos;
    }

    /// FUN_00478ea0(frame, 0) after the sub-mover step (see `follow_sub`).
    pub fn eval(&mut self, frame: i32) {
        let m = self.def.clone();
        let (cx, cy) = (self.center[0], self.center[1]);
        let period = m.time as i32;
        if m.mover_type == 0 || period <= 0 {
            self.pos = self.center;
            self.angle = m.f44;
            return;
        }
        let typ = m.mover_type.abs();
        let dir: i32 = if m.mover_type >= 0 { 1 } else { -1 };
        let ph = self.phase(frame);
        let t = (x(ph) + (X::d(0.0) / X::d(period as f64))).r();
        let ampx = m.s18 as f32;
        let ampy = if m.s28 != 0 { m.s28 as f32 } else { ampx };
        let dirx = X::d(dir as f64);
        let a = || ((x(t) * X::d(TWO_PI)) * dirx).r();
        let k_or_pi = |k: f32| if k == 0.0 { PI_F } else { k };
        match typ {
            1 => {
                let a = a();
                self.pos[0] = cx;
                self.pos[1] = (X::d(cy as f64) - (x(sin_r(a)) * ampy)).r();
                self.angle = m.f44;
            }
            2 => {
                let a = a();
                self.pos[0] = ((x(cos_r(a)) * ampx) + cx).r();
                self.pos[1] = cy;
                self.angle = m.f44;
            }
            3 => {
                let a = a();
                self.pos[0] = ((x(cos_r(a)) * ampx) + cx).r();
                self.pos[1] = (X::d(cy as f64) - (x(sin_r(a)) * ampy)).r();
                self.angle = m.f44;
            }
            4 => {
                let a = a();
                self.pos[0] = ((x(cos_r(a)) * ampx) + cx).r();
                let a2 = (x(a) + a).r();
                self.pos[1] = (X::d(cy as f64) - ((x(sin_r(a2)) * ampy) * X::d(0.5))).r();
                self.angle = m.f44;
            }
            5 => {
                let a = a();
                let a2 = (x(a) + a).r();
                self.pos[0] = ((x(sin_r(a2)) * ampx) + cx).r();
                self.pos[1] = (X::d(cy as f64) - (x(sin_r(a)) * ampy)).r();
                self.angle = m.f44;
            }
            6 | 7 => {
                let k = k_or_pi(m.f3c);
                let b = ((x(t) * X::d(TWO_PI)) - X::d(HALF_PI)).r();
                let s = sin_r(b);
                let c2 = (((x(s) + X::d(1.0)) * k) * X::d(0.5)).r();
                if typ == 6 {
                    self.pos[0] = ((x(cos_r(c2)) * ampx) + cx).r();
                    self.pos[1] = (X::d(cy as f64) - (x(sin_r(c2)) * ampy)).r();
                } else {
                    self.pos[0] = ((x(sin_r(c2)) * ampx) + cx).r();
                    self.pos[1] = (X::d(cy as f64) - (x(cos_r(c2)) * ampy)).r();
                }
                self.angle = m.f44;
            }
            8 => {
                let v = if m.f3c == 0.0 { (x(t) * X::d(TWO_PI)) * dirx } else { dirx * m.f3c };
                self.angle = (v + m.f44).r();
                self.pos = self.center;
            }
            9 => {
                let k = k_or_pi(m.f3c);
                let b = (x(t) * X::d(TWO_PI)).r();
                let s = sin_r(b);
                let v = (((x(s) * k) + m.f24) * dirx).r();
                self.angle = (x(v) + m.f44).r();
                self.pos = self.center;
            }
            11 => {
                self.pos[0] = cx;
                let frac = (x(t) - X::d(ftol(x(t)) as f64)).r();
                self.pos[1] = (X::d(cy as f64) - ((x(frac) * ampy) * dirx)).r();
                self.angle = m.f44;
            }
            12 => {
                let frac = (x(t) - X::d(ftol(x(t)) as f64)).r();
                self.pos[0] = (((x(ampx) * frac) * dirx) + cx).r();
                self.pos[1] = cy;
                self.angle = m.f44;
            }
            13 => {
                let a = a();
                self.pos[0] = ((x(cos_r(a)) * ampx) + cx).r();
                self.pos[1] = (X::d(cy as f64) - (x(sin_r(a)) * ampy)).r();
                if m.s28 != 0 && m.s18 != 0 {
                    let a3 = (x(a) + X::d(HALF_PI)).r();
                    let py3 = (X::d(cy as f64) - (x(sin_r(a3)) * ampy)).r();
                    let dy3 = (X::d(cy as f64) - py3).r();
                    let px3 = ((x(cos_r(a3)) * ampx) + cx).r();
                    let dx3 = (x(px3) - cx).r();
                    // 0x47939a: fld dy3; fld dx3; call _CIatan2 (0x59e56a → the CRT's _cintrindisp2: first C argument in
                    // ST1, second in ST0) = atan2(dy3, dx3)
                    let at = ((dy3 as f64).atan2(dx3 as f64)) as f32;
                    self.angle = (x(at) + m.f44).r();
                } else {
                    self.angle = (x(m.f44) + a).r();
                }
            }
            _ => {} // 10 and unknown: unchanged
        }
        if m.f24 != 0.0 {
            self.pos = rotate_about(self.pos, self.center, m.f24);
        }
    }
}
