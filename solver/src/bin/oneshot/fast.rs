//! Fastest flip wins (class flip): along a flip schedule the pegs are lit in a seed-independent order at
//! seed-independent ticks, so a seed wins by tick T under an entry iff its oranges (after flipper_clear's
//! relocation) are all lit by T. Win tick of a seed = min over its usable entries of the max first-hit tick of
//! its oranges (harness ticks, = verify's win tick).
//!
//! oneshot fliptimes DIR LV                       per-tick prefix coverage and expected wins over 2^32 by win tick
//! oneshot fastscan DIR LV --t T [--chunks A-B] [--stop N]   seeds winning by tick T → DIR/fast/LV.tsv, DIR/cands/fast.tsv
//! oneshot fastcheck DIR LV [--t T] [--n N]       scanner win tick vs Game::shoot (seed, flips) on N seeds
use super::*;
use super::flipcov::{tmpl_game, Reloc};

pub const NOHIT: i32 = i32::MAX;

/// per flip entry: each peg's first-hit harness tick (NOHIT if never), from Game::shoot of the template with
/// the entry's flips (checked: the same set as the entry)
pub fn entry_ticks(l: &Lvl, conts: &[Cont], flips: &[Vec<(i32, bool)>]) -> Vec<Vec<i32>> {
    let angles = Angles::new();
    conts.par_iter().enumerate().map(|(k, c)| {
        let mut g = tmpl_game(l, c.p as usize);
        g.flips = flips[k].clone();
        g.shoot(angles.list[c.ai as usize].0, l.click(c.d));
        let blag = g.tick - g.board.btick;
        let mut t = vec![NOHIT; l.npeg()];
        for (&i, &bt) in g.board.shot_hits.iter().zip(&g.board.shot_hit_bticks) {
            if l.bit[i] < 0 { continue; }
            let q = l.bit[i] as usize; t[q] = t[q].min(bt + blag);
        }
        let mut s = [0u64; W]; for q in 0..l.npeg() { if t[q] != NOHIT { bset(&mut s, q); } }
        assert!(s == c.set, "{}: entry {k}: Game::shoot hits differ from the entry's set", l.id);
        t
    }).collect()
}

fn prefix(t: &[i32], tt: i32) -> Bits { let mut s = [0u64; W]; for (q, &x) in t.iter().enumerate() { if x <= tt { bset(&mut s, q); } } s }

/// expected wins per seed by tick tt (union bound over distinct (p, prefix), as flipcov::flip_estimate)
fn estimate(l: &Lvl, conts: &[Cont], ticks: &[Vec<i32>], tt: i32) -> (f64, u32) {
    let v = l.vars.len() as u32;
    let need = 25u32.saturating_sub(pc(&{ let c = flipcov::corner(l); let mut f = l.fixed; for x in 0..W { f[x] &= !c[x]; } f }));
    let pg = 2.0 / pc(&flipcov::green_cands2(l)).max(1) as f64;
    let (mut p, mut hmax) = (0.0, 0);
    let mut seen = HashSet::new();
    // fixed oranges in flipper_clear's corners are relocated (to random pegs: counted like variable ones)
    let cor = flipcov::corner(l);
    let mut fx = l.fixed; for x in 0..W { fx[x] &= !cor[x]; }
    let nfx = pc(&fx);
    for (k, c) in conts.iter().enumerate() {
        let s = prefix(&ticks[k], tt);
        if !seen.insert((c.p, s)) { continue; }
        let fixed_in = pc_and(&s, &fx);
        let hv = pc_and(&s, &l.orangeable) - pc_and(&s, &l.fixed);
        hmax = hmax.max(hv + fixed_in);
        if fixed_in < nfx { continue; }
        p += pg * (lnc(hv, need) - lnc(v, need)).exp();
    }
    (p, hmax)
}

fn load(dir: &str, l: &Lvl) -> (Vec<Cont>, Vec<Vec<(i32, bool)>>, Vec<Vec<i32>>) {
    let (conts, _) = load_cont(&cont_path(dir, &l.id, Class::Flip));
    let flips = flipcov::load_flips(dir, &l.id);
    assert_eq!(conts.len(), flips.len());
    let ticks = entry_ticks(l, &conts, &flips);
    (conts, flips, ticks)
}

