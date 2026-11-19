//! Exact Spooky Ball (Renfield, power 6) continuations for the one-shot search.
//!
//! The shot (ai, d) with peg p as its first green: up to the respawn (FUN_0045eca0) the path is seed-independent
//! (the trial counts read hit flags, not colours), so one template run gives the board at the respawn (`Snap`).
//! The respawn's 9 trial launches use the seed's keyed RNG (rng.rs SITE_SPOOKY, key = Board+0x1c4 - 1): trial 0 is
//! the capped velocity v0, trial t > 0 is best_v.x + 1 - (Rand() % 2000) / 1000, the strictly larger count wins.
//! Per seed: 8 draws -> the walk (trial counts memoized by the f32 vx) -> chosen vx -> the continuation from the
//! respawn (memoized by vx). Exact as long as the seed's other green is not hit before the respawn (it would arm a
//! second respawn and keep the bucket lid on during the trials: such seeds skip the entry) or after it (it re-arms
//! the ball: the template continuation is then a guess; every candidate is verified exactly anyway).
use super::*;
use peggle_sim::phys::{Ball, Board, Event};
use peggle_sim::x87::{x, X};
use std::sync::RwLock;

/// the board and ball right after the respawn of one (shot, first green) template run
pub struct Snap {
    pub board: Board,
    pub balls: Vec<Ball>,
    /// frames run so far (the respawn frame included)
    pub frames: i32,
    /// capped velocity (trial 0) and the RNG key (Board+0x1c4 - 1 at the respawn)
    pub v0: [f32; 2],
    pub key: u32,
    pub start: [f32; 2],
    pub r: f32,
    /// pegs hit before the respawn (p excluded), and p
    pub pre: Bits,
    pub p: usize,
    pub nhits: usize,
    /// the bucket lid was touched on the way (run_to_respawn)
    pub lid: bool,
    /// the other green when it is hit between p and this respawn (the ball stays armed: a second respawn follows), 255 = none
    pub arm: u8,
}

/// FUN_0045eca0's speed cap (x87 order as in phys.rs spooky_respawn)
fn cap(v: [f32; 2]) -> [f32; 2] {
    let (vx, vy) = (v[0], v[1]);
    let ss = ((x(vx) * vx) + (x(vy) * vy)).r();
    let speed = (ss as f64).sqrt() as f32;
    if 3.0 < speed {
        let (vx3, vy3) = ((x(vx) * 3.0).r(), (X::d(3.0) * vy).r());
        [(x(vx3) / speed).r(), (x(vy3) / speed).r()]
    } else { v }
}

/// Game::shoot's physics prefix: launch, begin_shot, the fire-time keyed roll
fn launch(g: &mut Game, angle: f32) -> Vec<Ball> {
    let bd = &mut g.board;
    let mut ball: Ball = g.gun.launch(angle, 6.0);
    ball.spooky = bd.spooky_left > 0;
    bd.triple = false;
    bd.begin_shot();
    let mut m = peggle_sim::rng::keyed(bd.rng_seed, peggle_sim::rng::SITE_SHOT, bd.btick - 1);
    let r1 = m.rand() as u32; let r2 = m.rand() as u32;
    bd.lucky_ctr = Some([r1, r2]);
    vec![ball]
}

/// From a state before the frame where object `obj` is hit first (or the launch), with `obj` already green: run to
/// the spooky respawn. `full` keeps the state before every frame (slow path, when the cheap exit test missed).
fn run_to_respawn(l: &Lvl, mut bd: Board, mut balls: Vec<Ball>, n0: i32, obj: usize, full: bool) -> Result<Result<Snap, (Board, bool)>, ()> {
    let mut ev = vec![];
    let mut lid = false;
    let h = bd.height as f32;
    for n in n0..30000 {
        ev.clear();
        // the state before a frame that may end with the respawn (the pre-cap velocity comes from replaying it)
        let b = &balls[0];
        let keep = if full || (!b.gone && b.pos[1] - b.r + b.vel[0].abs() + b.vel[1].abs() + 12.0 > h) { Some((bd.clone(), balls.clone())) } else { None };
        let left = bd.spooky_left;
        let alive = bd.frame_multi(&mut balls, &mut ev);
        for e in &ev { if let Event::Hit(i) = e { if bd.objs[*i].cover { lid = true; } } }
        if bd.spooky_left < left {
            let Some((mut b2, mut balls2)) = keep else { return Err(()) };
            balls2[0].spooky = false;
            let mut ev2 = vec![];
            b2.frame_multi(&mut balls2, &mut ev2);
            let v0 = cap(balls2[0].vel);
            let mut pre = [0u64; W];
            for &i in &bd.shot_hits { if i != obj { bset(&mut pre, l.bit[i] as usize); } }
            let key = (bd.btick - 1) as u32;
            let (start, r) = (balls[0].pos, balls[0].r);
            let nhits = bd.shot_hits.len();
            return Ok(Ok(Snap { board: bd, balls, frames: n + 1, v0, key, start, r, pre, p: l.bit[obj] as usize, nhits, lid, arm: 255 }));
        }
        if !alive || balls.iter().all(|b| b.gone) { return Ok(Err((bd, lid))); }
    }
    Ok(Err((bd, lid)))
}

fn run_or_retry(l: &Lvl, bd: Board, balls: Vec<Ball>, n0: i32, obj: usize) -> Result<Snap, (Board, bool)> {
    match run_to_respawn(l, bd.clone(), balls.clone(), n0, obj, false) { Ok(r) => r, Err(()) => run_to_respawn(l, bd, balls, n0, obj, true).unwrap() }
}

/// The template run of (angle, click) with board object `obj` green, to its spooky respawn. `base` is the all-blue
/// template advanced to click + FIRE_LAG. None if the ball never respawns (no green hit, or caught / stuck).
pub fn prepare(l: &Lvl, base: &Game, angle: f32, obj: usize) -> Option<Snap> {
    prepare_many(l, base, angle, &[(obj, usize::MAX)]).pop().unwrap()
}

