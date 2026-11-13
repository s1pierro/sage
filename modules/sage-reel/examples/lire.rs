//! Lit un lieu réel et en dit l'essentiel ; vérifie qu'il se réécrit à l'identique.
//!
//!   cargo run --release --example lire -- <lieu.reel>

fn main() {
    let f = std::env::args().nth(1).expect("usage : lire <lieu.reel>");
    let o = std::fs::read(&f).expect("lecture");
    let t = std::time::Instant::now();
    let l = sage_reel::Lieu::depuis_octets(&o).expect("pas un lieu réel lisible");
    println!("{} : {:.0} ms", l.nom, t.elapsed().as_secs_f64() * 1000.0);
    println!("centre {:.5}, {:.5} ; {} × {} points de {} m ; eau à {:.2} m", l.lat, l.lon, l.cote, l.cote, l.pas, l.niveau);
    println!("{} routes, {} ponts, {} bâtiments, sol {}", l.routes.len(), l.ponts.len(), l.batiments.len(), l.sol.len());
    println!("réécrit à l'identique : {}", l.octets() == o);
}
