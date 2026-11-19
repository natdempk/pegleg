//! oneshot: search for levels won by their first shot (all 25 oranges in one shot), over many RNG seeds.
//!
//! The hook's seed only decides colours (oranges, purple, greens) and keyed luck rolls, not geometry. So a
//! first shot's path is the same for every seed as long as the master's power does not change the ball's
//! physics on that shot:
//! - class free (Bjorn; Hu and Cinderbottom too: Super Guide / Zen / Fireball only act on later shots): the
//!   shot's hit set H is seed-independent; a seed wins iff its oranges ⊆ H.
//! - class tula (Flower Power lights the ceil(0.2 * oranges left) un-hit oranges nearest a green when it is
//!   hit) and splork (Space Blast lights every un-hit peg within 100 of the green, greens chain): lit pegs
//!   come from forced contacts, the ball's path is unchanged, so the lit set follows from the shot's ordered
//!   hits + the seed's colours (peg positions at the hit tick).
//! Pipeline: lib (physics, once per level: a dense fan of angles x click delays, all-blue board) → scan (per
//! seed: colours via the hook's reseeds (replica of colors.rs, checked against Game::new), best shot from the
//! library, integer work only) → verify (exact Game sim of every filter win, writes a plan).
//! Seeds are scanned in chunks of 2^24 (256 chunks = the whole u32 seed space); a finished chunk is one line
//! in DIR/ledger/HOST.tsv (append-only, one file per machine, so ledgers from several machines just merge).
//!
//! oneshot lib DIR LEVEL... [--angles N] [--delays D] [--hmin H]   build DIR/lib/LV.lib
//! oneshot scan DIR LEVEL CLASS [--chunks A-B] [--desc] [--host H] [--max N]   scan unfinished chunks
//! oneshot check DIR LEVEL CLASS [N]     colour replica + filter vs exact sim on N random (seed, shot) pairs
//! oneshot verify DIR [LEVEL...]         exact sim of the filter wins (and near misses) → DIR/verified.tsv, DIR/wins/
//! oneshot status DIR                    coverage and best results per level and class
//! oneshot next DIR [--classes C,..] [--desc] [--gpu]   the queue: prints "LEVEL CLASS CHUNK EST" to scan next
//! oneshot cont DIR LEVEL CLASS          build a physics class's continuation library (multi, spooky, flip, ...)
//! oneshot bench / probe / gpucheck      scan speed; power reach; GPU kernel vs CPU scanner on one range
//! oneshot spooky ... (oneshot/spooky.rs), flipcov / flipcheck / flipdbg / flipinfo / flipfinal (oneshot/flipcov.rs),
//!   fliptimes / fastscan / fastcheck (oneshot/fast.rs), flipplan (beam search over flips for one seed)
//! See solver/ONESHOT.md for the workflow.
use peggle_sim::game::{Game, FIRE_LAG};
use peggle_sim::rng::{tas_mix, Mt};
use peggle_sim::score::{BLUE, GREEN, ORANGE};
use peggle_solver::{levels, search::*};
use rayon::prelude::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;
use std::time::Instant;

#[path = "oneshot/spooky.rs"]
mod spooky;
#[path = "oneshot/flipcov.rs"]
mod flipcov;
#[path = "oneshot/fast.rs"]
mod fast;

const W: usize = 4;
type Bits = [u64; W];
pub const CHUNK_BITS: u32 = 24;
#[inline] fn bset(b: &mut Bits, i: usize) { b[i >> 6] |= 1u64 << (i & 63); }
#[inline] fn bget(b: &Bits, i: usize) -> bool { b[i >> 6] >> (i & 63) & 1 != 0 }
#[inline] fn pc(b: &Bits) -> u32 { b.iter().map(|w| w.count_ones()).sum() }
#[inline] fn pc_and(a: &Bits, b: &Bits) -> u32 { (0..W).map(|k| (a[k] & b[k]).count_ones()).sum() }

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum Class { Free, Tula, Splork, Multi, Spooky, Pyramid, Claude, Hat, Flip }
impl Class {
    fn parse(s: &str) -> Class { match s { "free" => Class::Free, "tula" => Class::Tula, "splork" => Class::Splork, "multi" => Class::Multi, "spooky" => Class::Spooky, "pyramid" => Class::Pyramid, "claude" => Class::Claude, "hat" => Class::Hat, "flip" => Class::Flip, _ => panic!("class free|tula|splork|multi|spooky|pyramid|claude|hat|flip") } }
    fn name(self) -> &'static str { match self { Class::Free => "free", Class::Tula => "tula", Class::Splork => "splork", Class::Multi => "multi", Class::Spooky => "spooky", Class::Pyramid => "pyramid", Class::Claude => "claude", Class::Hat => "hat", Class::Flip => "flip" } }
    /// the character played (Choose Character index)
    fn char(self) -> usize { match self { Class::Free => 0, Class::Tula => 6, Class::Splork => 3, Class::Multi => 1, Class::Spooky => 5, Class::Pyramid => 2, Class::Claude => 4, Class::Hat => 7, Class::Flip => 4 } }
    /// the power changes the ball's physics in the shot: continuation library (`Cont`). Pyramid (Kat): the
    /// "bumperhole" pyramid switches on at the first green; Claude: the flippers switch on (never pressed here; the
    /// flipper_clear relocation of oranges in the corner regions is not modelled: such seeds just score lower)
    /// Hat (Warren): the continuation as if the Lucky Spin at the first green gave the Magic Hat (a non-colliding
    /// 20-radius ball above the real one that lights what it touches: the path is unchanged); the real wheel result
    /// is keyed by the seed (about 1 in 4), so the exact verify keeps only seeds whose spin gives the hat.
    /// Flip (Claude): seed-independent flip schedules (flipcov.rs), flipper_clear relocation exact per seed
    fn physics(self) -> bool { matches!(self, Class::Multi | Class::Spooky | Class::Pyramid | Class::Claude | Class::Hat | Class::Flip) }
    fn power(self) -> u8 { peggle_sim::oracle::CHAR_POWERS[self.char()] }
}

fn level_id(st: usize, ix: usize) -> String { format!("{}-{}", st + 1, ix + 1) }
fn all_levels() -> Vec<String> { (0..11).flat_map(|s| (0..5).map(move |i| level_id(s, i))).collect() }

/// MT19937 outputs for the colour shuffles (FUN_004fce50 Rand(): genrand_int32 & 0x7fffffff), eager, the
/// first `n` draws only; same values as rng::Mt (checked by check_colours)
#[inline]
fn mt_draws(seed: u32, out: &mut [u32; 2000]) {
    let mut mt = [0u32; 624];
    mt[0] = if seed == 0 { 0x1105 } else { seed };
    for i in 1..624 { mt[i] = (mt[i - 1] ^ (mt[i - 1] >> 30)).wrapping_mul(0x6c078965).wrapping_add(i as u32); }
    let mut k = 0;
    while k < 2000 {
        for i in 0..624 {
            let y = (mt[i] & 0x8000_0000) | (mt[(i + 1) % 624] & 0x7fff_ffff);
            mt[i] = mt[(i + 397) % 624] ^ (y >> 1) ^ if y & 1 != 0 { 0x9908_b0df } else { 0 };
        }
        for i in 0..624.min(2000 - k) {
            let mut y = mt[i];
            y ^= y >> 11; y ^= (y << 7) & 0x9d2c_5680; y ^= (y << 15) & 0xefc6_0000; y ^= y >> 18;
            out[k + i] = y & 0x7fff_ffff;
        }
        k += 624;
    }
}

/// x % d for u32 x (Lemire's fastmod; exact for every 32-bit x and d)
#[derive(Clone, Copy)]
struct FastMod { m: u64, d: u32 }
impl FastMod {
    fn new(d: u32) -> FastMod { FastMod { m: (u64::MAX / d.max(1) as u64).wrapping_add(1), d } }
    #[inline] fn rem(self, x: u32) -> usize { (((self.m.wrapping_mul(x as u64)) as u128 * self.d as u128) >> 64) as usize }
}

/// the hook's 1000-swap shuffle (FUN_0043d430) of v with the reseed tas_mix(seed, n)
#[inline]
fn shuffle(seed: u32, n: u32, v: &mut [u8]) {
    if v.len() < 2 { return; }
    let mut r = [0u32; 2000];
    mt_draws(tas_mix(seed, n), &mut r);
    let f = FastMod::new(v.len() as u32);
    for k in 0..1000 { let a = f.rem(r[2 * k]); let c = f.rem(r[2 * k + 1]); v.swap(a, c); }
}

/// A level's colour model and all-blue physics template.
pub struct Lvl {
    pub st: usize, pub ix: usize, pub id: String,
    /// all pegs blue, orange count unreachable: the physics of any seed's first shot up to its last orange
    pub tmpl: Game,
    pub obj: Vec<usize>,
    pub bit: Vec<i16>,
    pub vars: Vec<u8>,
    pub fixed: Bits, pub nfixed: u32,
    pub coll: Vec<bool>, pub var: Vec<bool>,
    pub pos0: Vec<[f32; 2]>,
    /// pegs that can be orange (variable or fixed orange)
    pub orangeable: Bits,
}

impl Lvl {
    pub fn new(id: &str) -> Lvl {
        let (st, ix) = levels::parse_level_id(id).expect("level");
        let mut g = levels::new_game(st, ix, 1, 1);
        g.earliest_fire = rt::FIRST_CLICK;
        let order = peggle_sim::colors::peg_order(&g.board);
        assert!(order.len() <= W * 64, "{id}: {} pegs", order.len());
        let mut bit = vec![-1i16; g.board.objs.len()];
        for (b, &i) in order.iter().enumerate() { bit[i] = b as i16; }
        let (mut fixed, mut nfixed, mut orangeable) = ([0; W], 0, [0; W]);
        let mut vars = vec![];
        for (b, &i) in order.iter().enumerate() {
            let o = &g.board.objs[i];
            if o.variable { vars.push(b as u8); bset(&mut orangeable, b); }
            else if o.peg_type == ORANGE { bset(&mut fixed, b); nfixed += 1; bset(&mut orangeable, b); }
        }
        let coll = order.iter().map(|&i| g.board.objs[i].collidable).collect();
        let var = order.iter().map(|&i| g.board.objs[i].variable).collect();
        let pos0 = order.iter().map(|&i| g.board.obj_pos(g.board.objs[i].uid).unwrap_or([0.0; 2])).collect();
        for o in g.board.objs.iter_mut() { if o.has_peginfo && o.peg_type != 0 { o.peg_type = BLUE; } }
        g.board.orange_total = 1000;
        g.purple = None;
        Lvl { st, ix, id: level_id(st, ix), tmpl: g, obj: order, bit, vars, fixed, nfixed, coll, var, pos0, orangeable }
    }
    pub fn npeg(&self) -> usize { self.obj.len() }

    /// colors::assign_oranges with the hook's reseed n = 0
    #[inline]
    pub fn oranges(&self, seed: u32, v: &mut Vec<u8>) -> Bits {
        v.clear(); v.extend_from_slice(&self.vars);
        shuffle(seed, 0, v);
        let mut o = self.fixed;
        let need = 25usize.saturating_sub(self.nfixed as usize);
        for &b in v.iter().rev().take(need) { bset(&mut o, b as usize); }
        o
    }

