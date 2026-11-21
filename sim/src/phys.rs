//! Ball physics, matching the game's x87 arithmetic with exact f32 store points (see x87.rs).
//! Function names (FUN_xxxxxxxx) are addresses in the game executable. Field comments give Ball offsets.

use crate::level::{Level, Shape};
use crate::mover::Mover;
use crate::x87::{X, add, sqrt_r, sub, x};

// constants (addresses in .rdata)
const GRAVITY: f32 = 0.05; // @649d80
const MAX_SPEED: f32 = 15.0; // ModVal Ball.cpp#6, @5d9d70
const EPS: f64 = 0.009999999776482582; // @5d5be8 (double holding f32 0.01)
const PEG_BOUNCY_DEFAULT: f32 = 0.9; // @5de4d0 (0.89999998)
const PEG_ROLLY_DEFAULT: f32 = 1.0; // ModVal default (FLD1)
const LINE_BOUNCY_DEFAULT: f32 = 0.82; // ModVal Ball.cpp#2 @5f0c7c
const LINE_ROLLY_DEFAULT: f32 = 1.4; // ModVal Ball.cpp#3 @5f0c40
const BIG1: f64 = 999999995905.0; // @5f0bc0
const BIG2: f64 = 999999995904.0; // @5f0bb8

/// Magic Hat: a separate 20-radius ball (+0x18c) with the fireball flag, never collided with by
/// others; it follows its owner via the anchor and (from its 11th update, +0x4c >= 11) touches un-hit
/// PegInfo objects it overlaps (BallHitPeg credited to the owner, FUN_0046ecb0).
#[derive(Clone, Debug, Default)]
pub struct Hat { pub anchor: [f32; 2], pub pos: [f32; 2], pub ctr: i32 }

#[derive(Clone, Debug, Default)]
pub struct Ball {
    /// Ball+0xe8: ball slot (lowest unused in {0, 1} when added to board+0x1a0; FUN_00421070).
    /// Only slot 0 logs Long Shot contacts, increments catch streak, and scores quick-catch styles.
    pub slot: u8,
    /// Per-AIBall hit set (used only when `Board::ai` is true, mirroring FUN_004428e0).
    pub ai_hits: Vec<usize>,
    /// Magic Hat (Lucky Spin result 1; Ball+0x18d/+0x188): see `Hat`
    pub hat: Option<Hat>,
    /// Ball+0x178: LogicMgr tick of the first touch of a "freeball*" (bucket) object (0 = none)
    pub bucket_touch: i32,
    /// Ball+0x17c: later bucket touches more than 9 ticks after the first
    pub bucket_touches: i32,
    /// Ball+0x180: lowest ftol(y) since the first bucket touch (0 here = the initial 10000)
    pub bucket_miny: i32,
    /// Ball+0x162: touched a "bumperhole" (pyramid) object; a later bucket catch adds style 16
    pub bumperhole_touch: bool,
    /// Ball+0x150: brick-combo timer (20 after quick fast brick contacts; Ball::Update counts down)
    pub combo_timer: i32,
    /// Ball+0x154: first hits while the combo timer runs (12 → style 2); cleared when it expires
    pub combo_count: i32,
    /// Ball+0x158: tick of the last qualifying brick contact (None = never)
    pub last_brick: Option<i32>,
    pub pos: [f32; 2],     // 0xec
    pub prev: [f32; 2],    // 0xf4
    pub vel: [f32; 2],     // 0xfc
    pub stuck_samples: [[f32; 2]; 6], // 0x10c
    pub pos134: [f32; 2],  // 0x134 (pos at last hit / end of Update)
    pub r: f32,            // 0x13c
    pub bbox: [f32; 4],    // 0x14 minx, 0x18 miny, 0x1c maxx, 0x20 maxy
    pub stuck: i32,        // 0xe4
    pub stuck_flag: bool,  // 0x164
    pub frames: i32,       // 0x148 (frames since last 'roll'; reset by soft hits)
    pub moved: bool,       // 0x141
    pub hit_this_frame: bool, // 0x142
    pub hit_flag165: bool, // 0x165
    pub last_normal: [f32; 2], // 0x168
    pub last_impulse: f32, // 0x170
    pub fireball: bool,    // 0x161
    pub spooky: bool,      // 0x160 spooky ball armed (respawns at the top once it falls out)
    pub spooky_used: bool, // 0x146
    pub teleport: i32,     // 0x14c ticks left frozen before reappearing at `teleport_to`
    pub teleport_to: Option<[f32; 2]>, // 0x15c target object (its position)
    pub teleport_uid: Option<u32>, // target object uid, resolved when the countdown ends
    pub id: u32,           // +0x58 object id (creation order; pair-set / update order)
    pub sap: Vec<u8>,      // sweep-and-prune state vs each board object (see `sap_update`)
    /// bbox at the last broadphase update (None: the next update scans every object) and the objects
    /// whose `sap` state has the pair bit 0x10 (see `SapIndex`)
    pub sap_bb: Option<[f32; 4]>,
    /// with `sap_bb`: per `SAP_EDGE` j, the partition points of `SapIndex::edges[j]` at `sap_bb`'s ball edge
    /// (count of endpoints < it, and <= it); the next update walks them to the new edge instead of binary
    /// searching (the ball moves a few pixels per tick)
    pub sap_ix: [[u32; 2]; 4],
    pub sap_over: Vec<u32>,
    pub gone: bool,
    /// Ball+0x145: a fireball touched the bucket lid "freeballcover" (FUN_004704a0 then switches the lid
    /// off); catching it scores style 7
    pub lid_hit: bool,
    /// Ball+0x144: touched a "flipper" since its last first peg hit (FUN_004704a0)
    pub flipper_touch: bool,        // removed (lost / captured)
}

#[derive(Clone, Debug)]
pub struct Line {
    pub x1: f32, pub x2: f32, pub y1: f32, pub y2: f32, // 0xec 0xf0 0xf4 0xf8
    pub n: [f32; 2],   // 0x104
    pub c: f32,        // 0x10c
    pub steep: bool,   // 0x110
    pub oneside: bool, // 0x111 (no normal flip)
    pub vel: [f32; 2], // 0xfc (surface velocity)
    pub d: [f32; 2],   // 0xe4/0xe8 (x2-x1, y2-y1)
    pub bbox: [f32; 4],
}

impl Line {
    /// FUN_004782f0
    pub fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Line {
        let mut bbox = [x1, y1, x2, y2];
        if x2 < x1 { bbox.swap(0, 2); }
        if bbox[3] < bbox[1] { bbox.swap(1, 3); }
        let dy = sub(y1, y2).r();
        let ady = dy.abs();
        let adx = sub(x1, x2).r().abs();
        let steep = !(ady <= adx);
        let n = normalize([dy, sub(x2, x1).r()]);
        let c = -((x(n[1]) * y1) + (x(n[0]) * x1)).r();
        let d = [sub(x2, x1).r(), sub(y2, y1).r()];
        Line { x1, x2, y1, y2, n, c, steep, oneside: false, vel: [0.0; 2], d, bbox }
    }

    /// FUN_00478440 Line::SetPos: move start point, keep direction and normal.
    pub fn set_pos(&mut self, px: f32, py: f32) {
        self.x1 = px;
        self.y1 = py;
        self.x2 = add(self.d[0], px).r();
        self.y2 = add(self.d[1], py).r();
        self.c = -((x(self.n[1]) * py) + (x(self.n[0]) * px)).r();
        self.bbox = [px, py, self.x2, self.y2];
        if self.x2 < px { self.bbox.swap(0, 2); }
        if self.bbox[3] < self.bbox[1] { self.bbox.swap(1, 3); }
    }
}

#[derive(Clone, Debug)]
pub enum Geom {
    Peg { pos: [f32; 2], r: f32, vel: [f32; 2] },
    Line(Line),
    /// `center` = 0x114/0x118 position; `offs` = per-line start vertex offsets (0xf8 array).
    /// `verts` = unrotated vertex offsets (0xe4 vector), `rot` = 0x11c, `dirty_rot` = 0x145 (lines
    /// rebuilt lazily by FUN_0047d5b0 when the poly is next a collision candidate).
    Poly { lines: Vec<Line>, offs: Vec<[f32; 2]>, inside_test: bool, convex: bool, angvel: f32, center: [f32; 2],
           /// emptied after load: the vertices live in `SapIndex::verts` (shared by clones)
           verts: Vec<(f32, f32)>, rot: f32, oneside: bool, dirty_rot: bool,
           /// lines not yet moved to `center` (+ `offs`) with velocity `pvel`: the per-tick SetPos of every line
           /// (FUN_0047bb90/FUN_0047bcd0) is a pure overwrite, so it is applied only when collision code reads them
           stale: bool, pvel: [f32; 2] },
    /// Hole (internal type 1): pos 0xf4/0xf8, size 0x104/0x108, elliptical test if 0x10c.
    /// `teleport`: Hole+0xe4 target object (`Obj::uid`) and Hole+0xe8 delay in ticks.
    Hole { pos: [f32; 2], w: i32, h: i32, ellipse: bool, teleport: Option<(u32, i32)> },
}

/// Precomputed trajectory of a mover with no sub-mover: its output (pos, angle) is a pure function of the
/// object counter with period `p` (checked bitwise over three periods at build), so `tick_objects` reads
/// `e[counter % p]` instead of evaluating it — valid only while the mover's def and centre are the ones
/// it was built from (fever / bucket easing / oracle state change them: checked at every use).
/// Incremental broadphase (exact): an object whose bbox never changes (no mover) can only change its `sap`
/// state when a ball edge crosses — or touches — one of its edges between two updates, so each frame only
/// the static objects whose edge lies in [old, new] of the matching ball edge (sorted edge lists) plus all
/// mover objects are updated; the rest would take `sap_update`'s no-change path.
#[derive(Debug, Default)]
pub struct SapIndex {
    /// (edge value, object): [obj max x (vs ball min x), obj min x (vs ball max x), obj max y (vs ball
    /// min y), obj min y (vs ball max y)]
    edges: [Vec<(f32, u32)>; 4],
    /// objects updated every frame (movers, or a NaN in the bbox)
    dynamic: Vec<u32>,
    /// immutable per-object data shared by all clones of the board: poly vertex offsets (Poly 0xe4,
    /// moved out of `Geom::Poly::verts` after load so clones don't copy them), the hole objects
    pub verts: Vec<Vec<(f32, f32)>>,
    pub holes: Vec<u32>,
    /// per object: its mover's precomputed trajectory (`MoverTable`)
    pub mtab: Vec<Option<MoverTable>>,
}
const SAP_EDGE: [(usize, usize); 4] = [(2, 0), (0, 2), (3, 1), (1, 3)]; // (object bbox index, ball bbox index)

fn build_sap_index(objs: &mut [Obj], movers: &[crate::mover::Mover]) -> SapIndex {
    let mut x = SapIndex::default();
    x.mtab = build_mover_tables(objs, movers);
    x.holes = objs.iter().enumerate().filter(|(_, o)| matches!(o.geom, Geom::Hole { .. })).map(|(i, _)| i as u32).collect();
    x.verts = objs.iter_mut().map(|o| if let Geom::Poly { verts, .. } = &mut o.geom { std::mem::take(verts) } else { vec![] }).collect();
    for (i, o) in objs.iter().enumerate() {
        if o.mover.is_some() || o.bbox.iter().any(|v| v.is_nan()) { x.dynamic.push(i as u32); continue; }
        for (j, &(oe, _)) in SAP_EDGE.iter().enumerate() { x.edges[j].push((o.bbox[oe], i as u32)); }
    }
    for e in x.edges.iter_mut() { e.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.cmp(&b.1))); }
    x
}

/// step_multi's per-frame buffers, kept between frames to avoid allocations
#[derive(Clone, Debug, Default)]
pub struct StepScratch { frozen: Vec<bool>, captured: Vec<bool>, lost: Vec<bool>, order: Vec<usize>, cands: Vec<Vec<usize>>, proc: Vec<u32>, seen: Vec<u32>, stamp: u32 }

#[derive(Debug)]
pub struct MoverTable {
    pub p: i32,
    pub version: u32,
    /// (pos, angle, rotated bbox of the poly at that angle about pos — FUN_0047a010; unused for non-polys)
    pub e: Vec<([f32; 2], f32, [f32; 4])>,
    /// Poly::Update angular velocity when the previous angle was the previous entry's: wrap(angle - prev)
    pub w: Vec<f32>,
}

#[derive(Clone, Debug)]
pub struct Obj {
    pub geom: Geom,
    pub bouncy: f32,
    pub rolly: f32,
    pub maxbouncevel: f32,
    pub ballstopreset: bool,
    pub collidable: bool,
    /// PhysObj+0x25: visible (cleared immediately when popped by FUN_00466a00, while `collidable`
    /// stays true for 4 more ticks via `remove_timer`, and cleared on fireball burn / flipper_clear).
    pub visible: bool,
    pub has_peginfo: bool,
    pub bbox: [f32; 4],
    pub src_index: usize,
    pub hit: bool,
    pub mover: Option<usize>, // index into Board::movers (PhysObj+0x48)
    pub counter: i32,         // PhysObj+0x4c frame counter
    pub moving: bool,         // PhysObj+0x26
    pub crumble: bool,        // PegInfo+0x16
    pub contacts: i32,        // PegInfo+0x1c: contact count (to 3), then crumble timer to 100
    pub since_hit: i32,       // PegInfo+0x18: ticks since first hit (0 = not hit)
    /// mover object at a fixed point: its last update moved/rotated nothing, so while it stays still
    /// (not moving, no sub-mover) every further update rewrites identical values and is skipped
    pub settled: bool,
    /// (counter, counter % table period) at the last table read: the next index without a division
    pub tidx: (i32, i32),
    pub remove_timer: i32,    // PhysObj+0xc8: countdown to non-collidable (set to 4 when popped)
    pub flipper: bool,        // id == "flipper": only collidable while the flipper power is active
    pub bucketish: bool,
    /// id starts with "freeball" (bucket, its parts and the lid)
    pub freeball: bool,
    pub bumper: Option<i32>,
    /// id starts with "fever": the end-of-level bonus buckets/dividers (FUN_00458ae0 turns them on)
    pub fever: bool,
    /// PhysObj+0x29 (foreground2)
    pub fg2: bool,
    /// Hole+0xf0: score value of a "fever" bonus hole (10000/50000/100000), else 0
    pub hole_value: i32,
    pub cover: bool,          // id "freeballcover": lid over the bucket while a spooky ball is armed (FUN_00440700)  // id "bumperhole": Some(subid) (pyramid parts: 0 = collision, 1 = graphics)      // id starts with "freeball" or is "bumperhole" (FUN_00440830 amplitude easing)
    /// Sweep-and-prune state vs the (single) ball, see `sap_update`. Bits: 0/2 = x/y "ball.min sorted
    /// before obj.max", 1/3 = x/y "obj.min sorted before ball.max", 4 = pair in the candidate set,
    /// 7 = initialised for the current ball.
    pub sap: u8,
    /// Stable id: (level number << 16) | object index in the level file (survives `objs` edits).
    pub uid: u32,
    /// logic "movetoggle" (Poly vt+0x70 FUN_0047b9d0): a contact sends the mover to the other end
    pub movetoggle: bool,
    /// PhysObj+0x50: ticks left before `moving` is cleared
    pub move_timer: i32,
    /// PegInfo+0x10 peg type (score::BLUE/ORANGE/PURPLE/GREEN); 0 = no PegInfo
    pub peg_type: u8,
    /// PhysObj+0xcc "flipperflags": 1 = always cleared by the flipper power, 2 = never
    pub fflags: u8,
    /// position in the level's top-level object list (the board list order; -1 = inline teleport target)
    pub top_ix: i32,
    /// PegInfo+0x15 peg_variable: colour chosen at level start (FUN_00457960)
    pub variable: bool,
}

/// Broadphase FUN_0044b1e0 (run for the x then the y endpoint list by FUN_0045add0): a persistent
/// insertion sort of bbox endpoints that swaps while `cur <= prev`. A min crossing left of a max adds
/// the pair (if the inclusive bbox test FUN_004326e0 passes); a max crossing left of a min removes it.
/// Because of `<=`, a run of equal endpoints reverses order every pass, so for each (ball endpoint,
/// object endpoint) the relative order is the strict value order, flipping on ties. Returns whether
/// the pair is in the candidate set.
/// `slice::partition_point` for a predicate on the endpoint value, walked from a nearby index `i` (exact for a
/// sorted list: the answer is unique).
#[inline(always)]
fn walk_partition(arr: &[(f32, u32)], mut i: usize, pred: impl Fn(f32) -> bool) -> usize {
    let i0 = i.min(arr.len());
    i = i0;
    while i < arr.len() && pred(arr[i].0) { i += 1; }
    if i == i0 { while i > 0 && !pred(arr[i - 1].0) { i -= 1; } }
    i
}

#[inline(always)]
fn sap_update(st: &mut u8, ball: &[f32; 4], obj: &[f32; 4]) -> bool {
    // fast path (exact): every comparison strict and equal to the stored bit → nothing toggles
    let lt = (ball[0] < obj[2]) as u8 | ((obj[0] < ball[2]) as u8) << 1 | ((ball[1] < obj[3]) as u8) << 2 | ((obj[1] < ball[3]) as u8) << 3;
    let gt = (ball[0] > obj[2]) as u8 | ((obj[0] > ball[2]) as u8) << 1 | ((ball[1] > obj[3]) as u8) << 2 | ((obj[1] > ball[3]) as u8) << 3;
    if *st & 0x80 != 0 && (lt | gt) == 0xF && lt == *st & 0xF { return *st & 0x10 != 0; }
    sap_update_slow(st, ball, obj)
}

#[inline(never)]
fn sap_update_slow(st: &mut u8, ball: &[f32; 4], obj: &[f32; 4]) -> bool {
    if *st & 0x80 == 0 {
        // ball endpoints inserted at the end and sorted in: they pass all endpoints <= themselves
        let mut v = 0x80;
        if ball[0] <= obj[2] { v |= 1; }
        if obj[0] < ball[2] { v |= 2; }
        if ball[1] <= obj[3] { v |= 4; }
        if obj[1] < ball[3] { v |= 8; }
        if bbox_overlap(ball, obj) { v |= 0x10; }
        *st = v;
        return v & 0x10 != 0;
    }
    for (bit, lo, hi) in [(1u8, ball[0], obj[2]), (2, obj[0], ball[2]), (4, ball[1], obj[3]), (8, obj[1], ball[3])] {
        let old = *st & bit != 0;
        let new = match lo.partial_cmp(&hi) {
            Some(std::cmp::Ordering::Less) => true,
            Some(std::cmp::Ordering::Greater) => false,
            Some(std::cmp::Ordering::Equal) => !old,
            None => old,
        };
        if new == old { continue; }
        *st ^= bit;
        if !new { *st &= !0x10; } else if bbox_overlap(ball, obj) { *st |= 0x10; }
    }
    *st & 0x10 != 0
}

/// FUN_00432660: normalize a stored f32 vector.
pub fn normalize(v: [f32; 2]) -> [f32; 2] {
    let l2 = ((x(v[1]) * v[1]) + (x(v[0]) * v[0])).r();
    let l = sqrt_r(l2);
    if l == 0.0 { return v; }
    [(x(v[0]) / l).r(), (x(v[1]) / l).r()]
}

