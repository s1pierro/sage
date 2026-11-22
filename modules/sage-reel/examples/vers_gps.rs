//! Un point (x, z) du jeu d'un lieu en (lat, lon) :
//!   cargo run --example vers_gps -- <lieu.reel> x z
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let l = sage_reel::Lieu::lire(std::path::Path::new(&a[1])).expect("lieu");
    let (x, z): (f64, f64) = (a[2].parse().unwrap(), a[3].parse().unwrap());
    let (cx, cz) = l.vers_jeu(49.44, 1.09);
    let (lx, ly) = sage_reel::lambert93(49.44, 1.09);
    let (lat, lon) = sage_reel::lambert93_inverse(lx + (x - cx), ly - (z - cz));
    println!("{lat:.6} {lon:.6}");
}