/// prepare() for several first greens of one shot: the all-blue path is shared up to each green's first hit
/// (with a second green object q != usize::MAX: the armed entry, q hit between p and the respawn; None if it is not)
pub fn prepare_many(l: &Lvl, base: &Game, angle: f32, objs: &[(usize, usize)]) -> Vec<Option<Snap>> {
    let mut g = base.clone();
    g.board.power = 6;
    let balls = launch(&mut g, angle);
    // pass 1 (all blue): the frame of each first hit
    let (mut bd, mut bs) = (g.board.clone(), balls.clone());
    let mut ev = vec![];
    let mut at: HashMap<usize, i32> = HashMap::new();
    for n in 0..30000 {
        ev.clear();
        let k = bd.shot_hits.len();
        let alive = bd.frame_multi(&mut bs, &mut ev);
        for &i in &bd.shot_hits[k..] { at.entry(i).or_insert(n); }
        if !alive || bs.iter().all(|b| b.gone) { break; }
    }
    // pass 2: the state before each of those frames, then each green's own run
    let mut want: Vec<(i32, usize)> = objs.iter().enumerate().filter_map(|(j, o)| at.get(&o.0).map(|&n| (n, j))).collect();
    want.sort();
    let mut out: Vec<Option<Snap>> = objs.iter().map(|_| None).collect();
    let (mut bd, mut bs) = (g.board, balls);
    let mut n = 0;
    for (f, j) in want {
        while n < f { ev.clear(); bd.frame_multi(&mut bs, &mut ev); n += 1; }
        let (obj, q) = objs[j];
        let mut b2 = bd.clone();
        b2.objs[obj].peg_type = GREEN;
        if q != usize::MAX { b2.objs[q].peg_type = GREEN; }
        out[j] = run_or_retry(l, b2, bs.clone(), n, obj).ok().and_then(|mut s| {
            if q == usize::MAX { return Some(s); }
            // armed: q must be hit after p, before the respawn
            let hits = &s.board.shot_hits;
            let ip = hits.iter().position(|&i| i == obj)?;
            if !hits[ip..].contains(&q) || s.board.spooky_left < 1 { return None; }
            s.arm = l.bit[q] as u8;
            Some(s)
        });
    }
    out
}

impl Snap {
    pub fn count(&self, vx: f32, first: bool) -> u16 {
        self.board.spooky_trial(self.start, [vx, self.v0[1]], self.r, if first { 400 } else { 350 }) as u16
    }
    /// the continuation with the chosen velocity: pegs hit after the respawn
    pub fn resume(&self, l: &Lvl, gc: &Bits, vx: f32) -> (Bits, u8) {
        let mut bd = self.board.clone();
        let mut balls = self.balls.clone();
        balls[0].vel = [vx, self.v0[1]];
        if self.arm != 255 {
            // armed: to the second respawn (or the end)
            let b = match run_or_retry(l, bd, balls, self.frames, l.obj[self.p]) { Ok(s) => s.board, Err((b, _)) => b };
            return (bits_of(l, &b.shot_hits[self.nhits..]), 255);
        }
        let mut ev = vec![];
        for _ in self.frames..30000 {
            ev.clear();
            let alive = bd.frame_multi(&mut balls, &mut ev);
            if !alive || balls.iter().all(|b| b.gone) { break; }
        }
        let mut s = [0u64; W];
        for &i in &bd.shot_hits[self.nhits..] { bset(&mut s, l.bit[i] as usize); }
        let q0 = bd.shot_hits[self.nhits..].iter().map(|&i| l.bit[i] as usize).find(|&b| bget(gc, b)).map_or(255, |b| b as u8);
        (s, q0)
    }
}

/// one entry with its memo tables (shared by the scan threads)
pub struct Entry {
    pub snap: Snap,
    pub c0: u16,
    /// memo tables: frozen (read without a lock; merged between chunks by `freeze`) and new values
    fc: HashMap<u32, u16>,
    fp: HashMap<u32, (Bits, u8)>,
    counts: RwLock<HashMap<u32, u16>>,
    posts: RwLock<HashMap<u32, (Bits, u8)>>,
}

impl Entry {
    pub fn new(snap: Snap) -> Entry {
        let c0 = snap.count(snap.v0[0], true);
        Entry { snap, c0, fc: HashMap::new(), fp: HashMap::new(), counts: RwLock::new(HashMap::new()), posts: RwLock::new(HashMap::new()) }
    }
    pub fn freeze(&mut self) {
        self.fc.extend(self.counts.get_mut().unwrap().drain());
        self.fp.extend(self.posts.get_mut().unwrap().drain());
    }
    fn count(&self, vx: f32) -> u16 {
        if let Some(&c) = self.fc.get(&vx.to_bits()) { return c; }
        if let Some(&c) = self.counts.read().unwrap().get(&vx.to_bits()) { return c; }
        let c = self.snap.count(vx, false);
        self.counts.write().unwrap().insert(vx.to_bits(), c);
        c
    }
    /// the respawn's chosen vx for this seed (phys.rs spooky_respawn)
    pub fn walk(&self, seed: u32) -> f32 {
        let mut rng = peggle_sim::rng::keyed(seed, peggle_sim::rng::SITE_SPOOKY, self.snap.key as i32);
        let (mut best, mut vx) = (self.c0, self.snap.v0[0]);
        for _ in 1..9 {
            let d = (rng.rand() % 2000) as f64 / 1000.0;
            let v = (1.0f64 * (1.0 - d) + vx as f64) as f32;
            let n = self.count(v);
            if best < n { best = n; vx = v; }
        }
        vx
    }
    /// pegs hit after the respawn with this vx, and the first green candidate among them (255: none)
    pub fn post(&self, l: &Lvl, vx: f32) -> (Bits, u8) {
        if let Some(&s) = self.fp.get(&vx.to_bits()) { return s; }
        if let Some(&s) = self.posts.read().unwrap().get(&vx.to_bits()) { return s; }
        let s = self.snap.resume(l, &green_cands(l), vx);
        self.posts.write().unwrap().insert(vx.to_bits(), s);
        s
    }
    /// every peg the shot hits for this seed (other green as blue)
    pub fn set(&self, l: &Lvl, seed: u32) -> Bits {
        let mut s = self.post(l, self.walk(seed)).0;
        for k in 0..W { s[k] |= self.snap.pre[k]; }
        bset(&mut s, self.snap.p);
        s
    }
    pub fn sizes(&self) -> (usize, usize) { (self.fc.len() + self.counts.read().unwrap().len(), self.fp.len() + self.posts.read().unwrap().len()) }
}