#[derive(Default, Clone, Copy)]
struct Hit {
    contact: [f32; 2],
    normal: [f32; 2],
    svel: [f32; 2],    // surface velocity projected on normal
    angv: [f32; 2],
}

/// FUN_00475660: (n · obj.vel) n
fn velproj(n: [f32; 2], v: [f32; 2]) -> [f32; 2] {
    let vv = [v[0], v[1]];
    let d = ((x(n[1]) * vv[1]) + (x(n[0]) * vv[0])).r();
    [(x(n[0]) * d).r(), (x(d) * n[1]).r()]
}

/// FUN_00475000 (this = peg): swept circle vs ball. p = ball pos, v = ball vel, br = ball radius.
fn sweep_peg(c: [f32; 2], cr: f32, p: [f32; 2], v: [f32; 2], br: f32, toi: &mut f32, out: &mut [f32; 2]) -> bool {
    let rr = ((x(cr) - X::d(EPS)) + br).r();
    let r2 = (x(rr) * rr).r();
    let dx = sub(p[0], c[0]).r();
    let dy = sub(p[1], c[1]).r();
    let d2 = ((x(dx) * dx) + (x(dy) * dy)).r();
    if d2 < r2 {
        *toi = 0.0;
        *out = p;
        return true;
    }
    let a = ((x(v[0]) * v[0]) + (x(v[1]) * v[1])).r();
    let b0 = ((x(dx) * v[0]) + (x(dy) * v[1])).r();
    let b = (x(b0) + b0).r();
    let s1 = ((x(p[0]) * p[0]) + (x(p[1]) * p[1])).r();
    let s2 = ((x(c[1]) * c[1]) + (x(c[0]) * c[0])).r();
    let dot = ((x(c[0]) * p[0]) + (x(c[1]) * p[1])).r();
    let cc = (((x(s1) + s2) - (x(dot) + dot)) - r2).r();
    let disc = ((x(b) * b) - (x(cc) * (x(a) * X::d(4.0)))).r();
    if !(disc >= 0.0) { return false; }
    let s = sqrt_r(disc);
    let a2 = x(a) + a;
    let mut t = ((-x(b) - s) / a2).r();
    if !(t >= 0.0) {
        t = ((x(s) - b) / a2).r();
    }
    if !(t >= 0.0) { return false; }
    if !(t < *toi) { return false; }
    *toi = t;
    let ox = (x(v[0]) * t).r();
    let oy = (x(t) * v[1]).r();
    *out = [(x(p[0]) + ox).r(), (x(p[1]) + oy).r()];
    true
}

/// FUN_00475bd0 (this = line): segment vs circle(c, r) intersections. Returns count; first point.
fn seg_circle(l: &Line, cx: f32, cy: f32, r: f32, p1: &mut [f32; 2]) -> i32 {
    let fx = sub(l.x1, cx).r();
    let fy = sub(l.y1, cy).r();
    let dx = sub(l.x2, l.x1).r();
    let dy = sub(l.y2, l.y1).r();
    let a = ((x(dy) * dy) + (x(dx) * dx)).r();
    let b0 = ((x(dx) * fx) + (x(dy) * fy)).r();
    let b = (x(b0) + b0).r();
    let c0 = ((x(fy) * fy) + (x(fx) * fx)).r();
    let c = (x(c0) - (x(r) * r)).r();
    let disc = ((x(b) * b) - (x(c) * (x(a) * X::d(4.0)))).r();
    if disc < 0.0 { return 0; }
    let s = sqrt_r(disc);
    let a2 = x(a) + a;
    let t1 = ((x(s) - b) / a2).r();
    let t2 = ((-x(b) - s) / a2).r();
    let bx = (x(fx) + cx).r();
    let by = (x(fy) + cy).r();
    let mut n = 0;
    if t1 >= 0.0 && t1 <= 1.0 {
        let ox = (x(dx) * t1).r();
        let oy = (x(t1) * dy).r();
        *p1 = [(x(ox) + bx).r(), (x(oy) + by).r()];
        n = 1;
    }
    if t2 < 0.0 || 1.0 < t2 { return n; }
    if n == 0 {
        let ox = (x(dx) * t2).r();
        let oy = (x(t2) * dy).r();
        *p1 = [(x(bx) + ox).r(), (x(by) + oy).r()];
        return 1;
    }
    n + 1
}

/// FUN_00478500 (this = line): ball already touching the line at p (static overlap).
fn line_overlap(l: &Line, p: [f32; 2], r: f32, h: &mut Hit) -> bool {
    let rr = (x(r) - X::d(EPS)).r();
    let s0 = ((x(l.n[1]) * p[1]) + (x(p[0]) * l.n[0])).r();
    let s = (x(s0) + l.c).r();
    let dist = s.abs();
    if rr < dist { return false; }
    if l.steep {
        if !(l.bbox[1] <= (x(r) + p[1])) { return false; }
        if l.bbox[3] < (x(p[1]) - r) { return false; }
    } else {
        if !(l.bbox[0] <= (x(p[0]) + r)) { return false; }
        if l.bbox[2] < (x(p[0]) - r) { return false; }
    }
    let mut ip = [0.0f32; 2];
    let k = seg_circle(l, p[0], p[1], rr, &mut ip);
    if k == 0 { return false; }
    if k == 1 {
        // touching near an endpoint: push from the nearer endpoint
        let a0 = sub(l.x1, p[0]).r();
        let a1 = sub(l.y1, p[1]).r();
        let b0 = sub(l.x2, p[0]).r();
        let b1 = sub(l.y2, p[1]).r();
        let e1 = (x((x(a0) * a0).r()) + (x(a1) * a1).r()).r();
        let e2 = (x((x(b0) * b0).r()) + (x(b1) * b1).r()).r();
        let (ex, ey, ox, oy) = if e2 <= e1 { (l.x2, l.y2, l.x1, l.y1) } else { (l.x1, l.y1, l.x2, l.y2) };
        let mut off = [sub(ex, ip[0]).r(), sub(ey, ip[1]).r()];
        if off[0] == 0.0 && off[1] == 0.0 {
            let nn = normalize([sub(ex, ox).r(), sub(ey, oy).r()]);
            off = [(x(nn[0]) * X::d(EPS)).r(), (X::d(EPS) * nn[1]).r()];
        }
        h.contact = [(x(p[0]) + off[0]).r(), (x(off[1]) + p[1]).r()];
        let n = normalize([sub(h.contact[0], ex).r(), sub(h.contact[1], ey).r()]);
        h.normal = n;
        let ex2 = (x(n[0]) * X::d(EPS)).r();
        let ey2 = (X::d(EPS) * n[1]).r();
        h.contact = [(x(h.contact[0]) + ex2).r(), (x(ey2) + h.contact[1]).r()];
        h.svel = velproj(n, l.vel);
        return true;
    }
    let mut n = l.n;
    let mut s = s;
    if !l.oneside && s < 0.0 {
        n = [(x(n[0]) * X::d(-1.0)).r(), (x(n[1]) * X::d(-1.0)).r()];
        s = (X::d(-1.0) * s).r();
    }
    h.normal = n;
    let k = (x(r) - s).r();
    let ox = (x(n[0]) * k).r();
    let oy = (x(k) * n[1]).r();
    h.contact = [(x(p[0]) + ox).r(), (x(oy) + p[1]).r()];
    h.svel = velproj(n, l.vel);
    true
}

/// FUN_00475890 (this = line): swept ball vs infinite line, clipped to the segment extent.
fn line_sweep(l: &Line, p: [f32; 2], v: [f32; 2], r: f32, h: &mut Hit, toi: &mut f32, first_iter: &mut bool) -> bool {
    let mut n = l.n;
    let mut c = l.c;
    if !l.oneside {
        let dx = sub(p[0], l.x1).r();
        let dy = sub(p[1], l.y1).r();
        let d = ((x(dx) * n[0]) + (x(dy) * n[1])).r();
        if d < 0.0 {
            n = [(X::d(-1.0) * n[0]).r(), (X::d(-1.0) * n[1]).r()];
            c = (X::d(-1.0) * l.c).r();
        }
    }
    let vn = ((x(v[0]) * n[0]) + (x(n[1]) * v[1])).r();
    if (vn as f64) > 0.20000000298023224 { return false; }
    if vn == 0.0 {
        if !*first_iter || n[1] != 0.0 { return false; }
        let fx = sub(p[0], (x(n[0]) * r).r()).r();
        let fy = sub(p[1], (x(n[1]) * r).r()).r();
        let f = if l.steep { fy } else { fx };
        let (lo, hi) = if l.steep { (l.bbox[1], l.bbox[3]) } else { (l.bbox[0], l.bbox[2]) };
        if !(lo <= f && f <= hi) { return false; }
        let dd = ((x(n[0]) * p[0]) + (x(n[1]) * p[1])).r();
        let dd = (x(dd) + c).r(); // NB: game uses the (possibly flipped) local c
        if r < dd.abs() { return false; }
        *toi = 0.0;
        h.contact = p;
        h.normal = n;
        h.svel = [0.0; 2];
        *first_iter = false;
        return true;
    }
    let dd = ((x(p[1]) * n[1]) + (x(n[0]) * p[0])).r();
    let t = ((((x(r) - X::d(EPS)) - c) - dd) / vn).r();
    if t < 0.0 || *toi <= t { return false; }
    let ox = (x(v[0]) * t).r();
    let oy = (x(t) * v[1]).r();
    let hx = (x(ox) + p[0]).r();
    let hy = (x(p[1]) + oy).r();
    let fx = (x(hx) - (x(n[0]) * r).r()).r();
    let fy = (x(hy) - (x(r) * n[1]).r()).r();
    if l.steep {
        if !(l.bbox[1] <= fy) || !(fy <= l.bbox[3]) { return false; }
    } else {
        if !(l.bbox[0] <= fx) || !(fx <= l.bbox[2]) { return false; }
    }
    *toi = t;
    let ex = (x(n[0]) * X::d(EPS)).r();
    let ey = (x(n[1]) * X::d(EPS)).r();
    h.contact = [(x(hx) + ex).r(), (x(hy) + ey).r()];
    h.normal = n;
    h.svel = velproj(n, l.vel);
    true
}

/// FUN_004756c0: swept ball vs line endpoint (ex, ey).
fn cap_sweep(l: &Line, p: [f32; 2], v: [f32; 2], r: f32, toi: &mut f32, h: &mut Hit, ex: f32, ey: f32) -> bool {
    let dx = sub(p[0], ex).r();
    let dy = sub(p[1], ey).r();
    let rr = (x(r) - X::d(EPS)).r();
    let vxx = x(v[0]) * v[0];
    let a = ((x(v[1]) * v[1]) + vxx).r();
    let b0 = ((x(v[0]) * dx) + (x(dy) * v[1])).r();
    let b = (x(b0) + b0).r();
    let c0 = ((x(dy) * dy) + (x(dx) * dx)).r();
    let c = (x(c0) - (x(rr) * rr)).r();
    let disc = ((x(b) * b) - (x(c) * (x(a) * X::d(4.0)))).r();
    if !(disc >= 0.0) { return false; }
    let nb = (-x(b)).r();
    let s = sqrt_r(disc);
    let t = ((x(nb) - s) / (x(a) + a)).r();
    if !(t >= 0.0) { return false; }
    if !(t < *toi) { return false; }
    let vv = ((x(v[1]) * v[1]) + X::d(vxx.0)).r(); // vxx stored as double (exact)
    let sv = sqrt_r(vv);
    let t2 = (x(t) - (X::d(EPS) / sv)).r();
    let ox = (x(v[0]) * t2).r();
    let oy = (x(t2) * v[1]).r();
    h.contact = [(x(p[0]) + ox).r(), (x(p[1]) + oy).r()];
    *toi = if t2 <= 0.0 { 0.0 } else { t2 };
    let n = normalize([sub(h.contact[0], ex).r(), sub(h.contact[1], ey).r()]);
    h.normal = n;
    h.svel = velproj(n, l.vel);
    true
}

/// FUN_00478940 (this = line)
fn line_collide(l: &Line, p: [f32; 2], v: [f32; 2], r: f32, toi: &mut f32, h: &mut Hit, both_caps: bool, first_iter: &mut bool) -> bool {
    const PAD: f64 = 20.0; // @5d4f20
    if !((x(l.bbox[0]) - X::d(PAD)).r() <= (x(p[0]) + r)) { return false; }
    if !((x(p[0]) - r) <= (x(l.bbox[2]) + X::d(PAD)).r()) { return false; }
    if !((x(l.bbox[1]) - X::d(PAD)).r() <= (x(p[1]) + r)) { return false; }
    if !((x(p[1]) - r) <= (X::d(PAD) + l.bbox[3]).r()) { return false; }
    if line_overlap(l, p, r, h) {
        *toi = 0.0;
        return true;
    }
    let a = line_sweep(l, p, v, r, h, toi, first_iter);
    let b = cap_sweep(l, p, v, r, toi, h, l.x1, l.y1);
    let mut hit = a || b;
    if both_caps && cap_sweep(l, p, v, r, toi, h, l.x2, l.y2) { hit = true; }
    hit
}

/// FUN_0047dfe0 (this = poly)
fn poly_collide(lines: &[Line], inside_test: bool, convex: bool, bbox: [f32; 4], p: [f32; 2], v: [f32; 2], r: f32,
                toi: &mut f32, h: &mut Hit, first_iter: &mut bool) -> bool {
    const PAD: f64 = 20.0;
    if !((x(bbox[0]) - X::d(PAD)).r() <= (x(p[0]) + r)) { return false; }
    if !((x(p[0]) - r) <= (x(bbox[2]) + X::d(PAD)).r()) { return false; }
    if !((x(bbox[1]) - X::d(PAD)).r() <= (x(p[1]) + r)) { return false; }
    if !((x(p[1]) - r) <= (X::d(PAD) + bbox[3]).r()) { return false; }
    let mut hit_any = false;
    let mut best: Option<usize> = None;
    let mut best_d: f32 = 10000000.0;
    let mut crossings = 0u32;
    for (i, l) in lines.iter().enumerate() {
        if line_collide(l, p, v, r, toi, h, false, first_iter) { hit_any = true; }
        if inside_test {
            let rx = sub(p[0], l.x1).r();
            let ry = sub(p[1], l.y1).r();
            // 0x47e182: fld n1; fmul ry; fld n0; fmul rx; faddp; fstp; fchs (sum order and sign are exact either way)
            let depth = (-((x(l.n[0]) * rx) + (x(l.n[1]) * ry))).r();
            let consider = if !convex {
                let y = p[1];
                let straddle = (l.y1 != l.y2 && l.y1 <= y && y < l.y2) || (l.y2 <= y && y < l.y1);
                if straddle {
                    // 0x47e204: (x2 - x1)·(y - y1) / (y2 - y1) + x1 in extended precision, one f32 store
                    let xi = (((x(y) - l.y1) * sub(l.x2, l.x1)) / sub(l.y2, l.y1) + l.x1).r();
                    if p[0] < xi { crossings += 1; }
                }
                0.0 < depth
            } else { true };
            if consider && depth < best_d { best_d = depth; best = Some(i); }
        }
    }
    if convex && 0.0 < best_d { crossings = 1; }
    if crossings & 1 != 0 {
        if let Some(bi) = best {
            if *toi != 0.0 {
                *toi = 0.0;
                let n = lines[bi].n;
                let k = x(best_d) + r;
                let ox = (k * n[0]).r();
                let oy = (k * n[1]).r();
                h.contact = [(x(ox) + p[0]).r(), (x(p[1]) + oy).r()];
                h.svel = velproj(n, lines[bi].vel);
                h.normal = n;
                hit_any = true;
            }
        }
    }
    hit_any
}

impl Ball {
    pub fn new(pos: [f32; 2], vel: [f32; 2]) -> Ball {
        let mut b = Ball { r: 6.0, ..Default::default() };
        b.set_pos(pos);
        b.vel = vel;
        b.prev = pos;
        b
    }
    /// vtable[17] FUN_00474e60
    pub fn set_pos(&mut self, p: [f32; 2]) {
        self.pos = p;
        self.bbox = [sub(p[0], self.r).r(), sub(p[1], self.r).r(), (x(self.r) + p[0]).r(), (x(p[1]) + self.r).r()];
    }
    /// vtable[18] FUN_00474eb0
    pub fn move_by(&mut self, d: [f32; 2]) {
        self.pos = [(x(d[0]) + self.pos[0]).r(), (x(d[1]) + self.pos[1]).r()];
        self.bbox[0] = (x(self.bbox[0]) + d[0]).r();
        self.bbox[2] = (x(self.bbox[2]) + d[0]).r();
        self.bbox[1] = (x(self.bbox[1]) + d[1]).r();
        self.bbox[3] = (x(d[1]) + self.bbox[3]).r();
    }

    /// Ball::Update FUN_0047a450 (normal free-flying path)
    /// Returns true while frozen in a teleport (the rest of the tick is skipped).
    /// FUN_00480f50(1, 1): put a Magic Hat on this ball; the anchor starts 200 to the board-centre side
    /// and 200 above the ball (the hat flies in).
    pub fn give_hat(&mut self) {
        if self.hat.is_some() { return; }
        let ax = if self.pos[0] <= 325.0 { add(self.pos[0], 200.0).r() } else { sub(self.pos[0], 200.0).r() };
        self.hat = Some(Hat { anchor: [ax, sub(self.pos[1], 200.0).r()], pos: [0.0; 2], ctr: 0 });
    }

    /// Ball::Update FUN_0047a450 hat part (+0x18d): ease the anchor (+0x104/+0x108) toward the ball (y
    /// never more than 2 below it) and SetPos the hat ball at anchor + (0, -21). Then the hat ball's own
    /// Update (it comes later in id order) only bumps its counter +0x4c.
    fn update_hat(&mut self, owner_moved: bool) {
        let (px, py) = (self.pos[0], self.pos[1]);
        let Some(h) = self.hat.as_mut() else { return };
        if owner_moved {
            let fy = add(py, 2.0).r();
            if fy < h.anchor[1] { h.anchor[1] = fy; }
            h.anchor[0] = (((x(px) - h.anchor[0]) * X::d(0.25)) + h.anchor[0]).r();
            h.anchor[1] = (x(h.anchor[1]) + (X::d(0.25) * (x(py) - h.anchor[1]))).r();
            h.pos = [(X::d(0.0) + h.anchor[0]).r(), (x(h.anchor[1]) - X::d(21.0)).r()];
        }
        h.ctr += 1;
    }

