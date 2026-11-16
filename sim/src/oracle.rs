//! Shared parsing of oracle state records (hook.c `dump_board` etc.).
use crate::phys::Board;

#[derive(Clone, Copy, Debug)]
pub struct BrdEntry { pub ix: usize, pub amp: i32, pub period: u32, pub coll: Option<bool>, pub has_mover: bool }

fn u(b: &[u8], o: usize) -> u32 { u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) }

/// 'BRD ' body: up to 33 × (word0, mover+0x18, mover+0x1c). v2 (word0 bit 27): word0 = board index |
/// collidable << 24 | visible << 25 | has mover << 26. v1: index only, entries with period 0 are empty.
pub fn parse_brd(b: &[u8]) -> Vec<BrdEntry> {
    let mut v = vec![];
    for e in 0..33 {
        let (w, amp, per) = (u(b, 12 * e), u(b, 12 * e + 4) as i32, u(b, 12 * e + 8));
        if w & (1 << 27) != 0 {
            v.push(BrdEntry { ix: (w & 0xffffff) as usize, amp, period: per, coll: Some(w & (1 << 24) != 0), has_mover: w & (1 << 26) != 0 });
        } else if per != 0 {
            v.push(BrdEntry { ix: w as usize, amp, period: per, coll: None, has_mover: true });
        }
    }
    v
}

/// Apply BRD state: board-list index → sim object (base-level objects come first in file order,
/// uid = (1 << 16) | file index).
pub fn apply_brd(board: &mut Board, entries: &[BrdEntry]) {
    for e in entries {
        let Some(oi) = board.objs.iter().position(|o| o.uid == (1 << 16) | e.ix as u32) else { continue };
        if e.has_mover { if let Some(mi) = board.objs[oi].mover { board.movers[mi].def.s18 = e.amp; board.movers[mi].version += 1; } }
        if let Some(c) = e.coll { board.objs[oi].collidable = c; }
    }
}

/// PEG record entry → sim object: by board-list index when the hook wrote one (type word bits 16..,
/// index + 1; base objects come first), else by position (old captures; fragile for moving pegs).
pub fn find_peg(board: &Board, idx1: u32, p: [f32; 2]) -> Option<usize> {
    use crate::phys::Geom;
    if idx1 != 0 {
        let ix = idx1 as usize - 1;
        // the board list holds the base objects first, minus one: level object j is at j + n_base - 1
        // (checked on Level1/Tubing PEG records: file pegs 0.. at board index 26..)
        // level objects are in top-level list order (the file's object array also holds inline children)
        let nb = board.n_base.saturating_sub(1);
        if ix < nb { return board.objs.iter().position(|o| o.uid == (1 << 16) | ix as u32 && o.peg_type != 0); }
        return board.objs.iter().position(|o| o.uid >> 16 == 0 && o.top_ix == (ix - nb) as i32 && o.peg_type != 0);
    }
    board.objs.iter().position(|o| o.peg_type != 0 && {
        let q = match &o.geom { Geom::Poly { center, .. } => *center, Geom::Peg { pos, .. } => *pos, _ => [f32::NAN; 2] };
        (q[0] - p[0]).abs() < 0.01 && (q[1] - p[1]).abs() < 0.01 })
}

/// Character powers in character-index order (Board+0x138 table, +0x6c): Bjorn SuperGuide, Jimmy
/// Multiball, Kat Pyramid, Splork SpaceBlast, Claude Flippers, Renfield Spooky, Tula FlowerPower,
/// Warren LuckySpin, Cinderbottom Fireball, Master Hu ZenShot.
pub const CHAR_POWERS: [u8; 10] = [1, 3, 4, 5, 2, 6, 9, 13, 12, 7];

