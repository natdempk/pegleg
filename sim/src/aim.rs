//! Human-reachable aim angles.
//!
//! Mouse aim = LogicMgr FUN_0043ece0 (x87 rounding at each FSTP): the angle from the
//! gun pivot to the cursor, bent by c = min(1, 1.3e-6 · d² · |atan2(dx, dy)|) (sign of dx), and — when
//! the cursor is farther than the barrel (g170 + g16c + 6 + 10) — the best of 20 angles
//! a + i·0.01 (i = -10..9) whose ballistic parabola from the muzzle passes closest (vertically) to the
//! cursor. The result goes through FUN_00436fd0 (wrap to [0, 2π), keep out of the upward cone
//! 7°..173°) into LogicMgr+0xe0, which the gun slews to and fires.
//!
//! The cursor position is in board coordinates: window pixel − board widget offset (Board+0x1a8/+0x1ac,
//! FUN_00407840/60). The window is 800×600; the board is 650 wide and centred, so the offset is
//! (75, 0) (pivot 325 = window x 400).
//!
//! Not modelled yet: mouse-wheel fine aim (FUN_0043d6a0: ±0.1° per notch, accelerated).
use crate::phys::{cos_r, sin_r, Gun};
use crate::x87::{x, X};

pub const BOARD_OFFSET: [i32; 2] = [75, 0];
pub const WINDOW: [i32; 2] = [800, 600];

const TWO_PI_D: f64 = 6.2831854820251465; // 5d8fe0 (f64 of f32 2π)
const PI_D: f64 = 3.1415927410125732; // 5d26d8

fn atan2_r(y: f32, xv: f32) -> f32 { (y as f64).atan2(xv as f64) as f32 }

/// FUN_004bc3b0: wrap to [0, 2π).
pub fn norm(a: f32) -> f32 {
    let ad = a as f64;
    if ad >= 0.0 && ad < TWO_PI_D { return a; }
    if ad < 0.0 {
        let f = ((-ad) % TWO_PI_D) as f32;
        return (X::d(TWO_PI_D) - f).r();
    }
    (ad % TWO_PI_D) as f32
}

/// FUN_00436fd0: wrap, then push out of the upward cone (ModVal ThunderCommon.cpp123 = 83°).
pub fn clamp_aim(a: f32) -> f32 {
    let n = norm(a);
    let l = 83.0f32;
    let m = (X::d(90.0) - l).r();
    let lo = ((x(m) * X::d(PI_D)) / X::d(180.0)).r();
    let p = (X::d(90.0) + l).r();
    let hi = ((X::d(PI_D) * p) / X::d(180.0)).r();
    if lo < n && n < hi {
        let mid = ((x(hi) + lo) * X::d(0.5)).r();
        if n <= mid { lo } else { hi }
    } else { n }
}

/// FUN_0043de50
fn dist(x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let dx = (x(x2) - x1).r();
    let dy = (x(y2) - y1).r();
    let s = (x((x(dx) * dx).r()) + (x(dy) * dy).r()).r();
    s.sqrt()
}

/// FUN_0043ece0 for a cursor at board coordinates (mx, my) (integers).
pub fn mouse_aim(gun: &Gun, mx: i32, my: i32) -> f32 {
    let (px, py) = (gun.pos[0], gun.pos[1]);
    let mxf = mx as f32;
    let myf = my as f32;
    let dxm = (x(mxf) - px).r();
    let t1 = (x(py) - myf).r();
    let a0 = atan2_r(t1, dxm);
    let dym = (x(myf) - py).r();
    let ab = atan2_r(dxm, dym).abs();
    let k = 1.3e-6f32;
    let r2 = ((dym as f64) * (dym as f64) + (dxm as f64) * (dxm as f64)) as f32;
    let mut c = ((x(k) * r2) * ab).r();
    if 1.0f32 < c { c = 1.0; }
    if dxm < 0.0 { c = (x(c) * X::d(-1.0)).r(); }
    let a = (x(c) + a0).r();
    let mut best_a = a;
    let mut best_err = 1_000_000.0f32;
    let pp = ((((x(gun.off170) + px) + gun.off16c) + 6.0f32)).r();
    let qq = (x(py) + X::d(1.0)).r();
    let d = dist(pp, qq, mxf, myf);
    let thr = ((x(gun.off170) + gun.off16c) + 6.0f32) + X::d(10.0);
    if thr < d {
        for i in -10..10 {
            let ang = ((X::d(i as f64) * X::d(0.009999999776482582)) + a).r();
            let r = crate::phys::rotate_about([pp, qq], gun.pos, ang);
            let ex = (x(mxf) - r[0]).r();
            let ey = (x(myf) - r[1]).r();
            let s = gun.speed;
            let co = cos_r(ang);
            let err = if co.abs() >= 0.001f32 {
                let t = (x(ex) / (x(s) * co)).r();
                if t < 0.0 { 1_000_000.0f32 } else {
                    let t2 = (x(t) * t).r();
                    let hd = ((x(0.05f32) * X::d(0.5)) * t2).0; // FSTP double
                    let si = sin_r(ang);
                    let u = (X::d(hd) - ((x(t) * s) * si)).r();
                    (x(u) - ey).r().abs()
                }
            } else if ex != 0.0 { 1_000_000.0 } else { 0.0 };
            if best_err > err { best_a = ang; best_err = err; }
        }
    }
    clamp_aim(best_a)
}

/// Every distinct angle a cursor inside the 800×600 window can produce, sorted ascending, with one
/// window pixel that produces it.
pub fn mouse_angles(gun: &Gun) -> Vec<(f32, [i32; 2])> {
    let mut m: std::collections::HashMap<u32, [i32; 2]> = std::collections::HashMap::new();
    for wy in 0..WINDOW[1] {
        for wx in 0..WINDOW[0] {
            let a = mouse_aim(gun, wx - BOARD_OFFSET[0], wy - BOARD_OFFSET[1]);
            m.entry(a.to_bits()).or_insert([wx, wy]);
        }
    }
    let mut v: Vec<(f32, [i32; 2])> = m.into_iter().map(|(b, p)| (f32::from_bits(b), p)).collect();
    v.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    v
}
