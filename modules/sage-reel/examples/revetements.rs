//! Les voies d'un lieu par revêtement : nombre et longueur (routes, ponts, tunnels).
//!   cargo run --release --example revetements -- <lieu.reel>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let l = sage_reel::Lieu::depuis_octets(&std::fs::read(&a[1]).unwrap()).expect("lieu illisible");
    for (quoi, v) in [("routes", &l.routes), ("ponts", &l.ponts), ("tunnels", &l.transports.tunnels)] {
        let mut n = [(0usize, 0f32); sage_reel::REVETEMENTS as usize];
        for r in v.iter() {
            let e = &mut n[r.revetement as usize];
            e.0 += 1;
            e.1 += r.points.windows(2).map(|s| (s[1][0] - s[0][0]).hypot(s[1][1] - s[0][1])).sum::<f32>();
        }
        let t: Vec<String> = sage_reel::NOMS_REVETEMENTS.iter().zip(n).map(|(nom, (c, m))| format!("{nom} {c} ({:.1} km)", m / 1000.0)).collect();
        println!("{quoi} : {}", t.join(", "));
    }
    // Les cases de 200 m les plus pavées
    let mut cases: std::collections::HashMap<(i32, i32), f32> = Default::default();
    for r in l.routes.iter().filter(|r| r.revetement == sage_reel::PAVES) {
        for s in r.points.windows(2) {
            *cases.entry(((s[0][0] / 200.0).floor() as i32, (s[0][1] / 200.0).floor() as i32)).or_default() += (s[1][0] - s[0][0]).hypot(s[1][1] - s[0][1]);
        }
    }
    let mut v: Vec<_> = cases.into_iter().collect();
    v.sort_by(|a, b| b.1.total_cmp(&a.1));
    for ((i, j), m) in v.iter().take(8) {
        println!("pavés : case ({} ; {}) {m:.0} m", i * 200 + 100, j * 200 + 100);
    }
}
