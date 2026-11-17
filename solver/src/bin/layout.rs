//! Orange/green layouts over many seeds: `layout LEVEL CHAR FROM TO` prints, per seed, how many oranges and greens
//! are bricks (on 2-4: the spiral, #51 its outer end, #0 its centre) and which, best first: the innermost
//! orange brick as far out as possible (the longest orange-free inner run), then the fewest orange bricks. The seed sets the colours.
use peggle_sim::phys::Geom;
use peggle_sim::score::{GREEN, ORANGE};
use peggle_solver::levels;
use rayon::prelude::*;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (st, ix) = levels::parse_level_id(&a[1]).expect("level");
    let ch: usize = a[2].parse().unwrap();
    let (from, to): (u32, u32) = (a[3].parse().unwrap(), a[4].parse().unwrap());
    let mut v: Vec<(usize, u32, Vec<usize>, Vec<usize>)> = (from..=to).into_par_iter().map(|seed| {
        let g = levels::new_game(st, ix, seed, peggle_sim::oracle::CHAR_POWERS[ch]);
        let brick = |o: &peggle_sim::phys::Obj| !matches!(o.geom, Geom::Peg { .. });
        let ob: Vec<usize> = g.board.objs.iter().enumerate().filter(|(_, o)| o.has_peginfo && o.peg_type == ORANGE && brick(o)).map(|(i, _)| i).collect();
        let gr: Vec<usize> = g.board.objs.iter().enumerate().filter(|(_, o)| o.has_peginfo && o.peg_type == GREEN).map(|(i, _)| i).collect();
        ((99 - ob.iter().min().copied().unwrap_or(99)) * 100 + ob.len(), seed, ob, gr)
    }).collect();
    v.sort();
    for x in v.iter().take(40) { println!("seed {}: innermost orange brick #{}, {} orange bricks {:?}, greens {:?}", x.1, x.2.iter().min().unwrap_or(&99), x.2.len(), x.2, x.3); }
}
