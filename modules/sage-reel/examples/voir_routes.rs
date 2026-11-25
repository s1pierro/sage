//! Les voies d'un lieu autour d'un point : genre, largeur, files.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let l = sage_reel::Lieu::lire(std::path::Path::new(&a[1])).expect("lecture");
    let (x, z, r): (f32, f32, f32) = (a[2].parse().unwrap(), a[3].parse().unwrap(), a[4].parse().unwrap());
    for (nom, liste) in [("route", &l.routes), ("pont", &l.ponts), ("tunnel", &l.transports.tunnels)] {
        for (i, v) in liste.iter().enumerate() {
            if v.points.iter().any(|p| (p[0] - x).hypot(p[1] - z) < r) {
                let (a, b) = (v.points[0], v.points[v.points.len() - 1]);
                println!("{nom} {i} genre {} largeur {} files {:?} {} points ({:.0},{:.0})→({:.0},{:.0})", v.genre, v.largeur, v.files, v.points.len(), a[0], a[1], b[0], b[1]);
            }
        }
    }
}
