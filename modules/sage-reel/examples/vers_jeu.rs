//! Un point (lat, lon) au repère du jeu d'un lieu centré en (lat0, lon0) :
//!   cargo run --example vers_jeu -- lat0 lon0 lat lon
fn main() {
    let a: Vec<f64> = std::env::args().skip(1).map(|s| s.parse().unwrap()).collect();
    let (xc, yc) = sage_reel::lambert93(a[0], a[1]);
    let (x0, y0) = (xc - sage_reel::DEPART[0] as f64, yc + sage_reel::DEPART[1] as f64);
    let (x, y) = sage_reel::lambert93(a[2], a[3]);
    println!("x {:.1} z {:.1}", x - x0, y0 - y);
}