/// the green candidates hit after p and before the respawn (board objects): the armed entries of this snapshot
pub fn armed_qs(l: &Lvl, gc: &Bits, s: &Snap) -> Vec<usize> {
    let hits = &s.board.shot_hits[..s.nhits];
    let p = l.obj[s.p];
    let Some(ip) = hits.iter().position(|&i| i == p) else { return vec![] };
    hits[ip + 1..].iter().copied().filter(|&i| bget(gc, l.bit[i] as usize)).collect()
}

/// every entry of one shot: per green candidate p it hits, p as the first green, and p armed again by each green
/// candidate it hits before the respawn
pub fn shot_entries(l: &Lvl, gc: &Bits, base: &Game, angle: f32) -> Vec<Snap> {
    let mut g = base.clone(); g.board.power = 6;
    // the shot's hits (all blue) give the candidate list
    let objs: Vec<(usize, usize)> = {
        let balls = launch(&mut g, angle);
        let (mut bd, mut bs) = (g.board, balls);
        let mut ev = vec![];
        for _ in 0..30000 { ev.clear(); let alive = bd.frame_multi(&mut bs, &mut ev); if !alive || bs.iter().all(|b| b.gone) { break; } }
        bd.shot_hits.iter().filter(|&&i| bget(gc, l.bit[i] as usize)).map(|&i| (i, usize::MAX)).collect()
    };
    let normal: Vec<Snap> = prepare_many(l, base, angle, &objs).into_iter().flatten().collect();
    let armed: Vec<(usize, usize)> = normal.iter().flat_map(|s| armed_qs(l, gc, s).into_iter().map(move |q| (l.obj[s.p], q))).collect();
    let mut out = normal;
    out.extend(prepare_many(l, base, angle, &armed).into_iter().flatten());
    out
}

/// the entries of the top library shots: (shot index, entry), one per green candidate the shot hits
pub fn entries(l: &Lvl, lib: &Lib, shots: &[usize]) -> Vec<(usize, Entry)> {
    let angles = Angles::new();
    let gc = green_cands(l);
    let mut bases: HashMap<u8, Game> = HashMap::new();
    for &si in shots { let d = lib.shots[si].d; bases.entry(d).or_insert_with(|| { let mut b = l.tmpl.clone(); b.advance_to(l.click(d) + FIRE_LAG); b }); }
    shots.par_iter().flat_map_iter(|&si| {
        let s = &lib.shots[si];
        let base = &bases[&s.d];
        let angle = angles.list[s.ai as usize].0;
        shot_entries(l, &gc, base, angle).into_iter().map(|sn| (si, Entry::new(sn))).collect::<Vec<_>>().into_iter()
    }).collect()
}