    /// colors::purple_reroll (n = 1) then colors::assign_greens (n = 2): the green pegs
    pub fn greens(&self, seed: u32, o: &Bits, v: &mut Vec<u8>) -> Bits {
        let np = self.npeg();
        let inset = |b: usize| self.coll[b] && !bget(o, b);
        let ncand = (0..np).filter(|&b| inset(b)).count();
        let mut purple = usize::MAX;
        if ncand > 0 {
            let mut m = Mt::new(tas_mix(seed, 1));
            let k = (m.rand() % ncand as i32) as usize;
            purple = (0..np).filter(|&b| inset(b)).nth(k).unwrap();
        }
        v.clear();
        v.extend((0..np).filter(|&b| inset(b) && b != purple && self.pos0[b][1] < 370.0).map(|b| b as u8));
        if v.is_empty() { v.extend((0..np).filter(|&b| inset(b) && b != purple && self.var[b]).map(|b| b as u8)); }
        let mut gr = [0; W];
        if v.is_empty() { return gr; }
        shuffle(seed, 2, v);
        let mut need = 2;
        let mut last = [-1000.0f32, -1000.0f32];
        for pass in 0..2 {
            for &b in v.iter() {
                if need <= 0 { return gr; }
                let b = b as usize;
                if bget(&gr, b) { continue; }
                if pass == 0 {
                    let p = self.pos0[b];
                    let dx = last[0] - p[0];
                    let dy = last[1] - p[1];
                    let s = dx * dx + dy * dy;
                    if 22500.0f32 > s { continue; }
                    last = p;
                }
                bset(&mut gr, b);
                need -= 1;
            }
        }
        gr
    }

    /// the click tick of delay d (frames after the earliest first click, fast forward frame grid)
    pub fn click(&self, d: u8) -> i32 { let p = Params::default(); grid_click(&p, 0, earliest(&p, &self.tmpl), d as i32) }
}

#[derive(Clone)]
pub struct Shot { pub ai: u32, pub d: u8, pub hits: Vec<u8>, pub ticks: Vec<u16>, pub set: Bits }

pub struct Lib { pub shots: Vec<Shot>, pub hash: String, pub params: String }

fn lib_path(dir: &str, id: &str) -> String { format!("{dir}/lib/{id}.lib") }

fn fnv(b: &[u8]) -> u64 { let mut h = 0xcbf29ce484222325u64; for &x in b { h ^= x as u64; h = h.wrapping_mul(0x100000001b3); } h }

fn build_lib(l: &Lvl, n_ang: usize, ds: &[u8], hmin: usize, keep: usize, keep_power: usize) -> Vec<Shot> {
    let angles = Angles::new();
    let fan = angles.coarse(n_ang);
    let mut out = vec![];
    let mut seen: HashSet<Vec<u8>> = HashSet::new();
    for &d in ds {
        let click = l.click(d);
        let mut base = l.tmpl.clone();
        base.advance_to(click + FIRE_LAG);
        let v: Vec<Shot> = fan.par_iter().filter_map(|&ai| {
            let mut h = base.clone();
            h.shoot(angles.list[ai].0, click);
            let b = &h.board;
            if b.shot_hits.len() < hmin { return None; }
            let hits: Vec<u8> = b.shot_hits.iter().map(|&i| l.bit[i] as u8).collect();
            let ticks = b.shot_hit_bticks.iter().map(|&t| t.clamp(0, 65535) as u16).collect();
            let mut set = [0; W]; for &x in &hits { bset(&mut set, x as usize); }
            Some(Shot { ai: ai as u32, d, hits, ticks, set })
        }).collect();
        for s in v { if seen.insert(s.hits.clone()) { out.push(s); } }
    }
    // the scan's cost grows with the library and wins come from the biggest shots: keep the top `keep`; with
    // keep_power also the top keep_power by Splork reach (ranking only: start positions, two best blasts)
    let reach: Vec<u32> = if keep_power > 0 { splork_reach(l, &out) } else { vec![] };
    let mut idx: Vec<usize> = (0..out.len()).collect();
    idx.sort_by_key(|&k| (std::cmp::Reverse(pc_and(&out[k].set, &l.orangeable)), k));
    let mut take: Vec<usize> = idx.iter().copied().take(keep).collect();
    if keep_power > 0 {
        let mut by: Vec<usize> = (0..out.len()).collect();
        by.sort_by_key(|&k| (std::cmp::Reverse(reach[k]), k));
        let have: HashSet<usize> = take.iter().copied().collect();
        take.extend(by.into_iter().filter(|k| !have.contains(k)).take(keep_power));
        take.sort_by_key(|&k| (std::cmp::Reverse(pc_and(&out[k].set, &l.orangeable)), k));
    }
    let mut out: Vec<Option<Shot>> = out.into_iter().map(Some).collect();
    take.into_iter().map(|k| out[k].take().unwrap()).collect()
}

/// library ranking for Splork: orange-able pegs in H ∪ the two largest blasts (radius 100, start positions) of
/// pegs in H that can be green
fn splork_reach(l: &Lvl, shots: &[Shot]) -> Vec<u32> {
    let np = l.npeg();
    let gc = green_cands(l);
    let blast: Vec<Bits> = (0..np).map(|p| { let mut b = [0u64; W]; for q in 0..np { if l.coll[q] && d2(l.pos0[q], l.pos0[p]) <= 10000.0 { bset(&mut b, q); } } b }).collect();
    shots.par_iter().map(|s| {
        let base = pc_and(&s.set, &l.orangeable);
        let mut gains: Vec<(u32, usize)> = s.hits.iter().map(|&p| p as usize).filter(|&p| bget(&gc, p)).map(|p| {
            let mut u = s.set; for x in 0..W { u[x] |= blast[p][x]; } (pc_and(&u, &l.orangeable) - base, p) }).collect();
        gains.sort_by(|a, b| b.cmp(a));
        let mut u = s.set;
        for &(_, p) in gains.iter().take(2) { for x in 0..W { u[x] |= blast[p][x]; } }
        pc_and(&u, &l.orangeable)
    }).collect()
}

fn save_lib(path: &str, params: &str, shots: &[Shot]) {
    let mut b: Vec<u8> = vec![];
    b.extend(b"OSL1");
    b.extend((params.len() as u32).to_le_bytes()); b.extend(params.as_bytes());
    b.extend((shots.len() as u32).to_le_bytes());
    for s in shots {
        b.extend(s.ai.to_le_bytes()); b.push(s.d); b.push(s.hits.len() as u8);
        b.extend(&s.hits); for t in &s.ticks { b.extend(t.to_le_bytes()); }
    }
    std::fs::create_dir_all(std::path::Path::new(path).parent().unwrap()).unwrap();
    std::fs::write(format!("{path}.tmp"), &b).unwrap();
    std::fs::rename(format!("{path}.tmp"), path).unwrap();
}

fn load_lib(path: &str) -> Lib {
    let b = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e} (run oneshot lib first)"));
    assert_eq!(&b[..4], b"OSL1");
    let hash = format!("{:08x}", fnv(&b) as u32);
    let mut p = 4;
    let u32at = |p: usize| u32::from_le_bytes(b[p..p + 4].try_into().unwrap());
    let pl = u32at(p) as usize; p += 4;
    let params = String::from_utf8(b[p..p + pl].to_vec()).unwrap(); p += pl;
    let n = u32at(p) as usize; p += 4;
    let mut shots = Vec::with_capacity(n);
    for _ in 0..n {
        let ai = u32at(p); let d = b[p + 4]; let nh = b[p + 5] as usize; p += 6;
        let hits = b[p..p + nh].to_vec(); p += nh;
        let ticks = (0..nh).map(|k| u16::from_le_bytes([b[p + 2 * k], b[p + 2 * k + 1]])).collect(); p += 2 * nh;
        let mut set = [0; W]; for &x in &hits { bset(&mut set, x as usize); }
        shots.push(Shot { ai, d, hits, ticks, set });
    }
    Lib { shots, hash, params }
}

/// Physics powers (Multiball, Spooky Ball): the shot (ai, d) with peg p as the first green it hits. `pre` = the
/// pegs hit before p (the seed's other green must not be among them), `set` = everything the shot hits then.
/// Approximate: the seed's second green acts as blue, and Spooky's respawn trials use this template's keyed RNG
/// (the real ones are keyed by the seed), so filter results are verified exactly.
#[derive(Clone)]
pub struct Cont { pub ai: u32, pub d: u8, pub p: u8, pub pre: Bits, pub set: Bits, /** Spooky: the other green, hit between p and the first respawn (armed twice), 255 = none */ pub q: u8 }

fn cont_path(dir: &str, id: &str, cls: Class) -> String { format!("{dir}/lib/{id}.{}.cont", cls.name()) }

/// pegs that can be green (assign_greens candidates: y < 370, or variable pegs when none is left)
fn green_cands(l: &Lvl) -> Bits {
    let mut m = [0u64; W];
    for b in 0..l.npeg() { if l.coll[b] && (l.pos0[b][1] < 370.0 || l.var[b]) { bset(&mut m, b); } }
    m
}

fn build_cont(l: &Lvl, cls: Class, n_ang: usize, delays: u8, cmin: u32, keep: usize) -> Vec<Cont> {
    let angles = Angles::new();
    let fan = angles.coarse(n_ang);
    let gc = green_cands(l);
    let mut out = vec![];
    for d in 0..delays {
        let click = l.click(d);
        let mut base = l.tmpl.clone();
        base.advance_to(click + FIRE_LAG);
        let v: Vec<Cont> = fan.par_iter().flat_map_iter(|&ai| {
            let mut h = base.clone();
            h.shoot(angles.list[ai].0, click);
            let hits: Vec<usize> = h.board.shot_hits.clone();
            let mut pre = [0u64; W];
            let mut res = vec![];
            for &i in &hits {
                let p = l.bit[i] as usize;
                if bget(&gc, p) {
                    let mut g = base.clone();
                    g.board.objs[i].peg_type = GREEN;
                    g.board.power = cls.power();
                    if cls == Class::Hat { g.board.force_lucky = Some(1); }
                    g.shoot(angles.list[ai].0, click);
                    let mut set = [0u64; W];
                    for &j in &g.board.shot_hits { bset(&mut set, l.bit[j] as usize); }
                    if pc_and(&set, &l.orangeable) >= cmin { res.push(Cont { ai: ai as u32, d, p: p as u8, pre, set, q: 255 }); }
                }
                bset(&mut pre, p);
            }
            res.into_iter()
        }).collect();
        out.extend(v);
    }
    out.sort_by_key(|c| std::cmp::Reverse(pc_and(&c.set, &l.orangeable)));
    out.truncate(keep);
    out
}

fn save_cont(path: &str, params: &str, v: &[Cont]) {
    let mut b: Vec<u8> = vec![];
    let v2 = v.iter().any(|c| c.q != 255);
    b.extend(if v2 { b"OSC2" } else { b"OSC1" });
    b.extend((params.len() as u32).to_le_bytes()); b.extend(params.as_bytes());
    b.extend((v.len() as u32).to_le_bytes());
    for c in v {
        b.extend(c.ai.to_le_bytes()); b.push(c.d); b.push(c.p);
        for w in c.pre.iter().chain(c.set.iter()) { b.extend(w.to_le_bytes()); }
        if v2 { b.push(c.q); }
    }
    std::fs::write(format!("{path}.tmp"), &b).unwrap();
    std::fs::rename(format!("{path}.tmp"), path).unwrap();
}

