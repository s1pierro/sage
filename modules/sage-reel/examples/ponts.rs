//! Les ponts d'un lieu réel autour d'un point, et leurs tabliers : de quoi voir de près
//! comment ils se suivent, se longent et montent.
//!
//!   cargo run --release --example ponts -- <lieu.reel> <lat> <lon> [rayon en m]

use sage_reel::tabliers::tabliers;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let l = sage_reel::Lieu::lire(std::path::Path::new(&a[1])).expect("lieu");
    let (lat, lon): (f64, f64) = (a[2].parse().unwrap(), a[3].parse().unwrap());
    let r: f32 = a.get(4).and_then(|v| v.parse().ok()).unwrap_or(300.0);
    let (cx, cz) = l.vers_jeu(lat, lon);
    let (cx, cz) = (cx as f32, cz as f32);
    let pres = |p: &[f32; 3]| (p[0] - cx).hypot(p[1] - cz) < r;
    let mut proches = Vec::new();
    for (k, p) in l.ponts.iter().enumerate() {
        if !p.points.iter().any(pres) {
            continue;
        }
        proches.push(p.clone());
        println!("pont {k} : {} {:.1} m, {} points", sage_reel::NOMS_GENRES[p.genre as usize], p.largeur, p.points.len());
        for q in &p.points {
            println!("    {:8.1} {:8.1}  h {:6.2}", q[0] - cx, q[1] - cz, q[2]);
        }
    }
    println!("sol au centre {:.2}, eau à {:.2}", l.relief(cx as f64, cz as f64), l.niveau);
    let tous = tabliers(&proches);
    for (i, t) in tous.iter().enumerate() {
        let c = &t.coupes;
        let larg = |i: usize| (c[i].0[0] - c[i].1[0]).hypot(c[i].0[1] - c[i].1[1]);
        println!("tablier : {} coupes, de ({:.1}, {:.1}) à ({:.1}, {:.1}), largeur {:.1} → {:.1} → {:.1}", c.len(), c[0].0[0] - cx, c[0].0[1] - cz, c[c.len() - 1].0[0] - cx, c[c.len() - 1].0[1] - cz, larg(0), larg(c.len() / 2), larg(c.len() - 1));
        let h: Vec<String> = c.iter().step_by((c.len() / 16).max(1)).map(|k| format!("{:.1}", k.0[2])).collect();
        println!("    hauteurs {}", h.join(" "));
        // Ses bouts sur un autre tablier : la hauteur de l'autre là
        for (k, b) in [(0, c[0]), (c.len() - 1, c[c.len() - 1])] {
            let m = [(b.0[0] + b.1[0]) / 2.0, (b.0[1] + b.1[1]) / 2.0];
            for (j, u) in tous.iter().enumerate().filter(|(j, _)| *j != i) {
                if let Some((h, d)) = u.hauteur_en(m, sage_reel::tabliers::RACCORD) {
                    println!("    bout {k} ({:.2} m) sur le tablier {j} : {h:.2} m, à {d:.1} m", b.0[2]);
                }
            }
        }
    }
}