pub fn cmd(a: &[String], dir: &str) {
    let sub = a[3].as_str();
    let l = Lvl::new(&a[4]);
    let lib = load_lib(&lib_path(dir, &l.id));
    let angles = Angles::new();
    let nshot: usize = arg(a, "--shots", 20);
    let nseed: u32 = arg(a, "--seeds", 50);
    match sub {
        "check" => {
            // per (entry, seed, other green q): the predicted hit set vs Game::shoot of the template with p and q green
            // and the seed's keyed RNG; then oranges vs the real game for random seeds whose first green is an entry's
            let t = Instant::now();
            let shots: Vec<usize> = (0..nshot.min(lib.shots.len())).collect();
            let es = entries(&l, &lib, &shots);
            eprintln!("{}: {} entries from {} shots in {:.1}s", l.id, es.len(), shots.len(), t.elapsed().as_secs_f64());
            let gc = green_cands(&l);
            let t = Instant::now();
            let meta: Vec<usize> = es.iter().map(|x| x.0).collect();
            let all = [u64::MAX; W];
            let ex = Exact::new(&l, es.into_iter().map(|x| x.1).collect(), vec![all; meta.len()]);
            let res: Vec<(u32, u32, u32)> = (0..meta.len()).into_par_iter().map(|k| {
                let e = &ex.entries[k];
                let s = &lib.shots[meta[k]];
                let qs: Vec<usize> = (0..l.npeg()).filter(|&q| q != e.snap.p && bget(&gc, q) && !bget(&e.snap.pre, q)).collect();
                let (mut ok, mut n, mut two) = (0, 0, 0);
                for j in 0..nseed {
                    let seed = tas_mix(j, 0x5b00 + k as u32);
                    let q = if e.snap.arm != 255 { Some(e.snap.arm as usize) } else if j % 4 == 0 || qs.is_empty() { None } else { Some(qs[tas_mix(seed, 3) as usize % qs.len()]) };
                    let pred = ex.final_set(&l, k, seed, q, None);
                    let mut g = l.tmpl.clone();
                    g.board.rng_seed = seed;
                    g.board.power = 6;
                    g.board.objs[l.obj[e.snap.p]].peg_type = GREEN;
                    if let Some(q) = q { g.board.objs[l.obj[q]].peg_type = GREEN; }
                    let r = g.shoot(angles.list[s.ai as usize].0, l.click(s.d));
                    if r.activations.len() > 1 { two += 1; }
                    let got = bits_of(&l, &g.board.shot_hits);
                    n += 1; if got == pred { ok += 1; } else if std::env::var_os("SPOOKY_DIAG").is_some() { eprintln!("  mismatch entry {k} p {} q {q:?} seed {seed}: pred {} got {}", e.snap.p, pc(&pred), pc(&got)); }
                }
                (ok, n, two)
            }).collect();
            let (ok, n, two) = res.iter().fold((0, 0, 0), |a, b| (a.0 + b.0, a.1 + b.1, a.2 + b.2));
            let st: Vec<u64> = ex.stats.iter().map(|x| x.load(std::sync::atomic::Ordering::Relaxed)).collect();
            println!("{} template check: {ok}/{n} hit sets equal, {two} with both greens activated ({:.1}s; second-respawn lookups {} built {})", l.id, t.elapsed().as_secs_f64(), st[1], st[2]);
            // real game: random seeds, each paired with one library shot; the entry of the shot's first green of the seed
            let by: HashMap<(usize, usize, u8), usize> = (0..meta.len()).map(|k| ((meta[k], ex.entries[k].snap.p, ex.entries[k].snap.arm), k)).collect();
            let nreal: u32 = arg(a, "--real", 4000);
            let rows: Vec<(bool, i32, u32)> = (0..nreal).into_par_iter().filter_map(|j| {
                let seed = tas_mix(j, 0xabc);
                let mut v = vec![];
                let o = l.oranges(seed, &mut v);
                let gr = l.greens(seed, &o, &mut v);
                let si = shots[(j as usize) % shots.len()];
                let s = &lib.shots[si];
                let p = *s.hits.iter().find(|&&p| bget(&gr, p as usize))? as usize;
                let q = (0..l.npeg()).find(|&q| q != p && bget(&gr, q));
                let mut k = *by.get(&(si, p, 255))?;
                if let Some(q) = q { if bget(&ex.entries[k].snap.pre, q) { k = *by.get(&(si, p, q as u8))?; } } // q before the respawn: the armed entry
                let pred = pc_and(&ex.final_set(&l, k, seed, q, None), &o);
                let mut g = levels::new_game(l.st, l.ix, seed, 6);
                g.earliest_fire = rt::FIRST_CLICK;
                let r = g.shoot(angles.list[s.ai as usize].0, l.click(s.d));
                Some((r.activations.len() > 1, r.oranges, pred))
            }).collect();
            let one: Vec<_> = rows.iter().filter(|r| !r.0).collect();
            let two: Vec<_> = rows.iter().filter(|r| r.0).collect();
            println!("{} real check: oranges equal {}/{} (one green hit), {}/{} (both greens hit)", l.id,
                one.iter().filter(|r| r.1 as u32 == r.2).count(), one.len(), two.iter().filter(|r| r.1 as u32 == r.2).count(), two.len());
        }
        "measure" => {
            // every (library shot, first green) entry: sampled seeds' hit sets -> the per-seed win odds; writes
            // OUT/LV.measure.tsv (best first): si ai d p pre_size est h_samples
            let out: String = arg(a, "--out", format!("{dir}/../oneshot_spooky"));
            let samples: u32 = arg(a, "--samples", 4);
            let nshot = nshot.min(lib.shots.len());
            let gc = green_cands(&l);
            // P(peg is green) from the colour model
            let mut pg = vec![0f64; l.npeg()];
            let ns = 20000u32;
            { let mut v = vec![]; for k in 0..ns { let seed = tas_mix(k, 31337); let o = l.oranges(seed, &mut v); let g = l.greens(seed, &o, &mut v); for p in 0..l.npeg() { if bget(&g, p) { pg[p] += 1.0 / ns as f64; } } } }
            let pgsum: f64 = pg.iter().sum();
            let vmask = { let mut m = [0u64; W]; for &b in &l.vars { bset(&mut m, b as usize); } m };
            let nv = l.vars.len() as u32;
            let need = 25u32.saturating_sub(l.nfixed);
            let t = Instant::now();
            let done = std::sync::atomic::AtomicUsize::new(0);
            let two = a.iter().any(|x| x == "--two");
            let mut rows: Vec<(f64, usize, u32, u8, usize, u32, Vec<u32>, f64)> = (0..nshot).into_par_iter().flat_map_iter(|si| {
                let s = &lib.shots[si];
                let mut base = l.tmpl.clone(); base.advance_to(l.click(s.d) + FIRE_LAG);
                let angle = angles.list[s.ai as usize].0;
                let mut res = vec![];
                let ps: Vec<(usize, usize)> = s.hits.iter().filter(|&&p| bget(&gc, p as usize)).map(|&p| (l.obj[p as usize], usize::MAX)).collect();
                for sn in prepare_many(&l, &base, angle, &ps).into_iter().flatten() {
                    let p = sn.p;
                    let e = Entry::new(sn);
                    let pre_other: f64 = (0..l.npeg()).filter(|&q| bget(&e.snap.pre, q)).map(|q| pg[q]).sum();
                    let pgreen = pg[p] * (1.0 - pre_other / (pgsum - pg[p]).max(1e-9)).max(0.0);
                    let (mut est, mut hs) = (0.0, vec![]);
                    let (mut est2, mut hs2) = (0.0, vec![]);
                    let odds = |set: &Bits| -> f64 {
                        if pc_and(set, &l.fixed) < l.nfixed { return 0.0; }
                        let pv = l.var[p] as u32;
                        let hv = pc_and(set, &vmask) - pv;
                        (lnc(hv, need) - lnc(nv - pv, need)).exp()
                    };
                    // the other green: drawn by P(green) among the candidates not hit before the respawn
                    let qs: Vec<(usize, f64)> = (0..l.npeg()).filter(|&q| q != p && pg[q] > 0.0 && !bget(&e.snap.pre, q)).map(|q| (q, pg[q])).collect();
                    let qsum: f64 = qs.iter().map(|x| x.1).sum();
                    for k in 0..samples {
                        let seed = tas_mix(k, 0x5b5b ^ (si as u32) << 8 ^ p as u32);
                        let set = e.set(&l, seed);
                        hs.push(pc_and(&set, &l.orangeable));
                        est += odds(&set) / samples as f64;
                        if two && !qs.is_empty() {
                            let mut u = (tas_mix(seed, 7) as f64 / 4294967296.0) * qsum;
                            let q = qs.iter().find(|x| { u -= x.1; u < 0.0 }).unwrap_or(qs.last().unwrap()).0;
                            let mut g = l.tmpl.clone();
                            g.board.rng_seed = seed; g.board.power = 6;
                            g.board.objs[l.obj[p]].peg_type = GREEN; g.board.objs[l.obj[q]].peg_type = GREEN;
                            g.shoot(angle, l.click(s.d));
                            let mut set2 = [0u64; W];
                            for &i in &g.board.shot_hits { bset(&mut set2, l.bit[i] as usize); }
                            set2[q >> 6] &= !(1u64 << (q & 63));
                            hs2.push(pc_and(&set2, &l.orangeable));
                            est2 += odds(&set2) / samples as f64;
                        }
                    }
                    hs.extend(hs2);
                    res.push((est * pgreen, si, s.ai, s.d, p, pc_and(&e.snap.pre, &l.orangeable), hs, est2 * pgreen));
                }
                let n = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if n % 1000 == 999 { eprintln!("  {} shots {:.0}s", n + 1, t.elapsed().as_secs_f64()); }
                res.into_iter()
            }).collect();
            rows.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
            std::fs::create_dir_all(&out).unwrap();
            let mut txt = String::new();
            for r in &rows { txt += &format!("{}\t{}\t{}\t{}\t{}\t{:.3e}\t{:.3e}\t{}\n", r.1, r.2, r.3, r.4, r.5, r.0, r.7, r.6.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",")); }
            std::fs::write(format!("{out}/{}.measure.tsv", l.id), txt).unwrap();
            let tot: f64 = rows.iter().map(|r| r.0).sum();
            let tot2: f64 = rows.iter().map(|r| r.7).sum();
            let fixed_ok = rows.iter().filter(|r| r.0 > 0.0).count();
            let hmax = rows.iter().flat_map(|r| r.6.iter()).max().copied().unwrap_or(0);
            let mut hist = [0usize; 8];
            for r in &rows { let m = *r.6.iter().max().unwrap_or(&0); for (k, th) in [40, 45, 50, 55, 60, 65, 70, 75].iter().enumerate() { if m >= *th { hist[k] += 1; } } }
            println!("{}\tspooky measure\t{} shots {} entries {:.0}s\tvar {nv} fixed {} (entries holding them {fixed_ok})\thmax {hmax} (lib hmax {})\tentries with h>=40,45,..75: {:?}\test wins/2^32 {:.3e} (union bound; top entry {:.3e}); with the other green {:.3e}",
                l.id, nshot, rows.len(), t.elapsed().as_secs_f64(), l.nfixed, lib.shots.iter().map(|s| pc_and(&s.set, &l.orangeable)).max().unwrap_or(0), hist, tot * 4294967296.0, rows.first().map_or(0.0, |r| r.0) * 4294967296.0, tot2 * 4294967296.0);
        }
        "bench" => {
            let shots: Vec<usize> = (0..nshot.min(lib.shots.len())).collect();
            let es = entries(&l, &lib, &shots);
            let e = &es[es.len() / 2].1;
            let t = Instant::now(); let mut z = 0; for _ in 0..200 { z += e.snap.board.clone().objs.len(); } let tc = t.elapsed().as_secs_f64() / 200.0;
            let t = Instant::now(); for k in 0..200 { z += e.snap.count(e.snap.v0[0] + k as f32 * 0.001, false) as usize; } let tt = t.elapsed().as_secs_f64() / 200.0;
            let t = Instant::now(); for k in 0..50 { z += pc(&e.snap.resume(&l, &green_cands(&l), e.snap.v0[0] + k as f32 * 0.01).0) as usize; } let tr = t.elapsed().as_secs_f64() / 50.0;
            println!("{}: board clone {:.1} us, trial {:.1} us, resume {:.1} us ({} objs, {z})", l.id, tc * 1e6, tt * 1e6, tr * 1e6, e.snap.board.objs.len());
            // memo growth: distinct trial vx / chosen vx after n seeds
            let e = Entry::new(prepare(&l, &{ let s = &lib.shots[es[es.len() / 2].0]; let mut b = l.tmpl.clone(); b.advance_to(l.click(s.d) + FIRE_LAG); b }, angles.list[lib.shots[es[es.len() / 2].0].ai as usize].0, l.obj[e.snap.p]).unwrap());
            let mut n = 0u32;
            let t = Instant::now();
            for lim in [100u32, 1000, 10000, 100000, 1000000] {
                let chunk: Vec<u32> = (n..lim).collect();
                let vs: Vec<f32> = chunk.par_iter().map(|&k| e.walk(tas_mix(k, 999))).collect();
                n = lim;
                let mut ch: Vec<u32> = vs.iter().map(|v| v.to_bits()).collect(); ch.sort(); ch.dedup();
                println!("  {lim} seeds: {} distinct trial vx, chosen vx in last batch {} ({:.0}s)", e.sizes().0, ch.len(), t.elapsed().as_secs_f64());
                if t.elapsed().as_secs_f64() > 300.0 { break; }
            }
        }
        "gate" => {
            // share of the win odds kept by the second-stage gate R: per sampled (entry, seed, other green), oranges
            // uniform over the variable pegs: P(O ⊆ S) split by j = oranges outside the first stage's set S1
            let conts = load_cont(&cont_path(dir, &l.id, Class::Spooky)).0;
            let ex = exact_of(&l, &conts);
            let gc = green_cands(&l);
            let vmask = { let mut m = [0u64; W]; for &b in &l.vars { bset(&mut m, b as usize); } m };
            let nv = l.vars.len() as u32;
            let need = 25u32.saturating_sub(l.nfixed);
            let acc: Vec<[f64; 26]> = (0..conts.len()).into_par_iter().map(|k| {
                let e = &ex.entries[k];
                let qs: Vec<usize> = (0..l.npeg()).filter(|&q| q != e.snap.p && bget(&gc, q) && !bget(&e.snap.pre, q)).collect();
                let mut h = [0f64; 26];
                for j in 0..nseed {
                    let seed = tas_mix(j, 0x6a7e ^ k as u32);
                    if qs.is_empty() { continue; }
                    let q = qs[tas_mix(seed, 5) as usize % qs.len()];
                    let s = ex.final_set(&l, k, seed, Some(q), None);
                    let s1 = ex.final_set(&l, k, seed, Some(q), Some((&[u64::MAX; W], 0)));
                    if pc_and(&s, &l.fixed) < l.nfixed { continue; }
                    let a = pc_and(&s1, &vmask);
                    let b = pc_and(&s, &vmask) - a;
                    for jj in 0..=need.min(b) { if jj <= need && need - jj <= a { h[jj as usize] += (lnc(a, need - jj) + lnc(b, jj) - lnc(nv, need)).exp(); } }
                }
                h
            }).collect();
            let mut h = [0f64; 26];
            for x in &acc { for j in 0..26 { h[j] += x[j]; } }
            let tot: f64 = h.iter().sum();
            let mut cum = 0.0;
            for j in 0..26 { cum += h[j]; println!("R {j:2}: {:.3} of the odds", cum / tot); }
        }
        _ => panic!("spooky check|measure|scan"),
    }
}