fn load_cont(path: &str) -> (Vec<Cont>, u64) {
    let b = std::fs::read(path).unwrap();
    assert!(&b[..4] == b"OSC1" || &b[..4] == b"OSC2");
    let v2 = &b[..4] == b"OSC2";
    let u32at = |p: usize| u32::from_le_bytes(b[p..p + 4].try_into().unwrap());
    let mut p = 4;
    let pl = u32at(p) as usize; p += 4 + pl;
    let n = u32at(p) as usize; p += 4;
    let mut v = Vec::with_capacity(n);
    let w64 = |p: usize| u64::from_le_bytes(b[p..p + 8].try_into().unwrap());
    for _ in 0..n {
        let (ai, d, pp) = (u32at(p), b[p + 4], b[p + 5]); p += 6;
        let mut pre = [0u64; W]; let mut set = [0u64; W];
        for k in 0..W { pre[k] = w64(p + 8 * k); set[k] = w64(p + 8 * (W + k)); }
        p += 16 * W;
        let q = if v2 { let q = b[p]; p += 1; q } else { 255 };
        v.push(Cont { ai, d, p: pp, pre, set, q });
    }
    (v, fnv(&b))
}

/// the ledger key of a class's search space: the library hash (with the continuation library's for physics classes)
fn class_hash(dir: &str, id: &str, cls: Class) -> Option<String> {
    let b = std::fs::read(lib_path(dir, id)).ok()?;
    let h = fnv(&b);
    if !cls.physics() { return Some(format!("{:08x}", h as u32)); }
    let c = std::fs::read(cont_path(dir, id, cls)).map(|b| fnv(&b)).unwrap_or(0);
    Some(format!("{:08x}", (h ^ c.rotate_left(17)) as u32))
}

/// ln C(n, k)
fn lnc(n: u32, k: u32) -> f64 { if k > n { return f64::NEG_INFINITY; } (0..k).map(|i| ((n - i) as f64).ln() - ((i + 1) as f64).ln()).sum() }

/// Free class: P(a random seed's oranges ⊆ H) summed over the distinct sets (union bound; oranges ~ uniform
/// over the variable pegs)
fn free_estimate(l: &Lvl, lib: &Lib) -> (f64, u32) {
    let v = l.vars.len() as u32;
    let need = 25u32.saturating_sub(l.nfixed);
    let mut seen = HashSet::new();
    let (mut p, mut hmax) = (0.0, 0);
    for s in &lib.shots {
        if !seen.insert(s.set) { continue; }
        let fixed_in = pc_and(&s.set, &l.fixed);
        let hv = pc_and(&s.set, &l.orangeable) - fixed_in;
        hmax = hmax.max(hv + fixed_in);
        if fixed_in < l.nfixed { continue; }
        p += (lnc(hv, need) - lnc(v, need)).exp();
    }
    (p, hmax)
}

/// Peg positions per Board tick (for the power filters); one row if no peg moves.
pub struct PosTab { pub rows: Vec<Vec<[f32; 2]>>, pub t0: i32 }
impl PosTab {
    fn new(l: &Lvl, lib: &Lib) -> PosTab {
        let maxt = lib.shots.iter().flat_map(|s| s.ticks.iter()).copied().max().unwrap_or(0) as i32;
        let mut b = l.tmpl.board.clone();
        let t0 = b.btick;
        let mut rows = vec![];
        while b.btick <= maxt + 1 {
            rows.push(l.obj.iter().map(|&i| b.obj_pos(b.objs[i].uid).unwrap_or([0.0; 2])).collect::<Vec<_>>());
            b.tick_objects();
        }
        if rows.iter().all(|r| *r == rows[0]) { rows.truncate(1); }
        PosTab { rows, t0 }
    }
    #[inline] fn at(&self, t: u16) -> &[[f32; 2]] { let k = (t as i32 - self.t0).clamp(0, self.rows.len() as i32 - 1) as usize; &self.rows[k] }
}

#[inline] fn d2(a: [f32; 2], b: [f32; 2]) -> f64 { let dx = (a[0] - b[0]) as f64; let dy = (a[1] - b[1]) as f64; dx * dx + dy * dy }

/// Tula / Splork: oranges lit by the shot for this seed (forced contacts replayed on the shot's ordered hits)
fn power_eval(l: &Lvl, cls: Class, s: &Shot, o: &Bits, gr: &Bits, pos: &PosTab, near: &[Vec<u8>]) -> u32 {
    let mut lit = [0u64; W];
    let mut left = pc(o) as i32;
    let np = l.npeg();
    let mut queue: Vec<usize> = vec![];
    for (j, &p) in s.hits.iter().enumerate() {
        let p = p as usize;
        if bget(&lit, p) { continue; }
        bset(&mut lit, p);
        if bget(o, p) { left -= 1; }
        if !bget(gr, p) { continue; }
        let ps = pos.at(s.ticks[j]);
        match cls {
            Class::Tula if !near.is_empty() => {
                // static board: pegs by distance from p (ties in object order), precomputed
                let mut k = ((left as f32 as f64 * 0.20000000298023224) as f32).ceil() as usize;
                for &q in &near[p] {
                    if k == 0 { break; }
                    let q = q as usize;
                    if bget(o, q) && !bget(&lit, q) { bset(&mut lit, q); left -= 1; k -= 1; }
                }
            }
            Class::Tula => {
                let k = ((left as f32 as f64 * 0.20000000298023224) as f32).ceil() as usize;
                let mut c: Vec<(f64, usize, usize)> = (0..np).filter(|&q| bget(o, q) && !bget(&lit, q) && l.coll[q]).map(|q| (d2(ps[q], ps[p]) as f32 as f64, l.obj[q], q)).collect();
                c.sort_by(|a, b| a.partial_cmp(b).unwrap());
                for &(_, _, q) in c.iter().take(k) { bset(&mut lit, q); left -= 1; }
            }
            Class::Splork => {
                // the ball's own later hits stay hits, so only blasted pegs matter: H ∪ blasts (greens chain)
                queue.clear(); queue.push(p);
                while let Some(g) = queue.pop() {
                    for q in 0..np {
                        if bget(&lit, q) || !l.coll[q] || d2(ps[q], ps[g]) > 10000.0 { continue; }
                        bset(&mut lit, q);
                        if bget(gr, q) { queue.push(q); }
                    }
                }
                let _ = left;
            }
            Class::Free | Class::Multi | Class::Spooky | Class::Pyramid | Class::Claude | Class::Hat | Class::Flip => {}
        }
    }
    pc_and(&lit, o)
}

/// The per-seed evaluator: the library prepared for one class.
pub struct Scanner<'a> {
    l: &'a Lvl, cls: Class, lib: &'a Lib, pos: PosTab,
    /// (hit set, size of its orangeable part, shot index), largest first
    order: Vec<(Bits, u32, u32)>,
    /// Splork: every peg within blast range of p at any tick (p's own blast superset)
    nmax: Vec<Bits>,
    /// results below this are only counted as "below" (no exact work spent on them)
    pub floor: u32,
    /// per peg: positions in `order` of the shots that hit it (ascending = largest first)
    inv: Vec<Vec<u32>>,
    /// Tula on a static board: per peg, the coll pegs by distance (ties in object order)
    near: Vec<Vec<u8>>,
    /// Splork on a static board: the exact blast set of each peg
    nstat: Vec<Bits>,
    /// physics classes: continuation entries, and per peg the entries with that first green (largest first)
    conts: Vec<Cont>,
    cinv: Vec<Vec<(u32, u32)>>,
    /// Spooky: the exact evaluator of the continuation entries (spooky.rs)
    exact: Option<spooky::Exact>,
    /// flip class: flipper_clear's hiding + relocation per entry (exact per seed, CPU only)
    reloc: Option<flipcov::Reloc>,
}

impl<'a> Scanner<'a> {
    pub fn with_conts(mut self, conts: Vec<Cont>) -> Self {
        let np = self.l.npeg();
        let mut cinv: Vec<Vec<(u32, u32)>> = vec![vec![]; np];
        // flip: relocated oranges can land on any peg of the set (bound = all its pegs, + the unplaceable ones)
        let flip = self.cls == Class::Flip;
        for (k, c) in conts.iter().enumerate() { cinv[c.p as usize].push((if flip { pc(&c.set) + 2 } else { pc_and(&c.set, &self.l.orangeable) }, k as u32)); }
        if flip { self.reloc = Some(flipcov::reloc(self.l, &conts)); }
        for v in cinv.iter_mut() { v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1))); }
        self.conts = conts; self.cinv = cinv;
        self
    }

    pub fn new(l: &'a Lvl, cls: Class, lib: &'a Lib, floor: u32) -> Scanner<'a> {
        let pos = PosTab::new(l, lib);
        let np = l.npeg();
        let mut nmax = vec![[0u64; W]; np];
        if cls == Class::Splork {
            for row in &pos.rows { for p in 0..np { for q in 0..np { if d2(row[q], row[p]) <= 10000.5 { bset(&mut nmax[p], q); } } } }
        }
        let mut seen = HashSet::new();
        let mut order: Vec<(Bits, u32, u32)> = lib.shots.iter().enumerate().filter_map(|(k, s)| {
            if cls == Class::Free && !seen.insert(s.set) { return None; }
            Some((s.set, pc_and(&s.set, &l.orangeable), k as u32))
        }).collect();
        order.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)));
        let mut inv = vec![vec![]; np];
        for (j, x) in order.iter().enumerate() { for q in 0..np { if bget(&x.0, q) { inv[q].push(j as u32); } } }
        let mut near = vec![];
        if cls == Class::Tula && pos.rows.len() == 1 {
            let r = &pos.rows[0];
            near = (0..np).map(|p| {
                let mut c: Vec<(f64, usize, u8)> = (0..np).filter(|&q| l.coll[q] && bget(&l.orangeable, q)).map(|q| (d2(r[q], r[p]) as f32 as f64, l.obj[q], q as u8)).collect();
                c.sort_by(|a, b| a.partial_cmp(b).unwrap());
                c.into_iter().map(|x| x.2).collect()
            }).collect();
        }
        let mut nstat = vec![];
        if cls == Class::Splork && pos.rows.len() == 1 {
            let r = &pos.rows[0];
            nstat = (0..np).map(|p| { let mut b = [0u64; W]; for q in 0..np { if l.coll[q] && d2(r[q], r[p]) <= 10000.0 { bset(&mut b, q); } } b }).collect();
        }
        Scanner { l, cls, lib, pos, order, nmax, floor, inv, near, nstat, conts: vec![], cinv: vec![], exact: None, reloc: None }
    }

    /// Max over library sets (skipping sets that hit `skip`) of |O ∩ H|, above `best`. A set with more than `best`
    /// oranges misses at most 24 - best of them, so it holds one of any 25 - best oranges: only the inverted lists
    /// of the 25 - best rarest oranges of this seed are walked (each largest first). True when a set holds all 25.
    #[inline]
    fn pass1(&self, o: &Bits, skip: &Bits, best: &mut u32, bi: &mut u32) -> bool {
        let need = 25u32.saturating_sub(*best) as usize;
        let mut rare: [(u32, u16); 32] = [(u32::MAX, 0); 32];
        let mut nr = 0;
        for x in 0..W { let mut w = o[x]; while w != 0 { let p = x * 64 + w.trailing_zeros() as usize; w &= w - 1; if nr < 32 { rare[nr] = (self.inv[p].len() as u32, p as u16); nr += 1; } } }
        let rare = &mut rare[..nr];
        rare.sort_unstable();
        let has_skip = skip.iter().any(|&w| w != 0);
        for &(_, p) in rare.iter().take(need) {
            for &j in &self.inv[p as usize] {
                let (ref h, sz, k) = self.order[j as usize];
                if sz.min(25) <= *best { break; }
                if has_skip && pc_and(h, skip) > 0 { continue; }
                let c = pc_and(h, o);
                if c > *best { *best = c; *bi = k; if c >= 25 { return true; } }
            }
        }
        false
    }

    /// best oranges lit by one library shot for this seed, and that shot
    #[inline]
    pub fn eval(&self, seed: u32, v: &mut Vec<u8>) -> (u32, u32) {
        let o = self.l.oranges(seed, v);
        let gr = if self.cls == Class::Free { [0; W] } else { self.l.greens(seed, &o, v) };
        if self.cls.physics() {
            // shot refs: k < library size = a library shot without a green; above = continuation entry k - size
            let (mut best, mut bi) = (self.floor.saturating_sub(1), u32::MAX);
            if self.pass1(&o, &gr, &mut best, &mut bi) { return (best, bi); }
            let n0 = self.lib.shots.len() as u32;
            if let Some(ex) = &self.exact {
                let mut k = u32::MAX;
                ex.eval(self.l, seed, &o, &gr, &mut best, &mut k);
                if k != u32::MAX { bi = n0 + k; }
                return if bi == u32::MAX { (0, bi) } else { (best, bi) };
            }
            for g in 0..self.l.npeg() {
                if !bget(&gr, g) { continue; }
                for &(sz, k) in &self.cinv[g] {
                    if sz.min(25) <= best { break; }
                    let c = &self.conts[k as usize];
                    if pc_and(&c.pre, &gr) > 0 { continue; } // the other green comes first
                    let n = match &self.reloc { Some(r) => flipcov::count(r, k as usize, &c.set, &o, seed, best, v), None => pc_and(&c.set, &o) };
                    if n > best { best = n; bi = n0 + k; if best >= 25 { return (best, bi); } }
                }
            }
            return if bi == u32::MAX { (0, bi) } else { (best, bi) };
        }
        // extra oranges the power can add on top of the ball's hits, at most
        let (o2, extra) = match self.cls {
            Class::Free | Class::Multi | Class::Spooky | Class::Pyramid | Class::Claude | Class::Hat | Class::Flip => (o, 0),
            Class::Tula => (o, 10),
            Class::Splork => {
                let mut b = [0u64; W];
                for g in 0..self.l.npeg() { if bget(&gr, g) { for k in 0..W { b[k] |= self.nmax[g][k]; } } }
                let mut o2 = o; for k in 0..W { o2[k] &= !b[k]; }
                (o2, pc_and(&o, &b))
            }
        };
        let (mut best, mut bi) = (self.floor.saturating_sub(1), u32::MAX);
        // pass 1: the ball's own hits (a lower bound for the power classes: lit ⊇ H; exact without a green in H)
        if self.pass1(&o, &[0; W], &mut best, &mut bi) { return (best, bi); }
        if self.cls == Class::Free { return if bi == u32::MAX { (0, bi) } else { (best, bi) }; }
        // pass 2: shots that hit one of this seed's greens (the power adds at most `extra` oranges)
        let _ = o2;
        for g in 0..self.l.npeg() {
            if !bget(&gr, g) { continue; }
            for &j in &self.inv[g] {
                let (ref h, sz, k) = self.order[j as usize];
                if sz.min(25) + extra <= best { break; }
                let c0 = pc_and(h, &o);
                if c0 + extra <= best { continue; }
                if self.cls == Class::Splork && pc_and(h, &o2) + extra <= best { continue; }
                if self.cls == Class::Tula && c0 + 5 * pc_and(h, &gr) <= best { continue; }
                if self.cls == Class::Splork && !self.nstat.is_empty() {
                    // static board: H ∪ the blasts of the greens it hits (closure over chained greens)
                    let (mut u, mut act) = (*h, [0u64; W]);
                    for x in 0..W { act[x] = h[x] & gr[x]; }
                    let mut done = [0u64; W];
                    loop {
                        let Some(x0) = (0..W).find(|&x| act[x] & !done[x] != 0) else { break };
                        let b = x0 * 64 + (act[x0] & !done[x0]).trailing_zeros() as usize;
                        bset(&mut done, b);
                        for x in 0..W { u[x] |= self.nstat[b][x]; act[x] |= self.nstat[b][x] & gr[x]; }
                    }
                    let c = pc_and(&u, &o);
                    if c > best { best = c; bi = k; if best >= 25 { return (best, bi); } }
                    continue;
                }
                let s = &self.lib.shots[k as usize];
                let c = power_eval(self.l, self.cls, s, &o, &gr, &self.pos, &self.near);
                if c > best { best = c; bi = k; if best >= 25 { return (best, bi); } }
            }
        }
        if bi == u32::MAX { (0, bi) } else { (best, bi) }
    }
}

