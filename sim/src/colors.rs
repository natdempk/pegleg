//! Peg colour assignment:
//! - FUN_00457960 level start: every PegInfo object is reset; non-variable pegs keep their file type
//!   (orange → orange set LogicMgr+0x358, others → set +0x364); variable pegs become blue and go into a
//!   vector (board-list order) that is shuffled by FUN_0043d430 (1000 × swap(v[Rand()%n], v[Rand()%n]));
//!   oranges are popped from the back until the orange set has NumOrange (+0x130, 0 → 25); the rest join
//!   the +0x364 set as blue; non-blue pegs of that set are reset to blue (profile check FUN_00405c00 is
//!   false in adventure).
//! - FUN_00458110 purple (progress >= 3): candidates = un-hit blue pegs of +0x364; the old purple
//!   (+0x170) turns blue unless it was hit (PegInfo+0x14) or is orange; new = cand[Rand() % n].
//! - FUN_00457dc0 greens (progress >= 2): need = 2 − existing greens; candidates = blue pegs with
//!   GetY < 370 (else: variable blue pegs), shuffled (FUN_0043d430); pass 0 takes candidates at squared
//!   distance >= 150² (ModVal LogicMgr.cpp1057) from the last green placed (start (-1000,-1000)), pass 1
//!   takes any.
//! The +0x364 set is ordered by heap pointer in the game; the TAS hook sorts the green and purple
//! candidates by board-list index, which is what we use here. Hit pegs are erased from the sets on
//! their first hit (BallHitPeg → FUN_00448e60), so "in the set" = PegInfo object not yet hit.
//!
//! RNG: the hook (PEGGLE_RNG_SEED=S) reseeds MT with tas_mix(S, n) before each of these calls (and
//! before FUN_004582f0), n = call index since the `level` command (rng.rs).
use crate::phys::Board;
use crate::rng::{tas_mix, Mt};
use crate::score::{BLUE, GREEN, ORANGE, PURPLE};

/// Indices of PegInfo objects in board-list order (base objects first, then level objects in
/// top-level order).
pub fn peg_order(b: &Board) -> Vec<usize> {
    let mut v: Vec<usize> = (0..b.objs.len()).filter(|&i| b.objs[i].has_peginfo && b.objs[i].top_ix >= 0).collect();
    v.sort_by_key(|&i| (b.objs[i].uid >> 16 == 0, b.objs[i].top_ix));
    v
}

/// The RNG state the hook installs before colour call number `n`.
pub fn hook_rng(seed: u32, n: u32) -> Mt { Mt::new(tas_mix(seed, n)) }

/// FUN_0043d430
pub fn shuffle(v: &mut [usize], rng: &mut Mt) {
    if v.len() < 2 { return; }
    let n = v.len() as i32;
    for _ in 0..1000 {
        let a = rng.rand() % n;
        let c = rng.rand() % n;
        v.swap(a as usize, c as usize);
    }
}

/// FUN_00457960. `num_orange` = LogicMgr+0x130 (0 → 25). Returns the orange count (+0x360).
pub fn assign_oranges(b: &mut Board, rng: &mut Mt, num_orange: i32) -> i32 {
    let order = peg_order(b);
    let mut oranges = 0;
    let mut vars = vec![];
    for &i in &order {
        let o = &mut b.objs[i];
        if o.variable { o.peg_type = BLUE; vars.push(i); } else if o.peg_type == ORANGE { oranges += 1; }
    }
    shuffle(&mut vars, rng);
    let want = if num_orange == 0 { 25 } else { num_orange };
    while oranges < want {
        let Some(i) = vars.pop() else { break };
        b.objs[i].peg_type = ORANGE;
        oranges += 1;
    }
    // +0x364 pegs that are not blue: reset to blue unless FUN_00405c00 (profile check; false in all
    // adventure captures — Level3/Level4 have fixed green/purple pegs in the file that come out blue)
    for &i in &order { let o = &mut b.objs[i]; if o.peg_type != ORANGE && o.peg_type != BLUE { o.peg_type = BLUE; } }
    oranges
}

fn in_set(b: &Board, i: usize) -> bool { let o = &b.objs[i]; !o.hit && o.collidable && o.peg_type != ORANGE }

/// FUN_00458110. `purple` = LogicMgr+0x170 (object index), updated.
pub fn purple_reroll(b: &mut Board, rng: &mut Mt, purple: &mut Option<usize>) {
    let order = peg_order(b);
    let mut cands = vec![];
    for &i in &order {
        if !in_set(b, i) { continue; }
        let t = b.objs[i].peg_type;
        if t == PURPLE && Some(i) != *purple { *purple = Some(i); return; }
        if t == BLUE { cands.push(i); }
    }
    if cands.is_empty() { return; }
    if let Some(p) = purple.take() {
        if b.objs[p].peg_type != ORANGE && !b.objs[p].hit { b.objs[p].peg_type = BLUE; }
    }
    let k = rng.rand() % cands.len() as i32;
    let i = cands[k as usize];
    b.objs[i].peg_type = PURPLE;
    *purple = Some(i);
}

/// FUN_00457dc0 (adventure: 2 greens).
pub fn assign_greens(b: &mut Board, rng: &mut Mt) {
    let order = peg_order(b);
    let mut need = 2i32;
    let mut last = [-1000.0f32, -1000.0f32];
    let mut cands = vec![];
    for &i in &order {
        if !in_set(b, i) { continue; }
        let t = b.objs[i].peg_type;
        let p = b.obj_pos(b.objs[i].uid).unwrap_or([0.0; 2]);
        if t == GREEN { last = p; need -= 1; } else if t == BLUE && p[1] < 370.0 { cands.push(i); }
    }
    if need <= 0 { return; }
    if cands.is_empty() {
        cands = order.iter().copied().filter(|&i| in_set(b, i) && b.objs[i].peg_type == BLUE && b.objs[i].variable).collect();
    }
    if cands.is_empty() { return; }
    shuffle(&mut cands, rng);
    let d2 = (150 * 150) as f32;
    for pass in 0..2 {
        for &i in &cands {
            if need <= 0 { return; }
            if b.objs[i].peg_type == GREEN { continue; }
            if pass == 0 {
                let p = b.obj_pos(b.objs[i].uid).unwrap_or([0.0; 2]);
                let dx = last[0] - p[0];
                let dy = last[1] - p[1];
                let s = dx * dx + dy * dy;
                if d2 > s { continue; }
                last = p;
            }
            b.objs[i].peg_type = GREEN;
            need -= 1;
        }
    }
}
