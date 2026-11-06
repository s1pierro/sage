//! La carte du monde entier, pour l'onglet Topologie : un pixel par tuile (313 × 313),
//! la couleur du sol au centre de la tuile, ombrée par le relief (lumière du nord-ouest),
//! la mer bleue d'autant plus sombre qu'elle est profonde.
//!
//! Le fond vient du générateur, calculé une fois par graine. Les tuiles retouchées s'y
//! posent par-dessus : celles du dépôt à la construction, celles chargées à chaque
//! retouche.

use crate::campagne::Campagne;
use crate::image::Image;
use crate::moteur::tuiles::DEMI_CARTE;
use crate::moteur::{Monde, Relief, Tuiles, TAILLE};
use std::collections::HashMap;

/// Tuiles par côté de la carte.
pub const COTE: usize = (2 * DEMI_CARTE + 1) as usize;

/// Le sol au centre d'une tuile : sa hauteur et sa couleur.
type Point = (f32, [f32; 3]);

pub struct Carte {
    pub graine: u32,
    /// La surface de l'eau.
    mer: f32,
    fond: Vec<Point>,
    /// Les tuiles retouchées, par-dessus le fond.
    retouchees: HashMap<(i32, i32), Point>,
    /// Le compteur de retouches des tuiles à la dernière image faite.
    pub vue: u64,
}

fn rang(i: i32, j: i32) -> Option<usize> {
    let (a, b) = (i + DEMI_CARTE, j + DEMI_CARTE);
    (0..COTE as i32).contains(&a).then_some(())?;
    (0..COTE as i32).contains(&b).then_some((b as usize) * COTE + a as usize)
}

/// Le centre d'une tuile retouchée : sa hauteur et sa couleur, au milieu de la grille.
fn centre(r: &Relief, couleurs: &[[f32; 3]]) -> Point {
    let m = r.n / 2;
    (r.sommet(m, m), couleurs.get(m * (r.n + 1) + m).copied().unwrap_or([0.5; 3]))
}

impl Carte {
    /// Le fond de la carte du relief `c`, et les tuiles écrites du dépôt.
    pub fn new(c: &Campagne, monde: &mut Monde, tuiles: &Tuiles) -> Carte {
        let graine = c.graine;
        let d = DEMI_CARTE;
        let t = TAILLE as f64;
        // Une hauteur par tuile ; la pente, pour la couleur, se lit entre voisines
        let mut hs = Vec::with_capacity(COTE * COTE);
        for j in -d..=d {
            for i in -d..=d {
                hs.push(c.hauteur((i as f64 + 0.5) * t, (j as f64 + 0.5) * t));
            }
        }
        let h = |a: usize, b: usize| hs[b.min(COTE - 1) * COTE + a.min(COTE - 1)];
        let mut fond = Vec::with_capacity(COTE * COTE);
        for b in 0..COTE {
            for a in 0..COTE {
                let dx = (h(a + 1, b) - h(a.saturating_sub(1), b)) / (2.0 * t);
                let dz = (h(a, b + 1) - h(a, b.saturating_sub(1))) / (2.0 * t);
                let l = (dx * dx + dz * dz + 1.0).sqrt();
                let n = [(-dx / l) as f32, (1.0 / l) as f32, (-dz / l) as f32];
                let (x, z) = ((a as f64 - d as f64 + 0.5) * t, (b as f64 - d as f64 + 0.5) * t);
                fond.push((h(a, b) as f32, c.couleur(x, z, h(a, b), n)));
            }
        }
        let mut retouchees = HashMap::new();
        for (i, j) in tuiles.depot.tuiles_ecrites() {
            if let Ok(Some(t)) = tuiles.depot.lire_tuile(i, j, &mut monde.modeles) {
                retouchees.insert((i, j), centre(&t.relief, &t.couleurs));
            }
        }
        Carte { graine, mer: c.mer(), fond, retouchees, vue: u64::MAX }
    }