/// the scanner of a class, building a missing continuation library first (physics classes)
fn scanner<'a>(dir: &str, l: &'a Lvl, cls: Class, lib: &'a Lib, floor: u32) -> Scanner<'a> {
    let sc = Scanner::new(l, cls, lib, floor);
    if !cls.physics() { return sc; }
    let path = cont_path(dir, &l.id, cls);
    if cls == Class::Spooky {
        if std::fs::metadata(&path).is_err() {
            let t = Instant::now();
            let (ns, sm, keep, us) = (env_or("SPOOKY_SHOTS", 2000), env_or("SPOOKY_SAMPLES", 8) as u32, env_or("SPOOKY_KEEP", 3000), env_or("SPOOKY_USAMP", 64) as u32);
            let v = spooky::build_cont(l, lib, ns, sm, keep, us);
            save_cont(&path, &format!("spooky exact: shots {ns} samples {sm} keep {keep} usamp {us}"), &v);
            eprintln!("{} spooky: continuation library {} entries ({:.0}s)", l.id, v.len(), t.elapsed().as_secs_f64());
        }
        let (conts, tag) = load_cont(&path);
        let t = Instant::now();
        let mut ex = spooky::exact_of(l, &conts);
        let memo = ex.load_memo(&memo_path(dir, &l.id), tag);
        ex.memo = Some((memo_path(dir, &l.id), tag));
        eprintln!("{} spooky: {} entry snapshots in {:.1}s{}", l.id, conts.len(), t.elapsed().as_secs_f64(), if memo { " (memo loaded)" } else { "" });
        let mut sc = sc.with_conts(conts);
        sc.exact = Some(ex);
        return sc;
    }
    if cls == Class::Flip && std::fs::metadata(&path).is_err() {
        // flip schedules: a long beam search per template (oneshot flipcov), with default settings
        let a: Vec<String> = ["oneshot", "flipcov", dir, &l.id].iter().map(|x| x.to_string()).collect();
        flipcov::cmd_flipcov(&a);
    }
    if std::fs::metadata(&path).is_err() {
        let t = Instant::now();
        let (na, nd, cmin, keep) = (env_or("ONESHOT_CANGLES", 60000), env_or("ONESHOT_CDELAYS", 2) as u8, 18, 300000);
        let v = build_cont(l, cls, na, nd, cmin, keep);
        save_cont(&path, &format!("angles {na} delays {nd} cmin {cmin} keep {keep}"), &v);
        eprintln!("{} {}: continuation library {} entries ({:.0}s)", l.id, cls.name(), v.len(), t.elapsed().as_secs_f64());
    }
    sc.with_conts(load_cont(&path).0)
}

/// Spooky: the first-stage memo tables (trial counts per vx), saved after each scanned chunk
fn memo_path(dir: &str, id: &str) -> String { format!("{dir}/lib/{id}.spooky.memo") }

/// after a chunk: freeze the memo tables (lock-free reads) and save them
fn spooky_save(dir: &str, sc: &mut Scanner) {
    let id = sc.l.id.clone();
    if let Some(ex) = sc.exact.as_mut() {
        let tag = load_cont(&cont_path(dir, &id, Class::Spooky)).1;
        ex.save_memo(&memo_path(dir, &id), tag);
    }
}

fn env_or(k: &str, d: usize) -> usize { std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d) }

/// GPU scan spec (solver/gpu/oneshot_gpu.cu reads it): the colour model and this class's scanner, little endian.
/// None if the class needs the per-tick position table (Tula / Splork on boards with moving pegs): CPU only.
fn export_spec(sc: &Scanner) -> Option<Vec<u8>> {
    let l = sc.l;
    if sc.cls == Class::Spooky || sc.cls == Class::Flip { return None; }
    let mut b: Vec<u8> = vec![];
    let u32s = |b: &mut Vec<u8>, x: u32| b.extend(x.to_le_bytes());
    let bits = |b: &mut Vec<u8>, x: &Bits| for w in x { b.extend(w.to_le_bytes()); };
    b.extend(b"OSG2");
    let cid = match sc.cls { Class::Free => 0, Class::Tula => 1, Class::Splork => 2, Class::Multi | Class::Pyramid | Class::Claude | Class::Hat | Class::Flip => 3, Class::Spooky => 4 };
    let near = match sc.cls { Class::Free => 25, Class::Spooky => 23, _ => 24 };
    for x in [cid, l.npeg() as u32, l.vars.len() as u32, l.nfixed, sc.floor, near, sc.lib.shots.len() as u32] { u32s(&mut b, x); }
    let mut vars = l.vars.clone(); vars.resize(256, 0); b.extend(&vars);
    bits(&mut b, &l.fixed);
    let mut coll = [0u64; W]; let mut var = [0u64; W];
    for p in 0..l.npeg() { if l.coll[p] { bset(&mut coll, p); } if l.var[p] { bset(&mut var, p); } }
    bits(&mut b, &coll); bits(&mut b, &var); bits(&mut b, &l.orangeable);
    for p in 0..256 { let q = l.pos0.get(p).copied().unwrap_or([0.0; 2]); b.extend(q[0].to_le_bytes()); b.extend(q[1].to_le_bytes()); }
    u32s(&mut b, sc.order.len() as u32);
    for x in &sc.order { bits(&mut b, &x.0); }
    for x in &sc.order { u32s(&mut b, x.1); }
    for x in &sc.order { u32s(&mut b, x.2); }
    let lists = |b: &mut Vec<u8>, v: &Vec<Vec<u32>>| { let mut o = 0u32; for p in 0..256 { u32s(b, o); o += v.get(p).map_or(0, |x| x.len() as u32); } u32s(b, o); for x in v { for &y in x { u32s(b, y); } } };
    lists(&mut b, &sc.inv);
    // tula near lists (u8 pegs), splork static blast sets and supersets
    let near_l: Vec<Vec<u32>> = sc.near.iter().map(|v| v.iter().map(|&x| x as u32).collect()).collect();
    lists(&mut b, &near_l);
    for p in 0..256 { bits(&mut b, sc.nstat.get(p).unwrap_or(&[0; W])); }
    for p in 0..256 { bits(&mut b, sc.nmax.get(p).unwrap_or(&[0; W])); }
    // multi continuation entries and per-green lists (sz, k)
    u32s(&mut b, sc.conts.len() as u32);
    for c in &sc.conts { bits(&mut b, &c.pre); }
    for c in &sc.conts { bits(&mut b, &c.set); }
    let mut o = 0u32;
    for p in 0..256 { u32s(&mut b, o); o += sc.cinv.get(p).map_or(0, |x| x.len() as u32); }
    u32s(&mut b, o);
    for x in &sc.cinv { for &(sz, k) in x { u32s(&mut b, sz); u32s(&mut b, k); } }
    // the library shots' ordered hits (Tula replays them)
    u32s(&mut b, sc.lib.shots.len() as u32);
    let mut o = 0u32;
    for s in &sc.lib.shots { u32s(&mut b, o); o += s.hits.len() as u32; }
    u32s(&mut b, o);
    for s in &sc.lib.shots { b.extend(&s.hits); }
    // boards with moving pegs (Tula / Splork replay at the hit tick): object order (Tula ties), positions per
    // tick from t0, the hits' ticks (parallel to the hits)
    let moving = matches!(sc.cls, Class::Tula | Class::Splork) && sc.pos.rows.len() > 1;
    u32s(&mut b, moving as u32);
    for p in 0..256 { u32s(&mut b, l.obj.get(p).copied().unwrap_or(0) as u32); }
    let rows: &[Vec<[f32; 2]>] = if moving { &sc.pos.rows } else { &[] };
    u32s(&mut b, rows.len() as u32); u32s(&mut b, sc.pos.t0 as u32);
    for row in rows { for p in 0..256 { let q = row.get(p).copied().unwrap_or([0.0; 2]); b.extend(q[0].to_le_bytes()); b.extend(q[1].to_le_bytes()); } }
    if moving { for s in &sc.lib.shots { for t in &s.ticks { b.extend(t.to_le_bytes()); } } }
    Some(b)
}

