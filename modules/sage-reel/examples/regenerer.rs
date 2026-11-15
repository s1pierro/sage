//! Refait un lieu réel d'après son propre fichier (nom, centre, pas, emprise) et son cache :
//! ce qui manque au cache (une nouveauté du format) se télécharge.
//!
//!   cargo run --release --example regenerer -- <lieu.reel> <largeur en m> <longueur en m>

use sage_reel::generer::{generer, Curl, Demande, Suivi};
use std::path::PathBuf;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let sortie = PathBuf::from(&a[1]);
    // Le nom, le centre et le pas : dans l'en-tête (lisible même si la suite ne l'est plus)
    let o = std::fs::read(&sortie).expect("lieu illisible");
    let n = u32::from_le_bytes(o[12..16].try_into().unwrap()) as usize;
    let nom = String::from_utf8_lossy(&o[16..16 + n]).to_string();
    let f = |k: usize| f64::from_le_bytes(o[16 + n + 8 * k..24 + n + 8 * k].try_into().unwrap());
    let (lat, lon, pas) = (f(0), f(1), f(4));
    let n = |k: usize| a.get(k).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
    let d = Demande { nom, lat, lon, sortie, pas, toits: true, largeur: n(2), longueur: n(3) };
    let mut suivi = Suivi::default();
    suivi.imprimer = true;
    match generer(&d, &Curl, &suivi) {
        Ok(l) => {
            let t = &l.transports;
            println!("{} tunnels, {} voies ferrées, {} arrêts, {} lignes", t.tunnels.len(), t.rails.len(), t.arrets.len(), t.lignes.len());
        }
        Err(e) => {
            eprintln!("échec : {e}");
            std::process::exit(1);
        }
    }
}
