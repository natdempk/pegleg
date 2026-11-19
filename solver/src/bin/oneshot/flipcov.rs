//! Class "flip": Claude (char 4, Flippers) one-shots with player-pressed flippers, planned seed-independently.
//!
//! After the shot's first green p (flippers on, mid-shot) the ball's path depends only on the shot, p and the
//! flip schedule, not on colours (until the last orange: the win). So per template (library shot, first green
//! p) a beam search over flip schedules maximises the coverage h (orange-able pegs hit), and every kept
//! schedule is a continuation entry (pre = pegs hit before p, set = all pegs hit), scanned like Multiball's
//! (GPU cid 3 / the CPU physics path). The schedules live next to the continuation library (LV.flip.flips).
//!
//! flipper_clear (first activation: corner pegs hidden, each hidden orange relocated to a random non-orange peg)
//! is treated conservatively: the scanner counts |O ∩ set|; a hidden orange is never in set unless it was hit
//! before p, and relocated oranges only add hits, so a counted 25 is a win whatever the relocation does.
//!
//! oneshot flipcov DIR LV [--shots N] [--tmpl M] [--beam B] [--keep K] [--per-green G] [--rounds R]
//!   builds DIR/lib/LV.flip.cont + LV.flip.flips (resumable: per-template results in DIR/flipcov/LV.tsv)
use super::*;
use peggle_sim::phys::{Ball, Board, Event};
use peggle_sim::rng;

pub const CLAUDE: usize = 4;
pub fn power() -> u8 { peggle_sim::oracle::CHAR_POWERS[CLAUDE] }

/// A first shot in flight, steppable and clonable (Game::shoot's prelude and loop for a plain first shot).
#[derive(Clone)]
pub struct St { pub bd: Board, pub balls: Vec<Ball>, pub n: i32, pub ended: bool }

impl St {
    /// Game::shoot up to its loop (no Zen, no Fireball, no hat: a first shot)
    pub fn launch(g0: &Game, angle: f32, click: i32) -> (St, i32) {
        let mut g = g0.clone();
        assert!(!g.zen_armed() && !g.fireball() && !g.hat_now);
        g.advance_to(click + FIRE_LAG);
        g.tick += g.zen_search_ticks();
        let mut ball: Ball = g.gun.launch(angle, 6.0);
        ball.fireball = false;
        ball.spooky = g.board.spooky_left > 0;
        g.board.triple = g.trip_now;
        g.board.begin_shot();
        let mut m = rng::keyed(g.board.rng_seed, rng::SITE_SHOT, g.board.btick - 1);
        let r1 = m.rand() as u32; let r2 = m.rand() as u32;
        g.board.lucky_ctr = Some([r1, r2]);
        let t = g.tick;
        (St { bd: g.board, balls: vec![ball], n: 0, ended: false }, t)
    }
    #[inline]
    pub fn step(&mut self, ev: &mut Vec<Event>) {
        ev.clear();
        let alive = self.bd.frame_multi(&mut self.balls, ev);
        self.n += 1;
        if !alive || self.balls.iter().all(|b| b.gone) || self.n >= 30000 { self.ended = true; }
    }
    pub fn flip(&mut self, down: bool) { if self.bd.flippers_on() { self.bd.flip(down); } }
    pub fn set(&self, l: &Lvl) -> Bits { let mut s = [0u64; W]; for &i in &self.bd.shot_hits { bset(&mut s, l.bit[i] as usize); } s }
    /// the pegs first hit at or before board tick b
    pub fn set_by(&self, l: &Lvl, b: i32) -> Bits { let mut s = [0u64; W]; for (&i, &t) in self.bd.shot_hits.iter().zip(&self.bd.shot_hit_bticks) { if t <= b { bset(&mut s, l.bit[i] as usize); } } s }
}

/// the template game: all blue, p green, Claude's power
pub fn tmpl_game(l: &Lvl, p: usize) -> Game {
    let mut g = l.tmpl.clone();
    g.board.objs[l.obj[p]].peg_type = GREEN;
    g.board.power = power();
    g
}

#[derive(Clone)]
struct Node { st: St, sched: Vec<(i32, bool)>, set: Bits, end: i32, h: u32, pegs: u32 }

/// horizon (harness tick, 0 = off): the time-aware objective (fastest wins): coverage of the pegs hit by the
/// horizon, presses before it only, no end limit
pub struct Params2 { pub beam: usize, pub rounds: usize, pub stall: usize, pub step: usize, pub holds: Vec<i32>, pub near: f32, pub keep: usize, pub maxt: i32, pub horizon: i32 }