/// after the first respawn with chosen vx1, the other green q hit: the ball is re-armed (second respawn) or the shot
/// ends without one (Final: every peg hit)
pub enum E2 { Final(Bits), Re(Entry) }

fn bits_of(l: &Lvl, hits: &[usize]) -> Bits { let mut s = [0u64; W]; for &i in hits { bset(&mut s, l.bit[i] as usize); } s }

impl Entry {
    /// (the second-stage entry, whether the bucket lid was touched after the re-arm: if not, the result holds for
    /// every other green hit at or after q)
    pub fn second(&self, l: &Lvl, vx1: f32, q: Option<usize>) -> (E2, bool) {
        let mut bd = self.snap.board.clone();
        if let Some(q) = q { bd.objs[l.obj[q]].peg_type = GREEN; }
        let mut balls = self.snap.balls.clone();
        balls[0].vel = [vx1, self.snap.v0[1]];
        match run_or_retry(l, bd, balls, self.snap.frames, l.obj[self.snap.p]) {
            Ok(s) => { let lid = s.lid; (E2::Re(Entry::new(s)), lid) }
            Err((b, lid)) => (E2::Final(bits_of(l, &b.shot_hits)), lid),
        }
    }
}

/// The exact Spooky evaluator: entries (shot, first green p) with their union bounds, a cache of second-respawn
/// entries keyed by (entry, vx1, q).
pub struct Exact {
    pub entries: Vec<Entry>,
    /// per entry: every peg the sampled seeds' shots hit (with a second green), the scan's bound
    pub u: Vec<Bits>,
    pub by_p: Vec<Vec<u32>>,
    e2: std::sync::Mutex<HashMap<(u32, u32, u8), std::sync::Arc<E2>>>,
    pub cap: usize,
    /// second stage gate (final_set)
    pub r: u32,
    /// memo file and tag: also saved every 10 minutes during a chunk (save_shared)
    pub memo: Option<(String, u64)>,
    last_save: std::sync::Mutex<Instant>,
    pub stats: [std::sync::atomic::AtomicU64; 6],
}

