//! Calcule l'érosion d'une graine avec des réglages donnés, en donne le temps et quelques
//! chiffres, et écrit une image (PGM) : le relief ombré avant, après, et l'écart.
//!
//!   cargo run --release --example erosion -- <graine> <sortie.pgm> [réglage=valeur…]
//!
//! Réglages : densite vie inertie capacite arrache depose pente_min evaporation gravite
//! rayon creuse_max echelle.

use sage::campagne::{Campagne, MER};
use sage::erosion::{Erosion, Reglage};
use std::sync::Arc;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let graine: u32 = a[1].parse().unwrap();
    let mut r = Reglage::default();
    for x in &a[3..] {
        let (k, v) = x.split_once('=').unwrap();
        let f: f64 = v.parse().unwrap();
        match k {
            "densite" => r.densite = f,
            "vie" => r.vie = f as usize,
            "inertie" => r.inertie = f as f32,
            "capacite" => r.capacite = f as f32,
            "arrache" => r.arrache = f as f32,
            "depose" => r.depose = f as f32,
            "pente_min" => r.pente_min = f as f32,
            "evaporation" => r.evaporation = f as f32,
            "gravite" => r.gravite = f as f32,
            "rayon" => r.rayon = f as i32,
            "creuse_max" => r.creuse_max = f as f32,
            "echelle" => r.echelle = f as f32,
            _ => panic!("réglage inconnu {k}"),
        }
    }
    let brute = Campagne::brute(graine);
    let relief = |x: f64, z: f64| brute.relief(x, z);
    let t = std::time::Instant::now();
    Erosion::calculer_avec(&Reglage { densite: 0.0, ..r }, graine, MER, &relief);
    let t0 = t.elapsed().as_secs_f64();
    let t = std::time::Instant::now();
    let e = Erosion::calculer_avec(&r, graine, MER, &relief);
    println!("graine {graine} : hauteurs {t0:.2} s, tout {:.2} s", t.elapsed().as_secs_f64());
    let mut v: Vec<f32> = e.ecart.iter().cloned().filter(|x| x.abs() > 0.01).collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let q = |p: f64| v[((v.len() - 1) as f64 * p) as usize];
    println!("écart : min {:.1} 1% {:.1} 10% {:.1} 50% {:.1} 90% {:.1} 99% {:.1} max {:.1}", q(0.0), q(0.01), q(0.1), q(0.5), q(0.9), q(0.99), q(1.0));
    let mut w: Vec<f32> = e.eau.iter().cloned().filter(|x| *x > 0.0).collect();
    w.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let q = |p: f64| w[((w.len() - 1) as f64 * p) as usize];
    println!("eau : 10% {:.2} 50% {:.2} 90% {:.2} 99% {:.2} 99.9% {:.2}", q(0.1), q(0.5), q(0.9), q(0.99), q(0.999));
    let c = Campagne { graine, erosion: Some(Arc::new(e)), reel: None };
    let (w, pas) = (700usize, 25.0);
    let mut img = vec![0u8; w * 3 * w];
    for b in 0..w {
        for x in 0..w {
            let (px, pz) = ((x as f64 - w as f64 / 2.0) * pas, (b as f64 - w as f64 / 2.0) * pas);
            for (k, cc) in [&brute, &c].iter().enumerate() {
                let h = cc.hauteur(px, pz);
                let dx = (cc.hauteur(px + 8.0, pz) - cc.hauteur(px - 8.0, pz)) / 16.0;
                let dz = (cc.hauteur(px, pz + 8.0) - cc.hauteur(px, pz - 8.0)) / 16.0;
                let l = (dx * dx + dz * dz + 1.0).sqrt();
                let ombre = ((dx + dz) / l * -0.7 + 1.0 / l * 0.7).clamp(0.0, 1.0);
                img[b * 3 * w + k * w + x] = if h < MER as f64 { 30 } else { (40.0 + ombre * 215.0) as u8 };
            }
            let d = c.erosion.as_ref().unwrap().ecart(px, pz);
            img[b * 3 * w + 2 * w + x] = (128.0 + d * 4.0).clamp(0.0, 255.0) as u8;
        }
    }
    let mut f = format!("P5 {} {} 255\n", 3 * w, w).into_bytes();
    f.extend(img);
    std::fs::write(&a[2], f).unwrap();
}