/// distance from (x, y) to a bbox
fn bdist(b: &[f32; 4], x: f32, y: f32) -> f32 {
    let dx = (b[0] - x).max(0.0).max(x - b[2]);
    let dy = (b[1] - y).max(0.0).max(y - b[3]);
    (dx * dx + dy * dy).sqrt()
}

/// weight of a covered set: orange-able pegs, and the fixed oranges outside the flipper corners (they must
/// all be hit; a corner one is hidden at the activation and relocated) first
fn score(l: &Lvl, s: &Bits) -> u32 {
    thread_local! { static FNC: std::cell::RefCell<HashMap<String, Bits>> = std::cell::RefCell::new(HashMap::new()); }
    let fnc = FNC.with(|m| *m.borrow_mut().entry(l.id.clone()).or_insert_with(|| { let c = corner(l); let mut f = l.fixed; for k in 0..W { f[k] &= !c[k]; } f }));
    pc_and(s, &l.orangeable) + 10 * pc_and(s, &fnc)
}

/// rollout without flips from st to the end: snapshots every SNAP ticks (index = n / SNAP from the start's
/// floor), per-tick "a ball is near a flipper" flags, the end state
const SNAP: i32 = 8;
fn rollout(st: &St, flippers: &[usize], near: f32) -> (Vec<St>, Vec<bool>, St) {
    let mut s = st.clone();
    let mut snaps = vec![];
    let mut nf = vec![];
    let mut ev = vec![];
    snaps.push(s.clone());
    while !s.ended {
        if s.n % SNAP == 0 && s.n != st.n { snaps.push(s.clone()); }
        let on = s.bd.flippers_on();
        nf.push(on && s.balls.iter().any(|b| !b.gone && flippers.iter().any(|&i| bdist(&s.bd.objs[i].bbox, b.pos[0], b.pos[1]) < near)));
        s.step(&mut ev);
    }
    (snaps, nf, s)
}

/// a child: from the snapshot at or before t, press at t, release at t + h, then roll out to the end
fn child(l: &Lvl, snaps: &[St], t: i32, h: i32) -> Option<(St, St)> {
    let k = snaps.partition_point(|s| s.n <= t);
    if k == 0 { return None; }
    let mut s = snaps[k - 1].clone();
    let mut ev = vec![];
    while s.n < t && !s.ended { s.step(&mut ev); }
    if s.ended || !s.bd.flippers_on() { return None; }
    s.flip(true);
    while s.n < t + h && !s.ended { s.step(&mut ev); }
    if s.ended { return None; }
    s.flip(false);
    let mid = s.clone();
    while !s.ended { s.step(&mut ev); }
    let _ = l;
    Some((mid, s))
}