impl Exact {
    pub fn new(l: &Lvl, entries: Vec<Entry>, u: Vec<Bits>) -> Exact {
        let mut by_p: Vec<Vec<u32>> = vec![vec![]; l.npeg()];
        for (k, e) in entries.iter().enumerate() { by_p[e.snap.p].push(k as u32); }
        for v in by_p.iter_mut() { v.sort_by_key(|&k| (std::cmp::Reverse(pc_and(&u[k as usize], &l.orangeable)), k)); }
        Exact { entries, u, by_p, e2: std::sync::Mutex::new(HashMap::new()), cap: env_or("SPOOKY_E2_CAP", 4000), r: env_or("SPOOKY_R", 8) as u32, memo: None, last_save: std::sync::Mutex::new(Instant::now()), stats: Default::default() }
    }
    pub fn freeze(&mut self) { for e in self.entries.iter_mut() { e.freeze(); } }
    /// the first-stage memo tables (trial counts, continuations) of every entry: `tag` = the continuation library's hash
    pub fn save_memo(&mut self, path: &str, tag: u64) {
        self.freeze();
        let mut b: Vec<u8> = vec![];
        b.extend(b"OSM1"); b.extend(tag.to_le_bytes()); b.extend((self.entries.len() as u32).to_le_bytes());
        for e in &self.entries {
            b.extend((e.fc.len() as u32).to_le_bytes());
            for (&k, &c) in &e.fc { b.extend(k.to_le_bytes()); b.extend(c.to_le_bytes()); }
            b.extend((e.fp.len() as u32).to_le_bytes());
            for (&k, (s, q)) in &e.fp { b.extend(k.to_le_bytes()); for w in s { b.extend(w.to_le_bytes()); } b.push(*q); }
        }
        let _ = std::fs::write(format!("{path}.tmp"), &b);
        let _ = std::fs::rename(format!("{path}.tmp"), path);
    }
    /// save_memo without &mut (frozen + new values, read under the locks)
    fn save_shared(&self) {
        let Some((path, tag)) = &self.memo else { return };
        let mut b: Vec<u8> = vec![];
        b.extend(b"OSM1"); b.extend(tag.to_le_bytes()); b.extend((self.entries.len() as u32).to_le_bytes());
        for e in &self.entries {
            let c = e.counts.read().unwrap(); let ps = e.posts.read().unwrap();
            b.extend(((e.fc.len() + c.len()) as u32).to_le_bytes());
            for (&k, &v) in e.fc.iter().chain(c.iter()) { b.extend(k.to_le_bytes()); b.extend(v.to_le_bytes()); }
            b.extend(((e.fp.len() + ps.len()) as u32).to_le_bytes());
            for (&k, (s, q)) in e.fp.iter().chain(ps.iter()) { b.extend(k.to_le_bytes()); for w in s { b.extend(w.to_le_bytes()); } b.push(*q); }
        }
        let _ = std::fs::write(format!("{path}.tmp"), &b);
        let _ = std::fs::rename(format!("{path}.tmp"), path);
    }
    pub fn load_memo(&mut self, path: &str, tag: u64) -> bool {
        let Ok(b) = std::fs::read(path) else { return false };
        if b.len() < 16 || &b[..4] != b"OSM1" || u64::from_le_bytes(b[4..12].try_into().unwrap()) != tag { return false; }
        let u32at = |p: usize| u32::from_le_bytes(b[p..p + 4].try_into().unwrap());
        if u32at(12) as usize != self.entries.len() { return false; }
        let mut p = 16;
        for e in self.entries.iter_mut() {
            let n = u32at(p) as usize; p += 4;
            for _ in 0..n { e.fc.insert(u32at(p), u16::from_le_bytes([b[p + 4], b[p + 5]])); p += 6; }
            let n = u32at(p) as usize; p += 4;
            for _ in 0..n {
                let k = u32at(p); p += 4;
                let mut s = [0u64; W]; for x in 0..W { s[x] = u64::from_le_bytes(b[p..p + 8].try_into().unwrap()); p += 8; }
                e.fp.insert(k, (s, b[p])); p += 1;
            }
        }
        true
    }
    fn st(&self, i: usize) { self.stats[i].fetch_add(1, std::sync::atomic::Ordering::Relaxed); }
    /// every peg entry k's shot hits for this seed whose other green is q (None: q not on the board's path)
    /// `gate` = (oranges, R): the second stage only runs when at most R oranges are left after the first (else the
    /// first stage's set, a lower bound, is returned)
    pub fn final_set(&self, l: &Lvl, k: usize, seed: u32, q: Option<usize>, gate: Option<(&Bits, u32)>) -> Bits {
        let e = &self.entries[k];
        let vx1 = e.walk(seed);
        let (post, q0) = e.post(l, vx1);
        let mut s1 = post;
        for x in 0..W { s1[x] |= e.snap.pre[x]; }
        bset(&mut s1, e.snap.p);
        if e.snap.arm != 255 {
            // armed twice: the second respawn always follows
            if let Some((o, r)) = gate { if pc(o) - pc_and(o, &s1) > r { self.st(5); return s1; } }
            self.st(1);
            let key = (k as u32, vx1.to_bits(), 255u8);
            let hit = self.e2.lock().unwrap().get(&key).cloned();
            let e2 = match hit { Some(x) => x, None => {
                self.st(2);
                let x = std::sync::Arc::new(e.second(l, vx1, None).0);
                let mut m = self.e2.lock().unwrap();
                if m.len() >= self.cap { m.clear(); self.st(3); }
                m.insert(key, x.clone());
                x
            } };
            return match &*e2 { E2::Final(s) => *s, E2::Re(e2) => e2.set(l, seed) };
        }
        let Some(q) = q.filter(|&q| bget(&post, q)) else { return s1; };
        if let Some((o, r)) = gate { if pc(o) - pc_and(o, &s1) > r { self.st(5); return s1; } }
        self.st(1);
        // the earliest green candidate q0 on the path re-arms first: without a lid touch after that the second stage
        // is the same for every q (key 255)
        let (kany, kq) = ((k as u32, vx1.to_bits(), 255u8), (k as u32, vx1.to_bits(), q as u8));
        let hit = { let m = self.e2.lock().unwrap(); m.get(&kany).or_else(|| m.get(&kq)).cloned() };
        let e2 = match hit {
            Some(x) => x,
            None => {
                self.st(2);
                let (x0, lid) = e.second(l, vx1, Some(q0 as usize));
                let x0 = std::sync::Arc::new(x0);
                let x = if !lid || q as u8 == q0 { x0.clone() } else { self.st(4); std::sync::Arc::new(e.second(l, vx1, Some(q)).0) };
                let mut m = self.e2.lock().unwrap();
                if m.len() >= self.cap { m.clear(); self.st(3); }
                if !lid { m.insert(kany, x0); } else { m.insert((k as u32, vx1.to_bits(), q0), x0); m.insert(kq, x.clone()); }
                x
            }
        };
        match &*e2 { E2::Final(s) => *s, E2::Re(e2) => e2.set(l, seed) }
    }
    /// best oranges over the entries for this seed's colours (above `best`), and the entry
    pub fn eval(&self, l: &Lvl, seed: u32, o: &Bits, gr: &Bits, best: &mut u32, bi: &mut u32) {
        if seed & 0xffff == 0 && self.memo.is_some() {
            if let Ok(mut t) = self.last_save.try_lock() { if t.elapsed().as_secs() > 600 { self.save_shared(); *t = Instant::now(); } }
        }
        let gs: Vec<usize> = (0..l.npeg()).filter(|&g| bget(gr, g)).collect();
        for (a, &p) in gs.iter().enumerate() {
            let q = gs.iter().enumerate().find(|x| x.0 != a).map(|x| *x.1);
            for &k in &self.by_p[p] {
                let k = k as usize;
                if pc_and(&self.u[k], &l.orangeable).min(25) <= *best { break; }
                let arm = self.entries[k].snap.arm;
                if arm != 255 { if q != Some(arm as usize) { continue; } }
                else if let Some(q) = q { if bget(&self.entries[k].snap.pre, q) { continue; } }
                if pc_and(&self.u[k], o) <= *best { continue; }
                self.st(0);
                let s = self.final_set(l, k, seed, q, Some((o, self.r)));
                let n = pc_and(&s, o);
                if n > *best { *best = n; *bi = k as u32; if n >= 25 { return; } }
            }
        }
    }
}

