//! Réduit un lieu réel à un rectangle centré sur son départ.
//!
//!   cargo run --release --example recadrer -- <lieu.reel> <largeur km> <longueur km> <sortie.reel>

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 5 {
        eprintln!("usage : recadrer <lieu.reel> <largeur km> <longueur km> <sortie.reel>");
        std::process::exit(1);
    }
    let l = sage_reel::Lieu::lire(std::path::Path::new(&a[1])).unwrap_or_else(|e| panic!("{e}"));
    let km = |s: &str| s.replace(',', ".").parse::<f64>().expect("un nombre") * 1000.0;
    let r = l.recadrer(sage_reel::Grille::emprise(l.pas, km(&a[2]), km(&a[3])));
    r.ecrire(std::path::Path::new(&a[4])).expect("écriture");
    println!("{:?} : {} routes, {} ponts, {} bâtiments", r.grille, r.routes.len(), r.ponts.len(), r.batiments.len());
}