/// One chunk on the GPU: the kernel prints "hist ..." and "e SEED BEST K" for every seed at or above the floor;
/// top-8 and candidates are rebuilt here with the CPU's rules (per 2^14 block the first 64 at or above `near`,
/// then the best 32), so ledger lines are the same as a CPU scan's.
fn gpu_range(bin: &str, spec: &str, near: u32, ranges: &[(u64, u64)]) -> Option<Vec<ChunkRes>> {
    let mut cmd = std::process::Command::new(bin);
    cmd.arg(spec);
    for &(f, t) in ranges { cmd.arg(f.to_string()).arg(t.to_string()); }
    let out = cmd.output().ok()?;
    if !out.status.success() { eprintln!("gpu: {}", String::from_utf8_lossy(&out.stderr)); return None; }
    if std::env::var_os("ONESHOT_GPU_PROF").is_some() { eprint!("{}", String::from_utf8_lossy(&out.stderr)); }
    let txt = String::from_utf8_lossy(&out.stdout);
    let mut res = vec![];
    let mut cur: Option<(ChunkRes, Vec<(u32, u32, u32)>, bool)> = None;
    let mut flush = |cur: &mut Option<(ChunkRes, Vec<(u32, u32, u32)>, bool)>, res: &mut Vec<ChunkRes>| {
        if let Some((mut r, mut e, ok)) = cur.take() {
            if !ok { return false; }
            let (from, to) = ranges[res.len()];
            e.sort_by_key(|x| x.0);
            let mut per_block: HashMap<u64, u32> = HashMap::new();
            for &(seed, best, k) in &e {
                if best >= near { let c = per_block.entry(seed as u64 >> 14).or_insert(0); if *c < 64 { *c += 1; r.cands.push((seed, best, k)); } }
            }
            r.cands.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(&y.0))); r.cands.truncate(32);
            let mut top = e.clone();
            top.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(&y.0))); top.truncate(8);
            // fewer than 8 at the floor: the CPU's list continues with the lowest seeds below it (best 0, no shot)
            let mut s = from;
            while top.len() < 8 && s < to { if e.binary_search_by_key(&(s as u32), |x| x.0).is_err() { top.push((s as u32, 0, u32::MAX)); } s += 1; }
            r.top = top;
            res.push(r);
        }
        true
    };
    for line in txt.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        match f.first() {
            Some(&"range") => { if !flush(&mut cur, &mut res) { return None; } cur = Some((ChunkRes { hist: [0; 26], top: vec![], cands: vec![] }, vec![], false)); }
            Some(&"hist") => { let c = cur.as_mut()?; for k in 0..26 { c.0.hist[k] = f[k + 1].parse().ok()?; } c.2 = true; }
            Some(&"e") => cur.as_mut()?.1.push((f[1].parse().ok()?, f[2].parse().ok()?, f[3].parse().ok()?)),
            _ => {}
        }
    }
    if !flush(&mut cur, &mut res) || res.len() != ranges.len() { return None; }
    Some(res)
}

struct ChunkRes { hist: [u64; 26], top: Vec<(u32, u32, u32)>, cands: Vec<(u32, u32, u32)> }

fn scan_range(sc: &Scanner, from: u64, to: u64) -> ChunkRes {
    const BLK: u64 = 1 << 14;
    let blocks: Vec<u64> = (from..to).step_by(BLK as usize).collect();
    let near = match sc.cls { Class::Free => 25, Class::Spooky => 23, _ => 24 };
    blocks.par_iter().map(|&b0| {
        let mut r = ChunkRes { hist: [0; 26], top: vec![], cands: vec![] };
        let mut v = Vec::with_capacity(256);
        for s in b0..(b0 + BLK).min(to) {
            let seed = s as u32;
            let (best, k) = sc.eval(seed, &mut v);
            r.hist[best.min(25) as usize] += 1;
            if best >= near && r.cands.len() < 64 { r.cands.push((seed, best, k)); }
            if r.top.len() < 8 || best > r.top[7].1 { r.top.push((seed, best, k)); r.top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0))); r.top.truncate(8); }
        }
        r
    }).reduce(|| ChunkRes { hist: [0; 26], top: vec![], cands: vec![] }, |mut a, b| {
        for k in 0..26 { a.hist[k] += b.hist[k]; }
        a.top.extend(b.top); a.top.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(&y.0))); a.top.truncate(8);
        a.cands.extend(b.cands); a.cands.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(&y.0))); a.cands.truncate(32); a
    })
}

/// Tula / Splork run on the GPU only on boards whose pegs do not move (cached in DIR/lib/LV.static)
/// (flip: the CPU models flipper_clear's relocation exactly, the GPU's cid 3 would count conservatively)
fn gpu_capable(_dir: &str, _id: &str, cls: Class) -> bool { cls != Class::Spooky && cls != Class::Flip }

/// expected power-free wins over the whole seed space (free_estimate x 2^32), cached in DIR/lib/LV.freep
fn free_wins_2_32(dir: &str, id: &str) -> f64 {
    let p = format!("{dir}/lib/{id}.freep");
    if let Some(v) = std::fs::read_to_string(&p).ok().and_then(|s| s.trim().parse().ok()) { return v; }
    let l = Lvl::new(id);
    let lib = load_lib(&lib_path(dir, id));
    let v = free_estimate(&l, &lib).0 * 4294967296.0;
    let _ = std::fs::write(&p, format!("{v:e}\n"));
    v
}

/// a chunk being scanned on this machine (DIR/claims; several workers on one box share them). Stale after 3 h.
fn claim(dir: &str, key: &str) -> bool {
    let _ = std::fs::create_dir_all(format!("{dir}/claims"));
    let p = format!("{dir}/claims/{key}");
    if std::fs::OpenOptions::new().write(true).create_new(true).open(&p).is_ok() { return true; }
    let old = std::fs::metadata(&p).and_then(|m| m.modified()).map(|t| t.elapsed().map_or(false, |e| e.as_secs() > 3 * 3600)).unwrap_or(false);
    if old { let _ = std::fs::remove_file(&p); return std::fs::OpenOptions::new().write(true).create_new(true).open(&p).is_ok(); }
    false
}

fn host() -> String { std::env::var("ONESHOT_HOST").unwrap_or_else(|_| std::process::Command::new("hostname").arg("-s").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or("host".into())) }

/// ledger rows of all hosts: (level, class, lib hash, chunk) → row fields
fn read_ledgers(dir: &str) -> Vec<Vec<String>> {
    let mut v = vec![];
    if let Ok(rd) = std::fs::read_dir(format!("{dir}/ledger")) {
        for e in rd.flatten() {
            if !e.file_name().to_string_lossy().ends_with(".tsv") { continue; }
            for line in std::fs::read_to_string(e.path()).unwrap_or_default().lines() {
                let f: Vec<String> = line.split('\t').map(|x| x.to_string()).collect();
                if f.len() >= 10 { v.push(f); }
            }
        }
    }
    v
}

fn append(path: &str, line: &str) {
    std::fs::create_dir_all(std::path::Path::new(path).parent().unwrap()).unwrap();
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
    f.write_all(line.as_bytes()).unwrap();
}

fn arg<T: std::str::FromStr>(a: &[String], k: &str, d: T) -> T { a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(d) }