/// The Spooky continuation library: entries (library shot, first green) of the first `nshot` library shots, ranked by
/// the sampled per-seed win odds (other green drawn by P(green), second respawn included); the top `keep` with
/// their union bound over `usamp` sampled seeds. Cont.pre = pegs hit before the first respawn, Cont.set = the bound.
pub fn build_cont(l: &Lvl, lib: &Lib, nshot: usize, samples: u32, keep: usize, usamp: u32) -> Vec<Cont> {
    let gc = green_cands(l);
    let mut pg = vec![0f64; l.npeg()];
    let ns = 20000u32;
    { let mut v = vec![]; for k in 0..ns { let seed = tas_mix(k, 31337); let o = l.oranges(seed, &mut v); let g = l.greens(seed, &o, &mut v); for p in 0..l.npeg() { if bget(&g, p) { pg[p] += 1.0 / ns as f64; } } } }
    let pgsum: f64 = pg.iter().sum();
    let vmask = { let mut m = [0u64; W]; for &b in &l.vars { bset(&mut m, b as usize); } m };
    let nv = l.vars.len() as u32;
    let need = 25u32.saturating_sub(l.nfixed);
    let angles = Angles::new();
    let nshot = nshot.min(lib.shots.len());
    let t = Instant::now();
    let done = std::sync::atomic::AtomicUsize::new(0);
    // one sample: (seed, q) -> final set, q drawn among the candidates not hit before the respawn (None: no other green on the path)
    let sample = |ex: &Exact, k: usize, seed: u32| -> Bits {
        let e = &ex.entries[k];
        if e.snap.arm != 255 { return ex.final_set(l, k, seed, Some(e.snap.arm as usize), None); }
        let qs: Vec<(usize, f64)> = (0..l.npeg()).filter(|&q| q != e.snap.p && pg[q] > 0.0 && !bget(&e.snap.pre, q)).map(|q| (q, pg[q])).collect();
        let qsum: f64 = qs.iter().map(|x| x.1).sum();
        let mut u = (tas_mix(seed, 7) as f64 / 4294967296.0) * qsum;
        let q = qs.iter().find(|x| { u -= x.1; u < 0.0 }).or(qs.last()).map(|x| x.0);
        ex.final_set(l, k, seed, q, None)
    };
    let mut rows: Vec<(f64, Cont)> = (0..nshot).into_par_iter().flat_map_iter(|si| {
        let s = &lib.shots[si];
        let mut base = l.tmpl.clone(); base.advance_to(l.click(s.d) + FIRE_LAG);
        let angle = angles.list[s.ai as usize].0;
        let es: Vec<Entry> = shot_entries(l, &gc, &base, angle).into_iter().map(Entry::new).collect();
        let n = es.len();
        let ex = Exact::new(l, es, vec![[0; W]; n]);
        let mut res = vec![];
        for k in 0..n {
            let e = &ex.entries[k];
            let p = e.snap.p;
            let pre_other: f64 = (0..l.npeg()).filter(|&q| bget(&e.snap.pre, q)).map(|q| pg[q]).sum();
            let pgreen = if e.snap.arm != 255 { pg[p] * pg[e.snap.arm as usize] / (pgsum - pg[p]).max(1e-9) }
                         else { pg[p] * (1.0 - pre_other / (pgsum - pg[p]).max(1e-9)).max(0.0) };
            let mut est = 0.0;
            for j in 0..samples {
                let set = sample(&ex, k, tas_mix(j, 0x5b5b ^ (si as u32) << 8 ^ p as u32));
                if pc_and(&set, &l.fixed) < l.nfixed { continue; }
                let pv = l.var[p] as u32;
                let hv = pc_and(&set, &vmask) - pv;
                est += (lnc(hv, need) - lnc(nv - pv, need)).exp() / samples as f64;
            }
            res.push((est * pgreen, Cont { ai: s.ai, d: s.d, p: p as u8, pre: e.snap.pre, set: [0; W], q: e.snap.arm }));
        }
        let c = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if c % 500 == 499 { eprintln!("  spooky cont: {} shots {:.0}s", c + 1, t.elapsed().as_secs_f64()); }
        res.into_iter()
    }).collect();
    rows.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap().then(a.1.ai.cmp(&b.1.ai)).then(a.1.d.cmp(&b.1.d)).then(a.1.p.cmp(&b.1.p)).then(a.1.q.cmp(&b.1.q)));
    let mut cum = 0.0;
    let all: f64 = rows.iter().map(|r| r.0).sum();
    let mut marks = vec![];
    for (i, r) in rows.iter().enumerate() { cum += r.0; if [100, 300, 1000, 3000, 10000, 30000].contains(&(i + 1)) { marks.push(format!("{}: {:.3e}", i + 1, cum * 4294967296.0)); } }
    eprintln!("{} spooky cont: {} entries, est wins/2^32 {:.3e} in all; best k: {}", l.id, rows.len(), all * 4294967296.0, marks.join(", "));
    rows.truncate(keep);
    let tot: f64 = rows.iter().map(|r| r.0).sum();
    eprintln!("{} spooky cont: kept {} entries, est wins/2^32 {:.3e} (union bound), {:.0}s; union bounds ...", l.id, rows.len(), tot * 4294967296.0, t.elapsed().as_secs_f64());
    // union bounds of the kept entries
    let mut conts: Vec<Cont> = rows.into_iter().map(|r| r.1).collect();
    let ex = exact_of(l, &conts);
    let sets: Vec<Bits> = (0..conts.len()).into_par_iter().map(|k| {
        let e = &ex.entries[k];
        let mut u = e.snap.pre; bset(&mut u, e.snap.p);
        for j in 0..usamp { let s = sample(&ex, k, tas_mix(j, 0x7e57 ^ k as u32)); for x in 0..W { u[x] |= s[x]; } }
        u
    }).collect();
    for (c, s) in conts.iter_mut().zip(sets) { c.set = s; }
    eprintln!("{} spooky cont: done {:.0}s, mean bound {:.1} orangeable", l.id, t.elapsed().as_secs_f64(), conts.iter().map(|c| pc_and(&c.set, &l.orangeable) as f64).sum::<f64>() / conts.len().max(1) as f64);
    conts
}

