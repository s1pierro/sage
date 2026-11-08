//! Les revêtements : de petites textures génériques, faites au lancement, qui se
//! répètent sur les surfaces (les trottoirs, pour commencer).
//!
//! - le bitume (`BITUME`) : gris sombre grenu, `METRES_BITUME` mètres par répétition, posé
//!   à plat dans le repère du monde ;
//! - les pavés (`PAVES`) : un seul rang de `PAVES_PAR_IMAGE` pavés clairs ; le long de la
//!   rue (u), une répétition fait `PAVES_PAR_IMAGE` fois la longueur d'un pavé, au choix
//!   de qui la pose ; en travers (v, de 0 à 1), la largeur de la bande qui les porte.

use crate::image::Image;

/// L'indice du bitume parmi les revêtements (`moteur::REVETEMENTS`).
pub const BITUME: usize = 0;
/// L'indice des pavés.
pub const PAVES: usize = 1;
/// L'indice des surfaces sans texture (la couleur des sommets seule) : les chaussées, les
/// accotements, les chants ; elles vont avec les revêtus pour que la voirie se dessine à
/// part des bâtiments.
pub const UNI: usize = 2;

/// Les mètres couverts par une répétition de chaque texture.
pub const METRES_BITUME: f32 = 2.0;
/// Les pavés d'une répétition de la texture des pavés.
pub const PAVES_PAR_IMAGE: u32 = 4;

/// Le côté des images, en pixels.
const COTE: u32 = 128;

/// Les identifiants des textures, dans l'ordre des indices.
pub const IDENTIFIANTS: [&str; 2] = ["revetement/bitume", "revetement/paves"];

/// Les images, dans l'ordre des indices.
pub fn images() -> [Image; 2] {
    [bitume(), paves()]
}

/// Un hasard fixe tiré de trois entiers, dans [0, 1).
fn hasard(a: u32, b: u32, c: u32) -> f32 {
    let mut h = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA77) ^ c.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// Un bruit doux qui se répète sur `periode` cases (l'image se raccorde à elle-même).
fn bruit(x: f32, y: f32, periode: u32, graine: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let c = |a: f32, b: f32| hasard((a as i32).rem_euclid(periode as i32) as u32, (b as i32).rem_euclid(periode as i32) as u32, graine);
    let (h00, h10, h01, h11) = (c(x0, y0), c(x0 + 1.0, y0), c(x0, y0 + 1.0), c(x0 + 1.0, y0 + 1.0));
    (h00 * (1.0 - sx) + h10 * sx) * (1.0 - sy) + (h01 * (1.0 - sx) + h11 * sx) * sy
}

fn octet(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Le bitume : un gris sombre, des nuances larges, du grain et des gravillons clairs.
pub fn bitume() -> Image {
    let mut rgba = Vec::with_capacity((COTE * COTE * 4) as usize);
    for y in 0..COTE {
        for x in 0..COTE {
            let (fx, fy) = (x as f32, y as f32);
            let large = bruit(fx / 32.0, fy / 32.0, COTE / 32, 1) - 0.5;
            let moyen = bruit(fx / 8.0, fy / 8.0, COTE / 8, 2) - 0.5;
            let grain = hasard(x, y, 3) - 0.5;
            let mut g = 0.25 + large * 0.05 + moyen * 0.04 + grain * 0.07;
            // Les gravillons : quelques pixels plus clairs, d'autres plus sombres
            let p = hasard(x, y, 4);
            if p > 0.965 {
                g += 0.12 + hasard(x, y, 5) * 0.08;
            } else if p < 0.03 {
                g -= 0.07;
            }
            rgba.extend_from_slice(&[octet(g), octet(g * 0.99), octet(g * 1.02), 255]);
        }
    }
    Image { l: COTE, h: COTE, rgba }
}

/// Les pavés : un rang de `PAVES_PAR_IMAGE` pavés clairs (256 pixels), aux joints sombres en bout et sur les deux rives ; chaque pavé sa nuance.
pub fn paves() -> Image {
    let (l, h) = (256u32, 32u32);
    let long = l / PAVES_PAR_IMAGE;
    let joint = 2;
    let mut rgba = Vec::with_capacity((l * h * 4) as usize);
    for y in 0..h {
        for x in 0..l {
            let (n, dx) = (x / long, x % long);
            let (r, g, b) = if dx < joint || y < 1 || y + 1 >= h {
                let s = 0.36 + (hasard(x, y, 6) - 0.5) * 0.06;
                (s, s * 0.98, s * 0.95)
            } else {
                // La nuance du pavé, son grain, et ses arêtes un peu usées
                let nuance = (hasard(n, 0, 7) - 0.5) * 0.10;
                let grain = (hasard(x, y, 8) - 0.5) * 0.06 + (bruit(x as f32 / 4.0, y as f32 / 4.0, l / 4, 9) - 0.5) * 0.05;
                let bord = dx.min(long - 1 - dx).min(y).min(h - 1 - y);
                let arete = if bord < joint + 2 { -0.05 } else { 0.0 };
                let v = 0.72 + nuance + grain + arete;
                (v, v * 0.975, v * 0.92)
            };
            rgba.extend_from_slice(&[octet(r), octet(g), octet(b), 255]);
        }
    }
    Image { l, h, rgba }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_images_sont_pleines_et_opaques() {
        for i in images() {
            assert_eq!(i.rgba.len(), (i.l * i.h * 4) as usize);
            assert!(!i.transparente());
        }
    }

    #[test]
    fn le_bitume_est_sombre_et_les_paves_clairs() {
        let moyenne = |i: &Image| i.rgba.chunks_exact(4).map(|p| p[0] as f32).sum::<f32>() / (i.l * i.h) as f32 / 255.0;
        assert!(moyenne(&bitume()) < 0.35);
        assert!(moyenne(&paves()) > 0.55);
    }
}
