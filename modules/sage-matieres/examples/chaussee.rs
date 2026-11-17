//! Écrit des chaussées d'essai en PNG : `cargo run --release --example chaussee -- <dossier>`.
use sage_matieres::{generer, png, Genre, Reglages, Tsl};

fn main() {
    let dossier = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let mut route = Reglages::defaut(Genre::Chaussee);
    route.resolution = 512;
    let mut piste = route.clone();
    piste.chaussee.voies = 3;
    piste.chaussee.largeur_voie = 4.0;
    piste.chaussee.rive = 3;
    piste.chaussee.separation = 0;
    piste.appareil = 1;
    piste.chaussee.usure = 0.1;
    piste.chaussee.usure_marquage = 0.0;
    let mut vieille = route.clone();
    vieille.chaussee.voies = 4;
    vieille.chaussee.axe = 4;
    vieille.chaussee.usure = 0.9;
    vieille.chaussee.usure_marquage = 0.8;
    vieille.irregularite = 0.7;
    vieille.couleur = Tsl::new(220.0, 0.03, 0.33);
    vieille.couleur_joint = Tsl::new(48.0, 0.9, 0.55);
    for (nom, mut r) in [("route", route), ("piste", piste), ("vieille", vieille)] {
        r.borner();
        let i = generer(&r);
        std::fs::write(format!("{dossier}/chaussee-{nom}.png"), png::encoder(i.l, i.h, &i.rgba)).unwrap();
        println!("{nom} : {}", r.resume());
    }
}
