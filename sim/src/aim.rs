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
//! FUN_00407840/60). The window is 800×600; the board sits at `(75, 40)` (650 wide, below the 40 px top
//! HUD bar, verified live from `Board+0x1a8/+0x1ac` via `WidgetManager::MouseMove`, pivot 325 = window x 400,
//! pivot y 30 = window y 70).
//!
//! Mouse-wheel fine aim (`wheel_aim`, FUN_0043d6a0) and arrow-key aiming (`arrow_key_aim`,
//! FUN_00402930 / FUN_00410e10) are modelled alongside `mouse_aim`.
use crate::phys::{cos_r, sin_r, Gun};
use crate::x87::{x, X};

pub const BOARD_OFFSET: [i32; 2] = [75, 40];
pub const WINDOW: [i32; 2] = [800, 600];

const TWO_PI_D: f64 = 6.2831854820251465; // 5d8fe0 (f64 of f32 2π)
const PI_D: f64 = 3.1415927410125732; // 5d26d8
const DEG_TO_RAD_D: f64 = 0.01745329238474369; // 5dfa60 (f32 pi/180)

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

/// FUN_0043d6a0: mouse-wheel fine aim. `delta` is the notch count (±1 per wheel detent, or ±5 per
/// held arrow-key tick in FUN_00410e10). `prev_step` is LogicMgr+0x70 (degrees, starts at 0.1).
/// When the aim guide is active (`guide_on`), `guide_dist` (Gun+0x17c) <= 1.0 and `|delta| < 2`,
/// the step scales by `0.02 / guide_dist` clamped to `[0.01, 0.1]`. Returns the new clamped aim.
pub fn wheel_aim(angle: f32, delta: i32, prev_step: &mut f32, guide_on: bool, guide_dist: f32) -> f32 {
    let mut step = 0.10000000149011612f32; // FUN_00436f10 ModVal LogicMgr.cpp1053 (_DAT_005d26bc)
    if guide_on && guide_dist <= 1.0 && delta.abs() < 2 {
        let ratio = (x(0.019999999552965164f32) / guide_dist).r(); // _DAT_005d5764
        step = (x(*prev_step) * ratio).r().clamp(0.009999999776482582f32, 0.10000000149011612f32);
    }
    *prev_step = step;
    let rad = ((x(step) * X::d(DEG_TO_RAD_D)) * (delta as f32)).r();
    clamp_aim((x(rad) + angle).r())
}

