//! Feature census of level files: which sim-relevant features each level uses.
//! usage: census LEVELDIR NAME... (lowercase names without .dat) — prints one JSON line per level.
use peggle_sim::level::{self, Shape};
use std::collections::BTreeMap;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = &args[1];
    for name in &args[2..] {
        let Ok(d) = std::fs::read(format!("{dir}/{name}.dat")) else { println!(r#"{{"level":"{name}","error":"missing"}}"#); continue };
        let lvl = level::parse(&d).unwrap();
        let mut f: BTreeMap<String, usize> = BTreeMap::new();
        let mut inc = |k: &str| *f.entry(k.to_string()).or_default() += 1;
        for r in lvl.top.iter().flatten() {
            let o = &lvl.objects[*r];
            let p = &o.phys;
            if !p.collidable { continue; }
            let kind = match &o.shape {
                Shape::Generic => "generic", Shape::Line { .. } => "line", Shape::Poly(_) => "poly",
                Shape::Circle(_) => "circle", Shape::Brick { .. } => "brick", Shape::Gun { .. } => "gun",
                Shape::Hole(_) => "hole",
            };
            inc(kind);
            if let Some(m) = p.mover {
                let mv = &lvl.movers[m];
                inc(&format!("{kind}+mover{}", mv.mover_type.abs()));
                if mv.sub.is_some() { inc("submover"); }
                if mv.f24 != 0.0 { inc("mover_rot_f24"); }
                if !p.moving { inc("mover_not_moving"); }
            }
            match &o.shape {
                Shape::Poly(pd) | Shape::Brick { poly: pd, .. } => {
                    let mangle = p.mover.map(|m| lvl.movers[m].f44).unwrap_or(pd.rotation);
                    if pd.rotation != 0.0 || mangle != pd.rotation { inc(&format!("{kind}_rotated")); }
                    if pd.scale != 1.0 { inc(&format!("{kind}_scaled")); }
                    if pd.f120 != 0.0 { inc(&format!("{kind}_angvel")); }
                    if pd.growtype != 0 { inc("poly_growtype"); }
                }
                Shape::Hole(h) => { if h.target.is_some() { inc("teleport"); } }
                Shape::Line { f_fc, f_100, .. } => { if *f_fc != 0.0 || *f_100 != 0.0 { inc("line_vel"); } }
                Shape::Circle(c) => { if !c.peg { inc("ball_obj"); } }
                _ => {}
            }
            if p.flipperflags != 0 { inc("flipper"); }
            if !p.logic.is_empty() { inc(&format!("logic:{}", p.logic)); }
            if let Some(pi) = &p.peg { if pi.crumble { inc("crumble"); } }
        }
        let body: Vec<String> = f.iter().map(|(k, v)| format!("\"{k}\":{v}")).collect();
        println!(r#"{{"level":"{name}",{}}}"#, body.join(","));
    }
}