/// Beam search over flip schedules for one template. Returns (sched relative to the flight start, set, end)
/// of up to keep diverse full schedules (the unflipped shot included), best coverage first.
pub fn beam(l: &Lvl, root: &St, fs: i32, prm: &Params2) -> Vec<(Vec<(i32, bool)>, Bits, i32)> {
    let flippers: Vec<usize> = (0..root.bd.objs.len()).filter(|&i| root.bd.objs[i].flipper).collect();
    let (_, _, end0) = rollout(root, &flippers, prm.near);
    let s0 = end0.set(l);
    // horizon: board tick limit of the hits that count, flight-tick limit of the presses
    let hz = prm.horizon > 0;
    let hb = prm.horizon - (fs - root.bd.btick);
    let maxt = if hz { prm.horizon - fs } else { prm.maxt };
    let sc = |s: &St| if hz { score(l, &s.set_by(l, hb)) } else { score(l, &s.set(l)) };
    let mut archive: Vec<(Vec<(i32, bool)>, Bits, i32, u32)> = vec![(vec![], s0, end0.n, sc(&end0))];
    let mut cur = vec![Node { st: root.clone(), sched: vec![], set: s0, end: end0.n, h: sc(&end0), pegs: pc(&s0) }];
    let mut best = cur[0].h;
    let mut stall = 0;
    for _round in 0..prm.rounds {
        // per node: the unflipped rollout (snapshots, near-flipper ticks), then every (node, press, hold) child in parallel
        let rolls: Vec<(Vec<St>, Vec<bool>, St)> = cur.par_iter().map(|nd| rollout(&nd.st, &flippers, prm.near)).collect();
        let mut cands: Vec<(usize, i32, i32)> = vec![];
        for (ni, nd) in cur.iter().enumerate() {
            let (_, nf, end) = &rolls[ni];
            let n0 = nd.st.n;
            if std::env::var_os("FLIP_DIAG").is_some() {
                eprintln!("  node n0 {n0} end {} near {} flippers on {} at end; end ball {:?}", end.n, nf.iter().filter(|&&x| x).count(), end.bd.flippers_on(), end.balls.iter().map(|b| b.pos).collect::<Vec<_>>());
            }
            let mut k = 0;
            for t in n0..end.n {
                let i = (t - n0) as usize;
                if !(i..(i + 16).min(nf.len())).any(|j| nf[j]) { continue; }
                if k % prm.step == 0 { for &h in &prm.holds { if t + h < maxt { cands.push((ni, t, h)); } } }
                k += 1;
            }
        }
        let kids: Vec<Node> = cands.par_iter().filter_map(|&(ni, t, h)| {
            let nd = &cur[ni];
            let (snaps, _, end) = &rolls[ni];
            let (mid, fin) = child(l, snaps, t, h)?;
            let set = fin.set(l);
            if fin.n <= end.n + 30 && set == nd.set { return None; }
            let mut sched = nd.sched.clone();
            sched.push((t, true)); sched.push((t + h, false));
            Some(Node { st: mid, sched, set, end: fin.n, h: sc(&fin), pegs: pc(&set) })
        }).collect();
        if kids.is_empty() { break; }
        for k in &kids { archive.push((k.sched.clone(), k.set, k.end, k.h)); }
        let mut kids = kids;
        // alive (relaunched) first by coverage, then pegs, then flight
        kids.sort_by(|a, b| (b.h, b.pegs, b.end).cmp(&(a.h, a.pegs, a.end)));
        let mut seen = HashSet::new();
        kids.retain(|k| (hz || k.end < prm.maxt) && seen.insert((k.set, k.end)));
        // diversity: at most half the beam from one parent schedule prefix
        let mut per: HashMap<Vec<(i32, bool)>, usize> = HashMap::new();
        let mut next = vec![];
        for k in kids {
            let c = per.entry(k.sched[..k.sched.len() - 2].to_vec()).or_insert(0);
            if *c >= (prm.beam + 1) / 2 { continue; }
            *c += 1;
            next.push(k);
            if next.len() >= prm.beam { break; }
        }
        if next.is_empty() { break; }
        if next[0].h > best { best = next[0].h; stall = 0; } else { stall += 1; }
        if stall >= prm.stall { break; }
        cur = next;
    }
    // diverse picks: best coverage first; later picks must add >= 2 pegs not in any earlier pick's set... or
    // differ by >= 4 pegs from each
    archive.sort_by(|a, b| (b.3, std::cmp::Reverse(a.2)).cmp(&(a.3, std::cmp::Reverse(b.2))));
    let mut out: Vec<(Vec<(i32, bool)>, Bits, i32)> = vec![];
    for (sched, set, end, _) in archive {
        if out.len() >= prm.keep { break; }
        let ok = out.iter().all(|o| { let mut d = 0; for x in 0..W { d += (o.1[x] ^ set[x]).count_ones(); } d >= 4 });
        if ok { out.push((sched, set, end)); }
    }
    out
}

/// pegs hidden by the first flipper activation (flipper_clear's corner regions)
pub fn corner(l: &Lvl) -> Bits {
    let mut b = l.tmpl.board.clone();
    b.flipper_clear(0);
    let mut c = [0u64; W];
    for (p, &i) in l.obj.iter().enumerate() { if l.coll[p] && !b.objs[i].collidable { bset(&mut c, p); } }
    c
}

pub fn cmd_info(a: &[String]) {
    let dir = a[2].clone();
    for id in &a[3..] {
        let l = Lvl::new(id);
        let c = corner(&l);
        let fx: Vec<String> = (0..l.npeg()).filter(|&p| bget(&l.fixed, p)).map(|p| format!("{}({:.0},{:.0}){}", p, l.pos0[p][0], l.pos0[p][1], if bget(&c, p) { "C" } else { "" })).collect();
        let (conts, _) = if std::path::Path::new(&cont_path(&dir, id, Class::Flip)).exists() { load_cont(&cont_path(&dir, id, Class::Flip)) } else { (vec![], 0) };
        let best = conts.iter().max_by_key(|c| (pc_and(&c.set, &l.fixed), pc_and(&c.set, &l.orangeable)));
        let miss: Vec<usize> = best.map(|b| (0..l.npeg()).filter(|&p| bget(&l.fixed, p) && !bget(&b.set, p)).collect()).unwrap_or_default();
        println!("{id}: pegs {} var {} fixed {} corner {} green cands {}; fixed {:?}; best entry misses fixed {:?}", l.npeg(), l.vars.len(), l.nfixed, pc(&c), pc(&green_cands(&l)), fx, miss);
    }
}

