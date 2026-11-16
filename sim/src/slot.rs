//! Lucky Spin slot machine (SlotMachineDialog). Physics is paused while it spins, so only the final
//! segment matters; it is a pure function of the two Rand() draws made when the dialog opens.
use crate::x87::{x, X};

const PI_D: f64 = 3.1415927410125732; // DAT_005d26d8
const TWO_PI_D: f64 = 6.2831854820251465; // DAT_005d8fe0
/// segment (+0x180) → result passed to FUN_0045e830: 0 random power, 1 magic hat, 2 score tripler, 3 extra ball
const RESULT: [u32; 4] = [2, 1, 0, 3];

fn deg(d: i32) -> f32 { ((X::d(d as f64) * X::d(PI_D)) / X::d(180.0)).r() }

/// FUN_00496730: segment and light index for an angle.
fn seg_light(angle: f32) -> (usize, i32) {
    let b = [deg(45), deg(135), deg(225), deg(315)];
    let mut s = 0;
    while s < 4 && !(angle < b[s]) { s += 1; }
    let o = deg(-5);
    let v = (((x(angle) - o) * X::d(20.0)) / X::d(PI_D)).0;
    let v = v.trunc() as i32;
    let light = if v < 0 { let m = (-v) % 40; if m == 0 { 40 } else { 40 - m } } else { v % 40 };
    (s & 3, light)
}

/// FUN_0049cd40 (open: angle = (r1 % 360)°, speed 0.15, countdown r2 % 100, 40 idle frames) then
/// FUN_0049b3d0 per frame until the speed reaches 0. Returns the wheel result.
pub fn lucky_wheel(r1: u32, r2: u32) -> u32 { lucky_wheel_steps(r1, r2).0 }

/// The wheel result and the number of wheel updates until it stops (idle frames included).
pub fn lucky_wheel_steps(r1: u32, r2: u32) -> (u32, i32) {
    let mut angle = ((X::d((r1 % 360) as f64) * X::d(PI_D)) / X::d(180.0)).r();
    let mut speed: f32 = 0.15;
    let mut cd = (r2 % 100) as i32;
    let mut delay = 40;
    let mut light = 0;
    let mut seg = 0;
    for n in 1..100_000 {
        let old = light;
        let idle = delay >= 1;
        if idle { delay -= 1; } else { angle = (x(angle) - speed).r(); }
        (seg, light) = seg_light(angle);
        if X::d(TWO_PI_D) < angle { angle = (x(angle) - X::d(TWO_PI_D)).r(); }
        else if angle < 0.0 { angle = (X::d(TWO_PI_D) + angle).r(); }
        if idle { continue; }
        if cd > 0 { cd -= 1; }
        if (light - 5) % 10 != 0 && cd == 0 {
            let dec: f32 = if (light + 4) % 10 == 0 && old != light { 0.0016 } else { 0.0008 };
            speed = (x(speed) - dec).r();
        }
        if speed <= 0.0 { return (RESULT[seg], n); }
    }
    (RESULT[seg], 100_000)
}