    pub fn update(&mut self) -> bool {
        self.frames += 1;
        if self.combo_timer != 0 { self.combo_timer -= 1; if self.combo_timer == 0 { self.combo_count = 0; } }
        self.stuck += 1;
        if self.stuck % 10 == 0 {
            let idx = (self.stuck / 10).clamp(1, 6) - 1;
            self.stuck_samples[idx as usize] = self.pos;
            let mut flag = self.stuck_flag;
            self.stuck_flag = false;
            for j in 0..idx as usize {
                let dx = sub(self.stuck_samples[j][0], self.pos[0]).r();
                let dy = sub(self.stuck_samples[j][1], self.pos[1]).r();
                let d2 = (x((x(dx) * dx).r()) + (x(dy) * dy).r()).r();
                if 4.0 <= d2 {
                    self.stuck = 0;
                    break;
                }
                if 0.04f32 <= d2 && !flag {
                    self.stuck -= 5;
                    flag = true;
                    self.stuck_flag = true;
                }
            }
        }
        self.prev = self.pos;
        if self.teleport > 0 {
            self.teleport -= 1;
            self.stuck = 0;
            if self.teleport != 0 {
                self.moved = true;
                return true;
            }
            if let Some(t) = self.teleport_to.take() { self.set_pos_full(t); }
        }
        self.vel[1] = (x(self.vel[1]) + GRAVITY).r();
        let (vx, vy) = (self.vel[0], self.vel[1]);
        let m2 = (x(MAX_SPEED) * MAX_SPEED).r();
        let (mut vx, mut vy) = (vx, vy);
        if (m2 as f64) < ((x(vy) * vy) + (x(vx) * vx)).0 {
            let n = normalize([vx, vy]);
            vx = (x(n[0]) * MAX_SPEED).r();
            vy = (x(MAX_SPEED) * n[1]).r();
            self.vel = [vx, vy];
        }
        let lx = if 0.0 <= vx { 0.0 } else { vx };
        self.bbox[0] = ((x(self.pos[0]) - self.r) + lx).r();
        let hx = if vx <= 0.0 { 0.0 } else { vx };
        self.bbox[2] = ((x(self.r) + self.pos[0]) + hx).r();
        let ly = if 0.0 <= vy { 0.0 } else { vy };
        self.bbox[1] = ((x(self.pos[1]) - self.r) + ly).r();
        let hy = if vy <= 0.0 { 0.0 } else { vy };
        self.moved = false;
        self.hit_this_frame = false;
        self.bbox[3] = ((x(self.r) + self.pos[1]) + hy).r();
        self.pos134 = self.pos;
        false
    }

    /// FUN_00474dd0: SetPos plus prev (0xf4), 0x134 and 0x104 (smoothed pos, not modelled).
    /// FUN_00474dd0: SetPos, then +0xf4 (prev), +0x134 and +0x104 (the Magic Hat anchor) = the new pos
    pub fn set_pos_full(&mut self, p: [f32; 2]) {
        self.set_pos(p);
        self.prev = p;
        self.pos134 = p;
        if let Some(h) = self.hat.as_mut() { h.anchor = p; }
    }


    /// FUN_00477340 peg bounce (this = ball, peg = circle)
    fn peg_bounce(&mut self, pc: [f32; 2], pr: f32, pvel: [f32; 2], bouncy: f32, rolly: f32, ballstopreset: bool) {
        let ddx = sub(self.pos[0], pc[0]).r();
        let ddy = sub(self.pos[1], pc[1]).r();
        let d2 = (x((x(ddx) * ddx).r()) + (x(ddy) * ddy).r()).r();
        let rb = self.r;
        let n = normalize([sub(pc[0], self.pos[0]).r(), sub(pc[1], self.pos[1]).r()]);
        let vb = self.vel;
        self.last_normal = [-n[0], -n[1]];
        let bouncy = if bouncy == 0.0 { PEG_BOUNCY_DEFAULT } else { bouncy };
        let rolly = if rolly == 0.0 { PEG_ROLLY_DEFAULT } else { rolly };
        let dvx = sub(vb[0], pvel[0]).r();
        let dvy = sub(vb[1], pvel[1]).r();
        let dot = ((x(dvy) * n[1]) + (x(dvx) * n[0])).r();
        let mut j = (x(dot) * bouncy).r();
        if rolly < j.abs() {
            j = (x(j) + j).r();
        } else {
            self.frames = 0;
        }
        self.last_impulse = j.abs();
        // fireball passes through pegs with PegInfo: handled by caller
        let q = (x(j) / X::d(BIG1)).r();
        let k = ((-x(q)) * X::d(BIG2)).r();
        let kx = (x(n[0]) * k).r();
        let ky = (x(k) * n[1]).r();
        self.vel = [(x(vb[0]) + kx).r(), (x(vb[1]) + ky).r()];
        let rsum = x(pr) + rb; // stored as double
        let rsum = X::d(rsum.0);
        let dist = sqrt_r(d2);
        let ov = (rsum - dist).r();
        let mx = (x(n[0]) * ov).r();
        let my = (x(ov) * n[1]).r();
        self.move_by([-mx, -my]);
        if ballstopreset { self.stuck = 0; }
    }

    /// FUN_004775f0 line/poly bounce. `v` = CCD-local velocity (2nd argument).
    /// Returns false when the contact is separating (p5 < 0): FUN_004775f0 then returns before the
    /// contact callback DAT_00650a6c, so no peg hit is registered.
    fn line_bounce(&mut self, v: &mut [f32; 2], obj: &Obj, h: &mut Hit) -> bool {
        let n = h.normal;
        self.last_normal = n;
        self.set_pos(h.contact);
        let rx = sub(v[0], h.svel[0]).r();
        let ry = sub(v[1], h.svel[1]).r();
        let rx2 = sub(rx, h.angv[0]).r();
        let ry2 = sub(ry, h.angv[1]).r();
        let p5a = ((x(n[1]) * ry2) + (x(n[0]) * rx2)).r();
        let mut p5 = (-x(p5a)).r();
        if p5 < 0.0 {
            self.last_impulse = 0.0;
            return false;
        }
        self.last_impulse = p5;
        let bouncy = if obj.bouncy == 0.0 { LINE_BOUNCY_DEFAULT } else { obj.bouncy };
        let rolly = if obj.rolly == 0.0 { LINE_ROLLY_DEFAULT } else { obj.rolly };
        let anx = n[0].abs();
        if (anx as f64) > 0.9900000095367432 {
            p5 = (x(p5) * X::d(5.0)).r();
        }
        if p5 <= rolly {
            // roll: keep tangential velocity
            self.frames = 0;
            let f = ((x(v[1]) * n[1]) + (x(v[0]) * n[0])).r();
            let tx = (x(n[0]) * f).r();
            let ty = (x(f) * n[1]).r();
            let ux = sub(v[0], tx).r();
            let uy = sub(v[1], ty).r();
            let g = ((x(n[1]) * h.angv[1]) + (x(n[0]) * h.angv[0])).r();
            if g <= 0.0 {
                h.angv = [0.0, 0.0];
            } else {
                let g2 = ((x(n[0]) * h.angv[0]) + (x(n[1]) * h.angv[1])).r();
                h.angv = [(x(n[0]) * g2).r(), (x(g2) * n[1]).r()];
            }
            self.vel = [((x(h.svel[0]) + ux) + h.angv[0]).r(), ((x(h.svel[1]) + uy) + h.angv[1]).r()];
        } else {
            let d1 = ((x(v[1]) * n[1]) + (x(v[0]) * n[0])).r();
            if d1 < 0.0 {
                let d = ((x(v[0]) * n[0]) + (x(v[1]) * n[1])).r();
                let tx = (x(n[0]) * d).r();
                let ty = (x(d) * n[1]).r();
                let t2x = (x(tx) * X::d(2.0)).r();
                let t2y = (X::d(2.0) * ty).r();
                let ux = sub(v[0], t2x).r();
                let uy = sub(v[1], t2y).r();
                v[0] = (x(ux) * bouncy).r();
                v[1] = (x(uy) * bouncy).r();
            }
            let sx = (x(h.svel[0]) + h.angv[0]).r();
            let sy = (x(h.svel[1]) + h.angv[1]).r();
            let k = (x(bouncy) + X::d(1.0)).r();
            let mx = (x(sx) * k).r();
            let my = (x(k) * sy).r();
            let dd = ((x(n[1]) * my) + (x(n[0]) * mx)).r();
            if dd > 0.0 {
                self.move_by([mx, my]);
            }
            v[0] = (x(mx) + v[0]).r();
            v[1] = (x(my) + v[1]).r();
            self.vel = *v;
            let mbv = obj.maxbouncevel;
            if 0.0 < mbv {
                let m2 = (x(mbv) * mbv).r();
                let s2 = (x(self.vel[1]) * self.vel[1]) + (x(self.vel[0]) * self.vel[0]);
                if (m2 as f64) < s2.0 {
                    let nn = normalize(self.vel);
                    self.vel = [(x(nn[0]) * mbv).r(), (x(mbv) * nn[1]).r()];
                }
            }
        }
        if obj.ballstopreset { self.stuck = 0; }
        true
    }
}

/// A board: static geometry + the ball(s).
#[derive(Clone)]
pub struct Board {
    /// sorted indices of the objects tick_objects must visit: movers, pegs once hit (PegInfo+0x18 != 0),
    /// objects with a removal countdown. Every other object's update is a no-op.
    pub ticking: Vec<u32>,
    pub scratch: StepScratch,
    pub sapx: std::sync::Arc<SapIndex>,
    pub objs: Vec<Obj>,
    pub movers: Vec<Mover>,
    pub width: i32,  // Board+0x1b0
    pub height: i32, // Board+0x1b4
    /// LogicMgr+0x34c list: PegInfo objects in first-hit order this shot (count = +0x354)
    pub hit_list: Vec<usize>,
    /// LogicMgr+0xc: ticks until the next hit-peg pop (stuck-ball clearing)
    pub pop_delay: i32,
    /// Ball+0x15c: teleport target object (uid) whose position the ball takes when unfrozen
        /// LogicMgr+0x38 shot timer (FUN_00472f60), -2 at fire (FUN_00473d90)
    pub shot_timer: i32,
    /// LogicMgr+0xf7: forced pop round pending (consumed by FUN_00466df0 when the delay is 0)
    pub force_pop: bool,
    /// PegInfo objects in first-hit order this shot (never popped; for scoring)
    pub shot_hits: Vec<usize>,
    /// Board+0x1c4 counter at each first hit (parallel to `shot_hits`; free balls drop into the tube then)
    pub shot_hit_bticks: Vec<i32>,
    /// Character power (LogicMgr+0x1c4): see `powers` module docs. 0 = none.
    pub power: u8,
    /// LogicMgr+0xff: the flipper power's first activation (peg clearing) has happened
    pub flippers_init: bool,
    /// first hits since the last queued-contact pass (for BallHitPeg-time effects)
    /// (ball index, object index, is_hat_ball, forced_power_hit) first hits queued for BallHitPeg side effects
    new_hits: Vec<(usize, usize, bool, bool)>,
    /// Pyramid power active (LogicMgr+0x1f4 > 0): kept so mid-shot power activations compute amp_target accurately
    pub pyramid_on: bool,
    /// Allow `movetoggle` in AI mode (set on per-AIBall board clones in `zen.rs`)
    pub ai_movetoggle: bool,
    cur_ball: usize,
    pair_sap: Vec<(u32, u32, u8)>,  // ball-ball sweep-and-prune state (lower id, higher id)
    next_ball_id: u32,
    /// Lucky Spin "Random Powerup" pick (FUN_00451ac0), set from LogicMgr state by the caller
    /// Lucky Spin random-power list (oracle::lucky_list) and this shot's LogicMgr+0x2c/+0x30 (rolled at fire).
    pub lucky_list: Vec<u8>,
    pub lucky_ctr: Option<[u32; 2]>,
    /// LogicMgr+0x360 at fire: remaining oranges (Flower Power count base; shrinks with this shot's orange hits)
    pub orange_total: i32,
    /// number of objects in the base level file (board-list indices below this are base objects)
    pub n_base: usize,
    /// LogicMgr+0xf6: Extreme Fever started (last orange hit)
    pub fever_on: bool,
    /// value of the fever bonus hole the ball fell into this shot (0 = none)
    pub fever_hole: i32,
    /// LogicMgr+0xb8: every PegInfo contact this shot as BallHitPeg sees it (btick, peg type, ball
    /// position after the board update)
    pub contact_log: Vec<(i32, u8, [f32; 2])>,
    /// queued PegInfo contacts for the end-of-tick BallHitPeg pass: (ball, object, first hit)
    /// (ball, object, first hit, from the ball's Magic Hat)
    pending_contacts: Vec<(usize, usize, bool, bool)>,
    /// set while the hat balls' contacts run (`pending_contacts` .3)
    hat_contact: bool,
    pending_bucket: Vec<usize>,
    /// lid contacts by a fireball, handled in the LogicMgr pass (FUN_004704a0): (ball, object)
    pending_lid: Vec<(usize, usize)>,
    /// flipper contacts queued for the LogicMgr pass: (ball, number of PegInfo contacts queued before it)
    pending_flipper: Vec<(usize, usize)>,
    /// LogicMgr+0x48: first peg hits right after a flipper touch this shot (4 → style 1)
    pub flipper_hits: i32,
    /// BallHitPeg(..., 4th argument = 1) from a power (Space Blast, Flower Power): skips the style log
    forced_contact: bool,
    /// LogicMgr+0x320: free-ball catch streak (every 10th catch: style 14)
    pub catch_streak: i32,
    /// LogicMgr+0x19: a ball was caught this shot
    pub caught_this_shot: bool,
    /// style-shot bonuses awarded this shot: (FUN_00404b70 style id, value); added to the total only
    pub styles: Vec<(u8, i32)>,
    /// Lucky Spin wheel results this shot (0 random power, 1 magic hat, 2 score tripler, 3 extra ball)
    pub lucky_results: Vec<u32>,
    /// Board ticks (`btick`) when each Lucky Spin in `lucky_results` occurred (for exact Extra Ball tube drops)
    pub lucky_bticks: Vec<i32>,
    /// one-shot search only (never set by replays): force the Lucky Spin wheel result (1 = Magic Hat)
    pub force_lucky: Option<u32>,
    /// wheel updates of each spin this shot (slot::lucky_wheel_steps; the board is frozen meanwhile)
    pub lucky_steps: Vec<i32>,
    /// LogicMgr+0x68 at fire: Score Tripler active for this shot (Lucky Spin result 2 last turn)
    pub triple: bool,
    spawned: Vec<Ball>,
    /// Board+0x1c4 board update counter (parity gates FUN_00440830)
    pub btick: i32,
    /// LogicMgr+0x134: bucket mover amplitude target (0 = idle). 0x104 normal, 0x82 flippers, 0xc8 pyramid
    pub amp_target: i32,
    /// LogicMgr+0x204[player]: spooky respawns left
    pub spooky_left: i32,
    /// TAS RNG seed (PEGGLE_RNG_SEED) for keyed random effects (see rng.rs)
    pub rng_seed: u32,
    /// Zen AI search world (AIMgr, see zen.rs): no LogicMgr contact handling, no out-of-bounds loss
    pub ai: bool,
    /// slow-mo look-ahead step (with `ai`): no hit flags, bucket / fever-hole / style side effects
    peek: bool,
    /// powers activated this shot (FUN_0046edf0 calls, in order; Lucky Spin's random power included)
    pub activations: Vec<u8>,
    /// FUN_004582f0 calls this shot (each one consumes a hook colour-RNG reseed)
    pub flipper_applies: u32,
    /// hook colour-RNG call index (PEGGLE_RNG_SEED reseeds before FUN_00457960/458110/457dc0/4582f0)
    pub rng_n: u32,
}

const SHOT_FORCE_POP: i32 = 1500; // DAT_00649ac4
const SHOT_HIT_CAP: i32 = 1000; // DAT_00649ac8
const SHOT_POP_CAP: i32 = 500; // DAT_00649acc

/// Events produced during a step (for scoring / analysis).
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Hit(usize),
    /// Ball centre entered a hole (FUN_004781c0 → callback FUN_0045ad30). obj index.
    Captured(usize),
    /// Ball entered a teleport hole (LogicMgr FUN_00470230 → Ball FUN_0047a350). obj index.
    Teleport(usize),
    Lost,
}