/// one round of relaunches from the unflipped shot (press ticks near a flipper, step 3, holds 8 / 24): the best
/// child's score, or None if no press re-launches the ball
fn probe(l: &Lvl, root: &St, near: f32) -> Option<u32> {
    let flippers: Vec<usize> = (0..root.bd.objs.len()).filter(|&i| root.bd.objs[i].flipper).collect();
    let (snaps, nf, end) = rollout(root, &flippers, near);
    let mut best = None;
    let mut t = root.n;
    while t < end.n {
        let i = (t - root.n) as usize;
        if (i..(i + 16).min(nf.len())).any(|j| nf[j]) {
            for h in [8, 24] {
                if let Some((_, fin)) = child(l, &snaps, t, h) {
                    if fin.n > end.n + 30 { let s = score(l, &fin.set(l)); if best.map_or(true, |b| s > b) { best = Some(s); } }
                }
            }
        }
        t += 3;
    }
    best
}

/// flipper_clear per template (shot, first green): the pegs it hides (C, collidable corner pegs at the
/// activation), the relocation candidates before the seed's oranges are removed (collidable after the
/// hiding, not hit by the shot so far; peg order) and the colour-RNG call index
pub struct Reloc { pub ent: Vec<u32>, pub tm: Vec<(Bits, Bits, u32)> }

pub fn reloc(l: &Lvl, conts: &[Cont]) -> Reloc {
    let angles = Angles::new();
    let cor = corner(l);
    let mut idx: HashMap<(u32, u8, u8), u32> = HashMap::new();
    let mut keys = vec![];
    let ent = conts.iter().map(|c| *idx.entry((c.ai, c.d, c.p)).or_insert_with(|| { keys.push((c.ai, c.d, c.p)); keys.len() as u32 - 1 })).collect();
    let tm = keys.par_iter().map(|&(ai, d, p)| {
        let g = tmpl_game(l, p as usize);
        let (mut st, _) = St::launch(&g, angles.list[ai as usize].0, l.click(d));
        let mut ev = vec![];
        let mut prev = st.clone();
        while !st.ended && !st.bd.flippers_on() { prev = st.clone(); st.step(&mut ev); }
        let (mut c, mut cb) = ([0u64; W], [0u64; W]);
        for (q, &i) in l.obj.iter().enumerate() {
            if st.bd.objs[i].collidable && !st.bd.shot_hits.contains(&i) { bset(&mut cb, q); }
        }
        // the hidden pegs, probed: a peg that stopped colliding in the activation tick (hidden, or popped in that
        // tick's object update) is hidden iff an orange there is relocated; the relocated peg must be the one the
        // model picks for the template's own seed
        for (q, &i) in l.obj.iter().enumerate() {
            if !(prev.bd.objs[i].collidable && !st.bd.objs[i].collidable) { continue; }
            let mut g2 = g.clone();
            g2.board.objs[i].peg_type = ORANGE;
            let (mut s2, _) = St::launch(&g2, angles.list[ai as usize].0, l.click(d));
            while !s2.ended && !s2.bd.flippers_on() { s2.step(&mut ev); }
            let moved: Vec<usize> = (0..l.npeg()).filter(|&x| x != q && s2.bd.objs[l.obj[x]].peg_type == ORANGE).collect();
            if moved.is_empty() { continue; }
            bset(&mut c, q);
            let mut v: Vec<u8> = (0..l.npeg()).filter(|&x| bget(&cb, x) && x != q).map(|x| x as u8).collect();
            shuffle(g.board.rng_seed, prev.bd.rng_n, &mut v);
            assert!(moved == vec![v[0] as usize], "{}: flipper_clear relocation model differs (shot {ai}/{d} p {p}: peg {q} -> {moved:?}, model {})", l.id, v[0]);
        }
        let _ = &cor;
        (c, cb, prev.bd.rng_n)
    }).collect();
    Reloc { ent, tm }
}

/// a flip entry's oranges for this seed, flipper_clear included: the hidden un-hit oranges (m of them, counted
/// with the hit ones by the game) are replaced by m random non-orange candidates; the score counts the hits
/// plus the oranges the relocation could not place (orange_total drops by them). Above `best` only.
#[inline]
pub fn count(rel: &Reloc, k: usize, set: &Bits, o: &Bits, seed: u32, best: u32, v: &mut Vec<u8>) -> u32 {
    let n = pc_and(set, o);
    let (c, cb, rn) = &rel.tm[rel.ent[k] as usize];
    let m = pc_and(c, o);
    if m == 0 || n + m <= best { return n; }
    v.clear();
    for x in 0..W { let mut w = cb[x] & !o[x]; while w != 0 { v.push((x * 64 + w.trailing_zeros() as usize) as u8); w &= w - 1; } }
    shuffle(seed, *rn, v);
    let kk = (m as usize).min(v.len());
    let r = v[..kk].iter().filter(|&&q| bget(set, q as usize)).count() as u32;
    n + r + (m - kk as u32)
}

