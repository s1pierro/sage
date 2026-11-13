//! Prépare un lieu réel pour S.A.G.E. : 20 km autour d'un point de France, le relief de
//! l'IGN, les toits du LiDAR HD et OpenStreetMap (eau, routes, ponts, bâtiments,
//! occupation du sol).
//!
//!   cargo run --release --example lieu_reel -- <nom> <latitude> <longitude> <sortie.reel> [pas en m] [largeur en km] [longueur en km]
//!
//! Le pas vaut 4 m par défaut. La largeur (est-ouest) et la longueur (nord-sud) bornent la
//! zone téléchargée, centrée sur le point ; par défaut, la carte entière (20 km). Le travail est fait par `sage_reel::generer` (partagé avec
//! Flow, qui génère depuis le téléphone) ; ici le réseau passe par curl. Les
//! téléchargements sont gardés dans `<sortie>.cache/` : relancer ne les refait pas. Le
//! fichier produit s'ouvre dans le jeu (Éditeur › Topologie › Carte › « Importer un lieu
//! réel… »).
//!
//! Le fichier est dérivé d'OpenStreetMap : il reste sous ODbL (citer « © les
//! contributeurs OpenStreetMap »), et ne se met pas dans le dépôt.

use sage_reel::generer::{generer, Curl, Demande, Suivi};
use std::path::PathBuf;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 5 {
        eprintln!("usage : lieu_reel <nom> <latitude> <longitude> <sortie.reel> [pas en m] [largeur en km] [longueur en km]");
        std::process::exit(1);
    }
    let nombre = |s: &str| s.replace(',', ".").parse::<f64>().unwrap_or_else(|_| panic!("« {s} » n'est pas un nombre"));
    let d = Demande { nom: a[1].clone(), lat: nombre(&a[2]), lon: nombre(&a[3]), sortie: PathBuf::from(&a[4]), pas: a.get(5).map_or(4.0, |p| nombre(p)), toits: true, largeur: a.get(6).map_or(0.0, |v| nombre(v) * 1000.0), longueur: a.get(7).map_or(0.0, |v| nombre(v) * 1000.0) };
    let mut suivi = Suivi::default();
    suivi.imprimer = true;
    if let Err(e) = generer(&d, &Curl, &suivi) {
        eprintln!("échec : {e}");
        std::process::exit(1);
    }
}
