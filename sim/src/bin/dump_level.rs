//! Parse level .dat files. `dump_level FILE...` prints a summary; `--json FILE` prints full JSON.
use peggle_sim::level::{self, Shape};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.iter().any(|a| a == "--json");
    let mut fails = 0;
    for path in args.iter().filter(|a| !a.starts_with("--")) {
        let data = std::fs::read(path).expect("read");
        match level::parse(&data) {
            Ok(l) => {
                if json {
                    println!("{}", serde_json::to_string_pretty(&l).unwrap());
                    continue;
                }
                let mut counts = std::collections::BTreeMap::new();
                for o in &l.objects {
                    let k = match &o.shape {
                        Shape::Generic => "generic",
                        Shape::Line { .. } => "line",
                        Shape::Poly(_) => "poly",
                        Shape::Circle(c) if c.peg => "circle",
                        Shape::Circle(_) => "ball",
                        Shape::Brick { .. } => "brick",
                        Shape::Gun { .. } => "gun",
                        Shape::Hole(_) => "hole",
                    };
                    *counts.entry(k).or_insert(0) += 1;
                }
                let pegs = l.objects.iter().filter(|o| o.phys.peg.is_some()).count();
                println!("OK  {path}: v={:#x} objs={} movers={} pegs={} {:?}", l.version, l.objects.len(), l.movers.len(), pegs, counts);
            }
            Err(e) => {
                fails += 1;
                println!("ERR {path}: {e}");
            }
        }
    }
    if fails > 0 {
        std::process::exit(1);
    }
}