/// oneshot flipcheck DIR LV [N]: seeds whose best shot is a flip entry: scanner count vs the exact Game sim
pub fn cmd_check(a: &[String]) {
    let dir = a[2].clone();
    let l = Lvl::new(&a[3]);
    let n: usize = a.get(4).and_then(|x| x.parse().ok()).unwrap_or(300);
    let lib = load_lib(&lib_path(&dir, &l.id));
    let sc = scanner(&dir, &l, Class::Flip, &lib, arg(a, "--floor", 18));
    let flips = load_flips(&dir, &l.id);
    let angles = Angles::new();
    let n0 = lib.shots.len() as u32;
    let mut picks = vec![];
    let mut k = 0u32;
    while picks.len() < n && k < 50_000_000 {
        let v: Vec<(u32, u32, u32)> = (k..k + 200_000).into_par_iter().filter_map(|i| {
            let seed = tas_mix(i, 993);
            let mut v = vec![];
            let (f, r) = sc.eval(seed, &mut v);
            (r != u32::MAX && r >= n0).then_some((seed, f, r))
        }).collect();
        picks.extend(v);
        k += 200_000;
    }
    picks.truncate(n);
    let res: Vec<(u32, u32, i32, bool, u32)> = picks.par_iter().map(|&(seed, f, r)| {
        let c = &sc.conts[(r - n0) as usize];
        let mut g = levels::new_game(l.st, l.ix, seed, power());
        g.earliest_fire = rt::FIRST_CLICK;
        g.flips = flips[(r - n0) as usize].clone();
        let x = g.shoot(angles.list[c.ai as usize].0, l.click(c.d));
        let m = sc.reloc.as_ref().map_or(0, |rl| { let o = l.oranges(seed, &mut vec![]); pc_and(&rl.tm[rl.ent[(r - n0) as usize] as usize].0, &o) });
        (seed, f, if x.won { 25 } else { x.oranges }, x.won, m)
    }).collect();
    let eq = res.iter().filter(|r| r.1 as i32 == r.2).count();
    let reloc = res.iter().filter(|r| r.4 > 0).count();
    let eqr = res.iter().filter(|r| r.4 > 0 && r.1 as i32 == r.2).count();
    let wins = res.iter().filter(|r| r.3).count();
    let bad: Vec<_> = res.iter().filter(|r| r.1 as i32 != r.2).take(8).collect();
    println!("{} flip: {} seeds ({} scanned) whose best is a flip entry: filter = exact on {eq} ({eqr}/{reloc} with relocated oranges); exact wins {wins}; mismatches {:?}", l.id, res.len(), k, bad);
}

/// pegs that are green for some seeds in practice: assign_greens draws from the pegs above y 370 (the variable
/// pegs only when none is left there)
pub fn green_cands2(l: &Lvl) -> Bits {
    let mut m = [0u64; W];
    for b in 0..l.npeg() { if l.coll[b] && l.pos0[b][1] < 370.0 { bset(&mut m, b); } }
    if pc(&m) < 2 { return green_cands(l); }
    m
}