    /// Les tuiles chargées et lues du dépôt (retouchées) se posent sur la carte.
    pub fn suivre(&mut self, monde: &Monde, tuiles: &Tuiles) {
        for (k, t) in &tuiles.chargees {
            if !t.lue && !tuiles.est_sale(*k) {
                continue;
            }
            if let (Some(r), Some(c)) = (monde.relief.get(t.terrain), tuiles.couleurs(*k)) {
                self.retouchees.insert(*k, centre(r, c));
            }
        }
        self.vue = tuiles.retouches;
    }

    /// L'image de la carte.
    pub fn image(&self) -> Image {
        let mut g = self.fond.clone();
        for (&(i, j), p) in &self.retouchees {
            if let Some(k) = rang(i, j) {
                g[k] = *p;
            }
        }
        let h = |a: usize, b: usize| g[b.min(COTE - 1) * COTE + a.min(COTE - 1)].0;
        let mut rgba = Vec::with_capacity(COTE * COTE * 4);
        for b in 0..COTE {
            for a in 0..COTE {
                let (y, c) = g[b * COTE + a];
                let c = if y < self.mer {
                    let t = ((self.mer - y) / 30.0).clamp(0.0, 1.0);
                    [0.50 + (0.12 - 0.50) * t, 0.72 + (0.30 - 0.72) * t, 0.78 + (0.40 - 0.78) * t]
                } else {
                    // La pente vers la lumière, venue du nord-ouest (-x, -z)
                    let pente = (h(a.saturating_sub(1), b) - h(a + 1, b) + h(a, b.saturating_sub(1)) - h(a, b + 1)) / (2.0 * TAILLE);
                    let s = (1.0 + pente * 2.5).clamp(0.55, 1.35);
                    c.map(|v| v * s)
                };
                rgba.extend(c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
                rgba.push(255);
            }
        }
        Image { l: COTE as u32, h: COTE as u32, rgba }
    }
}

/// Un point du monde (absolu) en fraction de la carte, de 0 à 1 sur x et z.
pub fn vers_carte(x: f64, z: f64) -> [f32; 2] {
    let cote = COTE as f64 * TAILLE as f64;
    let o = -(DEMI_CARTE as f64) * TAILLE as f64;
    [((x - o) / cote) as f32, ((z - o) / cote) as f32]
}

/// Une fraction de la carte en point du monde (absolu).
pub fn depuis_carte(u: f32, v: f32) -> [f64; 2] {
    let cote = COTE as f64 * TAILLE as f64;
    let o = -(DEMI_CARTE as f64) * TAILLE as f64;
    [o + u as f64 * cote, o + v as f64 * cote]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_carte_va_et_vient_entre_le_monde_et_l_image() {
        let p = depuis_carte(0.25, 0.75);
        let q = vers_carte(p[0], p[1]);
        assert!((q[0] - 0.25).abs() < 1e-6 && (q[1] - 0.75).abs() < 1e-6);
        // Le centre de la carte est au milieu de la tuile (0, 0)
        let c = depuis_carte(0.5, 0.5);
        assert!((c[0] - 32.0).abs() < 1e-6 && (c[1] - 32.0).abs() < 1e-6);
        assert_eq!(rang(-DEMI_CARTE, -DEMI_CARTE), Some(0));
        assert_eq!(rang(DEMI_CARTE + 1, 0), None);
    }

    #[test]
    fn l_image_montre_l_ile_et_la_mer() {
        let mut m = Monde::default();
        let t = Tuiles::new(Box::new(Campagne::brute(1)), 1, 1);
        let c = Carte::new(&Campagne::brute(1), &mut m, &t);
        let im = c.image();
        assert_eq!((im.l, im.h, im.rgba.len()), (COTE as u32, COTE as u32, COTE * COTE * 4));
        let px = |a: usize, b: usize| &im.rgba[(b * COTE + a) * 4..(b * COTE + a) * 4 + 3];
        let coin = px(0, 0);
        assert!(coin[2] > coin[0], "la mer au coin : {coin:?}");
        let milieu = px(COTE / 2, COTE / 2);
        assert!(milieu[1] > milieu[2], "la terre au milieu : {milieu:?}");
    }
}
