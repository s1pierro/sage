//! Lit un lieu réel et en dit l'essentiel ; vérifie qu'il se réécrit à l'identique (au
//! format courant, et qu'il se relit pareil).
//!
//!   cargo run --release --example lire -- <lieu.reel>

fn main() {
    let f = std::env::args().nth(1).expect("usage : lire <lieu.reel>");
    let o = std::fs::read(&f).expect("lecture");
    let t = std::time::Instant::now();
    let l = sage_reel::Lieu::depuis_octets(&o).expect("pas un lieu réel lisible");
    println!("{} : {:.0} ms", l.nom, t.elapsed().as_secs_f64() * 1000.0);
    println!("centre {:.5}, {:.5} ; {} × {} points de {} m ; eau à {:.2} m", l.lat, l.lon, l.grille.nx, l.grille.nz, l.pas, l.niveau);
    println!("{} routes, {} ponts, {} bâtiments, sol {}", l.routes.len(), l.ponts.len(), l.batiments.len(), l.sol.len());
    let t = &l.transports;
    println!("{} tunnels, {} voies ferrées, {} arrêts ({} sans nom), {} lignes", t.tunnels.len(), t.rails.len(), t.arrets.len(), t.arrets.iter().filter(|a| a.nom.is_empty()).count(), t.lignes.len());
    let mut modes = [0usize; 5];
    for li in &t.lignes {
        modes[li.mode as usize] += 1;
    }
    println!("lignes par mode {modes:?}");
    for li in t.lignes.iter().filter(|li| li.mode != sage_reel::transports::BUS).take(6) {
        println!("  {} : {} arrêts, {} morceaux de tracé — {}", li.titre(), li.arrets.len(), li.trace.len(), li.arrets.iter().map(|&k| { let a = &t.arrets[k as usize]; if a.niveau < 0 { format!("{} ({})", a.nom, a.niveau) } else { a.nom.clone() } }).collect::<Vec<_>>().join(", "));
        let long = |p: &[[f32; 2]]| p.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum::<f32>();
        println!("    tracé : {}", li.trace.iter().map(|m| format!("{}{:.0} m", ["sol ", "pont ", "tunnel "][m.ou.min(2) as usize], long(&m.points))).collect::<Vec<_>>().join(" · "));
    }
    let r = l.octets();
    println!("format {} ; réécrit à l'identique : {}", u32::from_le_bytes(o[8..12].try_into().unwrap()), r == o);
    println!("relu pareil : {}", sage_reel::Lieu::depuis_octets(&r).as_ref() == Some(&l));
    println!("grille {:?}, étendue {:?}", l.grille, l.etendue());
    for sf in &l.surfaces_eau {
        let bt = sf.anneaux.iter().flatten().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |a, p| [a[0].min(p[0]), a[1].min(p[1]), a[2].max(p[0]), a[3].max(p[1])]);
        println!("surface d'eau {} : {} anneaux ({:?} points), boîte {bt:?}", sf.nature, sf.anneaux.len(), sf.anneaux.iter().map(|r| r.len()).collect::<Vec<_>>());
        for r in &sf.anneaux {
            // Sur la berge : de l'eau d'un côté à 6 m, de la terre de l'autre
            let berge = r.iter().filter(|p| { let e = |dx: f32, dz: f32| l.eau((p[0] + dx) as f64, (p[1] + dz) as f64) >= 0.5; let v = [e(6.0, 0.0), e(-6.0, 0.0), e(0.0, 6.0), e(0.0, -6.0)]; v.iter().any(|x| *x) && v.iter().any(|x| !*x) }).count();
            println!("  anneau de {} points : {} sur la berge de la grille ; premiers {:?}", r.len(), berge, &r[..4.min(r.len())]);
        }
    }
}
