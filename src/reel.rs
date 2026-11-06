//! Un lieu réel : le relief de l'IGN et les surfaces d'eau d'OpenStreetMap, préparés
//! pour toute la carte de 20 km par `examples/lieu_reel.rs`, et relus par le jeu
//! (`monde/lieu.reel`).
//!
//! - La carte se cale en Lambert-93 (EPSG:2154, en mètres) : le point (x, z) du jeu est
//!   en X = `x0 + x`, Y = `y0 - z` (x vers l'est, z vers le sud).
//! - Le relief est une grille de `pas` mètres qui couvre la carte, comme celle de
//!   l'érosion, les hauteurs sur 16 bits (`base + v × echelle`) ; il se lit en douceur
//!   (Catmull-Rom).
//! - L'eau est un masque d'un bit par point ; sa surface est à `niveau`, et son fond déjà
//!   creusé dans le relief, plus profond loin des berges.
//!
//! Sources : relief © IGN (RGE ALTI, Licence Ouverte 2.0) ; eau © les contributeurs
//! OpenStreetMap (ODbL). Un fichier préparé est une base de données dérivée d'OSM : il
//! reste sous ODbL, et ne va pas dans le dépôt.

use crate::moteur::tuiles::DEMI_CARTE;
use crate::moteur::TAILLE;

const MAGIQUE: &[u8; 8] = b"SAGEREEL";
const FORMAT: u32 = 1;

/// Le texte à citer.
pub const SOURCES: &str = "Relief © IGN (RGE ALTI) · eau © les contributeurs OpenStreetMap";

#[derive(Clone, Debug, PartialEq)]
pub struct Reel {
    pub nom: String,
    /// Le centre demandé, en degrés.
    pub lat: f64,
    pub lon: f64,
    /// Lambert-93 du point (0, 0) du jeu.
    pub x0: f64,
    pub y0: f64,
    pub pas: f64,
    pub cote: usize,
    /// La surface de l'eau, en mètres.
    pub niveau: f32,
    pub base: f32,
    pub echelle: f32,
    pub h: Vec<u16>,
    /// Un bit par point : de l'eau.
    pub eau: Vec<u8>,
}

/// Le coin de la grille (au repère du jeu) : le coin de la première tuile de la carte.
pub fn origine() -> f64 {
    -(DEMI_CARTE as f64) * TAILLE as f64
}

/// Points de la grille par côté, pour un pas de `pas` mètres.
pub fn cote(pas: f64) -> usize {
    ((2 * DEMI_CARTE + 1) as f64 * TAILLE as f64 / pas).round() as usize + 1
}

/// Lambert-93 (X, Y) d'un point en degrés, sur l'ellipsoïde GRS 80 (formules de l'IGN).
pub fn lambert93(lat: f64, lon: f64) -> (f64, f64) {
    const E: f64 = 0.081_819_191_042_815_8;
    const N: f64 = 0.725_607_765_053_267;
    const C: f64 = 11_754_255.426_096;
    const XS: f64 = 700_000.0;
    const YS: f64 = 12_655_612.049_876;
    let p = lat.to_radians();
    let s = p.sin();
    let l = ((std::f64::consts::FRAC_PI_4 + p / 2.0).tan() * ((1.0 - E * s) / (1.0 + E * s)).powf(E / 2.0)).ln();
    let r = C * (-N * l).exp();
    let g = N * (lon - 3.0).to_radians();
    (XS + r * g.sin(), YS - r * g.cos())
}

fn cr(a: f64, b: f64, c: f64, d: f64, t: f64) -> f64 {
    b + 0.5 * t * (c - a + t * (2.0 * a - 5.0 * b + 4.0 * c - d + t * (3.0 * (b - c) + d - a)))
}

impl Reel {
    /// Les hauteurs `h` (en mètres) et l'eau, d'une grille de `cote(pas)` points ; les
    /// hauteurs s'encodent sur 16 bits.
    #[allow(clippy::too_many_arguments)]
    pub fn new(nom: &str, lat: f64, lon: f64, x0: f64, y0: f64, pas: f64, niveau: f32, h: &[f32], eau: &[bool]) -> Reel {
        let n = cote(pas);
        assert!(h.len() == n * n && eau.len() == n * n);
        let lo = h.iter().cloned().fold(f32::MAX, f32::min);
        let hi = h.iter().cloned().fold(f32::MIN, f32::max);
        let echelle = ((hi - lo) / 65535.0).max(0.01);
        let hv = h.iter().map(|v| ((v - lo) / echelle).round().clamp(0.0, 65535.0) as u16).collect();
        let mut bits = vec![0u8; (n * n).div_ceil(8)];
        for (i, _) in eau.iter().enumerate().filter(|(_, e)| **e) {
            bits[i / 8] |= 1 << (i % 8);
        }
        Reel { nom: nom.into(), lat, lon, x0, y0, pas, cote: n, niveau, base: lo, echelle, h: hv, eau: bits }
    }

    fn point(&self, a: i64, b: i64) -> f64 {
        let n = self.cote as i64;
        let i = (b.clamp(0, n - 1) * n + a.clamp(0, n - 1)) as usize;
        (self.base + self.h[i] as f32 * self.echelle) as f64
    }

    /// Le point (a, b) de la grille est-il dans l'eau ?
    pub fn est_eau(&self, a: i64, b: i64) -> bool {
        let n = self.cote as i64;
        if a < 0 || b < 0 || a >= n || b >= n {
            return false;
        }
        let i = (b * n + a) as usize;
        self.eau[i / 8] & (1 << (i % 8)) != 0
    }

