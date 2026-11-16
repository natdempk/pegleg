//! Brick geometry: FUN_00485670 (Brick::BuildVerts), with exact f32 store points.
//! Produces the closed vertex loop relative to the poly position (0x114/0x118), which is then fed
//! to the generic Poly line builder FUN_00484730.
use crate::level::BrickData;
use crate::phys::{cos_r, sin_r};
use crate::x87::{X, x};

const PI_D: f64 = 3.1415927410125732; // @5d26d8
const SNAP: f32 = 0.01; // @5d56ac
const MAX_R5: f32 = 600.0; // @5f2848

fn deg(v: f32) -> f32 { ((x(v) * X::d(PI_D)) / X::d(180.0)).r() }

/// (cos a · R, sin a · (−R)), each product stored.
fn pt(a: f32, r: f32) -> (f32, f32) {
    let nr = -r;
    ((x(cos_r(a)) * r).r(), (x(sin_r(a)) * nr).r())
}

fn half_sum(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    let sx = (x(a.0) + b.0).r();
    let sy = (x(b.1) + a.1).r();
    ((x(sx) * X::d(0.5)).r(), (X::d(0.5) * sy).r())
}

/// Snap B to A when nearly aligned (@485bd4 / @48612b).
fn snap(a: (f32, f32), b: &mut (f32, f32)) {
    let dx = (x(a.0) - b.0).r().abs();
    if !(SNAP <= dx) {
        b.0 = a.0;
    } else {
        let dy = (x(a.1) - b.1).r().abs();
        if !(SNAP <= dy) { b.1 = a.1; }
    }
}

pub fn brick_verts(b: &BrickData) -> Vec<(f32, f32)> {
    let t = b.brick_type as i32;
    let mut radius = b.radius;
    let mut arc_l = !(t == 1 || t == 2);
    let mut arc_r = !(t == 3 || t == 4);
    if t == 5 {
        if MAX_R5 < radius { radius = MAX_R5; }
        arc_l = false;
        arc_r = false;
    }
    let n = if (b.subdiv as i32) <= 0 { 0 } else { b.subdiv as i32 };
    let r1 = radius; // -0x14
    let r2 = (x(b.thickness) + radius).r(); // -0x1c
    let rot = b.rotation;
    let a20 = deg(rot);
    let half = x(b.angle) * X::d(0.5);
    let al = (x(rot) - half).r();
    let a8 = deg(al);
    let ar = (x(rot) + half).r();
    let ac = ((X::d(PI_D) * ar) / X::d(180.0)).r();
    let th = b.thickness;
    let mut v: Vec<(f32, f32)> = vec![];

    // ---- inner (left) side ----
    let (mid1, a_pt, b_pt);
    if arc_l {
        v.push(pt(ac, r1));
        let np1 = (n + 1) as f32;
        for j in 0..n {
            let f = (X::d((j + 1) as f64) / np1).r();
            let a = ((x(f) * a8) + ((X::d(1.0) - f) * ac)).r();
            v.push(pt(a, r1));
        }
        v.push(pt(a8, r1));
        mid1 = half_sum(v[((n + 2) / 2) as usize], v[((n + 1) / 2) as usize]);
        a_pt = (0.0, 0.0);
        b_pt = (0.0, 0.0);
    } else {
        let (a, mut bb);
        if t == 2 {
            let c20 = cos_r(a20);
            let s20n = -sin_r(a20);
            let dx = (x(c20) * th).r();
            let dy = (x(th) * s20n).r();
            let p = pt(a8, r2);
            a = ((x(p.0) - dx).r(), (x(p.1) - dy).r());
            let q = pt(ac, r2);
            bb = ((x(q.0) - dx).r(), (x(q.1) - dy).r());
        } else if t == 5 {
            let c = cos_r(a20);
            let c1 = (x(c) * r1).r();
            let s = sin_r(a20);
            let c2 = (x(-r1) * s).r();
            let h1 = ((x(s) * radius) * X::d(0.5)).r();
            let h2 = (X::d(0.5) * (x(c) * radius)).r();
            a = ((x(h1) + c1).r(), (x(h2) + c2).r());
            bb = ((x(c1) - h1).r(), (x(c2) - h2).r());
        } else {
            a = pt(a8, r1);
            bb = pt(ac, r1);
        }
        snap(a, &mut bb);
        v.push(bb);
        v.push(a);
        mid1 = half_sum(bb, a);
        a_pt = a;
        b_pt = bb;
    }

    // ---- outer (right) side ----
    let mid2;
    if arc_r {
        let base = v.len();
        v.push(pt(a8, r2));
        let np1 = (n + 1) as f32;
        for j in 0..n {
            let f = (X::d((j + 1) as f64) / np1).r();
            let a = ((x(f) * ac) + ((X::d(1.0) - f) * a8)).r();
            v.push(pt(a, r2));
        }
        v.push(pt(ac, r2));
        mid2 = half_sum(v[base + ((n + 2) / 2) as usize], v[base + ((n + 1) / 2) as usize]);
    } else {
        let (a, mut bb);
        if t == 4 {
            let c = cos_r(a20);
            let sn = -sin_r(a20);
            let dx = (x(c) * th).r();
            let dy = (x(th) * sn).r();
            let p = pt(a8, r1);
            a = ((x(p.0) + dx).r(), (x(p.1) + dy).r());
            let q = pt(ac, r1);
            bb = ((x(q.0) + dx).r(), (x(q.1) + dy).r());
        } else if t == 5 {
            let al5 = (x(deg(b.leftangle)) + a20).r();
            let ar5 = (x(a20) + ((X::d(PI_D) * b.rightangle) / X::d(180.0)).r()).r();
            let mth = -th;
            let dx = (x(cos_r(al5)) * th).r();
            let dy = (x(sin_r(al5)) * mth).r();
            a = ((x(dx) + a_pt.0).r(), (x(dy) + a_pt.1).r());
            let dx2 = (x(cos_r(ar5)) * th).r();
            let dy2 = (x(sin_r(ar5)) * mth).r();
            bb = ((x(dx2) + b_pt.0).r(), (x(dy2) + b_pt.1).r());
        } else {
            a = pt(a8, r2);
            bb = pt(ac, r2);
        }
        snap(a, &mut bb);
        v.push(a);
        v.push(bb);
        mid2 = half_sum(bb, a);
    }
    v.push(v[0]);

    // centre = midpoint of the two side midpoints, plus (f160, f164); verts made relative to it
    let sx = (x(mid2.0) + mid1.0).r();
    let sy = (x(mid2.1) + mid1.1).r();
    let cx = (x(b.f160) + (x(sx) * X::d(0.5)).r()).r();
    let cy = (x(b.f164) + (X::d(0.5) * sy).r()).r();
    for p in v.iter_mut() {
        p.0 = (x(p.0) - cx).r();
        p.1 = (x(p.1) - cy).r();
    }
    v
}