/// oneshot flipdbg DIR LV SEED K: one seed and flip entry K (scanner index - library size): model vs exact sim
pub fn cmd_dbg(a: &[String]) {
    let dir = a[2].clone();
    let l = Lvl::new(&a[3]);
    let seed: u32 = a[4].parse().unwrap();
    let k: usize = a[5].parse().unwrap();
    let (conts, _) = load_cont(&cont_path(&dir, &l.id, Class::Flip));
    let flips = load_flips(&dir, &l.id);
    let rel = reloc(&l, &conts);
    let c = &conts[k];
    let mut v = vec![];
    let o = l.oranges(seed, &mut v);
    let gr = l.greens(seed, &o, &mut v);
    let cnt = count(&rel, k, &c.set, &o, seed, 0, &mut v);
    let (cc, cb, rn) = &rel.tm[rel.ent[k] as usize];
    let angles = Angles::new();
    let mut g = levels::new_game(l.st, l.ix, seed, power());
    g.earliest_fire = rt::FIRST_CLICK;
    g.flips = flips[k].clone();
    if std::env::var_os("FLIP_TRACE").is_some() { g.trace = Some(vec![]); }
    let r = g.shoot(angles.list[c.ai as usize].0, l.click(c.d));
    if let (Some(p), Some(t)) = (std::env::var_os("FLIP_TRACE"), g.trace.as_ref()) {
        let s: String = t.iter().enumerate().map(|(n, (p, v))| format!("{n} {:?} {:?} {:?} {:?}\n", p[0], p[1], v[0], v[1])).collect();
        std::fs::write(p, s).unwrap();
    }
    let mut set2 = [0u64; W]; for &i in &g.board.shot_hits { bset(&mut set2, l.bit[i] as usize); }
    if let Some(w) = std::env::var("FLIP_HITS").ok() {
        let (a0, a1) = w.split_once('-').map(|(x, y)| (x.parse::<i32>().unwrap(), y.parse::<i32>().unwrap())).unwrap();
        for (j, &i) in g.board.shot_hits.iter().enumerate() {
            let bt = g.board.shot_hit_bticks[j];
            if bt >= a0 && bt <= a1 { let o = &g.board.objs[i]; println!("  hit btick {bt} obj {i} peg {} type {} pos {:?} bbox {:?} moving {}", l.bit[i], o.peg_type, g.board.obj_pos(o.uid), o.bbox, o.mover.is_some()); }
        }
        println!("  greens hit at: {:?}", g.board.shot_hits.iter().zip(&g.board.shot_hit_bticks).filter(|(i, _)| g.board.objs[**i].peg_type == GREEN).map(|(i, t)| (l.bit[*i], *t)).collect::<Vec<_>>());
    }
    let only_e: Vec<usize> = (0..l.npeg()).filter(|&q| bget(&set2, q) && !bget(&c.set, q)).collect();
    let only_t: Vec<usize> = (0..l.npeg()).filter(|&q| !bget(&set2, q) && bget(&c.set, q)).collect();
    let o2: Vec<usize> = (0..l.npeg()).filter(|&q| g.board.objs[l.obj[q]].peg_type == ORANGE).collect();
    println!("seed {seed} entry {k} p {} green {} pre∩G {}: model {cnt} (|O∩set| {}, m {} rng n {rn}, cands {}), exact oranges {} won {} frames {} activations {:?}",
             c.p, bget(&gr, c.p as usize), pc_and(&c.pre, &gr), pc_and(&c.set, &o), pc_and(cc, &o), pc(cb), r.oranges, r.won, r.frames, r.activations);
    println!("  hits only in the exact sim {:?}, only in the template {:?}; orange pegs after {:?} (orange_total {})", only_e, only_t, o2, g.board.orange_total);
    let cl: Vec<String> = (0..l.npeg()).filter(|&q| bget(cc, q)).map(|q| { let ob = &g.board.objs[l.obj[q]]; format!("{q}{}(coll {} hit {} ff {} pos {:?})", if bget(&o, q) { "*" } else { "" }, ob.collidable, ob.hit, ob.fflags, l.pos0[q]) }).collect();
    println!("  C {:?}", cl);
    let g2: Vec<usize> = (0..l.npeg()).filter(|&q| bget(&gr, q)).collect();
    println!("  greens {:?}; oranges {:?}", g2, (0..l.npeg()).filter(|&q| bget(&o, q)).collect::<Vec<_>>());
}

fn hex(b: &Bits) -> String { b.iter().map(|w| format!("{w:016x}")).collect::<Vec<_>>().join("") }
fn unhex(s: &str) -> Bits { let mut b = [0u64; W]; for k in 0..W { b[k] = u64::from_str_radix(&s[16 * k..16 * k + 16], 16).unwrap(); } b }

pub fn flips_path(dir: &str, id: &str) -> String { format!("{dir}/lib/{id}.flip.flips") }

/// flip schedules (harness ticks) of the continuation entries, by entry index
pub fn load_flips(dir: &str, id: &str) -> Vec<Vec<(i32, bool)>> {
    std::fs::read_to_string(flips_path(dir, id)).unwrap_or_default().lines().map(|line| {
        let f = line.split('\t').nth(1).unwrap_or("");
        f.split(',').filter(|x| !x.is_empty()).map(|x| { let (t, d) = x.split_once(':').unwrap(); (t.parse().unwrap(), d == "1") }).collect()
    }).collect()
}