    /// La hauteur du sol en (x, z) du jeu.
    pub fn hauteur(&self, x: f64, z: f64) -> f64 {
        let (gx, gz) = ((x - origine()) / self.pas, (z - origine()) / self.pas);
        let (a, b) = (gx.floor(), gz.floor());
        let (u, v) = (gx - a, gz - b);
        let (a, b) = (a as i64, b as i64);
        let l = |j: i64| cr(self.point(a - 1, j), self.point(a, j), self.point(a + 1, j), self.point(a + 2, j), u);
        cr(l(b - 1), l(b), l(b + 1), l(b + 2), v)
    }

    /// La part d'eau en (x, z), de 0 à 1 : 1 au milieu de la Seine, entre deux sur la
    /// berge.
    pub fn eau(&self, x: f64, z: f64) -> f64 {
        let (gx, gz) = ((x - origine()) / self.pas, (z - origine()) / self.pas);
        let (a, b) = (gx.floor(), gz.floor());
        let (u, v) = (gx - a, gz - b);
        let (a, b) = (a as i64, b as i64);
        let e = |a: i64, b: i64| if self.est_eau(a, b) { 1.0 } else { 0.0 };
        (e(a, b) * (1.0 - u) + e(a + 1, b) * u) * (1.0 - v) + (e(a, b + 1) * (1.0 - u) + e(a + 1, b + 1) * u) * v
    }

    pub fn octets(&self) -> Vec<u8> {
        let mut o = Vec::with_capacity(96 + self.h.len() * 2 + self.eau.len());
        o.extend_from_slice(MAGIQUE);
        o.extend_from_slice(&FORMAT.to_le_bytes());
        let nom = self.nom.as_bytes();
        o.extend_from_slice(&(nom.len() as u32).to_le_bytes());
        o.extend_from_slice(nom);
        for v in [self.lat, self.lon, self.x0, self.y0, self.pas] {
            o.extend_from_slice(&v.to_le_bytes());
        }
        o.extend_from_slice(&(self.cote as u32).to_le_bytes());
        for v in [self.niveau, self.base, self.echelle] {
            o.extend_from_slice(&v.to_le_bytes());
        }
        for v in &self.h {
            o.extend_from_slice(&v.to_le_bytes());
        }
        o.extend_from_slice(&self.eau);
        o
    }

    /// Relu ; rien si ce n'est pas un lieu réel de ce format, ou pour une autre carte.
    pub fn depuis_octets(o: &[u8]) -> Option<Reel> {
        if o.get(..8)? != MAGIQUE {
            return None;
        }
        let mut k = 8;
        let mut prendre = |n: usize| -> Option<&[u8]> {
            let s = o.get(k..k + n)?;
            k += n;
            Some(s)
        };
        let u32_ = |s: &[u8]| u32::from_le_bytes(s.try_into().unwrap());
        if u32_(prendre(4)?) != FORMAT {
            return None;
        }
        let l = u32_(prendre(4)?) as usize;
        let nom = String::from_utf8(prendre(l)?.to_vec()).ok()?;
        let mut f = [0f64; 5];
        for v in &mut f {
            *v = f64::from_le_bytes(prendre(8)?.try_into().ok()?);
        }
        let [lat, lon, x0, y0, pas] = f;
        let n = u32_(prendre(4)?) as usize;
        if !(pas > 0.0) || n != cote(pas) {
            return None;
        }
        let mut g = [0f32; 3];
        for v in &mut g {
            *v = f32::from_le_bytes(prendre(4)?.try_into().ok()?);
        }
        let [niveau, base, echelle] = g;
        let h = prendre(n * n * 2)?.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let eau = prendre((n * n).div_ceil(8))?.to_vec();
        Some(Reel { nom, lat, lon, x0, y0, pas, cote: n, niveau, base, echelle, h, eau })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lambert93_tombe_juste() {
        // L'origine de la projection, et Rouen (la cathédrale)
        let (x, y) = lambert93(46.5, 3.0);
        assert!((x - 700_000.0).abs() < 1e-6 && (y - 6_600_000.0).abs() < 1e-6);
        let (x, y) = lambert93(49.4402, 1.0951);
        assert!((x - 561_810.0).abs() < 5.0 && (y - 6_928_419.0).abs() < 5.0, "{x} {y}");
    }

    #[test]
    fn un_lieu_se_relit_et_se_lit_en_douceur() {
        let pas = 64.0;
        let n = cote(pas);
        // Une pente vers l'est, de l'eau dans la moitié ouest
        let h: Vec<f32> = (0..n * n).map(|i| (i % n) as f32 * 2.0).collect();
        let eau: Vec<bool> = (0..n * n).map(|i| i % n < n / 2).collect();
        let r = Reel::new("Essai", 49.0, 1.0, 1000.0, 2000.0, pas, 3.0, &h, &eau);
        let o = r.octets();
        assert_eq!(Reel::depuis_octets(&o), Some(r.clone()));
        assert!(Reel::depuis_octets(&o[..o.len() - 1]).is_none());
        // Sur un point, la hauteur du point ; entre deux, entre les deux
        let x = origine() + 10.0 * pas;
        assert!((r.hauteur(x, 0.0) - 20.0).abs() < 0.02);
        assert!((r.hauteur(x + pas / 2.0, 0.0) - 21.0).abs() < 0.02);
        assert_eq!(r.eau(origine() + pas, 0.0), 1.0);
        assert_eq!(r.eau(-origine() - pas, 0.0), 0.0);
    }
}