/// FUN_00451ac0 (Lucky Spin "Random Powerup") list: powers of characters 0..=LogicMgr+0x15c except 13
/// (Lucky Spin); SuperGuide (1) only if that is empty (at 0x451b4f the push is skipped for a non-empty
/// list). The pick is `lucky_pick`.
pub fn lucky_list(lgc: &[u8]) -> Vec<u8> {
    if lgc.len() < 0x4b0 { return vec![]; }
    let n = (u(lgc, 0x15c) as i32 + 1).clamp(0, 10) as usize;
    let mut list: Vec<u8> = CHAR_POWERS[..n].iter().copied().filter(|&p| p != 13).collect();
    if list.is_empty() { list.push(1); }
    list
}
/// FUN_00451ac0 index: LogicMgr+0x30 when +0x50 == 1 else +0x2c, modulo the list length. +0x50 counts the
/// shot's green hits (incremented before each spin, FUN_0046fxxx type-4 branch), so spin 1 uses +0x30 and
/// later spins +0x2c. +0x2c/+0x30 are two Rand() rolled at fire (FUN_0046ac70; keyed site 0x5B03 in TAS mode).
pub fn lucky_pick(list: &[u8], ctr: [u32; 2], spin_no: usize) -> Option<u8> {
    if list.is_empty() { return None; }
    let c = if spin_no == 1 { ctr[1] } else { ctr[0] } as i32;
    Some(list[c.rem_euclid(list.len() as i32) as usize])
}

/// Ready-point state dumped by the hook ('RDY ' then LGC ×3, PEG ×n, BRD).
#[derive(Clone, Debug, Default)]
pub struct Ready {
    pub tick: i32, pub shots: i32, pub level_over: bool, pub bucket: i32, pub level_ctr: i32,
    /// LogicMgr snapshot (0x4b0 bytes, empty if missing)
    pub lgc: Vec<u8>,
    /// (pos, type, hit, collidable, board index + 1)
    pub pegs: Vec<([f32; 2], u8, bool, bool, u32)>,
    pub btick: i32,
    pub brd: Vec<BrdEntry>,
}

impl Ready {
    pub fn li(&self, o: usize) -> i32 { if self.lgc.len() == 0x4b0 { u(&self.lgc, o) as i32 } else { 0 } }
}

/// All ready points of a capture plus the level name ('LVL ' record).
pub fn read_readies(data: &[u8]) -> (Option<String>, Vec<Ready>) {
    const REC: usize = 24 + 0x190;
    let f = |b: &[u8], o: usize| f32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let (mut lvl, mut out, mut cur, mut lgc): (Option<String>, Vec<Ready>, Option<Ready>, Vec<u8>) = (None, vec![], None, vec![]);
    let mut pos = 0;
    while pos + REC <= data.len() {
        let r = &data[pos..pos + REC];
        pos += REC;
        let (m, extra, b) = (u(r, 0), u(r, 12), &r[24..]);
        match &m.to_le_bytes() {
            b"LVL " => {
                let end = b[8..].iter().position(|&c| c == 0).unwrap_or(0);
                if lvl.is_none() { lvl = Some(String::from_utf8_lossy(&b[8..8 + end]).to_string()); }
            }
            b"RDY " => {
                cur = Some(Ready { tick: u(b, 0) as i32, shots: u(b, 4) as i32, level_over: u(b, 8) != 0, bucket: u(b, 12) as i32,
                                   level_ctr: u(b, 20) as i32, ..Default::default() });
                lgc.clear();
            }
            b"LGC " if cur.is_some() => {
                if extra == 0 { lgc.clear(); }
                lgc.extend_from_slice(b);
                if extra == 0x320 && lgc.len() == 0x4b0 { cur.as_mut().unwrap().lgc = std::mem::take(&mut lgc); }
            }
            b"PEG " if cur.is_some() => {
                let p = cur.as_mut().unwrap();
                for e in 0..25 {
                    let o = 16 * e;
                    let t = u(b, o + 8);
                    if t == 0 && f(b, o) == 0.0 { break; }
                    p.pegs.push(([f(b, o), f(b, o + 4)], t as u8, b[o + 12] != 0, b[o + 14] != 0, t >> 16));
                }
            }
            b"BRD " if cur.is_some() => {
                let mut p = cur.take().unwrap();
                p.btick = extra as i32;
                p.brd = parse_brd(b);
                out.push(p);
            }
            _ => {}
        }
    }
    (lvl, out)
}