pub fn cmd_flipcov(a: &[String]) {
    let dir = a[2].clone();
    let l = Lvl::new(&a[3]);
    let nshots: usize = arg(a, "--shots", 30000);
    let ntmpl: usize = arg(a, "--tmpl", 30);
    let per_green: usize = arg(a, "--per-green", 4);
    let prm = Params2 {
        beam: arg(a, "--beam", 32), rounds: arg(a, "--rounds", 400), stall: arg(a, "--stall", 25), step: arg(a, "--step", 2),
        holds: arg(a, "--holds", "4,8,14,24,40,60".to_string()).split(',').filter_map(|x| x.parse().ok()).collect(),
        near: arg(a, "--near", 24.0), keep: arg(a, "--keep", 8), maxt: arg(a, "--maxt", 28000), horizon: arg(a, "--horizon", 0),
    };
    let lib = load_lib(&lib_path(&dir, &l.id));
    let angles = Angles::new();
    let gc = green_cands2(&l);
    let t0 = Instant::now();
    // templates: (shot, p) for the library's first nshots shots and every green candidate p they hit; the
    // unflipped flipper shot's coverage ranks them
    let mut tm: Vec<(u32, usize, usize, Bits, Bits)> = lib.shots.iter().take(nshots).enumerate().collect::<Vec<_>>().par_iter().flat_map_iter(|&(si, s)| {
        let mut res = vec![];
        let mut pre = [0u64; W];
        let mut seen = HashSet::new();
        for &p in &s.hits {
            let p = p as usize;
            if bget(&gc, p) && seen.insert(p) {
                let g = tmpl_game(&l, p);
                let (mut st, _) = St::launch(&g, angles.list[s.ai as usize].0, l.click(s.d));
                let mut ev = vec![];
                while !st.ended && !st.bd.flippers_on() { st.step(&mut ev); }
                if !st.ended {
                    // flippable: after the activation a ball comes near a flipper before the shot ends
                    let fl: Vec<usize> = (0..st.bd.objs.len()).filter(|&i| st.bd.objs[i].flipper).collect();
                    let mut near = false;
                    while !st.ended {
                        st.step(&mut ev);
                        near |= st.balls.iter().any(|b| !b.gone && fl.iter().any(|&i| bdist(&st.bd.objs[i].bbox, b.pos[0], b.pos[1]) < prm.near));
                    }
                    let set = st.set(&l);
                    if near { res.push((score(&l, &set), si, p, pre, set)); }
                }
            }
            bset(&mut pre, p);
        }
        res.into_iter()
    }).collect();
    tm.sort_by(|x, y| y.0.cmp(&x.0).then(x.1.cmp(&y.1)).then(x.2.cmp(&y.2)));
    let nflip = tm.len();
    tm.truncate(arg(a, "--probe", 2000));
    // probe: rank by the best single relaunch (templates where no press re-launches the ball are dropped)
    let mut tm: Vec<(u32, usize, usize, Bits, Bits)> = tm.par_iter().filter_map(|x| {
        let s = &lib.shots[x.1];
        let g = tmpl_game(&l, x.2);
        let (root, _) = St::launch(&g, angles.list[s.ai as usize].0, l.click(s.d));
        probe(&l, &root, prm.near).map(|b| (b, x.1, x.2, x.3, x.4))
    }).collect();
    let nprobe = tm.len();
    tm.sort_by(|x, y| y.0.cmp(&x.0).then(x.1.cmp(&y.1)).then(x.2.cmp(&y.2)));
    let (mut per, mut pers): (HashMap<usize, usize>, HashMap<usize, usize>) = (HashMap::new(), HashMap::new());
    let per_shot: usize = arg(a, "--per-shot", 2);
    tm.retain(|x| {
        let c = per.entry(x.2).or_insert(0); let cs = pers.entry(x.1).or_insert(0);
        if *c >= per_green || *cs >= per_shot { return false; }
        *c += 1; *cs += 1; true
    });
    tm.truncate(ntmpl);
    eprintln!("{}: {nflip} flippable templates, {nprobe} relaunch in the probe", l.id);
    eprintln!("{}: {} templates ({:.0}s); unflipped coverage {:?}", l.id, tm.len(), t0.elapsed().as_secs_f64(), tm.iter().map(|x| x.0).take(12).collect::<Vec<_>>());
    let tsv = if prm.horizon > 0 { format!("{dir}/flipcov/{}.h{}.tsv", l.id, prm.horizon) } else { format!("{dir}/flipcov/{}.tsv", l.id) };
    let done: HashSet<(u32, u8, u8)> = std::fs::read_to_string(&tsv).unwrap_or_default().lines().filter_map(|x| {
        let f: Vec<&str> = x.split('\t').collect(); (f.len() > 3).then(|| (f[0].parse().ok().unwrap_or(0), f[1].parse().ok().unwrap_or(0), f[2].parse().ok().unwrap_or(0))) }).collect();
    let mut hbest = 0;
    for (k, &(h0, si, p, pre, _)) in tm.iter().enumerate() {
        let s = &lib.shots[si];
        if done.contains(&(s.ai, s.d, p as u8)) { continue; }
        let t = Instant::now();
        let g = tmpl_game(&l, p);
        let click = l.click(s.d);
        let (root, fs) = St::launch(&g, angles.list[s.ai as usize].0, click);
        let res = beam(&l, &root, fs, &prm);
        // exact check of every kept schedule: Game::shoot with g.flips (harness ticks) hits the same pegs
        let mut lines = String::new();
        let mut hs = vec![];
        for (sched, set, end) in &res {
            let mut gg = g.clone();
            gg.flips = sched.iter().map(|&(t, d)| (fs + t, d)).collect();
            let r = gg.shoot(angles.list[s.ai as usize].0, click);
            let mut set2 = [0u64; W]; for &i in &gg.board.shot_hits { bset(&mut set2, l.bit[i] as usize); }
            assert!(set2 == *set && r.frames == *end, "{}: stepped shot != Game::shoot (shot {si} p {p}: frames {} vs {end})", l.id, r.frames);
            let hv = pc_and(set, &l.orangeable);
            hs.push(hv);
            lines += &format!("{}\t{}\t{p}\t{}\t{}\t{hv}\t{end}\t{}\n", s.ai, s.d, hex(&pre), hex(set),
                              sched.iter().map(|&(t, d)| format!("{}:{}", fs + t, d as u8)).collect::<Vec<_>>().join(","));
        }
        append(&tsv, &lines);
        hbest = hbest.max(hs.first().copied().unwrap_or(0));
        eprintln!("{} template {k}/{} shot {si} p {p}: unflipped {h0} -> h {:?} (flips {:?}) {:.0}s; level best {hbest}", l.id, tm.len(), hs,
                  res.iter().map(|r| r.0.len() / 2).collect::<Vec<_>>(), t.elapsed().as_secs_f64());
    }
    finalize(&dir, &l);
}