fn bbox_overlap(a: &[f32; 4], b: &[f32; 4]) -> bool {
    // FUN_004326e0
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

fn shift_bbox(b: &mut [f32; 4], d: [f32; 2]) {
    b[0] = add(d[0], b[0]).r();
    b[2] = add(d[0], b[2]).r();
    b[1] = add(d[1], b[1]).r();
    b[3] = add(d[1], b[3]).r();
}

impl Board {
    pub fn from_levels(level: &Level, base: Option<&Level>) -> Board {
        let mut objs = vec![];
        let mut movers = vec![];
        let push_level = |lvl: &Level, lno: u32, objs: &mut Vec<Obj>, movers: &mut Vec<Mover>| {
            let mbase = movers.len();
            // teleport targets are kept (non-collidable) so their movers keep ticking
            let targets: Vec<usize> = lvl.objects.iter().filter_map(|o| match &o.shape {
                Shape::Hole(h) => h.target, _ => None }).collect();
            movers.extend(lvl.movers.iter().map(|d| {
                let mut m = Mover::new(d);
                m.sub = d.sub.map(|(lx, ly, si)| (si + mbase, [lx, ly]));
                m
            }));
            // inline (non-top-level) teleport targets are loaded too
            let extra: Vec<Option<usize>> = targets.iter().filter(|t| !lvl.top.contains(&Some(**t))).map(|t| Some(*t)).collect();
            for (ti, r) in lvl.top.iter().chain(extra.iter()).enumerate() {
                let top_ix = if ti < lvl.top.len() { ti as i32 } else { -1 };
                let Some(r) = r else { continue };
                let o = &lvl.objects[*r];
                let p = &o.phys;
                // Pegs saved in a hit state (PegInfo+0x14 set, e.g. TheAmoeban.dat: collidable/visible
                // cleared, +0x18 large) are restored when the level starts.
                let was_hit = p.peg.as_ref().is_some_and(|q| q.b14);
                // non-collidable objects are still loaded when they own a mover (it may be another
                // mover's sub-mover) or are teleport targets
                if !p.collidable && !was_hit && !targets.contains(r) && p.mover.is_none() && p.peg.is_none() { continue; }
                let geom = match &o.shape {
                    Shape::Circle(c) => Geom::Peg { pos: [c.x, c.y], r: c.radius, vel: [0.0; 2] },
                    &Shape::Line { x1, y1, x2, y2, f_fc, f_100, b111 } => {
                        let mut l = Line::new(x1, y1, x2, y2);
                        l.oneside = b111;
                        l.vel = [f_fc, f_100];
                        Geom::Line(l)
                    }
                    Shape::Poly(_) | Shape::Brick { .. } => {
                        let (pd, verts) = match &o.shape {
                            Shape::Poly(pd) => (pd, pd.verts.clone()),
                            Shape::Brick { poly, brick } => (poly, crate::brick::brick_verts(brick)),
                            _ => unreachable!(),
                        };
                        if verts.len() < 2 { continue; }
                        let oneside = pd.normaldir != 0;
                        // FUN_00484730: unrotated build, else FUN_0047d5b0(rotation)
                        let (lines, offs) = build_poly_lines(&verts, pd.rotation, [pd.x, pd.y], oneside);
                        let closed = verts.first() == verts.last();
                        let convex = closed && is_convex(&verts);
                        Geom::Poly { lines, offs, inside_test: pd.normaldir == 1 && closed, convex, angvel: pd.f120,
                                     center: [pd.x, pd.y], verts, rot: pd.rotation, oneside, dirty_rot: false,
                                     stale: false, pvel: [0.0; 2] }
                    }
                    Shape::Hole(h) => {
                        let teleport = h.target.map(|t| ((lno << 16) | t as u32, h.i0e8));
                        Geom::Hole { pos: [h.x, h.y], w: h.i104, h: h.i108, ellipse: h.b10c, teleport }
                    }
                    _ => continue,
                };
                let bbox = match &geom {
                    Geom::Peg { pos, r, .. } => [sub(pos[0], *r).r(), sub(pos[1], *r).r(), (x(*r) + pos[0]).r(), (x(pos[1]) + *r).r()],
                    Geom::Line(l) => l.bbox,
                    Geom::Poly { lines, .. } => lines_bbox(lines),
                    // FUN_00475580 (SetSize): pos (+0xf4/+0xf8) ± size (+0x104/+0x108) × 0.5. Checked on the 'MOV ' records
                    // (body +0x14..+0x20): 721/723 first records match; moves translate it (FUN_004754f0), exact on
                    // 99.8% of 1.09M steps, the rest are occasional rebuilds of this formula (caller not found;
                    // the bbox only feeds the broadphase, at 1 ulp)
                    Geom::Hole { pos, w, h, .. } => {
                        let hw = (X::d(*w as f64) * X::d(0.5)).r();
                        let hh = (X::d(*h as f64) * X::d(0.5)).r();
                        [sub(pos[0], hw).r(), sub(pos[1], hh).r(), add(pos[0], hw).r(), add(pos[1], hh).r()]
                    }
                };
                objs.push(Obj {
                    geom, bouncy: p.bouncy, rolly: p.rolly, maxbouncevel: p.maxbouncevel,
                    ballstopreset: p.ballstopreset, collidable: p.id != "flipper" && (p.collidable || was_hit),
                    visible: p.id != "flipper" && (p.collidable || was_hit),
                    has_peginfo: p.peg.is_some(),
                    bbox, src_index: *r, hit: false,
                    mover: p.mover.map(|m| m + mbase), counter: p.i4c, moving: p.moving,
                    crumble: p.peg.as_ref().is_some_and(|q| q.crumble),
                    contacts: p.peg.as_ref().map_or(0, |q| q.u1c as i32),
                    settled: false,
                    tidx: (i32::MIN, 0),
                    since_hit: if was_hit { 0 } else { p.peg.as_ref().map_or(0, |q| q.i18) },
                    remove_timer: 0,
                    flipper: p.id == "flipper",
                    bucketish: p.id.starts_with("freeball") || p.id == "bumperhole",
                    freeball: p.id.starts_with("freeball"),
                    bumper: if p.id == "bumperhole" { Some(p.subid) } else { None },
                    cover: p.id == "freeballcover",
                    fever: p.id.starts_with("fever"),
                    hole_value: match &o.shape { Shape::Hole(h) => h.i0f0, _ => 0 },
                    fg2: p.foreground2,
                    sap: 0,
                    uid: (lno << 16) | *r as u32,
                    movetoggle: p.logic.eq_ignore_ascii_case("movetoggle"),
                    move_timer: 0,
                    peg_type: p.peg.as_ref().map_or(0, |q| q.peg_type),
                    fflags: p.flipperflags,
                    top_ix,
                    variable: p.peg.as_ref().is_some_and(|q| q.variable),
                });
            }
        };
        push_level(level, 0, &mut objs, &mut movers);
        if let Some(b) = base { push_level(b, 1, &mut objs, &mut movers); }
        let ticking = objs.iter().enumerate().filter(|(_, o)| o.mover.is_some() || o.remove_timer != 0 || (o.has_peginfo && o.since_hit != 0))
            .map(|(i, _)| i as u32).collect();
        let sapx = std::sync::Arc::new(build_sap_index(&mut objs, &movers));
        Board { sapx, scratch: StepScratch::default(), ticking, objs, movers, width: 650, height: 560, hit_list: vec![], pop_delay: 0, shot_timer: -2, force_pop: false, shot_hits: vec![], shot_hit_bticks: vec![],
                power: 0, flippers_init: false, new_hits: vec![], pyramid_on: false, ai_movetoggle: false, cur_ball: 0, pair_sap: vec![], next_ball_id: 1, spawned: vec![], lucky_list: vec![], lucky_ctr: None, lucky_results: vec![], lucky_bticks: vec![], force_lucky: None, lucky_steps: vec![], triple: false, orange_total: 25, n_base: base.map_or(0, |b| b.objects.len()), fever_on: false, fever_hole: 0, contact_log: vec![], pending_contacts: vec![], hat_contact: false, pending_bucket: vec![], pending_lid: vec![], pending_flipper: vec![], flipper_hits: 0, forced_contact: false, catch_streak: 0, caught_this_shot: false, styles: vec![], btick: 0, amp_target: 0, spooky_left: 0, rng_seed: 0, ai: false, peek: false, activations: vec![], flipper_applies: 0, rng_n: 0 }
    }

    /// Per-tick object updates (vt+0x58 → FUN_00479a30) for objects with movers: advance the
    /// frame counter, evaluate the mover, SetPos(new) then SetVel(new - old).
    fn add_ticking(&mut self, i: usize) {
        if let Err(p) = self.ticking.binary_search(&(i as u32)) { self.ticking.insert(p, i as u32); }
    }

    pub fn tick_objects(&mut self) { self.tick_objects_where(0) }

    /// `group`: 0 = everything (a real tick); 1 = only base-level objects; 2 = only level objects
    /// (used to bring the two independently-counting groups to their real counters).
    pub fn tick_objects_where(&mut self, group: u8) {
        if group == 0 {
            // LogicMgr::Update runs before the object updates: FUN_00440830 every other board tick
            self.btick += 1;
            if self.amp_target != 0 && self.btick % 2 != 0 { self.ease_bucket(); }
        }
        let no_fast = *NO_FAST;
        let mut drop_any = false;
        let all: Vec<u32>;
        let list = if no_fast { all = (0..self.objs.len() as u32).collect(); &all } else { &self.ticking };
        for &oi in list {
            let o = &mut self.objs[oi as usize];
            if group != 0 && ((o.uid >> 16 == 1) != (group == 1)) { continue; }
            // FUN_00479a30 head: removal countdown → FUN_004769f0(0)
            if o.remove_timer != 0 {
                o.remove_timer -= 1;
                if o.remove_timer == 0 { o.collidable = false; o.visible = false; }
            }
            // FUN_00479680 PegInfo::Update: crumble pegs vanish 97 ticks after their 3rd contact
            if o.has_peginfo && o.since_hit != 0 && (o.since_hit < 20 || o.crumble || no_fast) {
                // PegInfo+0x18 is only ever compared with 20 (brick combo): past 20 the count is not needed
                o.since_hit += 1;
                if o.crumble && o.contacts >= 3 && o.contacts < 100 {
                    o.contacts += 1;
                    if o.contacts == 100 { o.collidable = false; o.visible = false; } // FUN_004769f0(0)
                }
            }
            // nothing left to do for this object until a contact / mover start re-adds it (`add_ticking`)
            let peg_idle = !(o.has_peginfo && o.since_hit != 0 && (o.since_hit < 20 || o.crumble));
            let Some(mi) = o.mover else {
                if o.remove_timer == 0 && peg_idle { drop_any = true; }
                continue;
            };
            let sub_pos = self.movers[mi].sub.map(|(si, _)| self.movers[si].pos);
            if o.settled && !o.moving && sub_pos.is_none() && !no_fast {
                if o.move_timer != 0 { o.move_timer -= 1; if o.move_timer == 0 { o.moving = false; } }
                if o.move_timer == 0 && o.remove_timer == 0 && peg_idle { drop_any = true; }
                continue;
            }
            let m = &mut self.movers[mi];
            let old = m.pos;
            let mut tab_bbox = None;
            let mut tab_w = None;
            if o.moving || sub_pos.is_some() {
                if o.moving { o.counter += 1; }
                if let Some(sp) = sub_pos { m.follow_sub(sp); }
                match self.sapx.mtab[oi as usize].as_ref() {
                    Some(t) if !no_fast && sub_pos.is_none() && o.counter >= t.p && t.version == m.version => {
                        let j = if o.tidx.0 == o.counter - 1 { let j = o.tidx.1 + 1; if j == t.p { 0 } else { j } } else { o.counter % t.p };
                        o.tidx = (o.counter, j);
                        let e = &t.e[j as usize];
                        tab_w = Some((t.e[if j == 0 { t.p - 1 } else { j - 1 } as usize].1, t.w[j as usize]));
                        m.pos = e.0;
                        m.angle = e.1;
                        tab_bbox = Some(e.2);
                    }
                    _ => m.eval(o.counter),
                }
            }
            let new = m.pos;
            let dv = [sub(new[0], old[0]).r(), sub(new[1], old[1]).r()];
            let mut still = dv[0] == 0.0 && dv[1] == 0.0;
            match &mut o.geom {
                Geom::Peg { pos, r, vel } => {
                    // FUN_00474e60
                    *pos = new;
                    o.bbox = [sub(new[0], *r).r(), sub(new[1], *r).r(), (x(*r) + new[0]).r(), (x(*r) + new[1]).r()];
                    *vel = dv;
                }
                Geom::Poly { lines, offs, center, rot, angvel, dirty_rot, stale, pvel, .. } => {
                    // FUN_00476d70 (bbox shift, dirty) + lazy FUN_0047bb90 (lines rebuilt from pos)
                    let d = [sub(new[0], center[0]).r(), sub(new[1], center[1]).r()];
                    if d[0] != 0.0 || d[1] != 0.0 { still = false; }
                    *center = new;
                    shift_bbox(&mut o.bbox, d);
                    *stale = true; // lines → center + offs, vel dv (FUN_0047bcd0): see `Poly::stale`
                    *pvel = dv;
                    if no_fast {
                        for (l, of) in lines.iter_mut().zip(offs.iter()) { l.set_pos(add(center[0], of[0]).r(), add(of[1], center[1]).r()); l.vel = dv; }
                        *stale = false;
                    }
                    // Poly::Update FUN_0047bd50: follow the mover angle (0x64)
                    let a = m.angle;
                    if a != *rot {
                        *angvel = match tab_w { Some((pa, w)) if pa.to_bits() == rot.to_bits() => w, _ => wrap_angvel(a, *rot) };
                        *rot = a;
                        o.bbox = match tab_bbox { Some(b) => b, None => rotated_bbox(&self.sapx.verts[oi as usize], a, *center) }; // FUN_0047a010
                        *dirty_rot = true;
                        still = false;
                    } else {
                        *angvel = 0.0;
                    }
                }
                Geom::Hole { pos, .. } => {
                    // FUN_00475540 → FUN_004754f0 translate by (new - pos)
                    let d = [sub(new[0], pos[0]).r(), sub(new[1], pos[1]).r()];
                    if d[0] != 0.0 || d[1] != 0.0 { still = false; }
                    pos[0] = add(d[0], pos[0]).r();
                    pos[1] = add(d[1], pos[1]).r();
                    shift_bbox(&mut o.bbox, d);
                }
                Geom::Line(l) => {
                    l.set_pos(new[0], new[1]);
                    l.vel = dv;
                }
            }
            o.settled = still;
            // FUN_00479a30 tail
            if o.move_timer != 0 {
                o.move_timer -= 1;
                if o.move_timer == 0 { o.moving = false; }
            }
        }
        if drop_any && !no_fast {
            let objs = &self.objs;
            let movers = &self.movers;
            self.ticking.retain(|&i| {
                let o = &objs[i as usize];
                let peg_idle = !(o.has_peginfo && o.since_hit != 0 && (o.since_hit < 20 || o.crumble));
                let rest = o.remove_timer == 0 && peg_idle;
                match o.mover {
                    None => !rest,
                    Some(mi) => !(rest && o.settled && !o.moving && o.move_timer == 0 && movers[mi].sub.is_none()),
                }
            });
        }
    }

    /// Poly vt+0x70 FUN_0047b9d0 ("movetoggle"), from LogicMgr FUN_004704a0 for each queued contact
    /// with an object that has a logic string. A resting mover runs to phase 0.5 if it is within 10
    /// ticks of the cycle start, else back to phase 0 (FUN_004799a0 → `move_timer`).
    fn movetoggle(&mut self, i: usize) {
        let o = &self.objs[i];
        if !o.movetoggle || o.moving { return; }
        let Some(mi) = o.mover else { return };
        let m = &self.movers[mi];
        let period = m.def.time as i32;
        if period <= 0 { return; }
        let target = if o.counter % period < 10 { 0.5 } else { 0.0 };
        let n = m.ticks_to_phase(o.counter, target);
        if n < 1 { return; }
        let o = &mut self.objs[i];
        o.moving = true;
        o.move_timer = n;
        self.add_ticking(i);
    }

    /// Shot score factor at shot end: ×3 while the Score Tripler flag LogicMgr+0x68 is set (it was set
    /// at fire, or a Lucky Spin this shot landed on result 2 — FUN_0045e830 sets it immediately).
    pub fn score_factor(&self) -> i64 {
        if self.triple || self.lucky_results.contains(&2) { 3 } else { 1 }
    }

    /// LogicMgr+0x360 now: oranges at fire minus this shot's orange first hits.
    pub fn oranges_left(&self) -> i32 {
        self.orange_total - self.shot_hits.iter().filter(|&&i| self.objs[i].peg_type == crate::score::ORANGE).count() as i32
    }

    /// Near-win slow-mo look-ahead targets (FUN_00410ae0 gate + FUN_00409d50), empty when the gate is shut:
    /// with one orange left, that orange; Splork (power 5) with <= 5 oranges left (ModVal Board.cpp100): the
    /// first two unhit greens with every unhit orange within 100 px (Board.cpp96, FUN_00409b40) — a Space Blast
    /// there clears the board; Tula (power 9, flower count +0xfb off) with one orange left: also the first two
    /// unhit greens.
    pub fn slowmo_targets(&self, out: &mut Vec<usize>) {
        out.clear();
        let left = self.oranges_left();
        let splork = self.power == 5 && left <= 5;
        if left != 1 && !splork { return; }
        let live = |o: &Obj, t: u8| o.has_peginfo && o.peg_type == t && !o.hit && o.collidable;
        if left == 1 {
            if let Some(i) = self.objs.iter().position(|o| live(o, crate::score::ORANGE)) { out.push(i); }
        }
        if splork || (self.power == 9 && left == 1) {
            let pos = |o: &Obj| match &o.geom { Geom::Peg { pos, .. } | Geom::Hole { pos, .. } => *pos, Geom::Poly { center, .. } => *center, Geom::Line(l) => [l.x1, l.y1] };
            let mut n = 0;
            for (i, g) in self.objs.iter().enumerate() {
                if n == 2 { break; }
                if !live(g, crate::score::GREEN) { continue; }
                if splork {
                    let gp = pos(g);
                    let far = self.objs.iter().any(|o| live(o, crate::score::ORANGE) && {
                        let p = pos(o);
                        let (dx, dy) = (p[0] - gp[0], p[1] - gp[1]);
                        10000.0 < dx * dx + dy * dy
                    });
                    if far { continue; }
                }
                out.push(i);
                n += 1;
            }
        }
    }

    /// FUN_00454dc0 + FUN_00454f90 predictor collision step (used by FUN_0045eca0 spooky trials and
    /// FUN_00409d50 slow-mo look-ahead): ±75 broadphase cache (skipping all PegInfo objects when
    /// `fireball` is set: CollisionMgr+0x78), instant delay-0 teleport, then swept CCD (FUN_0047f160)
    /// over pegs/lines/polys sorted by `(type, id)`, then move if `!moved`.
    fn predict_fill_cache(&self, b: &Ball, fireball: bool, bounds: &mut [f32; 4], cache: &mut Vec<usize>) {
        const PAD: f32 = 75.0; // ModVal CollisionMgr.cpp81
        *bounds = [sub(b.bbox[0], PAD).r(), sub(b.bbox[1], PAD).r(), add(b.bbox[2], PAD).r(), add(b.bbox[3], PAD).r()];
        cache.clear();
        for (i, o) in self.objs.iter().enumerate() {
            if fireball && o.has_peginfo { continue; }
            let ob = &o.bbox;
            if ob[0] <= bounds[2] && bounds[0] < ob[2] && ob[1] <= bounds[3] && bounds[1] < ob[3] {
                cache.push(i);
            }
        }
    }

    fn predict_step(&mut self, b: &mut Ball, fireball: bool, bounds: &mut [f32; 4], cache: &mut Vec<usize>, events: &mut Vec<Event>) {
        if bounds[2] < b.bbox[2] || b.bbox[0] < bounds[0] || b.bbox[1] < bounds[1] || bounds[3] < b.bbox[3] {
            self.predict_fill_cache(b, fireball, bounds, cache);
        }
        let mut cands: Vec<usize> = vec![];
        for &i in cache.iter() {
            let o = &self.objs[i];
            if !o.collidable { continue; }
            let ob = &o.bbox;
            if !(ob[0] <= b.bbox[2] && b.bbox[0] < ob[2] && ob[1] <= b.bbox[3] && b.bbox[1] < ob[3]) { continue; }
            match o.geom {
                Geom::Hole { pos, w, h, ellipse, teleport: Some((uid, _)) } => {
                    if hole_test(pos, w, h, ellipse, b.pos) {
                        if let Some(p) = self.obj_pos(uid) { b.set_pos_full(p); }
                    }
                }
                Geom::Hole { .. } => {}
                Geom::Peg { .. } | Geom::Line(_) | Geom::Poly { .. } => cands.push(i),
            }
        }
        if !cands.is_empty() {
            cands.sort_by_key(|&i| (match self.objs[i].geom { Geom::Hole { .. } => 1, Geom::Peg { .. } => 3, Geom::Line(_) => 4, Geom::Poly { .. } => 5 }, i));
            self.ccd(b, &cands, events);
        }
        if !b.moved {
            let v = b.vel;
            b.move_by(v);
        }
    }

    /// FUN_00409d50 for live ball `b`: a copy of the ball (hit callbacks off, movers frozen) runs 4 steps
    /// (ModVal Board.cpp97) via FUN_00454f90; after each it sweeps one step ahead against the targets with
    /// its radius + 10 (FUN_0047f4c0). True = the near-win slow-mo starts. Stops when the copy falls below the board.
    pub fn slowmo_peek(&mut self, b: &Ball, targets: &[usize]) -> bool {
        // reach of 4 moves + the final sweep (|v| <= 15) plus the margin: skip the copy when no target is near
        let reach = 5.0 * 15.0 + b.r + 10.0 + 1.0;
        let near = |i: usize| {
            // a moved / rotated poly's lines and bbox are rebuilt lazily: no early-out on a stale one
            if let Geom::Poly { stale, dirty_rot, .. } = &self.objs[i].geom { if *stale || *dirty_rot { return true; } }
            let bb = self.objs[i].bbox;
            b.pos[0] >= bb[0] - reach && b.pos[0] <= bb[2] + reach && b.pos[1] >= bb[1] - reach && b.pos[1] <= bb[3] + reach
        };
        if !targets.iter().any(|&i| near(i)) { return false; }
        let (ai, cur) = (self.ai, self.cur_ball);
        self.ai = true;
        self.peek = true;
        let mut c = Ball::new(b.pos, b.vel);
        c.r = b.r;
        c.set_pos(b.pos);
        // FUN_00409d50: CollisionMgr+0x78 = ball->fireball, while the copy itself is a normal Ball (+0x161 = 0)
        let mut bounds = [0.0f32; 4];
        let mut cache = vec![];
        self.predict_fill_cache(&c, b.fireball, &mut bounds, &mut cache);
        let mut ev = vec![];
        let mut fired = false;
        'steps: for _ in 0..4 {
            c.update();
            ev.clear();
            self.predict_step(&mut c, b.fireball, &mut bounds, &mut cache, &mut ev);
            if ((self.height + 20) as f32) < c.pos[1] { break; }
            let r = (x(c.r) + 10.0).r();
            for &i in targets {
                self.refresh_poly(i);
                let mut toi: f32 = 1.0;
                let hit = match &self.objs[i].geom {
                    Geom::Peg { pos, r: pr, .. } => { let mut out = [0.0; 2]; sweep_peg(*pos, *pr, c.pos, c.vel, r, &mut toi, &mut out) }
                    Geom::Poly { lines, inside_test, convex, .. } => {
                        let (mut h, mut fi) = (Hit::default(), true);
                        poly_collide(lines, *inside_test, *convex, self.objs[i].bbox, c.pos, c.vel, r, &mut toi, &mut h, &mut fi)
                    }
                    _ => false,
                };
                if hit { fired = true; break 'steps; }
            }
        }
        self.ai = ai;
        self.peek = false;
        self.cur_ball = cur;
        fired
    }

    /// FUN_004799a0(obj, 0.5, 0): run the mover until it reaches phase 0.5, then stop.
    fn run_to_half(&mut self, i: usize) {
        let Some(mi) = self.objs[i].mover else { return };
        let n = self.movers[mi].ticks_to_phase(self.objs[i].counter, 0.5);
        if n < 1 { return; }
        self.objs[i].moving = true;
        self.objs[i].move_timer = n;
        self.add_ticking(i);
    }

    /// FUN_00458ae0 (LogicMgr): Extreme Fever. "fever*" objects (bonus holes + round dividers) become
    /// collidable + visible and slide up to mover phase 0.5; "freeball*" / "bumperhole" objects stop
    /// colliding and (unless foreground2) get a type -11 mover (amp 100, period 20, centred at the current
    /// position, counter 1) that slides them down to phase 0.5; foreground2 ones just vanish.
    pub fn start_fever(&mut self) {
        self.fever_on = true;
        if std::env::var_os("SIM_DEBUG").is_some() { eprintln!("fever starts btick {}", self.btick); }
        for i in 0..self.objs.len() {
            if self.objs[i].fever {
                self.objs[i].collidable = true; // FUN_004769f0(1): +0x24/+0x25
                self.objs[i].visible = true;
                self.run_to_half(i);
            } else if self.objs[i].bucketish {
                self.objs[i].collidable = false;
                match self.objs[i].mover {
                    Some(mi) if !self.objs[i].fg2 => {
                        let m = &mut self.movers[mi];
                        m.version += 1;
                        m.def.mover_type = -11;
                        m.def.f20 = 0.0;
                        m.def.s18 = 100;
                        m.def.time = 20;
                        m.center = m.pos;
                        self.objs[i].counter = 1;
                        self.run_to_half(i);
                    }
                    _ => { self.objs[i].visible = false; }
                }
            }
        }
    }

    /// FUN_004612e0: enable/disable the flipper objects (flipper power; off by default, re-applied
    /// at each turn start by FUN_00461560 from the power's remaining-turn count).
    pub fn set_flippers(&mut self, on: bool) {
        for o in self.objs.iter_mut() { if o.flipper { o.collidable = on; } }
    }

    /// The flipper power is on (its objects collide): LogicMgr+0x1e4 >= 1, so a click flips.
    pub fn flippers_on(&self) -> bool { self.objs.iter().any(|o| o.flipper && o.collidable) }

    /// FUN_0045ea70(down) (LogicMgr::MouseDown → 1, MouseUp FUN_0045ebb0 → 0, while the flipper power is on):
    /// for each "flipper" object with a mover, c = counter % period; press: past half a period the counter is
    /// mirrored to period - c (FUN_00479980), then the mover runs to phase 0.5 (FUN_004799a0(0.5, 0): moving +
    /// ticks left); release: with c >= 1, it runs to phase 1.0 (back to rest).
    pub fn flip(&mut self, down: bool) {
        for i in 0..self.objs.len() {
            if !self.objs[i].flipper { continue; }
            let Some(mi) = self.objs[i].mover else { continue };
            let period = self.movers[mi].def.time as i32;
            if period < 1 { continue; }
            let c = self.objs[i].counter % period;
            let target = if down {
                if period / 2 < c { self.objs[i].counter = period - c; self.objs[i].settled = false; }
                0.5
            } else {
                if c < 1 { continue; }
                1.0
            };
            let n = self.movers[mi].ticks_to_phase(self.objs[i].counter, target);
            if n < 1 { continue; }
            self.objs[i].moving = true;
            self.objs[i].move_timer = n;
            self.add_ticking(i);
        }
    }

    /// Reset per-shot logic state (call at shot start).
    pub fn begin_shot(&mut self) {
        self.hit_list.clear();
        self.pair_sap.clear();
        self.next_ball_id = 1;
        self.spawned.clear();
        self.lucky_results.clear();
        self.lucky_bticks.clear();
        self.lucky_steps.clear();
        // FUN_0046ac70 (shot start) saves replay state, which syncs holes through Hole::SetSize
        // (FUN_00475580) rebuilding each hole's bbox from pos ± size/2 (exact 1-ulp reset at every launch).
        for o in self.objs.iter_mut() {
            if let Geom::Hole { pos, w, h, .. } = o.geom {
                let hw = (X::d(w as f64) * X::d(0.5)).r();
                let hh = (X::d(h as f64) * X::d(0.5)).r();
                o.bbox = [sub(pos[0], hw).r(), sub(pos[1], hh).r(), add(pos[0], hw).r(), add(pos[1], hh).r()];
            }
        }
        self.fever_hole = 0;
        self.contact_log.clear();
        self.pending_contacts.clear();
        self.pending_bucket.clear();
        self.pending_lid.clear();
        self.pending_flipper.clear();
        self.flipper_hits = 0;
        self.caught_this_shot = false;
        self.styles.clear();
        self.shot_timer = -2;
        self.force_pop = false;
        self.shot_hits.clear();
        self.shot_hit_bticks.clear();
        self.pop_delay = 0;
        self.activations.clear();
        self.flipper_applies = 0;
    }

    /// FUN_00466a00 (with LogicMgr+0x10 == 0): pop the oldest hit peg; it stops colliding 4 object
    /// updates later. Re-arms the delay while pegs remain.
    fn pop_hit(&mut self, balls: &mut [Ball]) {
        if self.hit_list.is_empty() { return; }
        let i = self.hit_list.remove(0);
        self.shot_timer = self.shot_timer.min(SHOT_POP_CAP); // FUN_00466a00
        self.objs[i].visible = false; // FUN_00466a00 hides (+0x25 = 0) immediately while remove_timer keeps collidable for 4 ticks
        if self.objs[i].collidable { self.objs[i].remove_timer = 4; self.add_ticking(i); }
        let rem = self.hit_list.len() as i32;
        if rem > 0 { self.pop_delay = (8 - rem / 10).clamp(1, 5); }
        for b in balls.iter_mut() { b.stuck = 0; } // FUN_00466a00 resets +0xe4 of every ball
    }

    /// One full game tick for a single-ball shot (balls spawned during the tick are dropped; use
    /// `frame_multi` for powers that spawn balls).
    pub fn frame(&mut self, b: &mut Ball, events: &mut Vec<Event>) -> bool {
        let mut v = vec![std::mem::take(b)];
        let alive = self.frame_multi(&mut v, events);
        *b = v.swap_remove(0);
        alive
    }

    /// One full game tick, in game order: LogicMgr::Update (pop delay, stuck check FUN_00466df0),
    /// Board::UpdatePhysics FUN_00410ae0 (object + ball updates, collision dispatcher FUN_0045add0,
    /// moves, out-of-bounds), then the queued contact events. Balls removed this tick get `gone`;
    /// balls spawned (Multiball) are appended. Returns false if the balls were removed because they
    /// are stuck with no hit pegs left (FUN_00410120).
    pub fn frame_multi(&mut self, balls: &mut Vec<Ball>, events: &mut Vec<Event>) -> bool {
        if self.pop_delay != 0 {
            self.pop_delay -= 1;
            if self.pop_delay == 0 { self.pop_hit(balls); }
        }
        // FUN_00472f60: shot timer; 1500 ticks without a new peg hit (then every 700) forces a pop round
        self.shot_timer += 1;
        if self.shot_timer >= SHOT_FORCE_POP && (self.shot_timer - SHOT_FORCE_POP) % 700 == 0 {
            self.force_pop = true;
            self.shot_timer = SHOT_POP_CAP;
        }
        // FUN_00466df0: only when every ball is stuck (+0xe4 >= 50) or a pop round is forced
        if self.pop_delay == 0 && (self.force_pop || balls.iter().filter(|b| !b.gone).all(|b| b.stuck >= 50)) {
            self.force_pop = false;
            for b in balls.iter_mut() { b.stuck = 0; }
            if self.hit_list.is_empty() {
                events.push(Event::Lost);
                for b in balls.iter_mut() { b.gone = true; }
                // FUN_00466df0 → FUN_00410120 removes the ball during LogicMgr::Update;
                // Board::UpdatePhysics (tick_objects) still runs on this tick.
                self.tick_objects();
                return false;
            }
            self.pop_hit(balls);
        }
        self.tick_objects();
        self.step_multi(balls, events);
        true
    }

    /// One 10ms physics tick for a single ball: Update, collide (FUN_0045add0 → FUN_0047f160),
    /// Move if no swept candidate, then out-of-bounds check (FUN_00410ae0).
    /// Call `tick_objects` first (objects update before collision in the same tick).
    /// Position of an object as returned by vt+0x78/0x7c (GetX/GetY).
    /// FUN_00440830: step the bucket-group movers' amplitude (mover+0x18) one unit toward
    /// LogicMgr+0x134 (once per object, in board order); clear the target when all have arrived.
    fn ease_bucket(&mut self) {
        let t = self.amp_target;
        let mut moved = false;
        for i in 0..self.objs.len() {
            if !self.objs[i].bucketish { continue; }
            let Some(mi) = self.objs[i].mover else { continue };
            let mv = &mut self.movers[mi];
            let d = &mut mv.def;
            if d.s18 != t && d.time > 0 {
                moved = true;
                mv.version += 1;
                d.s18 = if d.s18 > t { (d.s18 - 1).max(t) } else { (d.s18 + 1).min(t) };
            }
        }
        if !moved { self.amp_target = 0; }
    }

    /// The free-ball bucket (base hole with a mover) and the last moving level object.
    pub fn sync_refs(&self) -> (Option<usize>, Option<usize>) {
        let b = self.objs.iter().position(|o| matches!(o.geom, Geom::Hole { .. }) && o.moving && o.mover.is_some() && o.uid >> 16 == 1);
        let l = self.objs.iter().rposition(|o| o.uid >> 16 != 1 && o.moving && o.mover.is_some());
        (b, l)
    }

    /// Advance objects until the bucket counter equals `base` and the first moving level object's counter
    /// equals `level` (the base level persists across levels, so the two groups count independently).
    /// Returns false if a target is behind the sim (cannot rewind).
    pub fn sync_counters(&mut self, base: Option<i32>, level: Option<i32>) -> bool {
        let (bi, li) = self.sync_refs();
        let nb = match (bi, base) { (Some(i), Some(t)) => Some(t - self.objs[i].counter), _ => None };
        let nl = match (li, level) { (Some(i), Some(t)) => Some(t - self.objs[i].counter), _ => None };
        if nb.is_some_and(|n| n < 0) || nl.is_some_and(|n| n < 0) { return false; }
        if nb.is_some_and(|n| n > 10_000_000) || nl.is_some_and(|n| n > 10_000_000) { return false; }
        let common = match (nb, nl) { (Some(a), Some(b)) => a.min(b), (Some(a), None) => a, (None, Some(b)) => b, _ => 0 };
        for _ in 0..common { self.tick_objects(); }
        for _ in 0..nb.unwrap_or(0) - common.min(nb.unwrap_or(0)) { self.tick_objects_where(1); }
        for _ in 0..nl.unwrap_or(0) - common.min(nl.unwrap_or(0)) { self.tick_objects_where(2); }
        true
    }

    /// Bucket lid "freeballcover" (FUN_00440700): on while a fireball (FUN_004612e0 case 12) or spooky ball is pending.
    /// Guarded on `!self.fever_on || !on` (FUN_00440700: LogicMgr+0xf6 == 0).
    pub fn set_lid(&mut self, on: bool) {
        if self.fever_on && on { return; }
        for o in self.objs.iter_mut() { if o.cover { o.collidable = on; o.visible = on; } }
    }

    /// Pyramid state (power 4) from the turn counter LogicMgr+0x1f4: "bumperhole" subid 0 collidable while on.
    /// Guarded on `!self.fever_on || !on` (FUN_00440580: LogicMgr+0xf6 == 0).
    pub fn set_pyramid(&mut self, on: bool) {
        self.pyramid_on = on;
        if self.fever_on && on { return; }
        for o in self.objs.iter_mut() {
            if let Some(sub) = o.bumper { o.collidable = on && sub != 1; o.visible = on; }
        }
    }

    /// FUN_004612e0 tail (power 2/4 toggles): LogicMgr+0x134 from the flipper/pyramid turn counts.
    pub fn set_amp_target(&mut self, flippers: bool, pyramid: bool) {
        self.amp_target = if flippers { 0x82 } else if pyramid { 0xc8 } else { 0x104 };
    }

    /// FUN_0045eca0: spooky respawn. Speed capped at 3, then 9 trial launches from (x, 10): trial 0
    /// with the current velocity, trials 1..8 with vx += 1 - (Rand()%2000)/1000 applied to the best
    /// velocity so far; each trial is simulated (no object motion, no side effects) for 400/350 ticks
    /// or until y > height+20, counting distinct un-hit PegInfo objects touched (FUN_00451e10). The
    /// best strictly-greater count picks the velocity. RNG: keyed reseed (rng.rs SITE_SPOOKY).
    pub fn spooky_respawn(&mut self, b: &mut Ball) {
        self.spooky_respawn_at(b, false);
    }

    fn spooky_respawn_at(&mut self, b: &mut Ball, from_fever: bool) {
        b.spooky_used = true;
        b.bucket_touch = 0; b.bucket_touches = 0; // Ball+0x178/+0x17c cleared (+0x180 kept)
        self.spooky_left -= 1;
        if self.spooky_left < 1 {
            self.spooky_left = 0;
            b.spooky = false;
            self.set_lid(false); // FUN_00440700(0)
        }
        let (vx, vy) = (b.vel[0], b.vel[1]);
        let ss = ((x(vx) * vx) + (x(vy) * vy)).r(); // stored to float before the sqrt (FSTP [-0x24])
        let speed = (ss as f64).sqrt() as f32;
        if 3.0 < speed {
            // products are stored to float before the divisions (0x45edf7 / 0x45edfd)
            let (vx3, vy3) = ((x(vx) * 3.0).r(), (X::d(3.0) * vy).r());
            b.vel = [(x(vx3) / speed).r(), (x(vy3) / speed).r()];
        }
        // Board+0x1c4 at out-of-bounds respawn (FUN_0045f120) is btick - 1; at fever-hole respawn
        // (FUN_00470230, during contact callbacks like BallHitPeg/SITE_LUCKY) it is btick - 2.
        let key = self.btick - if from_fever { 2 } else { 1 };
        let mut rng = crate::rng::keyed(self.rng_seed, crate::rng::SITE_SPOOKY, key);
        if std::env::var_os("SIM_DEBUG").is_some() { eprintln!("spooky respawn btick {} key {} vel {:?}", self.btick, key, b.vel); }
        let start = [b.pos[0], 10.0];
        let mut best = 0usize;
        for t in 0..9 {
            let mut v = b.vel;
            if t > 0 {
                let d = (rng.rand() % 2000) as f64 / 1000.0;
                v[0] = (1.0f64 * (1.0 - d) + v[0] as f64) as f32;
            }
            let n = self.spooky_trial_ex(start, v, b.r, b.fireball, if t == 0 { 400 } else { 350 });
            if std::env::var_os("SIM_DEBUG").is_some() { eprintln!("  trial {t} v {v:?} count {n}"); }
            if best < n { best = n; b.vel = v; }
        }
        b.set_pos_full(start);
    }

    pub fn spooky_trial(&self, start: [f32; 2], vel: [f32; 2], r: f32, n: i32) -> usize {
        self.spooky_trial_ex(start, vel, r, false, n)
    }

    pub fn spooky_trial_ex(&self, start: [f32; 2], vel: [f32; 2], r: f32, fireball: bool, n: i32) -> usize {
        let mut bd = self.clone();
        bd.ai = true;
        bd.peek = true;
        let mut tb = Ball::new(start, vel);
        tb.r = r;
        tb.set_pos(start);
        let mut bounds = [0.0f32; 4];
        let mut cache = vec![];
        bd.predict_fill_cache(&tb, fireball, &mut bounds, &mut cache);
        let mut counted: Vec<usize> = vec![];
        let mut ev = vec![];
        for _ in 0..n {
            tb.update();
            ev.clear();
            bd.predict_step(&mut tb, fireball, &mut bounds, &mut cache, &mut ev);
            for e in ev.drain(..) {
                if let Event::Hit(i) = e {
                    if self.objs[i].has_peginfo && !self.objs[i].hit && !counted.contains(&i) { counted.push(i); }
                }
            }
            if ((self.height + 20) as f32) < tb.pos[1] { break; }
        }
        counted.len()
    }

    /// FUN_0046edf0: a green peg's first hit activates the character power.
    fn activate_power(&mut self, balls: &mut [Ball], k: usize, peg: usize) {
        self.activations.push(self.power);
        if self.power == 13 {
            // Lucky Spin (FUN_0046ecb0): wheel result Rand() & 3 (keyed reseed in TAS mode, like spooky);
            // 0 = a random power (FUN_00451ac0) applied now; 1 Magic Hat (non-collidable hat ball),
            // 2 Score Tripler, 3 Extra Ball: no physics effect
            // normal path (Board+0x130 == 0): SlotMachineDialog, result from its two Rand draws
            // Board+0x1c4 when BallHitPeg runs is two less than our counter (harness 'reseed site 5b01' lines)
            let mut rng = crate::rng::keyed(self.rng_seed, crate::rng::SITE_LUCKY, self.btick - 2);
            let (r1, r2) = (rng.rand() as u32, rng.rand() as u32);
            let (res, steps) = crate::slot::lucky_wheel_steps(r1, r2);
            let res = self.force_lucky.unwrap_or(res);
            self.lucky_steps.push(steps);
            if std::env::var_os("SIM_DEBUG").is_some() { eprintln!("lucky spin btick {} rands {r1} {r2} -> {res} (ctr {:?} list {:?})", self.btick - 2, self.lucky_ctr, self.lucky_list); }
            self.lucky_results.push(res as u32);
            self.lucky_bticks.push(self.btick);
            if res == 1 { balls[k].give_hat(); } // Magic Hat (on the ball that hit the green)
            if res == 0 {
                let spin_no = self.lucky_results.len(); // after the push: LogicMgr+0x50 at the pick
                if let Some(p) = self.lucky_ctr.and_then(|c| crate::oracle::lucky_pick(&self.lucky_list, c, spin_no)) {
                    let saved = self.power;
                    self.power = p;
                    self.activate_power(balls, k, peg);
                    self.power = saved;
                }
            }
            return;
        }
        match self.power {
            3 => { // Multiball (FUN_0046ead0): a new ball at the green peg, vel (±0.5, ModVal -3); the peg
                   // turns off (FUN_004769f0(0)). Direction: away from the hitting ball (tie: LogicMgr+0x2c parity).
                let Some(pp) = self.obj_pos(self.objs[peg].uid) else { return };
                let s = (pp[0] as f64 - balls[k].pos[0] as f64) as f32;
                let odd = self.lucky_ctr.map_or(false, |c| (c[0] as i32) & 1 != 0);
                let vx = if s > 0.0 || (s == 0.0 && odd) { 0.5 } else { -0.5 };
                let mut nb = Ball { r: 6.0, ..Default::default() };
                nb.set_pos(pp);
                nb.prev = pp;
                nb.vel = [vx, -3.0];
                nb.id = self.next_ball_id;
                // FUN_00421070: lowest unused slot in {0, 1} among live balls in board+0x1a0
                let slot0_used = balls.iter().any(|x| !x.gone && x.slot == 0) || self.spawned.iter().any(|x| !x.gone && x.slot == 0);
                nb.slot = if !slot0_used { 0 } else { 1 };
                // FUN_0046ead0 lines 250-252: inherit Magic Hat from parent ball (FUN_00480f50(1, 1))
                if balls[k].hat.is_some() { nb.give_hat(); }
                self.next_ball_id += 1;
                self.spawned.push(nb);
                self.objs[peg].collidable = false;
                self.objs[peg].visible = false;
                // FUN_0046ead0: in Extreme Fever with two balls in play (board+0x1a4, the new one included) → style 5
                // (25k; 4-3 real run: a Lucky Spin Multiball after the last orange)
                if self.fever_on && balls.iter().filter(|b| !b.gone).count() + self.spawned.len() == 2 { self.styles.push((5, 25000)); }
            }
            5 => { // SpaceBlast (FUN_0045e330): BallHitPeg(ball, peg, 1) on every un-hit visible peg within
                   // ModVal 100 of the green (LogicMgr+0x344 list order ~ board order)
                let Some(gp) = self.obj_pos(self.objs[peg].uid) else { return };
                for i in 0..self.objs.len() {
                    let o = &self.objs[i];
                    if !o.has_peginfo || o.hit || !o.collidable || !o.visible || self.shot_hits.contains(&i) { continue; }
                    let Some(p) = self.obj_pos(o.uid) else { continue };
                    let dx = sub(p[0], gp[0]).r();
                    let dy = sub(p[1], gp[1]).r();
                    // FUN_0045e330 (0x45e462..0x45e476): MSVC spills dx*dx, dy*dy and their sum to f32 (fstps)
                    let d2 = (x((x(dx) * dx).r()) + (x(dy) * dy).r()).r();
                    if d2 <= 10000.0f32 {
                        self.cur_ball = k;
                        self.forced_contact = true;
                        self.peg_contact(i);
                        self.forced_contact = false;
                    }
                }
            }
            9 => { // FlowerPower (FUN_0045e560): BallHitPeg(ball, peg, 1) on the ceil(0.2 * oranges) un-hit
                   // visible orange pegs nearest the green (multimap by squared distance, ties in list order)
                let Some(gp) = self.obj_pos(self.objs[peg].uid) else { return };
                let mut c: Vec<(f32, usize)> = vec![];
                for i in 0..self.objs.len() {
                    let o = &self.objs[i];
                    if !o.has_peginfo || o.hit || !o.collidable || !o.visible || o.peg_type != crate::score::ORANGE || self.shot_hits.contains(&i) { continue; }
                    let Some(p) = self.obj_pos(o.uid) else { continue };
                    let dx = sub(p[0], gp[0]).r();
                    let dy = sub(p[1], gp[1]).r();
                    // FUN_0045e560 (0x45e663..0x45e669): dx*dx, dy*dy and their sum each spill to f32
                    c.push(((x((x(dx) * dx).r()) + (x(dy) * dy).r()).r(), i));
                }
                c.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                // +0x360 is the live orange set: it shrinks as oranges are hit during the shot
                let left = self.orange_total - self.shot_hits.iter().filter(|&&i| self.objs[i].peg_type == crate::score::ORANGE).count() as i32;
                let n = ((x(left as f32) * X::d(0.20000000298023224)).r() as f64).ceil() as usize;
                if std::env::var_os("SIM_DEBUG").is_some() { eprintln!("flower power n {n} total {} cands {:?}", self.orange_total, c.iter().take(n + 2).map(|&(d, i)| (d, self.objs[i].src_index)).collect::<Vec<_>>()); }
                for &(_, i) in c.iter().take(n) {
                    self.cur_ball = k;
                    self.forced_contact = true;
                    self.peg_contact(i);
                    self.forced_contact = false;
                }
            }
            6 => { // SpookyBall (case 5): arm the current ball, close the bucket (FUN_00440700(1))
                balls[k].spooky = true;
                self.set_lid(true);
                self.spooky_left += 1; // ModVal 1: game_check compares ctr6 at every ready point (107 spooky shots exact)
            }
            4 => { // Pyramid (FUN_004612e0 case 4 → FUN_00440580(1)): "bumperhole" objects switch on;
                   // subid 0 → collidable/invisible, subid 1 → visible/non-collidable (verified via BRD flags)
                self.set_pyramid(true);
                self.set_amp_target(self.flippers_on(), true);
            }
            2 => { // Flippers (FUN_004612e0 case 2): first time clears the bottom corners
                self.flipper_applies += 1;
                let n = self.rng_n;
                self.rng_n += 1;
                if !self.flippers_init { self.flippers_init = true; self.flipper_clear(n); }
                self.set_flippers(true);
                self.set_amp_target(true, self.pyramid_on);
            }
            _ => {}
        }
    }

    /// FUN_004582f0 (first flipper activation; `n` = the hook's colour-RNG call index): visible
    /// PegInfo objects inside the flipper corner regions (FUN_00437160) are hidden and leave the
    /// colour sets; if fewer non-orange pegs remain in +0x364 than hidden oranges, previously hidden
    /// variable pegs outside the corners are resurrected as blue; then +0x364 (board order under the
    /// hook) is shuffled (FUN_0043d430) and its first `moved` entries become orange.
    pub fn flipper_clear(&mut self, n: u32) {
        let lpoly: [[f32; 2]; 4] = [[0.0, 380.0], [110.0, 470.0], [110.0, 600.0], [0.0, 600.0]];
        let rpoly: [[f32; 2]; 4] = [[650.0, 380.0], [540.0, 470.0], [540.0, 600.0], [650.0, 600.0]];
        fn inside(poly: &[[f32; 2]; 4], x: f32, y: f32) -> bool { // FUN_0047d930 crossing test
            let mut c = false;
            for k in 0..4 {
                let (a, b) = (poly[k], poly[(k + 1) % 4]);
                let (y1, y2) = (a[1], b[1]);
                let crosses = (y1 != y2 && y1 <= y && y2 > y) || (y >= y2 && y1 > y);
                if crosses && ((y - y1) * (b[0] - a[0])) / (y2 - y1) + a[0] > x { c = !c; }
            }
            c
        }
        let in_corner = |o: &Obj| -> bool {
            let p = match &o.geom { Geom::Peg { pos, .. } | Geom::Hole { pos, .. } => *pos,
                                    Geom::Poly { center, .. } => *center, Geom::Line(l) => [l.x1, l.y1] };
            match o.fflags {
                1 => true,
                2 => false,
                _ => p[1] >= 400.0 && ((p[0] < 150.0 && (p[0] < 80.0 || inside(&lpoly, p[0], p[1])))
                    || (p[0] as f64 > 500.0 && (p[0] > 570.0 || inside(&rpoly, p[0], p[1])))),
            }
        };
        let mut moved = 0i32;
        for i in 0..self.objs.len() {
            let o = &self.objs[i];
            if !o.has_peginfo || !o.visible || !o.collidable { continue; }
            if in_corner(o) {
                let o = &mut self.objs[i];
                if o.peg_type == crate::score::ORANGE && !o.hit { moved += 1; }
                o.collidable = false;
                o.visible = false;
            }
        }
        if moved == 0 { return; }
        let order = crate::colors::peg_order(self);
        // FUN_004582f0 lines 858-888: if +0x364 has fewer than `moved` pegs, resurrect hidden variable
        // pegs outside the flipper corners as blue until +0x364 has `moved` entries
        let mut cur_non_orange = order.iter().filter(|&&i| {
            let o = &self.objs[i]; !o.hit && o.visible && o.collidable && o.peg_type != crate::score::ORANGE && !self.shot_hits.contains(&i)
        }).count() as i32;
        if cur_non_orange < moved {
            for &i in &order {
                if cur_non_orange >= moved { break; }
                let o = &self.objs[i];
                if !o.visible && o.variable && !in_corner(o) {
                    let o = &mut self.objs[i];
                    o.peg_type = crate::score::BLUE;
                    o.hit = false;
                    o.contacts = 0;
                    o.since_hit = 0;
                    o.remove_timer = 0;
                    o.visible = true;
                    o.collidable = true;
                    cur_non_orange += 1;
                }
            }
        }
        let mut v: Vec<usize> = order.into_iter().filter(|&i| {
            let o = &self.objs[i]; !o.hit && o.visible && o.collidable && o.peg_type != crate::score::ORANGE && !self.shot_hits.contains(&i) }).collect();
        let mut rng = crate::colors::hook_rng(self.rng_seed, n);
        crate::colors::shuffle(&mut v, &mut rng);
        let k = (moved as usize).min(v.len());
        for &i in &v[..k] { self.objs[i].peg_type = crate::score::ORANGE; }
        self.orange_total += k as i32 - moved;
    }

    pub fn obj_pos(&self, uid: u32) -> Option<[f32; 2]> {
        self.objs.iter().find(|o| o.uid == uid).map(|o| match &o.geom {
            Geom::Peg { pos, .. } | Geom::Hole { pos, .. } => *pos,
            Geom::Poly { center, .. } => *center,
            // unused: teleport targets are all polys and no line has a PegInfo (checked over every level)
            Geom::Line(l) => [l.x1, l.y1],
        })
    }

    pub fn step(&mut self, b: &mut Ball, events: &mut Vec<Event>) {
        let mut v = vec![std::mem::take(b)];
        self.step_multi(&mut v, events);
        *b = v.swap_remove(0);
    }

    /// Ball updates + FUN_0045add0 + moves for all live balls. Pair-set order: grouped by ball id;
    /// within a group holes (inline), other balls with a higher id (inline, FUN_0047a3b0), then the
    /// accumulated pegs/lines/polys go to the swept CCD when the group ends.
    pub fn step_multi(&mut self, balls: &mut Vec<Ball>, events: &mut Vec<Event>) {
        let n = balls.len();
        let nobj = self.objs.len();
        // per-frame buffers reused across frames (allocation-free in the steady state)
        let mut sc = std::mem::take(&mut self.scratch);
        let frozen = &mut sc.frozen; frozen.clear(); frozen.resize(n, false);
        for (k, b) in balls.iter_mut().enumerate() {
            if b.gone { continue; }
            if b.teleport == 1 {
                // the target is read when the countdown ends (after this tick's mover updates)
                if let Some(uid) = b.teleport_uid.take() { b.teleport_to = self.obj_pos(uid); }
            }
            // teleporting: the ball is non-collidable and frozen; the broadphase still runs on its stale bbox
            frozen[k] = b.update();
            b.update_hat(!frozen[k]);
        }
        let ev0 = events.len();
        let mut teleports: Vec<(usize, u32, i32)> = vec![];
        let mut fever_spooky: Vec<usize> = vec![];
        let captured = &mut sc.captured; captured.clear(); captured.resize(n, false);
        if sc.cands.len() < n { sc.cands.resize_with(n, Vec::new); }
        let cands = &mut sc.cands;
        for c in cands.iter_mut() { c.clear(); }
        for k in 0..n {
            if balls[k].gone { continue; }
            let b = &mut balls[k];
            let full = *NO_FAST || b.sap.len() < nobj || b.bbox.iter().any(|v| v.is_nan());
            let prev_none = b.sap_bb.is_none();
            if b.sap.len() < nobj { b.sap.resize(nobj, 0); }
            match b.sap_bb {
                Some(prev) if !full => {
                    // incremental: mover objects + static objects with an edge in a ball edge's swept range
                    // each object's update is independent: order is irrelevant, only duplicates (an object in
                    // several edge ranges) are dropped, via a per-object stamp
                    let proc = &mut sc.proc;
                    proc.clear();
                    proc.extend_from_slice(&self.sapx.dynamic);
                    if sc.seen.len() < nobj { sc.seen.resize(nobj, 0); }
                    sc.stamp = sc.stamp.wrapping_add(1);
                    if sc.stamp == 0 { sc.seen.iter_mut().for_each(|v| *v = 0); sc.stamp = 1; }
                    for (j, &(_, be)) in SAP_EDGE.iter().enumerate() {
                        let arr = &self.sapx.edges[j];
                        // partition points at the new edge, walked from the previous ones (same as
                        // partition_point on the sorted, NaN-free static edge list)
                        let [plt, ple] = b.sap_ix[j];
                        let v = b.bbox[be];
                        let (lt, le) = (walk_partition(arr, plt as usize, |e| e < v), walk_partition(arr, ple as usize, |e| e <= v));
                        b.sap_ix[j] = [lt as u32, le as u32];
                        // swept range [lo, hi]: endpoints e with lo <= e <= hi
                        let (a, z) = if prev[be] <= v { (plt as usize, le) } else { (lt, ple as usize) };
                        for e in &arr[a..z.max(a)] {
                            let sv = &mut sc.seen[e.1 as usize];
                            if *sv != sc.stamp { *sv = sc.stamp; proc.push(e.1); }
                        }
                    }
                    for &i in proc.iter() {
                        let i = i as usize;
                        let was = b.sap[i] & 0x10 != 0;
                        let now = sap_update(&mut b.sap[i], &b.bbox, &self.objs[i].bbox);
                        if was != now {
                            if now { b.sap_over.push(i as u32); }
                            else if let Some(p) = b.sap_over.iter().position(|&q| q as usize == i) { b.sap_over.swap_remove(p); }
                        }
                    }
                }
                _ => {
                    b.sap_over.clear();
                    for i in 0..nobj {
                        if sap_update(&mut b.sap[i], &b.bbox, &self.objs[i].bbox) { b.sap_over.push(i as u32); }
                    }
                }
            }
            // a NaN edge (or the reference mode) forces a full scan next frame
            b.sap_bb = if *NO_FAST || b.bbox.iter().any(|v| v.is_nan()) { None } else { Some(b.bbox) };
            if b.sap_bb.is_some() && (full || prev_none) {
                for (j, &(_, be)) in SAP_EDGE.iter().enumerate() {
                    let (arr, v) = (&self.sapx.edges[j], b.bbox[be]);
                    b.sap_ix[j] = [arr.partition_point(|e| e.0 < v) as u32, arr.partition_point(|e| e.0 <= v) as u32];
                }
            }
            if frozen[k] { continue; }
            for &i in &b.sap_over {
                let o = &self.objs[i as usize];
                if o.collidable && !matches!(o.geom, Geom::Hole { .. }) { cands[k].push(i as usize); }
            }
        }
        // ball-ball pairs (both must have +0x163; the newer ball's endpoints are the inserted ones)
        let mut bpairs: Vec<(usize, usize)> = vec![];
        for k in 0..n {
            for j in k + 1..n {
                if balls[k].gone || balls[j].gone { continue; }
                let (ia, ib) = (balls[k].id.min(balls[j].id), balls[k].id.max(balls[j].id));
                let (lo, hi) = if balls[k].id < balls[j].id { (k, j) } else { (j, k) };
                let pos = match self.pair_sap.iter().position(|e| e.0 == ia && e.1 == ib) {
                    Some(p) => p,
                    None => { self.pair_sap.push((ia, ib, 0)); self.pair_sap.len() - 1 }
                };
                let (bb_new, bb_old) = (balls[hi].bbox, balls[lo].bbox);
                if sap_update(&mut self.pair_sap[pos].2, &bb_new, &bb_old) && !frozen[lo] && !frozen[hi] { bpairs.push((lo, hi)); }
            }
        }
        let order = &mut sc.order; order.clear(); order.extend((0..n).filter(|&k| !balls[k].gone));
        order.sort_by_key(|&k| balls[k].id);
        for &k in order.iter() {
            // holes (type 1), in object order
            if !frozen[k] {
                let bpos = balls[k].pos;
                for hi in 0..self.sapx.holes.len() {
                    let i = self.sapx.holes[hi] as usize;
                    let o = &self.objs[i];
                    if !o.collidable || balls[k].sap.get(i).map_or(true, |s| s & 0x10 == 0) { continue; }
                    if let Geom::Hole { pos, w, h, ellipse, teleport } = o.geom {
                        if hole_test(pos, w, h, ellipse, bpos) {
                            match teleport {
                                Some((uid, delay)) => { teleports.push((k, uid, delay)); events.push(Event::Teleport(i)); }
                                None => {
                                    events.push(Event::Captured(i));
                                    if self.peek {
                                        captured[k] = true;
                                    } else if self.objs[i].fever && self.objs[i].hole_value > 0 {
                                        // BallHitPeg: with every peg cleared (+0x360 == +0x36c == 0) all fever holes are worth 100000;
                                        // each ball in play (Multiball) adds the value of the hole it falls in
                                        let left = self.objs.iter().filter(|o| o.has_peginfo && o.peg_type != 0 && !o.hit && o.visible && o.collidable).count();
                                        self.fever_hole += if left == 0 { 100000 } else { self.objs[i].hole_value };
                                        // FUN_00470230 lines 148-154: a spooky ball falling into a fever hole respawns
                                        // (FUN_0045eca0) instead of being removed
                                        if balls[k].spooky { fever_spooky.push(k); } else { captured[k] = true; }
                                    } else if self.objs[i].freeball {
                                        captured[k] = true;
                                        if *COMBO_TRACE { eprintln!("CATCH btick {} ball {:?} bucket {:?}", self.btick, balls[k].pos, pos); }
                                        let b = balls[k].clone(); self.bucket_catch_styles(&b);
                                    } else {
                                        captured[k] = true;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // other balls (type 2)
            let mut others: Vec<usize> = bpairs.iter().filter(|p| p.0 == k).map(|p| p.1).collect();
            others.sort_by_key(|&j| balls[j].id);
            for j in others {
                let (a, b) = two_mut(balls, k, j);
                ball_ball(a, b);
            }
            if !cands[k].is_empty() {
                // pair set (FUN_00448be0 comparator): ordered by object type (+0x10: circle 3, line 4,
                // poly 5), then by object id (+0x58, creation order ~ our index order)
                let mut c = std::mem::take(&mut cands[k]);
                c.sort_by_key(|&i| (match self.objs[i].geom { Geom::Hole { .. } => 1, Geom::Peg { .. } => 3, Geom::Line(_) => 4, Geom::Poly { .. } => 5 }, i));
                self.cur_ball = k;
                self.ccd(&mut balls[k], &c, events);
                c.clear();
                cands[k] = c;
            }
        }
        let lost = &mut sc.lost; lost.clear(); lost.resize(n, false);
        for &k in order.iter() {
            let b = &mut balls[k];
            if frozen[k] { continue; }
            if !b.moved {
                let v = b.vel;
                b.move_by(v);
            }
            let r = b.r;
            if !self.ai && (self.height + 10) as f32 > 0.0 && (((self.height + 10) as f32) < sub(b.pos[1], r).r()
                || ((self.width + 10) as f32) < sub(b.pos[0], r).r()
                || (x(r) + b.pos[0]).r() < -10.0 || (x(r) + b.pos[1]).r() < -10.0) {
                lost[k] = true;
            }
        }
        // hat balls (created after their owners: later ids) — zero-velocity fireball contacts
        for &k in order.iter() {
            let Some(h) = balls[k].hat.clone() else { continue };
            if h.ctr < 11 || frozen[k] || balls[k].gone { continue; }
            let r: f32 = 20.0;
            let bb = [sub(h.pos[0], r).r(), sub(h.pos[1], r).r(), add(h.pos[0], r).r(), add(h.pos[1], r).r()];
            let mut c: Vec<usize> = (0..nobj).filter(|&i| {
                let o = &self.objs[i];
                // pure conjunction: the cheap bbox test first (same set)
                o.bbox[0] <= bb[2] && bb[0] <= o.bbox[2] && o.bbox[1] <= bb[3] && bb[1] <= o.bbox[3]
                    && o.collidable && o.has_peginfo && !o.hit && !matches!(o.geom, Geom::Hole { .. })
                    && !self.shot_hits.contains(&i)
            }).collect();
            if c.is_empty() { continue; }
            c.sort_by_key(|&i| (match self.objs[i].geom { Geom::Hole { .. } => 1, Geom::Peg { .. } => 3, Geom::Line(_) => 4, Geom::Poly { .. } => 5 }, i));
            let mut hb = Ball { pos: h.pos, prev: h.pos, r, fireball: true, bbox: bb, ..Default::default() };
            self.cur_ball = k;
            self.hat_contact = true;
            self.ccd(&mut hb, &c, events);
            self.hat_contact = false;
        }
        if self.ai {
            // AIMgr step: no LogicMgr pass; teleports start at once (FUN_00463080 → FUN_0047a350)
            if self.ai_movetoggle {
                for e in ev0..events.len() {
                    if let Event::Hit(i) = events[e] { self.movetoggle(i); }
                }
            }
            for (k, uid, delay) in teleports {
                let b = &mut balls[k];
                if delay > 0 { b.teleport = delay; b.teleport_uid = Some(uid); }
                else if let Some(p) = self.obj_pos(uid) { b.set_pos_full(p); }
            }
            for &k in order.iter() { if captured[k] { balls[k].gone = true; } }
            self.scratch = sc;
            return;
        }
        // queued contact events are handled by LogicMgr after the board update (FUN_004704a0)
        for e in ev0..events.len() {
            if let Event::Hit(i) = events[e] { self.movetoggle(i); }
        }
        // FUN_004704a0: a fireball touching "freeballcover" sets Ball+0x145 and switches the lid off (vt+0x84(0))
        for (k, i) in std::mem::take(&mut self.pending_lid) {
            if let Some(b) = balls.get_mut(k) { b.lid_hit = true; }
            self.objs[i].collidable = false;
        }
        // FUN_004704a0: "freeball*" contact with the ball inside 15 < x < width-15 (650)
        for k in std::mem::take(&mut self.pending_bucket) {
            let Some(b) = balls.get_mut(k) else { continue };
            if (b.pos[0] as f64) > 15.0 && b.pos[0] < (650 - 15) as f32 {
                if b.bucket_touch < 1 { b.bucket_touch = self.btick; }
                else if self.btick - b.bucket_touch > 9 { b.bucket_touches += 1; }
            }
        }
        // FUN_00467230: lowest y since the first bucket touch (not while teleporting)
        for b in balls.iter_mut() {
            if b.bucket_touch > 0 && b.teleport == 0 {
                let m = if b.bucket_miny == 0 { 10000 } else { b.bucket_miny };
                if b.pos[1] < m as f32 { b.bucket_miny = b.pos[1] as i32; }
            }
        }
        // BallHitPeg per queued contact. Brick combo (top of FUN_0046f480, any contact): a brick (type 5)
        // contact with Ball+0x148 == 0 and PegInfo+0x18 < 20 sets +0x158 = tick, and +0x150 = 20 when the
        // previous one was < 20 ticks ago at speed^2 > 2.5. Then already-hit pegs return early (+0x14):
        // only first hits reach the +0xb8 log, Long Shot, and the combo count (+0x154 == 12 → style 2).
        let flips = std::mem::take(&mut self.pending_flipper);
        // FUN_004704a0, a "flipper" contact: Ball+0x144 = 1 (below) and the shot timer LogicMgr+0x38 is capped
        // at DAT_00649acc, so a ball kept up by the flippers is not force-popped (FUN_00472f60)
        if !flips.is_empty() { self.shot_timer = self.shot_timer.min(SHOT_POP_CAP); }
        let contacts = std::mem::take(&mut self.pending_contacts);
        let nc = contacts.len();
        for (j, (k, i, first, hat)) in contacts.into_iter().enumerate() {
            if *COMBO_TRACE { let p = balls.get(k).map(|b| if hat { b.hat.as_ref().map_or([0.0; 2], |h| h.pos) } else { b.pos }); eprintln!("BHP t{} ball{k} hat{hat} obj{i} src{} type{} first{first} pos {:?}", self.btick, self.objs[i].src_index, self.objs[i].peg_type, p); }
            // flipper contacts queued before this one (same FIFO in the game)
            for &(fk, seq) in &flips { if seq == j { if let Some(b) = balls.get_mut(fk) { b.flipper_touch = true; } } }
            let t = self.objs[i].peg_type;
            let is_brick = matches!(self.objs[i].geom, Geom::Poly { .. });
            let since = self.objs[i].since_hit;
            let tick = self.btick;
            let mut style2 = false;
            // a Magic Hat contact is BallHitPeg with the hat ball (its own fields: no brick combo, no
            // flipper flag; Ball+0xe8 != 0 skips the Long Shot log/check); the hat's position counts
            if hat {
                if !first { continue; }
                let pos = balls.get(k).and_then(|b| b.hat.as_ref()).map_or([0.0; 2], |h| h.pos);
                self.all_clear_check(pos);
                continue;
            }
            if let Some(b) = balls.get_mut(k) {
                // FUN_0046f480: a live real fireball (not Magic Hat) immediately turns burned pegs off (FUN_004769f0(0))
                if b.fireball { self.objs[i].collidable = false; self.objs[i].visible = false; }
                if is_brick && b.frames == 0 && since < 20 {
                    if let Some(lb) = b.last_brick {
                        let sp = (b.vel[0] * b.vel[0] + b.vel[1] * b.vel[1]) as f64;
                        if tick - lb < 20 && sp > 2.5 { b.combo_timer = 20; }
                    }
                    b.last_brick = Some(tick);
                }
                if first && b.combo_timer > 0 { b.combo_count += 1; style2 = b.combo_count == 12; }
                if *COMBO_TRACE { eprintln!("COMBO t{tick} obj{i} brick{is_brick} first{first} since{since} fr{} lb{:?} v2 {} timer{} count{}", b.frames, b.last_brick, b.vel[0]*b.vel[0]+b.vel[1]*b.vel[1], b.combo_timer, b.combo_count); }
            }
            if !first { continue; }
            // style 1: first hit with Ball+0x144 set (cleared here); the 4th this shot (LogicMgr+0x48) scores
            if let Some(b) = balls.get_mut(k) {
                if b.flipper_touch {
                    b.flipper_touch = false;
                    self.flipper_hits += 1;
                    if self.flipper_hits == 4 { self.styles.push((1, 25000)); }
                }
            }
            let pos = balls.get(k).map_or([0.0; 2], |b| b.pos);
            // FUN_0046b910 (contact log + Long Shot) only for balls with Ball+0xe8 == 0
            if balls.get(k).map_or(false, |b| b.slot == 0) {
                self.contact_log.push((tick, t, pos));
                if *COMBO_TRACE { eprintln!("CONTACT t{tick} n{} type{t} ball{k} pos {:?}", self.contact_log.len(), pos); }
                self.long_shot_check(t);
            }
            if style2 { self.styles.push((2, 50000)); }
            self.all_clear_check(pos);
        }
        // flipper contacts queued after the last PegInfo contact of this tick
        for &(fk, seq) in &flips { if seq >= nc { if let Some(b) = balls.get_mut(fk) { b.flipper_touch = true; } } }
        // BallHitPeg (FUN_0046f480) side effects of first hits: green → character power
        // (a Space Blast can hit further greens: keep going until no new first hits are queued)
        while !self.new_hits.is_empty() {
            for (k, i, hat, forced) in std::mem::take(&mut self.new_hits) {
                if forced {
                    let pos = balls.get(k).map_or([0.0; 2], |b| b.pos);
                    self.all_clear_check(pos);
                }
                if self.objs[i].peg_type == crate::score::ORANGE { self.orange_attack_check(i); }
                if self.objs[i].peg_type == crate::score::GREEN {
                    // LogicMgr+0x50 (greens hit this shot) == 2 → style 10 (FUN_0046edf0)
                    let pos = self.shot_hits.iter().position(|&j| j == i).unwrap_or(0);
                    if self.shot_hits[..=pos].iter().filter(|&&j| self.objs[j].peg_type == crate::score::GREEN).count() == 2 {
                        self.styles.push((10, 25000));
                    }
                    self.activate_power(balls, k, i);
                }
                // FUN_0043d580: no oranges left (LogicMgr+0x360 == 0) and not yet in fever → FUN_00458ae0.
                // Check against the hit that actually took remaining oranges to 0 (not an earlier hit
                // queued on the same tick, e.g. main-ball blue + hat-ball last orange on fun 9-4 shot 3).
                let last_orange_hit = self.oranges_left() == 0 && (
                    self.shot_hits.iter().rposition(|&j| self.objs[j].peg_type == crate::score::ORANGE)
                        .map_or(true, |rp| self.shot_hits[rp] == i)
                );
                if !self.fever_on && last_orange_hit {
                    // style 5 when board+0x1a4 == 2 (Multiball); else style 11 when the hitting ball itself
                    // touched the bucket (a Magic Hat ball +0x18c has its own +0x178 == 0, while the main
                    // ball wearing a hat still has its own +0x178)
                    if balls.iter().filter(|b| !b.gone).count() == 2 { self.styles.push((5, 25000)); }
                    else if !hat && balls.get(k).map_or(false, |b| b.bucket_touch > 0) { self.styles.push((11, 25000)); }
                    self.start_fever();
                }
            }
        }
        // queued hole events are handled by LogicMgr after the board update (FUN_00470230)
        for (k, uid, delay) in teleports {
            let b = &mut balls[k];
            if delay > 0 {
                b.teleport = delay;
                b.teleport_uid = Some(uid);
            } else if let Some(p) = self.obj_pos(uid) {
                b.set_pos_full(p);
            }
        }
        for &k in order.iter() {
            // FUN_00470230 (fever hole with spooky ball) and FUN_0045f120 (below board height)
            let b = &balls[k];
            let from_fever = fever_spooky.contains(&k);
            if from_fever || (b.spooky && (self.height as f32) < sub(b.pos[1], b.r).r()) {
                lost[k] = false;
                let mut b = std::mem::take(&mut balls[k]);
                self.spooky_respawn_at(&mut b, from_fever);
                balls[k] = b;
            }
            if lost[k] { events.push(Event::Lost); }
            if lost[k] || captured[k] { balls[k].gone = true; }
        }
        balls.append(&mut self.spawned);
        self.scratch = sc;
    }

    /// Contact bookkeeping from LogicMgr::BallHitPeg FUN_0046f480 (FUN_004769b0 etc).
    /// FUN_0046b910 (BallHitPeg): Long Shot (style 0, 25000) / Super Long Shot (0xf, 50000) from the
    /// first contacts of the shot: two non-blue contacts, or non-blue, blue, non-blue (blue → third
    /// within 10 ticks); squared distance between the first two positions > 40000 / 202500.
    fn long_shot_check(&mut self, t: u8) {
        use crate::score::{BLUE, GREEN};
        if self.fever_on && t == GREEN && self.power == 3 { return; }
        let l = &self.contact_log;
        let ok = match l.len() {
            2 => l[0].1 != BLUE && l[1].1 != BLUE,
            3 => l[0].1 != BLUE && l[1].1 == BLUE && l[2].1 != BLUE && l[2].0 - l[1].0 < 10,
            _ => false,
        };
        if !ok { return; }
        let (a, b) = (l[0].2, l[1].2);
        let d = ((b[0] as f64 - a[0] as f64).powi(2) + (b[1] as f64 - a[1] as f64).powi(2)) as f32;
        if *COMBO_TRACE { eprintln!("LONGSHOT t{} log {:?} d2 {d}", self.btick, l); }
        if d > 202500.0 { self.styles.push((0xf, 50000)); } else if d > 40000.0 { self.styles.push((0, 25000)); }
    }

    /// BallHitPeg orange branch: style 3 (50000) when the oranges hit this shot (+0x4c) reach a
    /// threshold set by the oranges at shot start (+0x360 + +0x4c). Ignores the board flag
    /// (+0xb0)+0x7bc that would move 5 oranges to the n-2 row.
    fn orange_attack_check(&mut self, i: usize) {
        let o = crate::score::ORANGE;
        let pos = self.shot_hits.iter().position(|&j| j == i).unwrap_or(self.shot_hits.len());
        let hit = self.shot_hits[..=pos.min(self.shot_hits.len().saturating_sub(1))].iter().filter(|&&j| self.objs[j].peg_type == o).count() as i32;
        let start = self.orange_total;
        let need = if start >= 31 { 15 } else if start >= 21 { 10 } else if start >= 16 { 9 } else if start >= 11 { 8 }
                   else if start >= 7 { start - 2 } else if start >= 5 { start - 1 } else { 1000 };
        if hit == need { self.styles.push((3, 50000)); }
    }

    /// FUN_0046bbc0 (ball caught by the free-ball bucket): style chain (including fireball-lid style 7
    /// and bumperhole style 16; Duel-only +0x124 style 17 is omitted).
    fn bucket_catch_styles(&mut self, b: &Ball) {
        let pegs = self.shot_hits.len() as i32;
        let tick = self.btick;
        let l = &self.contact_log;
        let quick = b.slot == 0 && (pegs == 1 || (l.len() == 2 && l[1].0 - l[0].0 < 20 && tick - l[1].0 < 50));
        let mut t = if b.bucket_touches < 1 { 150 } else { 50 };
        let miny = if b.bucket_miny == 0 { 10000 } else { b.bucket_miny };
        if 560 - miny > 125 { t = (0.4f32 as f64 * t as f64) as i32; }
        let mut b2 = false;
        let mut style: Option<(u8, i32)> = None;
        let mut chain = true;
        if quick || (b.slot == 0 && pegs >= 1) {
            self.catch_streak = self.catch_streak.abs() + 1;
            if self.catch_streak % 10 != 0 { b2 = true; } else { style = Some((14, 100000)); chain = false; }
        }
        if chain {
            style = if b.hat.is_some() && pegs >= 1 { Some((6, 10000)) }
                // fireball counter LogicMgr+0x234[player] >= 1 throughout a fireball shot
                else if b.lid_hit && b.fireball { Some((7, 25000)) }
                else if self.caught_this_shot { Some((8, 100000)) }
                else if b.bucket_touch > 0 && pegs >= 1 && tick - b.bucket_touch > t { Some((12, 25000)) }
                else if b2 && self.catch_streak == 5 { Some((13, 25000)) }
                else if quick { Some((9, 5000)) }
                else { None };
        }
        if std::env::var_os("SIM_DEBUG").is_some() { eprintln!("bucket catch btick {tick} touch {} touches {} miny {} t {t} pegs {pegs} streak {} quick {quick} -> {style:?}", b.bucket_touch, b.bucket_touches, b.bucket_miny, self.catch_streak); }
        if let Some(s) = style { self.styles.push(s); }
        if b.bumperhole_touch { self.styles.push((16, 10000)); }
        self.caught_this_shot = true;
    }

    /// BallHitPeg: every peg cleared (+0x360 == 0 and +0x36c == 0) → style 4 (50000) when the log
    /// has > 2 entries, or 2 with the previous one not orange or > 120 px away.
    fn all_clear_check(&mut self, pos: [f32; 2]) {
        if self.styles.iter().any(|s| s.0 == 4) { return; }
        let left = self.objs.iter().filter(|o| o.has_peginfo && o.peg_type != 0 && !o.hit && o.visible && o.collidable).count();
        if left != 0 || self.oranges_left() != 0 { return; }
        let n = self.contact_log.len();
        if !(self.fever_on || n > 2) || n < 2 { return; }
        let e = self.contact_log[n - 2];
        let d = ((pos[0] as f64 - e.2[0] as f64).powi(2) + (pos[1] as f64 - e.2[1] as f64).powi(2)).sqrt() as f32;
        if d > 120.0 || e.1 != crate::score::ORANGE || n > 2 { self.styles.push((4, 50000)); }
    }

    fn peg_contact(&mut self, i: usize) {
        if self.ai { return; } // AIBall contacts go to FUN_00463080 instead of LogicMgr
        let o = &mut self.objs[i];
        if !o.has_peginfo { return; }
        // PegInfo+0x14 (set at the first BallHitPeg, kept after the peg pops from the hit list mid-flight)
        let first = !o.hit && !self.shot_hits.contains(&i);
        if !self.peek { o.hit = true; }
        if !self.forced_contact { self.pending_contacts.push((self.cur_ball, i, first, self.hat_contact)); }
        if first {
            self.hit_list.push(i);
            self.shot_hits.push(i);
            self.shot_hit_bticks.push(self.btick);
            self.new_hits.push((self.cur_ball, i, self.hat_contact, self.forced_contact));
            self.shot_timer = self.shot_timer.min(SHOT_HIT_CAP); // BallHitPeg, first hit only
        }
        let o = &mut self.objs[i];
        if o.contacts < 3 { o.contacts += 1; }
        // PegInfo+0x18 starts at the first contact (PegInfo::Update FUN_00479680 then counts it up while nonzero); the
        // store itself was not located statically, but brick-combo styles and crumble timing match on every capture
        if o.since_hit == 0 { o.since_hit = 1; self.add_ticking(i); }
    }

    /// FUN_0047f160
    /// FUN_0047d880 at the start of poly collision: lazy rotation rebuild (FUN_0047d5b0 + bbox)
    fn refresh_poly(&mut self, ci: usize) {
        let o = &mut self.objs[ci];
        if let Geom::Poly { lines, offs, center, stale: st @ true, pvel, .. } = &mut o.geom {
            for (l, of) in lines.iter_mut().zip(offs.iter()) {
                l.set_pos(add(center[0], of[0]).r(), add(of[1], center[1]).r());
                l.vel = *pvel;
            }
            *st = false;
        }
        if let Geom::Poly { lines, offs, rot, center, oneside, dirty_rot: d @ true, .. } = &mut o.geom {
            let vels: Vec<[f32; 2]> = lines.iter().map(|l| l.vel).collect();
            let (nl, no) = build_poly_lines(&self.sapx.verts[ci], *rot, *center, *oneside);
            *lines = nl;
            *offs = no;
            for (l, v) in lines.iter_mut().zip(vels) { l.vel = v; }
            o.bbox = lines_bbox(lines);
            *d = false;
        }
    }

    fn ccd(&mut self, b: &mut Ball, cands: &[usize], events: &mut Vec<Event>) {
        for &ci in cands { self.refresh_poly(ci); }
        let mut remaining: f32 = 1.0;
        let mut toi: f32 = 1.0;
        b.moved = true;
        let mut first_iter = true;
        let mut cands: Vec<usize> = cands.to_vec(); // fireball pass-through erases from this list
        for _iter in 0..20 {
            let p = b.pos;
            let mut v = b.vel;
            let mut h = Hit::default();
            let mut best: Option<usize> = None;
            for &ci in cands.iter() {
                let o = &self.objs[ci];
                // a fireball no longer collides with pegs it already burned this shot (FUN_0046f480 → FUN_004769f0(0);
                // in AI mode FUN_004428e0 checks the AIBall's own hit set `b.ai_hits`)
                if b.fireball && o.has_peginfo && (if self.ai { b.ai_hits.contains(&ci) } else { self.shot_hits.contains(&ci) }) { continue; }
                // FUN_004428e0: in AI mode with power 3 (Multiball), already-hit green pegs are skipped
                if self.ai && self.power == 3 && o.has_peginfo && o.peg_type == crate::score::GREEN && b.ai_hits.contains(&ci) { continue; }
                let mut hh = h;
                let hit = match &o.geom {
                    Geom::Line(l) => {
                        let r = line_collide(l, p, v, b.r, &mut toi, &mut hh, true, &mut first_iter);
                        if r { hh.angv = [0.0; 2]; }
                        r
                    }
                    Geom::Poly { lines, inside_test, convex, angvel, center, .. } => {
                        let r = poly_collide(lines, *inside_test, *convex, o.bbox, p, v, b.r, &mut toi, &mut hh, &mut first_iter);
                        if r && *angvel != 0.0 {
                            // FUN_0047dfe0 tail: surface velocity of the rotating poly at the contact
                            let dx = sub(hh.contact[0], center[0]).r();
                            let dy = sub(hh.contact[1], center[1]).r();
                            hh.angv[0] = ((x(dy) * *angvel) + hh.angv[0]).r();
                            hh.angv[1] = (x(hh.angv[1]) - (x(*angvel) * dx)).r();
                        }
                        r
                    }
                    Geom::Peg { pos, r, .. } => sweep_peg(*pos, *r, p, v, b.r, &mut toi, &mut hh.contact),
                    Geom::Hole { .. } => false,
                };
                // sweep functions write outputs only on success (mirrors in-place locals)
                if hit { h = hh; best = Some(ci); }
            }
            if *CCD_DEBUG { eprintln!("    ccd it{_iter} p={p:?} v={v:?} rem={remaining} best={best:?} toi={toi} contact={:?} n={:?} angv={:?}", h.contact, h.normal, h.angv); }
            let Some(bi) = best else {
                b.pos[0] = ((x(b.vel[0]) * remaining) + b.pos[0]).r();
                b.pos[1] = ((x(remaining) * b.vel[1]) + b.pos[1]).r();
                return;
            };
            let o = self.objs[bi].clone();
            if b.fireball && o.has_peginfo {
                // FUN_0047f160: contact callback, erase the object from the candidate vector; position,
                // velocity, remaining time and the current best toi are left unchanged.
                self.peg_contact(bi);
                events.push(Event::Hit(bi));
                // PegInfo+0x14 stays set like on a bounce: a popped peg (off the hit list, still
                // collidable for 4 updates) must not count as a first hit again
                if !self.peek && !self.ai { self.objs[bi].hit = true; }
                if let Some(k) = cands.iter().position(|&c| c == bi) { cands.remove(k); }
                continue;
            }
            b.hit_this_frame = true;
            b.hit_flag165 = true;
            let callback = match &o.geom {
                Geom::Peg { pos, r, vel } => {
                    b.set_pos(h.contact);
                    b.peg_bounce(*pos, *r, *vel, o.bouncy, o.rolly, o.ballstopreset);
                    true
                }
                _ => b.line_bounce(&mut v, &o, &mut h),
            };
            if callback && self.peek { events.push(Event::Hit(bi)); }
            else if callback {
                self.peg_contact(bi);
                events.push(Event::Hit(bi));
                if !self.ai { self.objs[bi].hit = true; }
                // FUN_004704a0 compares the id with "freeball" exactly (std::string::compare(0, size, "freeball", 8)):
                // the lid ("freeballcover", collidable for a fireball) is not a bucket touch (8-2 real run: no style 11)
                if o.freeball && !o.cover && !o.has_peginfo && !matches!(o.geom, Geom::Hole { .. }) { self.pending_bucket.push(self.cur_ball); }
                if o.cover && b.fireball && !self.ai { self.pending_lid.push((self.cur_ball, bi)); }
                if o.flipper && !self.ai { self.pending_flipper.push((self.cur_ball, self.pending_contacts.len())); }
                if o.bumper.is_some() && !o.has_peginfo && !matches!(o.geom, Geom::Hole { .. }) { b.bumperhole_touch = true; }
            }
            b.pos134 = b.pos;
            remaining = (x(remaining) - toi).r();
            toi = remaining;
        }
    }
}

/// FUN_004781c0: is ball centre `p` inside hole (bbox test, then optional ellipse test).
fn hole_test(pos: [f32; 2], w: i32, h: i32, ellipse: bool, p: [f32; 2]) -> bool {
    let hw = (X::d(0.5) * X::d(w as f64)).r();
    let hh = (X::d(0.5) * X::d(h as f64)).r();
    if !(sub(pos[0], hw) <= p[0]) || !(p[0] <= add(hw, pos[0])) { return false; }
    if !(sub(pos[1], hh) <= p[1]) || !(p[1] <= add(hh, pos[1])) { return false; }
    if !ellipse { return true; }
    let dx = sub(p[0], pos[0]);
    let dy = sub(p[1], pos[1]);
    let v = (((dx * dx) / (x(hw) * hw)) + ((dy * dy) / (x(hh) * hh))).r();
    v <= 1.0
}

/// FUN_0047d3a0 convexity test on closed vertex loop.
fn is_convex(v: &[(f32, f32)]) -> bool {
    if v.len() <= 3 { return true; }
    let n = v.len();
    let mut sign = 0.0f32;
    let (mut px, mut py) = v[0];
    let mut ex = sub(px, v[n - 2].0).r();
    let mut ey = sub(py, v[n - 2].1).r();
    for i in 1..n {
        let fx = sub(v[i].0, px).r();
        let fy = sub(v[i].1, py).r();
        let cr = ((x(fy) * ex) - (x(fx) * ey)).r();
        if (sign < 0.0 && 0.0 < cr) || (0.0 < sign && cr < 0.0) { return false; }
        if sign == 0.0 && cr != 0.0 { sign = cr; }
        px = v[i].0;
        py = v[i].1;
        ex = fx;
        ey = fy;
    }
    true
}

/// CRT cos/sin (SSE2 double path) of an f32 angle, result stored to f32.
/// SIM_NO_FAST=1: disable the exact fast paths (ticking list, settled skip, mover tables, lazy poly lines) —
/// the reference behaviour for A/B regression checks
pub static COMBO_TRACE: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| std::env::var_os("COMBO_TRACE").is_some());
pub static NO_FAST: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| std::env::var_os("SIM_NO_FAST").is_some());
static CCD_DEBUG: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| std::env::var_os("SIM_CCD_DEBUG").is_some());
const PI_D: f64 = 3.1415927410125732; // @5d26d8
const TWO_PI_D: f64 = 6.2831854820251465; // @5d8fe0

/// FUN_0047bab0: bbox = union of line bboxes.
fn lines_bbox(lines: &[Line]) -> [f32; 4] {
    let mut b = [999999995904.0f32, 999999995904.0, -999999995904.0, -999999995904.0];
    for l in lines {
        if !(b[0] <= l.bbox[0]) { b[0] = l.bbox[0]; }
        if b[2] < l.bbox[2] { b[2] = l.bbox[2]; }
        if !(b[1] <= l.bbox[1]) { b[1] = l.bbox[1]; }
        if b[3] < l.bbox[3] { b[3] = l.bbox[3]; }
    }
    b
}

/// FUN_00484730 (rot == 0) / FUN_0047d5b0 (rot != 0): build world lines from vertex offsets;
/// offs[i] = line start − pos (0xf8 array, used by translation rebuilds).
pub fn build_poly_lines(verts: &[(f32, f32)], rot: f32, pos: [f32; 2], oneside: bool) -> (Vec<Line>, Vec<[f32; 2]>) {
    let (c, s) = (cos_r(rot), sin_r(rot));
    let rv = |v: (f32, f32)| -> (f32, f32) {
        if rot == 0.0 { return v; }
        (((x(v.0) * c) + (x(v.1) * s)).r(), ((x(v.1) * c) - (x(v.0) * s)).r())
    };
    let mut lines = vec![];
    let mut offs = vec![];
    let mut p0 = rv(verts[0]);
    for v in &verts[1..] {
        let p1 = rv(*v);
        let mut l = Line::new(add(p0.0, pos[0]).r(), add(pos[1], p0.1).r(), add(p1.0, pos[0]).r(), add(pos[1], p1.1).r());
        l.oneside = oneside;
        offs.push([sub(l.x1, pos[0]).r(), sub(l.y1, pos[1]).r()]);
        lines.push(l);
        p0 = p1;
    }
    (lines, offs)
}

/// FUN_0047a010: bbox of the vertices rotated by `a` about `pos` (no intermediate rounding).
/// `MoverTable`s for mover objects whose mover has no sub-mover, a pure (state-free) type and a period
/// <= 4096 whose outputs repeat exactly: entries are taken from counters [p, 2p) and checked against
/// [2p, 3p) (the table is used for counter >= p only).
fn build_mover_tables(objs: &[Obj], movers: &[crate::mover::Mover]) -> Vec<Option<MoverTable>> {
    let mut tabs: Vec<Option<MoverTable>> = (0..objs.len()).map(|_| None).collect();
    for (oi, o) in objs.iter().enumerate() {
        let Some(mi) = o.mover else { continue };
        let m0 = &movers[mi];
        let d = &m0.def;
        let typ = d.mover_type.abs();
        if m0.sub.is_some() || d.sub.is_some() || typ == 10 || typ > 13 { continue; }
        let p = if d.u2c > 0 { d.time as i32 + d.u34 as i32 + d.u2c as i32 } else { d.time as i32 };
        if p < 1 || p > 4096 { continue; }
        let mut m = m0.clone();
        let mut out = |k: i32| { m.eval(k); (m.pos, m.angle) };
        let a: Vec<_> = (p..2 * p).map(&mut out).collect();
        let b: Vec<_> = (2 * p..3 * p).map(&mut out).collect();
        let same = a.iter().zip(&b).all(|(u, v)| u.0[0].to_bits() == v.0[0].to_bits() && u.0[1].to_bits() == v.0[1].to_bits() && u.1.to_bits() == v.1.to_bits());
        if !same { continue; }
        let verts = if let Geom::Poly { verts, .. } = &o.geom { Some(verts.clone()) } else { None };
        // entry index = counter % p; entry j comes from counter c in [p, 2p) with c % p == j
        let mut e = vec![([0.0f32; 2], 0.0f32, [0.0f32; 4]); p as usize];
        for (k, (pos, ang)) in a.into_iter().enumerate() {
            let c = p + k as i32;
            let bb = verts.as_ref().map_or([0.0; 4], |v| rotated_bbox(v, ang, pos));
            e[(c % p) as usize] = (pos, ang, bb);
        }
        let w = (0..p as usize).map(|j| wrap_angvel(e[j].1, e[(j + p as usize - 1) % p as usize].1)).collect();
        tabs[oi] = Some(MoverTable { p, version: m0.version, e, w });
    }
    tabs
}

/// Poly::Update FUN_0047bd50: angular velocity a - rot wrapped into [-π, π]
fn wrap_angvel(a: f32, rot: f32) -> f32 {
    let mut w = sub(a, rot).r();
    if X::d(PI_D) < x(w) {
        while x(w) > X::d(PI_D) { w = (x(w) - X::d(TWO_PI_D)).r(); }
    }
    if !(X::d(-PI_D) <= x(w)) {
        while x(w) < X::d(-PI_D) { w = (x(w) + X::d(TWO_PI_D)).r(); }
    }
    w
}

fn rotated_bbox(verts: &[(f32, f32)], a: f32, pos: [f32; 2]) -> [f32; 4] {
    let (c, s) = (cos_r(a), sin_r(a));
    let mut b = [999999995904.0f32, 999999995904.0, -999999995904.0, -999999995904.0];
    for v in verts {
        let px = (((x(v.0) * c) + (x(v.1) * s)) + pos[0]).r();
        let py = (((x(v.1) * c) - (x(v.0) * s)) + pos[1]).r();
        if !(b[0] <= px) { b[0] = px; }
        if b[2] < px { b[2] = px; }
        if !(b[1] <= py) { b[1] = py; }
        if b[3] < py { b[3] = py; }
    }
    b
}

pub fn cos_r(a: f32) -> f32 { (a as f64).cos() as f32 }
pub fn sin_r(a: f32) -> f32 { (a as f64).sin() as f32 }

/// FUN_004bc2d0: rotate (px,py) about (cx,cy) by angle a (in place).
pub fn rotate_about(p: [f32; 2], c: [f32; 2], a: f32) -> [f32; 2] {
    let dx = sub(p[0], c[0]).r();
    let dy = sub(p[1], c[1]).r();
    let co = cos_r(a);
    let si = sin_r(a);
    let nx = ((x(si) * dy) + ((x(co) * dx) + c[0])).r();
    let ny = (((x(dy) * co) + c[1]) - (x(si) * dx)).r();
    [nx, ny]
}

/// Gun geometry (Board+0xbc). Defaults observed in the oracle (all levels so far).
#[derive(Clone, Debug)]
pub struct Gun {
    pub pos: [f32; 2],  // 0x114/0x118 pivot
    pub off16c: f32,    // 0x16c
    pub off170: f32,    // 0x170
    pub speed: f32,     // 0x160
}

impl Default for Gun {
    fn default() -> Gun { Gun { pos: [325.0, 30.0], off16c: 60.0, off170: 26.0, speed: 4.0 } }
}

impl Gun {
    /// Custom gun geometry (pivot, barrel offsets +0x16c/+0x170, launch speed +0x160).
    pub fn new(pos: [f32; 2], off16c: f32, off170: f32, speed: f32) -> Gun {
        Gun { pos, off16c, off170, speed }
    }

    /// FUN_00484120 (`Gun::Update` angle slew): step `cur` (+0x150) toward `target` (+0x154) by
    /// ModVal Gun.cpp24 = 0.2 rad/tick (wrapping angles < 0 by +2π during comparison), clamping
    /// both through `aim::norm` on arrival.
    pub fn update_slew(cur: &mut f32, target: &mut f32) {
        if *target == *cur { return; }
        let mut t = *target;
        let mut c = *cur;
        let two_pi = TWO_PI_D as f32;
        if t < 0.0 { t = add(t, two_pi).r(); }
        if c < 0.0 { c = add(c, two_pi).r(); }
        let step = 0.20000000298023224f32; // _DAT_005d81f4
        if t <= c {
            *cur = sub(*cur, step).r();
            if sub(c, step).r() <= t {
                let n = crate::aim::norm(*target);
                *target = n;
                *cur = n;
            }
        } else {
            *cur = add(step, *cur).r();
            if t <= add(step, c).r() {
                let n = crate::aim::norm(*target);
                *target = n;
                *cur = n;
            }
        }
    }

    /// FUN_00484530 (`Gun::Fire` <10-frame old-angle fallback): when `use_old` is true and fewer
    /// than 10 gun ticks elapsed since the last aim change (`gun_ticks - last_change_tick < 10`),
    /// the shot launches at `old_angle` (+0x188) instead of `cur_angle` (+0x150).
    pub fn fire_angle(cur_angle: f32, old_angle: f32, ticks_since_change: u32, use_old: bool) -> f32 {
        if use_old && ticks_since_change < 10 { old_angle } else { cur_angle }
    }

    /// Ball placement (FUN_00483da0 tail) and launch velocity (Gun::Fire FUN_00484530).
    pub fn launch(&self, angle: f32, ball_r: f32) -> Ball {
        let lx = (x(self.off170) + ball_r).r();
        let lx = (x(lx) + self.pos[0]).r();
        let ly = (X::d(1.0) + self.pos[1]).r(); // (float)(int)ModVal Gun.cpp#17 (=1) + y
        let lx = (x(lx) + self.off16c).r();
        let p = rotate_about([lx, ly], self.pos, angle);
        let si = sin_r(angle);
        let co = cos_r(angle);
        let vy = ((-x(self.speed)) * si).r();
        let vx = (x(self.speed) * co).r();
        let mut b = Ball::new(p, [vx, vy]);
        b.r = ball_r;
        b.set_pos(p);
        b
    }
}

fn two_mut<T>(v: &mut [T], i: usize, j: usize) -> (&mut T, &mut T) {
    assert!(i != j);
    if i < j { let (a, b) = v.split_at_mut(j); (&mut a[i], &mut b[0]) } else { let (a, b) = v.split_at_mut(i); (&mut b[0], &mut a[j]) }
}

/// FUN_0047a3b0 (this = `a`, the lower-id ball): overlap test d² <= (rb + ra - 0.01)², then (neither
/// immovable) FUN_00477110: equal-mass impulse along the centre line with restitution 0.95 and a
/// half-and-half positional separation. Rounding follows the x87 code (0x477110..0x47731b).
fn ball_ball(a: &mut Ball, b: &mut Ball) {
    const EPS: f64 = 0.009999999776482582; // DAT_005d5be8
    let dx = sub(a.pos[0], b.pos[0]).r();
    let dy = sub(a.pos[1], b.pos[1]).r();
    let t = ((x(b.r) + a.r) - X::d(EPS)).r();
    let dx2 = (x(dx) * dx).r();
    let dy2 = (x(dy) * dy).r();
    let s = (x(dx2) + dy2).r();
    let t2 = (x(t) * t).r();
    if !(s <= t2) { return; }
    let n = normalize([sub(b.pos[0], a.pos[0]).r(), sub(b.pos[1], a.pos[1]).r()]);
    let (va, vb) = (a.vel, b.vel);
    let dvx = sub(va[0], vb[0]).r();
    let dvy = sub(va[1], vb[1]).r();
    let mut p = ((x(n[1]) * dvy) + (x(dvx) * n[0])).r();
    p = (x(p) * X::d(0.949999988079071)).r(); // DAT_005f0bb0 (double)
    p = (x(p) + p).r();
    a.last_impulse = p.abs();
    b.last_impulse = p.abs();
    let q = (x(p) * X::d(0.5)).r();
    let nq = -q;
    a.vel = [(x(va[0]) + (x(n[0]) * nq).r()).r(), (x(va[1]) + (x(n[1]) * nq).r()).r()];
    b.vel = [(x(vb[0]) + (x(n[0]) * q).r()).r(), (x(vb[1]) + (x(n[1]) * q).r()).r()];
    // separation
    let ddx = sub(a.pos[0], b.pos[0]).r();
    let ddy = sub(a.pos[1], b.pos[1]).r();
    let sumr = a.r as f64 + b.r as f64;
    let d2 = (x((x(ddx) * ddx).r()) + (x(ddy) * ddy).r()).r();
    let dist = sqrt_r(d2);
    let over = (X::d(sumr) - dist).r();
    if over > 0.0 {
        let px = (x(n[0]) * over).r();
        let py = (x(over) * n[1]).r();
        let h = [(X::d(0.5) * px).r(), (X::d(0.5) * py).r()];
        b.move_by(h);
        a.move_by([-h[0], -h[1]]);
    }
}
