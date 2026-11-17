//! Les matières de S.A.G.E. : des textures qui se répètent sans couture, générées par
//! le calcul à partir de quelques réglages (`generateur`), et leur écriture en PNG
//! (`png`). Une matière est un fichier texte de paires `clé = valeur`
//! (`Reglages::ecrire` / `Reglages::lire`), modifiable à la main.
//!
//! Issu de Texturine (qui devient le contexte Matières de Flow).

pub mod chaussee;
pub mod generateur;
pub mod png;

pub use generateur::{generer, Chaussee, Genre, Image, Reglages, Tsl, TRAITS};

/// Les paires `clé = valeur` d'un texte, sans les commentaires ni les lignes vides.
pub fn cles(texte: &str) -> Vec<(String, String)> {
    texte
        .lines()
        .filter_map(|l| {
            let l = l.split('#').next().unwrap_or("").trim();
            let (k, v) = l.split_once('=')?;
            Some((k.trim().to_lowercase(), v.trim().to_string()))
        })
        .collect()
}