pub fn cmd_times(a: &[String]) {
    let dir = a[2].clone();
    let l = Lvl::new(&a[3]);
    let (conts, _, ticks) = load(&dir, &l);
    let end = ticks.iter().map(|t| t.iter().filter(|&&x| x != NOHIT).max().copied().unwrap_or(0)).max().unwrap_or(0);
    let step: i32 = arg(a, "--step", 250);
    println!("{}: {} entries, var {} fixed {}, last hit tick {end}", l.id, conts.len(), l.vars.len(), l.nfixed);
    let mut marks = [(1.0, None), (10.0, None), (100.0, None), (1e4, None)];
    let mut tt = step;
    while tt <= end + step {
        let (p, h) = estimate(&l, &conts, &ticks, tt);
        let e = p * 4294967296.0;
        for m in marks.iter_mut() { if m.1.is_none() && e >= m.0 { m.1 = Some(tt); } }
        if e > 1e-3 && e < 1e7 { println!("  T {tt}: hmax {h}, expected wins/2^32 {e:.3e}"); }
        tt += step;
    }
    if l.nfixed > 0 {
        let cor = flipcov::corner(&l);
        let mut fx: Vec<(i32, usize, i32)> = ticks.iter().enumerate().map(|(k, t)| {
            let f = (0..l.npeg()).filter(|&q| bget(&l.fixed, q) && !bget(&cor, q)).map(|q| t[q]).max().unwrap_or(0);
            let h = (0..l.npeg()).filter(|&q| bget(&l.orangeable, q) && t[q] <= f).count() as i32;
            (f, k, h) }).collect();
        fx.sort();
        println!("{}: fixed oranges (outside the corners) all hit by (tick, entry, orange-able hit by then): {:?}", l.id, &fx[..fx.len().min(8)]);
    }
    println!("{}: first T with expected wins/2^32 >= 1 / 10 / 100 / 1e4: {:?}", l.id, marks.iter().map(|m| m.1).collect::<Vec<_>>());
}

pub struct Fast<'a> { l: &'a Lvl, conts: Vec<Cont>, ticks: Vec<Vec<i32>>, rel: Reloc, t: i32, pre: Vec<Bits>, hid: Vec<Bits> }