/// the snapshots of continuation entries (same order), with their stored bounds
pub fn exact_of(l: &Lvl, conts: &[Cont]) -> Exact {
    let angles = Angles::new();
    // group by shot (shared all-blue path)
    let mut by: BTreeMap<(u32, u8), Vec<usize>> = BTreeMap::new();
    for (k, c) in conts.iter().enumerate() { by.entry((c.ai, c.d)).or_default().push(k); }
    let groups: Vec<((u32, u8), Vec<usize>)> = by.into_iter().collect();
    let mut snaps: Vec<(usize, Snap)> = groups.par_iter().flat_map_iter(|((ai, d), ks)| {
        let mut base = l.tmpl.clone(); base.advance_to(l.click(*d) + FIRE_LAG);
        let objs: Vec<(usize, usize)> = ks.iter().map(|&k| (l.obj[conts[k].p as usize], if conts[k].q == 255 { usize::MAX } else { l.obj[conts[k].q as usize] })).collect();
        let v = prepare_many(l, &base, angles.list[*ai as usize].0, &objs);
        ks.iter().copied().zip(v.into_iter().map(|s| s.expect("spooky entry without a respawn"))).collect::<Vec<_>>().into_iter()
    }).collect();
    snaps.sort_by_key(|x| x.0);
    let entries: Vec<Entry> = snaps.into_par_iter().map(|x| Entry::new(x.1)).collect();
    Exact::new(l, entries, conts.iter().map(|c| c.set).collect())
}