/// DIR/flipcov/LV.tsv (+ LV.hH.tsv, horizon runs) → DIR/lib/LV.flip.cont (largest sets first) + LV.flip.flips (same order)
pub fn finalize(dir: &str, l: &Lvl) {
    let mut txt = std::fs::read_to_string(format!("{dir}/flipcov/{}.tsv", l.id)).unwrap_or_default();
    let mut hs: Vec<String> = std::fs::read_dir(format!("{dir}/flipcov")).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|f| f.starts_with(&format!("{}.h", l.id)) && f.ends_with(".tsv")).collect()).unwrap_or_default();
    hs.sort();
    for f in hs { txt += &std::fs::read_to_string(format!("{dir}/flipcov/{f}")).unwrap_or_default(); }
    let mut v: Vec<(Cont, String)> = vec![];
    let mut seen = HashSet::new();
    for x in txt.lines() {
        let f: Vec<&str> = x.split('\t').collect();
        if f.len() < 8 { continue; }
        let c = Cont { ai: f[0].parse().unwrap(), d: f[1].parse().unwrap(), p: f[2].parse().unwrap(), pre: unhex(f[3]), set: unhex(f[4]), q: 255 };
        if !seen.insert((c.ai, c.d, c.p, c.set)) { continue; }
        v.push((c, f.get(7).unwrap_or(&"").to_string()));
    }
    v.sort_by_key(|c| std::cmp::Reverse(pc_and(&c.0.set, &l.orangeable)));
    let conts: Vec<Cont> = v.iter().map(|x| x.0.clone()).collect();
    let flips: String = v.iter().enumerate().map(|(k, x)| format!("{k}\t{}\n", x.1)).collect();
    std::fs::write(format!("{}.tmp", flips_path(dir, &l.id)), flips).unwrap();
    std::fs::rename(format!("{}.tmp", flips_path(dir, &l.id)), flips_path(dir, &l.id)).unwrap();
    save_cont(&cont_path(dir, &l.id, Class::Flip), "flipcov", &conts);
    let (p, hmax) = flip_estimate(l, &conts);
    let fmax = conts.iter().map(|c| pc_and(&c.set, &l.fixed)).max().unwrap_or(0);
    println!("{}\tflip\t{} entries\thmax {hmax}\tpegs {} var {} fixed {} (best fixed hit {fmax})\tP/seed {p:.3e}\twins/2^32 {:.3e}", l.id, conts.len(), l.npeg(), l.vars.len(), l.nfixed, p * 4294967296.0);
}

/// expected wins per seed (union bound over distinct entries): P(p green, other green not in pre) x
/// C(h_var, need)/C(V, need), with P(green) ~ 2 / (green candidates); fixed oranges must be in the set
pub fn flip_estimate(l: &Lvl, conts: &[Cont]) -> (f64, u32) {
    let v = l.vars.len() as u32;
    let need = 25u32.saturating_sub(l.nfixed);
    let ng = pc(&green_cands2(l)).max(1) as f64;
    let pg = 2.0 / ng;
    let (mut p, mut hmax) = (0.0, 0);
    let mut seen = HashSet::new();
    for c in conts {
        if !seen.insert((c.p, c.set)) { continue; }
        let fixed_in = pc_and(&c.set, &l.fixed);
        let hv = pc_and(&c.set, &l.orangeable) - fixed_in;
        hmax = hmax.max(hv + fixed_in);
        if fixed_in < l.nfixed { continue; }
        p += pg * (lnc(hv, need) - lnc(v, need)).exp();
    }
    (p, hmax)
}
