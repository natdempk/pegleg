//! Adventure level table and game setup.
use peggle_sim::{game::Game, level, oracle};
use std::path::PathBuf;

/// (stage, level) → level file name, adventure order.
pub const LEVELS: [[&str; 5]; 11] = [
    ["Level1", "Level2", "Level3", "Level4", "Level5"],
    ["Plinko", "Ram", "ThreeValleys", "Spiral", "Owl"],
    ["TotemPole", "Infinity", "Eye", "FourDoors", "Fever"],
    ["TheAmoeban", "3D", "Bug", "Spaceballs", "SunFlower"],
    ["ThreeHills", "Toggle", "Sine", "InfiniteJest", "SideAction"],
    ["Waves", "SpiderWeb", "Blockers", "Baseball", "Vermin"],
    ["Windmills", "Tulips", "Tubing", "Car", "Sunny"],
    ["Monkey", "Pinball", "Cyclops", "Dice", "Cards"],
    ["Paperclips", "Gateway", "Blocks", "HollywoodCircles", "Bunches"],
    ["SpinCycle", "Vortex", "Holes", "YinYang", "DoubleHelix"],
    ["Whirlpool", "Crisscross", "Blindfold", "Aim", "DNA"],
];

/// Stage-10 ("random" character) powers seen under the playback DLL with seed 5.
/// For other seeds pass the power explicitly (it is Rand() % n keyed by tas_mix(seed ^ 0x5B04,
/// stage*5+level), rerolled against the previous pick — not modelled).
pub fn stage10_power_seed5(idx: usize) -> u8 { [3, 3, 3, 3, 4][idx] }

pub fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("game/levels")
}

pub fn parse_level_id(s: &str) -> Option<(usize, usize)> {
    // "S-L" (1-based, as on speedrun.com: 1-1 = Level1) or "S_L" (0-based stage/level, as the harness)
    if let Some((a, b)) = s.split_once('-') {
        let (a, b): (usize, usize) = (a.parse().ok()?, b.parse().ok()?);
        if (1..=11).contains(&a) && (1..=5).contains(&b) { return Some((a - 1, b - 1)); }
    }
    if let Some((a, b)) = s.split_once('_') {
        let (a, b): (usize, usize) = (a.parse().ok()?, b.parse().ok()?);
        if a < 11 && b < 5 { return Some((a, b)); }
    }
    for (st, row) in LEVELS.iter().enumerate() {
        for (i, n) in row.iter().enumerate() { if n.eq_ignore_ascii_case(s) { return Some((st, i)); } }
    }
    None
}

pub fn power_for(stage: usize, idx: usize, seed: u32, over: Option<u8>) -> u8 {
    if let Some(p) = over { return p; }
    if stage < 10 { oracle::CHAR_POWERS[stage] }
    else if seed == 5 { stage10_power_seed5(idx) }
    else { panic!("stage 10 power for seed {seed} unknown: pass --power") }
}

pub fn new_game(stage: usize, idx: usize, seed: u32, power: u8) -> Game {
    let dir = data_dir();
    let base = level::parse(&std::fs::read(dir.join("_base.dat")).unwrap()).unwrap();
    let name = LEVELS[stage][idx].to_lowercase();
    let lvl = level::parse(&std::fs::read(dir.join(format!("{name}.dat"))).unwrap()).unwrap();
    let mut g = Game::new(&lvl, &base, seed, power);
    g.set_stage_level(stage, idx);
    g
}
