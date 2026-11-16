//! Golden test: every run in `tests/golden/` is a real-game capture (packed from the final
//! verify runs of the four TAS sets plus a non-fast-forward closed-loop set). The sim must reproduce each one
//! exactly (`peggle_sim::check`): every peg, ball count, total and power counter at every ready point, the
//! model's earliest click never before the real one, and the real final total of every won level.
//!
//! Run with `cargo test --release --test golden` (debug builds are slow). Needs the game's level files in
//! `game/levels` (not in the repository; `./bootstrap.sh` extracts them from your Steam copy).
//! GOLDEN_FILTER=substr limits the runs. Shots are replayed at the real fire ticks (the plans' own ticks are no
//! check: an old plan may click before the game is ready, and the real fire then comes later).
use flate2::read::GzDecoder;
use peggle_sim::check::{self, RunFiles};
use peggle_sim::level;
use std::io::Read;
use std::path::{Path, PathBuf};

fn unpack(p: &Path) -> RunFiles {
    let mut b = vec![];
    GzDecoder::new(std::fs::File::open(p).unwrap()).read_to_end(&mut b).unwrap();
    let mut f = RunFiles::default();
    let mut i = 0;
    let u = |b: &[u8], i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap()) as usize;
    while i < b.len() {
        let n = u(&b, i); let name = String::from_utf8_lossy(&b[i + 4..i + 4 + n]).to_string(); i += 4 + n;
        let m = u(&b, i); let data = b[i + 4..i + 4 + m].to_vec(); i += 4 + m;
        match name.as_str() {
            "script.txt" => f.script = String::from_utf8(data).unwrap(),
            "peggle_oracle_info.txt" => f.info = String::from_utf8_lossy(&data).to_string(),
            "harness.txt" => f.harness = String::from_utf8_lossy(&data).to_string(),
            "oracle.bin" => f.oracle = data,
            _ => {}
        }
    }
    f
}

#[test]
fn golden_runs_match_the_real_game() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let levels = root.parent().unwrap().join("game/levels");
    let base = std::fs::read(levels.join("_base.dat")).unwrap_or_else(|_| panic!(
        "game level files missing in {}: run ./bootstrap.sh", levels.display()));
    let base = level::parse(&base).unwrap();
    let filter = std::env::var("GOLDEN_FILTER").unwrap_or_default();
    let mut runs: Vec<PathBuf> = std::fs::read_dir(root.join("tests/golden")).unwrap()
        .map(|e| e.unwrap().path()).filter(|p| p.to_string_lossy().ends_with(".run.gz") && p.to_string_lossy().contains(&filter)).collect();
    runs.sort();
    if filter.is_empty() { assert!(runs.len() >= 274, "golden set incomplete: {} runs", runs.len()); }
    let level_file = |n: &str| std::fs::read(levels.join(format!("{n}.dat"))).ok();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let chunk = runs.len().div_ceil(threads).max(1);
    let (base, level_file) = (&base, &level_file);
    let fails: Vec<String> = std::thread::scope(|s| {
        let hs: Vec<_> = runs.chunks(chunk).map(|c| s.spawn(move || {
            let mut fails = vec![];
            for p in c {
                let name = p.file_name().unwrap().to_string_lossy().trim_end_matches(".run.gz").to_string();
                let f = unpack(p);
                match check::check(&f, base, level_file, true, false) {
                    None => fails.push(format!("{name}: could not load")),
                    Some(r) if !r.ok() => fails.push(format!("{name}: {}", r.errors.join("; "))),
                    Some(r) if r.shots == 0 => fails.push(format!("{name}: no shots")),
                    _ => {}
                }
            }
            fails
        })).collect();
        hs.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });
    for f in &fails { eprintln!("FAIL {f}"); }
    assert!(fails.is_empty(), "{} of {} golden runs differ from the real game", fails.len(), runs.len());
    eprintln!("{} golden runs match the real game", runs.len());
}
