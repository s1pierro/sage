//! La campagne : une source de tuiles générée, toujours la même pour une graine donnée.
//! Le relief est une somme de bruits calculée en coordonnées absolues (double précision) :
//! deux tuiles voisines s'accordent exactement sur leur bord, à n'importe quelle distance.

use crate::moteur::{Contenu, Element, Genre, Objet, Relief, Source, TAILLE};

/// Points de grille par côté de tuile : un pas de 2 m.
pub const GRILLE: usize = 32;
/// Le départ, en absolu, et le rayon laissé libre autour.
pub const DEPART: [f32; 2] = [TAILLE / 2.0, TAILLE / 2.0];
const LIBRE: f32 = 8.0;

pub struct Campagne {
    pub graine: u32,
}

/// Un nombre de 0 à 1, toujours le même pour (graine, x, z).
fn hache(graine: u32, x: i64, z: i64) -> f64 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (z as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ graine as u64;
    h ^= h >> 31;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 29;
    (h >> 11) as f64 / (1u64 << 53) as f64
}

/// Bruit de valeurs, lissé, sur un réseau de pas 1.
fn bruit(graine: u32, x: f64, z: f64) -> f64 {
    let (x0, z0) = (x.floor(), z.floor());
    let (fx, fz) = (x - x0, z - z0);
    let (sx, sz) = (fx * fx * (3.0 - 2.0 * fx), fz * fz * (3.0 - 2.0 * fz));
    let (i, k) = (x0 as i64, z0 as i64);
    let a = hache(graine, i, k) + (hache(graine, i + 1, k) - hache(graine, i, k)) * sx;
    let b = hache(graine, i, k + 1) + (hache(graine, i + 1, k + 1) - hache(graine, i, k + 1)) * sx;
    a + (b - a) * sz
}

impl Campagne {
    /// La hauteur du sol en un point absolu.
    pub fn hauteur(&self, x: f64, z: f64) -> f64 {
        let mut h = 0.0;
        for (k, (onde, ampleur)) in [(240.0, 18.0), (96.0, 6.0), (40.0, 1.6), (14.0, 0.35)].iter().enumerate() {
            h += (bruit(self.graine.wrapping_add(k as u32 * 7919), x / onde, z / onde) - 0.5) * 2.0 * ampleur;
        }
        h
    }

    fn normale(&self, x: f64, z: f64) -> [f32; 3] {
        let e = 0.5;
        let dx = (self.hauteur(x + e, z) - self.hauteur(x - e, z)) / (2.0 * e);
        let dz = (self.hauteur(x, z + e) - self.hauteur(x, z - e)) / (2.0 * e);
        let l = (dx * dx + dz * dz + 1.0).sqrt();
        [(-dx / l) as f32, (1.0 / l) as f32, (-dz / l) as f32]
    }

    /// Herbe plus ou moins sèche, plus sombre dans les creux, roche sur les pentes fortes.
    fn couleur(&self, x: f64, z: f64, h: f64, n: [f32; 3]) -> [f32; 3] {
        let herbe = [0.50, 0.62, 0.40];
        let seche = [0.66, 0.66, 0.46];
        let roche = [0.58, 0.56, 0.52];
        let s = bruit(self.graine ^ 0xA5A5, x / 30.0, z / 30.0) as f32;
        let mut c: [f32; 3] = [0, 1, 2].map(|k| herbe[k] + (seche[k] - herbe[k]) * s * 0.8);
        let creux = (((-h) / 12.0).clamp(0.0, 1.0) * 0.18) as f32;
        c = c.map(|v| v * (1.0 - creux));
        let r = ((0.93 - n[1]) / 0.12).clamp(0.0, 1.0);
        [0, 1, 2].map(|k| c[k] + (roche[k] - c[k]) * r)
    }
}

