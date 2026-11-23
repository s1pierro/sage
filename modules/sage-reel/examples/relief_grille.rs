//! Le relief autour d'un point, en grille de texte (m, arrondi).
//!   cargo run --release --example relief_grille -- <lieu.reel> x0 z0 x1 z1 pas
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let l = sage_reel::Lieu::lire(std::path::Path::new(&a[1])).expect("lieu");
    let v: Vec<f64> = a[2..7].iter().map(|x| x.parse().unwrap()).collect();
    let (x0, z0, x1, z1, pas) = (v[0], v[1], v[2], v[3], v[4]);
    print!("z\\x  ");
    let mut x = x0;
    while x <= x1 {
        print!("{:>4.0}", x);
        x += pas;
    }
    println!();
    let mut z = z0;
    while z <= z1 {
        print!("{:>5.0}", z);
        let mut x = x0;
        while x <= x1 {
            print!("{:>4.0}", l.relief(x, z));
            x += pas;
        }
        println!();
        z += pas;
    }
}