impl<'a> Fast<'a> {
    pub fn new(dir: &str, l: &'a Lvl, t: i32) -> Fast<'a> {
        let (conts, _, ticks) = load(dir, l);
        let rel = flipcov::reloc(l, &conts);
        let pre = ticks.iter().map(|x| prefix(x, t)).collect();
        let hid = (0..conts.len()).map(|k| rel.tm[rel.ent[k] as usize].0).collect();
        Fast { l, conts, ticks, rel, t, pre, hid }
    }
    /// the seed's earliest win tick (<= t) and its entry
    #[inline]
    pub fn eval(&self, seed: u32, v: &mut Vec<u8>) -> Option<(i32, u32)> {
        let o = self.l.oranges(seed, v);
        let mut surv = [0u16; 64];
        let mut ns = 0;
        for k in 0..self.conts.len() {
            let (p, h) = (&self.pre[k], &self.hid[k]);
            if (0..W).all(|x| o[x] & !h[x] & !p[x] == 0) { if ns < 64 { surv[ns] = k as u16; ns += 1; } }
        }
        if ns == 0 { return None; }
        let gr = self.l.greens(seed, &o, v);
        let mut best: Option<(i32, u32)> = None;
        for &k in &surv[..ns] {
            let k = k as usize;
            let c = &self.conts[k];
            if !bget(&gr, c.p as usize) || pc_and(&c.pre, &gr) > 0 { continue; }
            let tk = &self.ticks[k];
            let h = &self.hid[k];
            let mut t = 0;
            for x in 0..W { let mut w = o[x] & !h[x]; while w != 0 { t = t.max(tk[x * 64 + w.trailing_zeros() as usize]); w &= w - 1; } }
            let (_, cb, rn) = &self.rel.tm[self.rel.ent[k] as usize];
            let m = pc_and(h, &o) as usize;
            if m > 0 {
                v.clear();
                for x in 0..W { let mut w = cb[x] & !o[x]; while w != 0 { v.push((x * 64 + w.trailing_zeros() as usize) as u8); w &= w - 1; } }
                shuffle(seed, *rn, v);
                for &q in v.iter().take(m) { t = t.max(tk[q as usize]); }
            }
            if t <= self.t && best.map_or(true, |b| t < b.0) { best = Some((t, k as u32)); }
        }
        best
    }
}

pub fn cmd_scan(a: &[String]) {
    let dir = a[2].clone();
    let l = Lvl::new(&a[3]);
    let t: i32 = arg(a, "--t", 0);
    assert!(t > 0, "--t T");
    let stop: usize = arg(a, "--stop", usize::MAX);
    let (c0, c1): (u32, u32) = { let s: String = arg(a, "--chunks", "0-255".to_string()); let (x, y) = s.split_once('-').unwrap(); (x.parse().unwrap(), y.parse().unwrap()) };
    let lib = load_lib(&lib_path(&dir, &l.id));
    let n0 = lib.shots.len() as u32;
    let hash = class_hash(&dir, &l.id, Class::Flip).unwrap();
    let f = Fast::new(&dir, &l, t);
    let donef = format!("{dir}/fast/{}.{hash}.T{t}.done", l.id);
    let done: HashSet<u32> = std::fs::read_to_string(&donef).unwrap_or_default().lines().filter_map(|x| x.split('\t').next()?.parse().ok()).collect();
    let resf = format!("{dir}/fast/{}.tsv", l.id);
    let mut found = std::fs::read_to_string(&resf).unwrap_or_default().lines().filter(|x| { let v: Vec<&str> = x.split('\t').collect(); v.len() > 3 && v[0] == hash && v[3].parse::<i32>().map_or(false, |y| y <= t) }).count();
    eprintln!("{} fastscan T {t}: {} entries, hash {hash}, {found} found before", l.id, f.conts.len());
    for c in c0..=c1 {
        if found >= stop { break; }
        if done.contains(&c) { continue; }
        let (from, to) = ((c as u64) << CHUNK_BITS, ((c as u64) + 1) << CHUNK_BITS);
        let tm = Instant::now();
        let blocks: Vec<u64> = (from..to).step_by(1 << 14).collect();
        let mut res: Vec<(i32, u32, u32)> = blocks.par_iter().flat_map_iter(|&b0| {
            let mut v = Vec::with_capacity(256);
            let mut r = vec![];
            for s in b0..b0 + (1 << 14) { if let Some((tk, k)) = f.eval(s as u32, &mut v) { r.push((tk, s as u32, k)); } }
            r.into_iter()
        }).collect();
        res.sort();
        let dt = tm.elapsed().as_secs_f64();
        let mut lines = String::new();
        let mut cl = String::new();
        for &(tk, s, k) in &res { lines += &format!("{hash}\t{s}\t{k}\t{tk}\n"); cl += &format!("{}\tflip\t{hash}\t{s}\t{}\t25\n", l.id, n0 + k); }
        append(&resf, &lines);
        append(&format!("{dir}/cands/fast.tsv"), &cl);
        append(&donef, &format!("{c}\t{}\t{dt:.1}\n", res.len()));
        found += res.len();
        eprintln!("{} fastscan T {t} chunk {c}: {} seeds (best {:?}), {:.2}M seeds/s; {found} total", l.id, res.len(), res.first(), (to - from) as f64 / dt / 1e6);
    }
}

pub fn cmd_check(a: &[String]) {
    let dir = a[2].clone();
    let l = Lvl::new(&a[3]);
    let t: i32 = arg(a, "--t", 1 << 30);
    let n: usize = arg(a, "--n", 200);
    let f = Fast::new(&dir, &l, t);
    let flips = flipcov::load_flips(&dir, &l.id);
    let angles = Angles::new();
    let mut picks = vec![];
    let mut s0 = 0u32;
    while picks.len() < n && s0 < 1 << 28 {
        let v: Vec<(u32, i32, u32)> = (s0..s0 + 100_000).into_par_iter().filter_map(|i| { let seed = tas_mix(i, 995); let mut v = vec![]; f.eval(seed, &mut v).map(|(tk, k)| (seed, tk, k)) }).collect();
        picks.extend(v); s0 += 100_000;
    }
    picks.truncate(n);
    let res: Vec<(u32, i32, i32, bool)> = picks.par_iter().map(|&(seed, tk, k)| {
        let c = &f.conts[k as usize];
        let mut g = levels::new_game(l.st, l.ix, seed, flipcov::power());
        g.earliest_fire = rt::FIRST_CLICK;
        g.flips = flips[k as usize].clone();
        let r = g.shoot(angles.list[c.ai as usize].0, l.click(c.d));
        let blag = g.tick - g.board.btick;
        let tw = g.board.shot_hits.iter().zip(&g.board.shot_hit_bticks).filter(|(i, _)| g.board.objs[**i].peg_type == ORANGE).map(|(_, &x)| x + blag).max().unwrap_or(-1);
        (seed, tk, tw, r.won)
    }).collect();
    let ok = res.iter().filter(|r| r.3 && r.1 == r.2).count();
    let bad: Vec<_> = res.iter().filter(|r| !(r.3 && r.1 == r.2)).take(8).collect();
    println!("{} fastcheck T {t}: {} seeds ({s0} scanned): won with the predicted win tick on {ok}; mismatches {:?}", l.id, res.len(), bad);
}
