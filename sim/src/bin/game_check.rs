//! Check captured real-game runs against the sim (see `peggle_sim::check`).
//! usage: game_check RUN_DIR...   env: GC_REAL_FIRE=1 replays the real fire ticks from harness.txt instead of
//! the script's (physics checked apart from the timing model); GC_VERBOSE=1 prints every shot and ready point.
//! Prints one line per run (plus its differences) and a summary to stdout.
use peggle_sim::check::{self, RunFiles};
use peggle_sim::level;
use std::fs;
use std::path::Path;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    let dir = root.join("game/levels");
    let base = level::parse(&fs::read(dir.join("_base.dat")).unwrap()).unwrap();
    let level_file = |n: &str| fs::read(dir.join(format!("{n}.dat"))).ok();
    let verbose = std::env::var_os("GC_VERBOSE").is_some();
    let real_fire = std::env::var_os("GC_REAL_FIRE").is_some();
    let (mut tot_runs, mut ok_runs, mut tot_shots, mut ok_shots) = (0, 0, 0, 0);
    let mut slacks = vec![];
    for run in std::env::args().skip(1) {
        let run = Path::new(&run);
        let Some(files) = RunFiles::read(run) else { continue };
        if verbose { eprintln!("{}", run.display()); }
        let Some(rep) = check::check(&files, &base, &level_file, real_fire, verbose) else { continue };
        tot_runs += 1;
        tot_shots += rep.shots;
        ok_shots += rep.shots_ok;
        slacks.extend(rep.slack.iter().copied());
        if rep.ok() { ok_runs += 1; }
        let end = match rep.real_end { Some((k, w)) => format!(", real {} at shot {k}", if w { "won" } else { "lost" }), None => String::new() };
        println!("{} {} seed {} power {}: {}/{} shots ok ({} fired before the model's earliest{end}){}", run.display(), rep.level, rep.seed, rep.power,
                 rep.shots_ok, rep.shots, rep.early, if rep.ok() { String::new() } else { format!("\n  {}", rep.errors.join("\n  ")) });
    }
    slacks.sort();
    let med = slacks.get(slacks.len() / 2).copied().unwrap_or(0);
    println!("== {ok_runs}/{tot_runs} runs fully consistent, {ok_shots}/{tot_shots} shots; model earliest fire − real (ready+3): min {} median {med} (must be >= 0)\n",
             slacks.first().copied().unwrap_or(0));
}
