//! L'occupation du sol de l'IGN autour d'un point, par classe (surfaces et km²) :
//!   cargo run --release --example sol_ign -- [lat lon [demi-côté en m]]
use sage_reel::generer::{sol_ign, Curl, Suivi};

fn main() {
    let a: Vec<f64> = std::env::args().skip(1).filter_map(|s| s.parse().ok()).collect();
    let (lat, lon, d) = (a.first().copied().unwrap_or(49.4431), a.get(1).copied().unwrap_or(1.0993), a.get(2).copied().unwrap_or(5000.0));
    let (dlat, dlon) = (d / 111_000.0, d / (111_000.0 * lat.to_radians().cos()));
    let cache = std::env::temp_dir().join("sage-sol-ign");
    std::fs::create_dir_all(&cache).unwrap();
    let t = std::time::Instant::now();
    // En mètres Lambert 93, tels quels
    let (fond, precises) = sol_ign(&Curl, &Suivi::default(), &cache, "", (lat - dlat, lon - dlon, lat + dlat, lon + dlon), &|x, y| (x, y)).unwrap();
    println!("{} s", t.elapsed().as_secs());
    let aire = |a: &Vec<(f64, f64)>| (a.windows(2).map(|w| w[0].0 * w[1].1 - w[1].0 * w[0].1).sum::<f64>() / 2.0).abs();
    for (nom, l) in [("fond (BD CARTO)", &fond), ("précises", &precises)] {
        let mut n = [0usize; 6];
        let mut km2 = [0f64; 6];
        for (c, anneaux) in l {
            n[*c as usize] += 1;
            km2[*c as usize] += anneaux.iter().map(aire).sum::<f64>() / 1e6;
        }
        println!("{nom} : {} surfaces", l.len());
        for (k, c) in ["", "bois", "prés", "champs", "habité", "zones"].iter().enumerate().skip(1) {
            println!("  {c:7} {:5} surfaces, {:6.1} km² (trous comptés en plus)", n[k], km2[k]);
        }
    }
}