/// Arrow-key aiming: `dir` = -1 (VK_LEFT 0x25) or +1 (VK_RIGHT 0x27).
/// Single press (FUN_00402930) passes ±1; held tick (FUN_00410e10, Board.cpp107/108) passes ±5.
pub fn arrow_key_aim(angle: f32, dir: i32, held: bool, prev_step: &mut f32, guide_on: bool, guide_dist: f32) -> f32 {
    let delta = dir.signum() * if held { 5 } else { 1 };
    wheel_aim(angle, delta, prev_step, guide_on, guide_dist)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Both host libm and MSVC SSE2 `__cos_pentium4`/`__sin_pentium4` (0x5a99e0) are accurate to
    /// < 0.6 f64-ULP. If no reachable mouse angle lands within 2 f64-ULP of an f32 rounding midpoint
    /// (low 29 bits == 0x10000000), `cos_r`/`sin_r` are guaranteed bit-identical to the CRT across
    /// all ~430k human-reachable angles.
    #[test]
    fn crt_trig_safe_across_all_mouse_angles() {
        let angles = mouse_angles(&Gun::default());
        assert!(angles.len() > 400_000);
        let mut min_dist = u64::MAX;
        for &(a, _) in &angles {
            for v in [(a as f64).cos(), (a as f64).sin()] {
                let low = v.to_bits() & 0x1fff_ffff;
                let dist = low.abs_diff(0x1000_0000);
                if dist < min_dist { min_dist = dist; }
            }
        }
        assert!(min_dist > 2, "found reachable angle within {min_dist} f64-ULP of f32 midpoint");
    }

    #[test]
    fn wheel_arrow_and_gun_slew_work() {
        // Live wheelshot/keyshot angles verified in peggle-wine (audit_input_wheel_key):
        let g = Gun::default();
        let mut a = mouse_aim(&g, 250 - BOARD_OFFSET[0], 400 - BOARD_OFFSET[1]);
        let mut step = 0.1f32;
        for _ in 0..5 { a = wheel_aim(a, 1, &mut step, false, 1.0); }
        assert_eq!(a.to_bits(), 0x4086739a);
        let mut a = mouse_aim(&g, 620 - BOARD_OFFSET[0], 320 - BOARD_OFFSET[1]);
        step = 0.1;
        for _ in 0..8 { a = arrow_key_aim(a, -1, false, &mut step, false, 1.0); }
        assert_eq!(a.to_bits(), 0x40b20d4c);
        let mut a = mouse_aim(&g, 401 - BOARD_OFFSET[0], 250 - BOARD_OFFSET[1]);
        step = 0.1;
        for _ in 0..12 { a = arrow_key_aim(a, 1, false, &mut step, false, 1.0); }
        assert_eq!(a.to_bits(), 0x4097f8cd);
        // Live Gun::Update (FUN_00484120) slew bits from 3.5 (0x40600000) to 4.0 (0x40800000):
        let mut cur = f32::from_bits(0x40600000);
        let mut target = f32::from_bits(0x40800000);
        for &expect in &[0x406ccccdu32, 0x4079999a, 0x40800000] {
            Gun::update_slew(&mut cur, &mut target);
            assert_eq!(cur.to_bits(), expect);
        }
        assert_eq!(Gun::fire_angle(4.0, 3.8, 5, true), 3.8);
        assert_eq!(Gun::fire_angle(4.0, 3.8, 10, true), 4.0);
    }

    /// 25 window pixels fed live into the real game's `WidgetManager::MouseMove` (FUN_00543270) →
    /// `Board::MouseMove` → `FUN_0043ece0(0)` → `LogicMgr+0xe0` (`peggle-wine`, Level 1-1).
    /// Covers near-barrel (< thr), vertical (|cos| < 0.001), upward-cone clamp (7°..173°), corners,
    /// board edges, and mid-board parabola fits across all quadrants.
    #[test]
    fn mouse_aim_matches_real_game_live_pixels() {
        const LIVE: &[(i32, i32, u32)] = &[
            (400, 35, 0x40413e2c),
            (405, 40, 0x3dfa35dd),
            (395, 40, 0x40413e2c),
            (400, 300, 0x40971dd0),
            (400, 599, 0x40971dd0),
            (0, 0, 0x40413e2c),
            (799, 0, 0x3dfa35dd),
            (0, 599, 0x40704e41),
            (799, 599, 0x40b55ba8),
            (75, 30, 0x40413e2c),
            (725, 30, 0x3dfa35dd),
            (120, 200, 0x4058ac2b),
            (250, 400, 0x40862c1e),
            (310, 150, 0x4077d423),
            (490, 150, 0x40b2518e),
            (550, 400, 0x40a76bab),
            (680, 200, 0x40c141b3),
            (180, 520, 0x40837fd4),
            (620, 520, 0x40aa69e0),
            (401, 250, 0x40974d3d),
            (399, 250, 0x40969c77),
            (80, 590, 0x407b18a9),
            (720, 590, 0x40b00b74),
            (350, 90, 0x4061208e),
            (450, 90, 0x40bd0782),
        ];
        let g = Gun::default();
        for &(wx, wy, expected) in LIVE {
            let got = mouse_aim(&g, wx - BOARD_OFFSET[0], wy - BOARD_OFFSET[1]);
            assert_eq!(
                got.to_bits(), expected,
                "pixel ({wx}, {wy}): got {:.9} (0x{:08x}) != real {:.9} (0x{expected:08x})",
                got, got.to_bits(), f32::from_bits(expected)
            );
        }
    }
}