fn check_colours(l: &Lvl, n: u32) {
    let mut v = vec![];
    for k in 0..n {
        let seed = tas_mix(k, 77);
        let g = levels::new_game(l.st, l.ix, seed, 1);
        let (mut o, mut gr) = ([0; W], [0; W]);
        for (b, &i) in l.obj.iter().enumerate() {
            if g.board.objs[i].peg_type == ORANGE { bset(&mut o, b); }
            if g.board.objs[i].peg_type == GREEN { bset(&mut gr, b); }
        }
        let fo = l.oranges(seed, &mut v);
        let fg = l.greens(seed, &fo, &mut v);
        assert!(fo == o && fg == gr, "{}: colour replica differs from Game::new at seed {seed}", l.id);
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let cmd = a.get(1).map(|s| s.as_str()).unwrap_or("");
    let dir = a.get(2).cloned().unwrap_or_default();
    match cmd {
        "lib" => {
            let n_ang: usize = arg(&a, "--angles", 100000);
            let delays: u8 = arg(&a, "--delays", 8);
            let hmin: usize = arg(&a, "--hmin", 12);
            let keep: usize = arg(&a, "--keep", 30000);
            let keep_power: usize = arg(&a, "--keep-power", 0);
            // click delays (FF frames after the first click): 0..delays, then --dstride steps up to --dmax
            let dmax: usize = arg(&a, "--dmax", delays as usize);
            let dstride: usize = arg(&a, "--dstride", 8).max(1);
            let mut ds: Vec<u8> = (0..delays).collect();
            let mut d = delays as usize; while d < dmax.min(255) { ds.push(d as u8); d += dstride; }
            let lvs: Vec<String> = a[3..].iter().take_while(|x| !x.starts_with("--")).cloned().collect();
            let lvs = if lvs.iter().any(|x| x == "all") { all_levels() } else { lvs };
            for id in lvs {
                let l = Lvl::new(&id);
                let path = lib_path(&dir, &l.id);
                if std::fs::metadata(&path).is_ok() && !a.iter().any(|x| x == "--force") { eprintln!("{}: {path} exists", l.id); continue; }
                let t = Instant::now();
                let shots = build_lib(&l, n_ang, &ds, hmin, keep, keep_power);
                let params = if dmax > delays as usize || keep_power > 0 { format!("angles {n_ang} delays {delays} dmax {dmax} dstride {dstride} hmin {hmin} keep {keep} keep_power {keep_power}") }
                             else { format!("angles {n_ang} delays {delays} hmin {hmin} keep {keep}") };
                save_lib(&path, &params, &shots);
                let lib = load_lib(&path);
                let (p, hmax) = free_estimate(&l, &lib);
                println!("{}\tlib {}\t{} shots ({})\t{:.0}s\tpegs {} var {} fixed {}\thmax {}\tfree P/seed {:.3e} wins/2^32 {:.3e}",
                         l.id, lib.hash, lib.shots.len(), params, t.elapsed().as_secs_f64(), l.npeg(), l.vars.len(), l.nfixed, hmax, p, p * 4294967296.0);
            }
        }
        "scan" => {
            let l = Lvl::new(&a[3]);
            let cls = Class::parse(&a[4]);
            check_colours(&l, 200);
            let t_lib = Instant::now();
            let lib = load_lib(&lib_path(&dir, &l.id));
            let t_lib = t_lib.elapsed().as_secs_f64();
            let t = Instant::now();
            let mut sc = scanner(&dir, &l, cls, &lib, arg(&a, "--floor", 22));
            let (c0, c1): (u32, u32) = { let s: String = arg(&a, "--chunks", "0-255".to_string()); let (x, y) = s.split_once('-').unwrap(); (x.parse().unwrap(), y.parse().unwrap()) };
            let desc = a.iter().any(|x| x == "--desc");
            let max: usize = arg(&a, "--max", usize::MAX);
            let hostn: String = arg(&a, "--host", host());
            eprintln!("{} {}: lib {} ({} shots, {} in scan order, {} position rows; setup: lib load {t_lib:.2}s, scanner {:.2}s)", l.id, cls.name(), lib.hash, lib.shots.len(), sc.order.len(), sc.pos.rows.len(), t.elapsed().as_secs_f64());
            let hash = class_hash(&dir, &l.id, cls).unwrap();
            let gpu: Option<String> = a.iter().position(|x| x == "--gpu").and_then(|i| a.get(i + 1)).cloned();
            let batch: usize = arg(&a, "--batch", 8);
            let t_spec = Instant::now();
            let spec = gpu.as_ref().and_then(|_| export_spec(&sc)).map(|b| {
                let p = format!("{dir}/gpu/{}.{}.{hash}.spec", l.id, cls.name());
                std::fs::create_dir_all(format!("{dir}/gpu")).unwrap();
                std::fs::write(&p, b).unwrap();
                p
            });
            if spec.is_some() { eprintln!("spec export {:.2}s", t_spec.elapsed().as_secs_f64()); }
            if gpu.is_some() && spec.is_none() { eprintln!("{} {}: not GPU-capable (moving pegs or spooky): CPU scan", l.id, cls.name()); if a.iter().any(|x| x == "--gpu-only") { return; } }
            let near = match cls { Class::Free => 25, Class::Spooky => 23, _ => 24 };
            let mut chunks: Vec<u32> = (c0..=c1).collect();
            if desc { chunks.reverse(); }
            let mut n = 0;
            let mut it = chunks.into_iter();
            loop {
                // the next chunks to do (claimed): one on the CPU, up to --batch in one GPU process
                let want = if spec.is_some() { batch.min(max - n) } else { 1.min(max - n) };
                if want == 0 { break; }
                let done: HashSet<u32> = read_ledgers(&dir).iter().filter(|f| f[0] == l.id && f[1] == cls.name() && f[2] == hash).filter_map(|f| f[3].parse().ok()).collect();
                let mut cs = vec![];
                while cs.len() < want {
                    let Some(c) = it.next() else { break };
                    if done.contains(&c) { continue; }
                    if !claim(&dir, &format!("{}.{}.{hash}.{c}", l.id, cls.name())) { continue; }
                    cs.push(c);
                }
                if cs.is_empty() { break; }
                let ranges: Vec<(u64, u64)> = cs.iter().map(|&c| ((c as u64) << CHUNK_BITS, ((c as u64) + 1) << CHUNK_BITS)).collect();
                let t = Instant::now();
                let rs: Vec<ChunkRes> = match (&gpu, &spec) {
                    (Some(bin), Some(sp)) => match gpu_range(bin, sp, near, &ranges) { Some(r) => r, None => {
                        for &c in &cs { let _ = std::fs::remove_file(format!("{dir}/claims/{}.{}.{hash}.{c}", l.id, cls.name())); }
                        eprintln!("gpu run failed: stopping"); return; } },
                    _ => ranges.iter().map(|&(f, t)| scan_range(&sc, f, t)).collect(),
                };
                spooky_save(&dir, &mut sc);
                let secs = t.elapsed().as_secs_f64() / cs.len() as f64;
                for (&c, r) in cs.iter().zip(&rs) {
                    let (from, to) = ((c as u64) << CHUNK_BITS, ((c as u64) + 1) << CHUNK_BITS);
                    let best = (0..26).rev().find(|&k| r.hist[k] > 0).unwrap_or(0);
                    for &(seed, cnt, k) in &r.cands {
                        append(&format!("{dir}/cands/{hostn}.tsv"), &format!("{}\t{}\t{}\t{seed}\t{k}\t{cnt}\n", l.id, cls.name(), hash));
                    }
                    let hist = r.hist.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",");
                    let top = r.top.iter().map(|x| format!("{}:{}:{}", x.0, x.1, x.2)).collect::<Vec<_>>().join(";");
                    append(&format!("{dir}/ledger/{hostn}.tsv"), &format!("{}\t{}\t{}\t{c}\t{}\t{secs:.1}\t{hostn}\t{best}\t{hist}\t{top}\n", l.id, cls.name(), hash, to - from));
                    let _ = std::fs::remove_file(format!("{dir}/claims/{}.{}.{hash}.{c}", l.id, cls.name()));
                    eprintln!("{} {} chunk {c}: best {best}, {} >= 24, {:.2}M seeds/s{}", l.id, cls.name(), r.hist[24] + r.hist[25], (to - from) as f64 / secs / 1e6, if spec.is_some() { format!(" (gpu, batch {})", cs.len()) } else { String::new() });
                    n += 1;
                }
            }
        }
        "bench" => {
            let l = Lvl::new(&a[3]);
            let cls = Class::parse(&a[4]);
            let lib = load_lib(&lib_path(&dir, &l.id));
            let mut sc = scanner(&dir, &l, cls, &lib, arg(&a, "--floor", 22));
            let n: u64 = arg(&a, "--n", 1 << 20);
            let warm: u64 = arg(&a, "--warm", 0);
            if warm > 0 { let t = Instant::now(); scan_range(&sc, 1 << 30, (1 << 30) + warm); eprintln!("warm-up {warm} seeds {:.1}s", t.elapsed().as_secs_f64()); if let Some(ex) = sc.exact.as_mut() { ex.freeze(); } }
            let t = Instant::now();
            let r = scan_range(&sc, 1 << 31, (1 << 31) + n);
            let dt = t.elapsed().as_secs_f64();
            if let Some(ex) = &sc.exact { eprintln!("spooky stats (entry evals, second-stage lookups, built, cache clears, q-specific, gated): {:?}", ex.stats.iter().map(|x| x.load(std::sync::atomic::Ordering::Relaxed)).collect::<Vec<_>>()); }
            let mut v = vec![];
            let t2 = Instant::now();
            let mut x = 0u64; for s in 0..20000u32 { let o = l.oranges(s, &mut v); x += o[0]; if cls != Class::Free { x += l.greens(s, &o, &mut v)[0]; } }
            let colour_us = t2.elapsed().as_secs_f64() / 20000.0 * 1e6;
            println!("{} {}: {} seeds in {dt:.2}s = {:.2}M seeds/s ({} threads); colours {colour_us:.2} us/seed single-thread ({x}); hist {:?}",
                     l.id, cls.name(), n, n as f64 / dt / 1e6, rayon::current_num_threads(), &r.hist[10..]);
        }
        "flipcov" => flipcov::cmd_flipcov(&a),
        "fliptimes" => fast::cmd_times(&a),
        "fastscan" => fast::cmd_scan(&a),
        "fastcheck" => fast::cmd_check(&a),
        "flipcheck" => flipcov::cmd_check(&a),
        "flipdbg" => flipcov::cmd_dbg(&a),
        "flipinfo" => flipcov::cmd_info(&a),
        "flipfinal" => { let l = Lvl::new(&a[3]); flipcov::finalize(&dir, &l); }
        "flipplan" => {
            // Claude (flippers): the first green hit turns the flippers on mid-shot; player flips (Game::flips) can
            // re-launch the ball for as long as it is kept up. Per level: seeds × library shots that reach a green
            // (exact sim, no flips), then a beam search over flip presses near each ball loss.
            // usage: oneshot flipplan DIR LV [--seeds N] [--shots K] [--beam B] [--rounds R] [--pairs P]
            let l = Lvl::new(&a[3]);
            let nseed: u32 = arg(&a, "--seeds", 64);
            let nshot: usize = arg(&a, "--shots", 300);
            let beam: usize = arg(&a, "--beam", 4);
            let rounds: usize = arg(&a, "--rounds", 60);
            let npairs: usize = arg(&a, "--pairs", 4);
            let win: i32 = arg(&a, "--window", 160);
            let step: usize = arg(&a, "--step", 2);
            let holds: Vec<i32> = arg(&a, "--holds", "4,10,20,40".to_string()).split(',').filter_map(|x| x.parse().ok()).collect();
            let stall_max: usize = arg(&a, "--stall", 6);
            let lib = load_lib(&lib_path(&dir, &l.id));
            let angles = Angles::new();
            const CLAUDE: usize = 4;
            let power = peggle_sim::oracle::CHAR_POWERS[CLAUDE];
            let t0 = Instant::now();
            let base = |seed: u32| { let mut g = levels::new_game(l.st, l.ix, seed, power); g.earliest_fire = rt::FIRST_CLICK; g };
            // stage 1: (seed, shot) pairs whose unflipped shot activates the flippers, by oranges hit
            let mut pairs: Vec<(i32, i32, u32, usize)> = (0..nseed).into_par_iter().flat_map(|k| {
                let seed = tas_mix(k, 4242);
                let g0 = base(seed);
                lib.shots.iter().take(nshot).enumerate().filter_map(|(si, s)| {
                    let mut g = g0.clone();
                    let r = g.shoot(angles.list[s.ai as usize].0, l.click(s.d));
                    if !r.activations.contains(&2) { return None; }
                    Some((r.oranges, r.frames, seed, si))
                }).collect::<Vec<_>>()
            }).collect();
            pairs.sort_by(|a, b| b.cmp(a));
            eprintln!("{}: {} activating (seed, shot) pairs of {} in {:.1}s; top {:?}", l.id, pairs.len(), nseed as usize * nshot.min(lib.shots.len()), t0.elapsed().as_secs_f64(), &pairs[..pairs.len().min(5)]);
            // stage 2: beam search over flip schedules
            type Sched = Vec<(i32, bool)>;
            let eval = |seed: u32, si: usize, f: &Sched| -> (i32, i32, i32, bool, i32) {
                let s = &lib.shots[si];
                let mut g = base(seed);
                g.flips = f.clone();
                let r = g.shoot(angles.list[s.ai as usize].0, l.click(s.d));
                (r.oranges, r.pegs, r.frames, r.won, r.end_tick)
            };
            for &(o0, _, seed, si) in pairs.iter().take(npairs) {
                let t = Instant::now();
                let mut cur: Vec<(Sched, (i32, i32, i32, bool, i32))> = vec![(vec![], eval(seed, si, &vec![]))];
                let mut best = cur[0].clone();
                let mut stall = 0;
                for round in 0..rounds {
                    let mut cand: Vec<(Sched, i32)> = vec![];
                    for (f, r) in &cur {
                        let end = r.4;
                        let from = f.last().map_or(end - win, |x| x.0 + 2).max(end - win);
                        for tp in (from..end).step_by(step) {
                            for &h in &holds {
                                let mut n = f.clone();
                                n.push((tp, true)); n.push((tp + h, false));
                                cand.push((n, r.2));
                            }
                        }
                    }
                    // children that re-launch the ball (flight at least 30 ticks longer than the parent's) or win
                    let mut ev: Vec<(Sched, (i32, i32, i32, bool, i32))> = cand.into_par_iter().filter_map(|(f, pf)| { let r = eval(seed, si, &f); (r.3 || r.2 > pf + 30).then_some((f, r)) }).collect();
                    // progress = more oranges, then more pegs (new ground), then a longer flight (the ball kept up)
                    ev.sort_by(|a, b| (b.1.3, b.1.0, b.1.1, b.1.2).cmp(&(a.1.3, a.1.0, a.1.1, a.1.2)));
                    // diversity: one per end tick
                    let mut seen = HashSet::new();
                    ev.retain(|x| seen.insert((x.1.0, x.1.1, x.1.4)));
                    ev.truncate(beam);
                    if ev.is_empty() { break; }
                    // rounds without a new orange or peg: the ball is lost or circles over lit pegs
                    if (ev[0].1.3, ev[0].1.0, ev[0].1.1) > (best.1.3, best.1.0, best.1.1) { best = ev[0].clone(); stall = 0; } else { stall += 1; }
                    if stall >= stall_max { break; }
                    eprintln!("  seed {seed} shot {si} round {round}: best oranges {} pegs {} frames {} won {} ({} flips) {:.0}s", ev[0].1.0, ev[0].1.1, ev[0].1.2, ev[0].1.3, ev[0].0.len() / 2, t.elapsed().as_secs_f64());
                    if ev[0].1.3 { break; }
                    cur = ev;
                }
                let s = &lib.shots[si];
                if std::env::var_os("FLIP_DIAG").is_some() {
                    let mut g = base(seed);
                    g.flips = best.0.clone();
                    g.shoot(angles.list[s.ai as usize].0, l.click(s.d));
                    let (mut hit, mut miss) = (vec![], vec![]);
                    for &i in &l.obj { let o = &g.board.objs[i]; let p = g.board.obj_pos(o.uid).unwrap_or([0.0; 2]);
                        if o.peg_type == ORANGE { if o.hit { hit.push((p[0] as i32, p[1] as i32)); } else { miss.push((p[0] as i32, p[1] as i32)); } } }
                    let lit: Vec<(i32, i32)> = l.obj.iter().filter(|&&i| g.board.objs[i].hit).map(|&i| { let p = g.board.obj_pos(g.board.objs[i].uid).unwrap_or([0.0; 2]); (p[0] as i32, p[1] as i32) }).collect();
                    eprintln!("  diag: oranges hit {:?}\n  diag: oranges missed {:?}\n  diag: all hit pegs {:?}", hit, miss, lit);
                }
                println!("{}\tflip\t{seed}\t{si}\t{o0}\t{}\t{}\t{}\t{}", l.id, best.1.0, best.1.3, best.1.4, best.0.iter().map(|x| format!("{}:{}", x.0, x.1 as u8)).collect::<Vec<_>>().join(","));
                if best.1.3 {
                    let wd = format!("{dir}/wins/{}_flip_{seed}", l.id);
                    std::fs::create_dir_all(&wd).unwrap();
                    let (st, ix) = (l.st, l.ix);
                    let (angle, px) = angles.list[s.ai as usize];
                    let click = l.click(s.d);
                    let flips: String = best.0.iter().map(|x| format!("flip {} {}\n", x.0, x.1 as u8)).collect();
                    std::fs::write(format!("{wd}/{st}_{ix}.txt"), format!("char {CLAUDE}\nlevel {st} {ix}\n{flips}shotx {:08x} {click}\nwaitms 3000\nquit\n", angle.to_bits())).unwrap();
                    std::fs::write(format!("{wd}/{st}_{ix}.plan.txt"), format!("# {} ({}) seed {seed} char Some({CLAUDE}) objective OneShot, flips (no fast forward)\n1: pixel ({}, {}) angle {:.7} click {click} | flips {} | end tick {} WIN\n",
                        levels::LEVELS[st][ix], l.id, px[0], px[1], angle, best.0.len() / 2, best.1.4)).unwrap();
                    std::fs::write(format!("{wd}/{st}_{ix}.seed"), format!("{seed}\n")).unwrap();
                    eprintln!("  WIN -> {wd}");
                    break;
                }
            }
        }
        "spooky" => spooky::cmd(&a, &dir),
        "check" => {
            // per seed: the scanner's best shot and its filter count vs the exact sim of that shot with that seed
            let l = Lvl::new(&a[3]);
            let cls = Class::parse(&a[4]);
            let n: u32 = a.get(5).and_then(|x| x.parse().ok()).unwrap_or(300);
            check_colours(&l, 2000);
            let lib = load_lib(&lib_path(&dir, &l.id));
            let sc = scanner(&dir, &l, cls, &lib, 1);
            let angles = Angles::new();
            let n0 = lib.shots.len() as u32;
            let flips = if cls == Class::Flip { flipcov::load_flips(&dir, &l.id) } else { vec![] };
            let res: Vec<(u32, i32)> = (0..n).into_par_iter().filter_map(|k| {
                let seed = tas_mix(k, 991);
                let mut v = vec![];
                let (f, r) = sc.eval(seed, &mut v);
                if r == u32::MAX { return None; }
                let (ai, d) = if r < n0 { (lib.shots[r as usize].ai, lib.shots[r as usize].d) } else { (sc.conts[(r - n0) as usize].ai, sc.conts[(r - n0) as usize].d) };
                let mut g = levels::new_game(l.st, l.ix, seed, cls.power());
                g.earliest_fire = rt::FIRST_CLICK;
                if r >= n0 { g.flips = flips.get((r - n0) as usize).cloned().unwrap_or_default(); }
                let x = g.shoot(angles.list[ai as usize].0, l.click(d));
                // a win counts 25 (the scanner's count includes oranges the relocation could not place)
                Some((f, if x.won { 25 } else { x.oranges }))
            }).collect();
            let eq = res.iter().filter(|r| r.0 as i32 == r.1).count();
            let mad = res.iter().map(|r| (r.0 as i32 - r.1).abs() as f64).sum::<f64>() / res.len() as f64;
            let top: Vec<_> = { let mut t = res.clone(); t.sort_by(|a, b| b.0.cmp(&a.0)); t.truncate(12); t };
            println!("{} {}: filter = exact on {eq}/{} seeds (their best shot), mean |diff| {mad:.2}; best seeds (filter, exact) {:?}", l.id, cls.name(), res.len(), top);
        }
        "verify" => {
            let lvs: HashSet<String> = a[3..].iter().cloned().collect();
            let done: HashSet<(String, String, String)> = std::fs::read_to_string(format!("{dir}/verified.tsv")).unwrap_or_default().lines()
                .map(|l| { let f: Vec<&str> = l.split('\t').collect(); (f[0].to_string(), f[1].to_string(), format!("{}:{}", f[2], f[3])) }).collect();
            let mut todo: BTreeMap<(String, String), Vec<(u32, u32, String)>> = BTreeMap::new();
            if let Ok(rd) = std::fs::read_dir(format!("{dir}/cands")) {
                for e in rd.flatten() {
                    for line in std::fs::read_to_string(e.path()).unwrap_or_default().lines() {
                        let f: Vec<&str> = line.split('\t').collect();
                        if f.len() < 6 || (!lvs.is_empty() && !lvs.contains(f[0])) { continue; }
                        if done.contains(&(f[0].to_string(), f[1].to_string(), format!("{}:{}", f[3], f[4]))) { continue; }
                        todo.entry((f[0].to_string(), f[1].to_string())).or_default().push((f[3].parse().unwrap(), f[4].parse().unwrap(), f[2].to_string()));
                    }
                }
            }
            let angles = Angles::new();
            for ((id, cname), v) in todo {
                let l = Lvl::new(&id);
                let cls = Class::parse(&cname);
                let lib = load_lib(&lib_path(&dir, &id));
                let hash = class_hash(&dir, &id, cls).unwrap();
                let conts = if cls.physics() { load_cont(&cont_path(&dir, &id, cls)).0 } else { vec![] };
                let flips = if cls == Class::Flip { flipcov::load_flips(&dir, &id) } else { vec![] };
                let v: Vec<_> = v.into_iter().filter(|x| x.2 == hash).collect();
                let rows: Vec<String> = v.par_iter().map(|&(seed, k, _)| {
                    let n0 = lib.shots.len() as u32;
                    let (s_ai, s_d) = if k < n0 { (lib.shots[k as usize].ai, lib.shots[k as usize].d) } else { (conts[(k - n0) as usize].ai, conts[(k - n0) as usize].d) };
                    let s = Shot { ai: s_ai, d: s_d, hits: vec![], ticks: vec![], set: [0; W] };
                    let s = &s;
                    let mut g = levels::new_game(l.st, l.ix, seed, cls.power());
                    g.earliest_fire = rt::FIRST_CLICK;
                    let (angle, px) = angles.list[s.ai as usize];
                    let click = l.click(s.d);
                    // flip class: the entry's flipper presses (harness ticks; plans run without fast forward)
                    let fl: Vec<(i32, bool)> = if k >= n0 { flips.get((k - n0) as usize).cloned().unwrap_or_default() } else { vec![] };
                    g.flips = fl.clone();
                    let g0 = g.clone();
                    let mut r = g.shoot(angle, click);
                    // presses after the win (the last orange's hit; the real game's tick stops there and the fever
                    // ball runs without them) or after the shot's end are left out of the script and the prediction
                    let mut fl: Vec<(i32, bool)> = fl.into_iter().filter(|x| x.0 <= r.end_tick).collect();
                    if r.won && !fl.is_empty() {
                        let blag = g.tick - g.board.btick;
                        let tw = g.board.shot_hits.iter().zip(&g.board.shot_hit_bticks).filter(|(i, _)| g.board.objs[**i].peg_type == ORANGE).map(|(_, &t)| t + blag).max().unwrap_or(i32::MAX);
                        let f2: Vec<(i32, bool)> = fl.iter().copied().filter(|x| x.0 < tw - 1).collect();
                        if f2.len() < fl.len() {
                            let mut g2 = g0.clone();
                            g2.flips = f2.clone();
                            let r2 = g2.shoot(angle, click);
                            if r2.won { g = g2; r = r2; fl = f2; }
                        }
                    }
                    let flines: String = fl.iter().map(|x| format!("flip {} {}\n", x.0, x.1 as u8)).collect();
                    if r.won {
                        let wd = format!("{dir}/wins/{id}_{cname}_{seed}");
                        std::fs::create_dir_all(&wd).unwrap();
                        let (st, ix) = (l.st, l.ix);
                        std::fs::write(format!("{wd}/{st}_{ix}.txt"), format!("char {}\nlevel {st} {ix}\n{flines}shotx {:08x} {click}\nwaitms 3000\nquit\n", cls.char(), angle.to_bits())).unwrap();
                        std::fs::write(format!("{wd}/{st}_{ix}.plan.txt"), format!("# {} ({id}) seed {seed} char Some({}) objective OneShot, {}\n1: pixel ({}, {}) angle {:.7} click {click} | {} pegs, {} orange, score {}, end tick {}, styles {:?}, catches {}, free {} WIN\n# total {} after 1 shots, balls left {}\n",
                            levels::LEVELS[st][ix], cls.char(), if fl.is_empty() { "fast forward held".to_string() } else { format!("flips {} (no fast forward)", fl.len() / 2) }, px[0], px[1], angle, r.pegs, r.oranges, r.score, r.end_tick, r.styles, r.catches, r.free_balls, g.total, g.balls)).unwrap();
                        std::fs::write(format!("{wd}/{st}_{ix}.seed"), format!("{seed}\n")).unwrap();
                    }
                    format!("{id}\t{cname}\t{seed}\t{k}\t{}\t{}\t{}\t{}\t{}\t{}\n", r.won, r.oranges, r.pegs, g.total, hash, r.end_tick)
                }).collect();
                for r in &rows { append(&format!("{dir}/verified.tsv"), r); }
                let wins = rows.iter().filter(|r| r.split('\t').nth(4) == Some("true")).count();
                println!("{id} {cname}: verified {} candidates, {wins} exact wins", rows.len());
            }
            // per level, the fastest exact win (earliest end tick, then highest total) → DIR/best (tas/pipeline.py layout)
            let ver = std::fs::read_to_string(format!("{dir}/verified.tsv")).unwrap_or_default();
            let mut best: BTreeMap<String, (i64, i64, String)> = BTreeMap::new();
            for line in ver.lines() {
                let f: Vec<&str> = line.split('\t').collect();
                if f.len() < 10 || f[4] != "true" { continue; }
                let key = (f[9].parse::<i64>().unwrap_or(i64::MAX), -f[7].parse::<i64>().unwrap_or(0), format!("{dir}/wins/{}_{}_{}", f[0], f[1], f[2]));
                let e = best.entry(f[0].to_string()).or_insert(key.clone());
                if key < *e { *e = key; }
            }
            for (id, (_, _, wd)) in &best {
                let (st, ix) = levels::parse_level_id(id).unwrap();
                std::fs::create_dir_all(format!("{dir}/best")).unwrap();
                for e in ["txt", "plan.txt", "seed"] { let _ = std::fs::copy(format!("{wd}/{st}_{ix}.{e}"), format!("{dir}/best/{st}_{ix}.{e}")); }
            }
            println!("levels with an exact one-shot win: {}", best.len());
        }
        "cont" => {
            // build a physics class's continuation library (CPU work; the scan then runs on the GPU)
            let l = Lvl::new(&a[3]);
            let cls = Class::parse(&a[4]);
            let lib = load_lib(&lib_path(&dir, &l.id));
            let _ = scanner(&dir, &l, cls, &lib, 22);
            println!("{} {}: continuation library {}", l.id, cls.name(), cont_path(&dir, &l.id, cls));
        }
        "next" => {
            // the queue: phase A gives every (level, class) with a library one chunk; then the (level, class)
            // with the most expected wins per chunk (tail of its near-miss histogram) of the levels not yet
            // won in one shot. Prints "LEVEL CLASS CHUNK" (lowest free chunk, or highest with --desc) or nothing.
            let desc = a.iter().any(|x| x == "--desc");
            let gpu = a.iter().any(|x| x == "--gpu");
            let prefer_cpu = a.iter().any(|x| x == "--prefer-cpu");
            let build_only = a.iter().any(|x| x == "--build-only");
            let claimed: HashSet<String> = std::fs::read_dir(format!("{dir}/claims")).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect()).unwrap_or_default();
            let classes: Vec<Class> = { let s: String = arg(&a, "--classes", "free,tula,splork".to_string()); s.split(',').map(Class::parse).collect() };
            let led = read_ledgers(&dir);
            let ver = std::fs::read_to_string(format!("{dir}/verified.tsv")).unwrap_or_default();
            let mut solved: HashSet<String> = ver.lines().filter_map(|l| { let f: Vec<&str> = l.split('\t').collect(); (f.len() > 4 && f[4] == "true").then(|| f[0].to_string()) }).collect();
            // free-class filter wins are exact (checked); count them as solved before `verify` runs
            if let Ok(rd) = std::fs::read_dir(format!("{dir}/cands")) { for e in rd.flatten() { for l in std::fs::read_to_string(e.path()).unwrap_or_default().lines() {
                let f: Vec<&str> = l.split('\t').collect(); if f.len() > 5 && f[1] == "free" && f[5] == "25" { solved.insert(f[0].to_string()); } } } }
            let mut best: Option<(f64, String, Class, u32)> = None;
            let mut best_cpu: Option<(f64, String, Class, u32)> = None;
            for id in all_levels() {
                if solved.contains(&id) || !std::path::Path::new(&lib_path(&dir, &id)).exists() { continue; }
                for &c in &classes {
                    // a missing continuation library hashes as 0 here: `scan` builds it, and the chunks then count under the new hash
                    let Some(hash) = class_hash(&dir, &id, c) else { continue };
                    // a missing continuation library is CPU work (`scan` builds it first)
                    let cont_missing = c.physics() && !std::path::Path::new(&cont_path(&dir, &id, c)).exists();
                    if build_only { if cont_missing { println!("{id} {} build", c.name()); return; } continue; }
                    // power-free on a level whose biggest shots cannot cover 25 oranges: expected wins over all 2^32 seeds
                    // (union bound, uniform oranges) below 1e-3: not worth a GPU minute
                    if c == Class::Free && free_wins_2_32(&dir, &id) < 1e-3 { continue; }
                    let capable = (gpu || prefer_cpu) && !cont_missing && gpu_capable(&dir, &id, c);
                    if gpu && !capable { continue; }
                    let rows: Vec<&Vec<String>> = led.iter().filter(|f| f[0] == id && f[1] == c.name() && f[2] == hash).collect();
                    let done: HashSet<u32> = rows.iter().filter_map(|f| f[3].parse().ok()).collect();
                    let free: Vec<u32> = (0..256).filter(|k| !done.contains(k) && !claimed.contains(&format!("{id}.{}.{hash}.{k}", c.name()))).collect();
                    let Some(&chunk) = (if desc { free.last() } else { free.first() }) else { continue };
                    let mut h = [0f64; 26];
                    for f in &rows { for (k, x) in f[8].split(',').enumerate() { h[k] += x.parse::<f64>().unwrap_or(0.0); } }
                    // expected 25s per chunk: observed, else 22s per chunk times q^3 (q = the 22 → 23 ratio), x 0.2: the
                    // last step is steeper (backtest 2026-11-18 over 65 pairs, first 4 chunks → the rest: rank correlation
                    // 0.68, vs 0.14 for the old 24 → 25 extrapolation, which also overestimated ~5x unscaled)
                    let n = rows.len().max(1) as f64;
                    let q = (h[23] + 0.5) / (h[22] + 0.5);
                    let est = if rows.is_empty() { 1e9 } else if h[25] > 0.0 { h[25] / n } else { 0.2 * h[22] / n * q * q * q };
                    if best.as_ref().map_or(true, |b| est > b.0) { best = Some((est, id.clone(), c, chunk)); }
                    if prefer_cpu && !capable && best_cpu.as_ref().map_or(true, |b| est > b.0) { best_cpu = Some((est, id.clone(), c, chunk)); }
                }
            }
            if prefer_cpu && best_cpu.is_some() { best = best_cpu; }
            if let Some((est, id, c, chunk)) = best { println!("{id} {} {chunk} {est:.3e}", c.name()); }
        }
        "probe" => {
            // physics powers: hits of a shot when its k-th hit peg is the (only) green, vs the power-free shot
            let l = Lvl::new(&a[3]);
            let ch: usize = a[4].parse().unwrap();
            let pw = peggle_sim::oracle::CHAR_POWERS[ch];
            let n: usize = a.get(5).and_then(|x| x.parse().ok()).unwrap_or(2000);
            let angles = Angles::new();
            let fan = angles.coarse(n);
            let res: Vec<(usize, usize, usize)> = fan.par_iter().flat_map(|&ai| {
                let mut base = l.tmpl.clone();
                let r0 = base.shoot(angles.list[ai].0, l.click(0));
                let _ = r0;
                let hits: Vec<usize> = base.board.shot_hits.clone();
                let h0 = hits.len();
                hits.iter().filter(|&&i| { let b = l.bit[i] as usize; l.coll[b] && l.pos0[b][1] < 370.0 }).map(|&i| {
                    let mut g = l.tmpl.clone();
                    g.board.objs[i].peg_type = GREEN;
                    g.board.power = pw;
                    g.shoot(angles.list[ai].0, l.click(0));
                    (h0, g.board.shot_hits.len(), ai)
                }).collect::<Vec<_>>()
            }).collect();
            let mut b: Vec<usize> = res.iter().map(|r| r.0).collect(); b.sort();
            let mut c: Vec<usize> = res.iter().map(|r| r.1).collect(); c.sort();
            let q = |v: &Vec<usize>, f: f64| v[((v.len() - 1) as f64 * f) as usize];
            println!("{} char {ch} (power {pw}): {} (shot, green) pairs; base hits med {} p90 {} p99 {} max {} | with power med {} p90 {} p99 {} p999 {} max {}",
                     l.id, res.len(), q(&b, 0.5), q(&b, 0.9), q(&b, 0.99), b[b.len() - 1], q(&c, 0.5), q(&c, 0.9), q(&c, 0.99), q(&c, 0.999), c[c.len() - 1]);
        }
        "gpucheck" => {
            // the GPU kernel vs the CPU scanner on one seed range: identical histograms, top lists and candidates
            let l = Lvl::new(&a[3]);
            let cls = Class::parse(&a[4]);
            let bin = a[5].clone();
            let n: u64 = arg(&a, "--n", 1 << 20);
            let from: u64 = arg(&a, "--from", 0);
            let lib = load_lib(&lib_path(&dir, &l.id));
            let sc = scanner(&dir, &l, cls, &lib, arg(&a, "--floor", 22));
            let Some(spec) = export_spec(&sc) else { println!("{} {}: not GPU-capable", l.id, cls.name()); return; };
            let p = format!("{dir}/gpu/check.spec"); std::fs::create_dir_all(format!("{dir}/gpu")).unwrap(); std::fs::write(&p, spec).unwrap();
            let near = match cls { Class::Free => 25, Class::Spooky => 23, _ => 24 };
            let t = Instant::now();
            let g = gpu_range(&bin, &p, near, &[(from, from + n)]).expect("gpu").remove(0);
            let tg = t.elapsed().as_secs_f64();
            let t = Instant::now();
            let c = scan_range(&sc, from, from + n);
            let tc = t.elapsed().as_secs_f64();
            let same = g.hist == c.hist && g.top == c.top && g.cands == c.cands;
            println!("{} {}: {n} seeds: GPU {:.2}s ({:.1}M/s incl. process start) CPU {:.2}s ({:.2}M/s): {}", l.id, cls.name(), tg, n as f64 / tg / 1e6, tc, n as f64 / tc / 1e6,
                     if same { "IDENTICAL".to_string() } else { format!("DIFFER\n gpu {:?} {:?}\n cpu {:?} {:?}", &g.hist[20..], &g.top[..3.min(g.top.len())], &c.hist[20..], &c.top[..3.min(c.top.len())]) });
        }
        "status" => {
            let led = read_ledgers(&dir);
            let mut m: BTreeMap<(usize, usize, String), (HashSet<u32>, u64, f64, u32, [u64; 26], String)> = BTreeMap::new();
            for f in &led {
                let (st, ix) = levels::parse_level_id(&f[0]).unwrap();
                let e = m.entry((st, ix, f[1].clone())).or_insert((HashSet::new(), 0, 0.0, 0, [0; 26], String::new()));
                if !e.0.insert(f[3].parse().unwrap()) { continue; }
                e.1 += f[4].parse::<u64>().unwrap(); e.2 += f[5].parse::<f64>().unwrap();
                let b: u32 = f[7].parse().unwrap();
                if b > e.3 || e.5.is_empty() { e.5 = f[9].split(';').next().unwrap_or("").to_string(); }
                e.3 = e.3.max(b);
                for (k, x) in f[8].split(',').enumerate() { e.4[k] += x.parse::<u64>().unwrap(); }
            }
            let ver = std::fs::read_to_string(format!("{dir}/verified.tsv")).unwrap_or_default();
            println!("level\tclass\tchunks\tseeds\twall-h\tbest\t#24\t#25(filter)\texact wins\tbest seed:oranges:shot");
            for ((st, ix, c), e) in &m {
                let id = level_id(*st, *ix);
                let wins = ver.lines().filter(|l| { let f: Vec<&str> = l.split('\t').collect(); f[0] == id && f[1] == c && f[4] == "true" }).count();
                println!("{id}\t{c}\t{}\t{:.3e}\t{:.2}\t{}\t{}\t{}\t{wins}\t{}", e.0.len(), e.1 as f64, e.2 / 3600.0, e.3, e.4[24], e.4[25], e.5);
            }
        }
        _ => { eprintln!("usage: oneshot lib|scan|verify|status|next|cont|check|bench|... DIR ... (see solver/ONESHOT.md)"); std::process::exit(2); }
    }
}