impl Source for Campagne {
    fn tuile(&self, i: i32, j: i32) -> Contenu {
        let (ox, oz) = (i as f64 * TAILLE as f64, j as f64 * TAILLE as f64);
        let n = GRILLE;
        let pas = TAILLE as f64 / n as f64;
        let k = (n + 1) * (n + 1);
        let (mut h, mut normales, mut couleurs) = (Vec::with_capacity(k), Vec::with_capacity(k), Vec::with_capacity(k));
        for b in 0..=n {
            for a in 0..=n {
                let (x, z) = (ox + a as f64 * pas, oz + b as f64 * pas);
                let y = self.hauteur(x, z);
                let nm = self.normale(x, z);
                h.push(y as f32);
                normales.push(nm);
                couleurs.push(self.couleur(x, z, y, nm));
            }
        }
        let relief = Relief { n, h };
        let mut objets = Vec::new();
        let mut poser = |genre: Genre, pos: [f32; 3], cap: f32, dim: [f32; 3], couleur: [f32; 3]| {
            let k = objets.iter().filter(|e: &&Element| e.objet.genre == genre).count();
            objets.push(Element { id: format!("tuile/{i}/{j}/{}/{k}", genre.ident()), objet: Objet { genre, dim, couleur }, pos, cap });
        };

        // Quelques blocs, posés sur le relief, jamais sur le départ
        let mut graine = (hache(self.graine ^ 0xB10C, i as i64, j as i64) * u32::MAX as f64) as u32 | 1;
        let mut hasard = move || {
            graine ^= graine << 13;
            graine ^= graine >> 17;
            graine ^= graine << 5;
            graine as f32 / u32::MAX as f32
        };
        let teintes = [[0.82, 0.78, 0.70], [0.70, 0.72, 0.76], [0.78, 0.66, 0.56], [0.62, 0.70, 0.62], [0.86, 0.84, 0.80]];
        let combien = 2 + (hasard() * 6.0) as usize;
        for _ in 0..combien {
            let demi = [0.4 + hasard() * 2.6, 0.3 + hasard() * 4.0, 0.4 + hasard() * 2.6];
            let (x, z) = (demi[0] + hasard() * (TAILLE - 2.0 * demi[0]), demi[2] + hasard() * (TAILLE - 2.0 * demi[2]));
            let couleur = teintes[(hasard() * teintes.len() as f32) as usize % teintes.len()];
            let (ax, az) = (ox as f32 + x - DEPART[0], oz as f32 + z - DEPART[1]);
            if ax.abs() < demi[0] + LIBRE && az.abs() < demi[2] + LIBRE {
                continue;
            }
            // Le pied au plus bas du sol sous le bloc, un peu enfoncé
            let bas = [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0), (0.0, 0.0)]
                .iter()
                .map(|(u, v)| relief.hauteur(x + u * demi[0], z + v * demi[2]))
                .fold(f32::INFINITY, f32::min)
                - 0.3;
            poser(Genre::Bloc, [x, bas, z], 0.0, demi.map(|d| 2.0 * d), couleur);
        }
        // Des dalles basses, qui se franchissent d'un pas
        let libre = |x: f32, z: f32, r: f32| {
            let (ax, az) = (ox as f32 + x - DEPART[0], oz as f32 + z - DEPART[1]);
            ax.abs() >= r + LIBRE || az.abs() >= r + LIBRE
        };
        // Le sol le plus bas et le plus haut sous un rectangle de demi-côtés (dx, dz)
        let sous = |x: f32, z: f32, dx: f32, dz: f32| {
            [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0), (0.0, 0.0)].iter().map(|(u, v)| relief.hauteur(x + u * dx, z + v * dz)).fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), h| (a.min(h), b.max(h)))
        };
        for _ in 0..1 + (hasard() * 3.0) as usize {
            let demi = [0.8 + hasard() * 1.6, 0.0, 0.8 + hasard() * 1.6];
            let (x, z) = (demi[0] + hasard() * (TAILLE - 2.0 * demi[0]), demi[2] + hasard() * (TAILLE - 2.0 * demi[2]));
            if !libre(x, z, demi[0].max(demi[2])) {
                continue;
            }
            let (bas, haut) = (sous(x, z, demi[0], demi[2]).0 - 0.1, sous(x, z, demi[0], demi[2]).1 + 0.12 + hasard() * 0.13);
            poser(Genre::Dalle, [x, bas, z], 0.0, [2.0 * demi[0], haut - bas, 2.0 * demi[2]], [0.74, 0.72, 0.68]);
        }

        // Des poteaux
        for _ in 0..(hasard() * 3.0) as usize {
            let (r, h) = (0.12 + hasard() * 0.13, 2.0 + hasard() * 2.0);
            let (x, z) = (1.0 + hasard() * (TAILLE - 2.0), 1.0 + hasard() * (TAILLE - 2.0));
            if !libre(x, z, r) {
                continue;
            }
            poser(Genre::Poteau, [x, relief.hauteur(x, z) - 0.2, z], 0.0, [2.0 * r, h, 2.0 * r], [0.55, 0.42, 0.30]);
        }

        // Une rampe sur une plate-forme, une fois sur deux
        if hasard() < 0.5 {
            let (long, large, plateau) = (5.0, 1.2, 1.2);
            let (x, z) = (8.0 + hasard() * (TAILLE - 16.0), 8.0 + hasard() * (TAILLE - 16.0));
            let sens: [f32; 2] = [[1.0, 0.0], [-1.0, 0.0], [0.0, 1.0], [0.0, -1.0]][(hasard() * 4.0) as usize % 4];
            if libre(x, z, long + 2.0) {
                // La plate-forme, au bout de la rampe
                let (px, pz) = (x + sens[0] * (long + 1.0), z + sens[1] * (long + 1.0));
                let haut = sous(px, pz, 1.0, 1.0).1 + plateau;
                let bas = sous(px, pz, 1.0, 1.0).0 - 0.3;
                poser(Genre::Bloc, [px, bas, pz], 0.0, [2.0, haut - bas, 2.0], [0.80, 0.76, 0.66]);
                let h0 = relief.hauteur(x, z);
                poser(Genre::Rampe, [x, h0, z], sens[0].atan2(sens[1]), [large, haut - h0, long], [0.70, 0.64, 0.54]);
            }
        }
        Contenu { relief, normales, couleurs, objets }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deux_tuiles_voisines_s_accordent_sur_leur_bord() {
        let c = Campagne { graine: 7 };
        for (i, j) in [(0, 0), (-3, 5), (40_000, -12_345)] {
            let (a, b, d) = (c.tuile(i, j), c.tuile(i + 1, j), c.tuile(i, j + 1));
            for k in 0..=GRILLE {
                assert_eq!(a.relief.sommet(GRILLE, k), b.relief.sommet(0, k), "bord est de ({i}, {j})");
                assert_eq!(a.relief.sommet(k, GRILLE), d.relief.sommet(k, 0), "bord sud de ({i}, {j})");
            }
        }
    }

    #[test]
    fn la_campagne_est_toujours_la_meme() {
        let (a, b) = (Campagne { graine: 3 }.tuile(2, -1), Campagne { graine: 3 }.tuile(2, -1));
        assert_eq!(a.relief, b.relief);
        assert_eq!(a.objets, b.objets);
        assert_ne!(Campagne { graine: 4 }.tuile(2, -1).relief, a.relief);
    }

    #[test]
    fn le_depart_est_libre_et_les_blocs_touchent_le_sol() {
        let c = Campagne { graine: 1 };
        let mut total = 0;
        for j in -2..=2 {
            for i in -2..=2 {
                let t = c.tuile(i, j);
                for e in t.objets.iter().filter(|e| matches!(e.objet.genre, Genre::Bloc | Genre::Dalle)) {
                    total += 1;
                    let demi = e.objet.dim.map(|d| d / 2.0);
                    let (ax, az) = (i as f32 * TAILLE + e.pos[0] - DEPART[0], j as f32 * TAILLE + e.pos[2] - DEPART[1]);
                    assert!(ax.abs() >= demi[0] + LIBRE || az.abs() >= demi[2] + LIBRE);
                    assert!(e.pos[1] < t.relief.hauteur(e.pos[0], e.pos[2]), "posé, pas en l'air");
                    assert!(e.id.starts_with(&format!("tuile/{i}/{j}/")), "{}", e.id);
                }
            }
        }
        assert!(total > 60, "{total}");
    }

    #[test]
    fn le_relief_reste_praticable() {
        let c = Campagne { graine: 1 };
        let t = c.tuile(0, 0);
        assert!(t.relief.h.iter().all(|h| h.abs() < 25.0));
        let p = t.relief.pas();
        let raide = (0..GRILLE).flat_map(|b| (0..GRILLE).map(move |a| (a, b))).map(|(a, b)| ((t.relief.sommet(a + 1, b) - t.relief.sommet(a, b)) / p).abs()).fold(0.0, f32::max);
        assert!(raide < 0.8, "pente au plus {raide}");
    }
}
