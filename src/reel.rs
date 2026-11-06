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
//! - Les routes sont des lignes (x, z du jeu, et la hauteur de la chaussée, lissée le long
//!   de la route) avec un genre et une largeur. Sur leur largeur, le sol prend la hauteur
//!   de la chaussée et sa couleur ; sur les bas-côtés (`EPAULE`), il rejoint le relief.
//!   Les ponts et les tunnels n'y sont pas.
//!
//! Sources : relief © IGN (RGE ALTI, Licence Ouverte 2.0) ; eau © les contributeurs
//! OpenStreetMap (ODbL). Un fichier préparé est une base de données dérivée d'OSM : il
//! reste sous ODbL, et ne va pas dans le dépôt.

use crate::moteur::tuiles::DEMI_CARTE;
use crate::moteur::TAILLE;

const MAGIQUE: &[u8; 8] = b"SAGEREEL";
const FORMAT: u32 = 2;

/// Le texte à citer.
pub const SOURCES: &str = "Relief © IGN (RGE ALTI) · eau et routes © les contributeurs OpenStreetMap";

/// Les genres de routes : chacun sa couleur.
pub const GRANDE: u8 = 0;
pub const ROUTE: u8 = 1;
pub const SERVICE: u8 = 2;
pub const PIETONNE: u8 = 3;
pub const CHEMIN: u8 = 4;
pub const SENTIER: u8 = 5;

/// La couleur d'une route de ce genre.
pub fn teinte(genre: u8) -> [f32; 3] {
    match genre {
        GRANDE => [0.27, 0.27, 0.29],
        ROUTE => [0.32, 0.32, 0.34],
        SERVICE => [0.38, 0.38, 0.39],
        PIETONNE => [0.60, 0.58, 0.54],
        CHEMIN => [0.56, 0.48, 0.37],
        _ => [0.52, 0.45, 0.34],
    }
}

/// La largeur des bas-côtés, où le sol rejoint le relief, selon le genre.
pub fn epaule(genre: u8) -> f32 {
    if genre <= SERVICE { 3.0 } else { 1.5 }
}

/// Côté des cases de l'index des routes, en mètres.
const CASE: f32 = 32.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub genre: u8,
    pub largeur: f32,
    /// x, z du jeu, et la hauteur de la chaussée.
    pub points: Vec<[f32; 3]>,
}

/// Le point de route le plus proche : son genre, sa largeur, la distance à l'axe et la
/// hauteur de la chaussée.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Proche {
    pub genre: u8,
    pub largeur: f32,
    pub distance: f32,
    pub hauteur: f32,
}

impl Proche {
    /// De combien on est hors de la chaussée (négatif : dessus).
    pub fn hors(&self) -> f32 {
        self.distance - self.largeur / 2.0
    }
}

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
    pub routes: Vec<Route>,
    /// Pour chaque case, les segments (route, premier point) qui l'approchent.
    index: std::collections::HashMap<(i32, i32), Vec<(u32, u32)>>,
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
    pub fn new(nom: &str, lat: f64, lon: f64, x0: f64, y0: f64, pas: f64, niveau: f32, h: &[f32], eau: &[bool], routes: Vec<Route>) -> Reel {
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
        Reel { nom: nom.into(), lat, lon, x0, y0, pas, cote: n, niveau, base: lo, echelle, h: hv, eau: bits, routes, index: Default::default() }.indexe()
    }

    /// Range chaque segment de route dans les cases qu'il approche.
    fn indexe(mut self) -> Reel {
        let mut index: std::collections::HashMap<(i32, i32), Vec<(u32, u32)>> = Default::default();
        for (k, r) in self.routes.iter().enumerate() {
            let m = r.largeur / 2.0 + epaule(r.genre);
            for (i, s) in r.points.windows(2).enumerate() {
                let (x0, x1) = (s[0][0].min(s[1][0]) - m, s[0][0].max(s[1][0]) + m);
                let (z0, z1) = (s[0][1].min(s[1][1]) - m, s[0][1].max(s[1][1]) + m);
                for cz in (z0 / CASE).floor() as i32..=(z1 / CASE).floor() as i32 {
                    for cx in (x0 / CASE).floor() as i32..=(x1 / CASE).floor() as i32 {
                        index.entry((cx, cz)).or_default().push((k as u32, i as u32));
                    }
                }
            }
        }
        self.index = index;
        self
    }

    /// La route la plus proche de (x, z), si l'on est sur elle ou ses bas-côtés ; la plus
    /// large l'emporte (on compte depuis son bord).
    pub fn route(&self, x: f64, z: f64) -> Option<Proche> {
        let (x, z) = (x as f32, z as f32);
        let l = self.index.get(&((x / CASE).floor() as i32, (z / CASE).floor() as i32))?;
        let mut mieux: Option<Proche> = None;
        for &(k, i) in l {
            let r = &self.routes[k as usize];
            let (a, b) = (r.points[i as usize], r.points[i as usize + 1]);
            let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
            let l2 = dx * dx + dz * dz;
            let t = if l2 > 0.0 { (((x - a[0]) * dx + (z - a[1]) * dz) / l2).clamp(0.0, 1.0) } else { 0.0 };
            let d = (x - a[0] - dx * t).hypot(z - a[1] - dz * t);
            let p = Proche { genre: r.genre, largeur: r.largeur, distance: d, hauteur: a[2] + (b[2] - a[2]) * t };
            if p.hors() < epaule(r.genre) && mieux.is_none_or(|m| p.hors() < m.hors()) {
                mieux = Some(p);
            }
        }
        mieux
    }

    /// La hauteur du relief seul, sans les routes.
    pub fn relief(&self, x: f64, z: f64) -> f64 {
        let (gx, gz) = ((x - origine()) / self.pas, (z - origine()) / self.pas);
        let (a, b) = (gx.floor(), gz.floor());
        let (u, v) = (gx - a, gz - b);
        let (a, b) = (a as i64, b as i64);
        let l = |j: i64| cr(self.point(a - 1, j), self.point(a, j), self.point(a + 1, j), self.point(a + 2, j), u);
        cr(l(b - 1), l(b), l(b + 1), l(b + 2), v)
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

    /// La hauteur du sol en (x, z) du jeu : le relief, aplani sous les routes.
    pub fn hauteur(&self, x: f64, z: f64) -> f64 {
        let h = self.relief(x, z);
        match self.route(x, z) {
            Some(p) => {
                let t = (p.hors() / epaule(p.genre)).clamp(0.0, 1.0) as f64;
                let t = t * t * (3.0 - 2.0 * t);
                p.hauteur as f64 + (h - p.hauteur as f64) * t
            }
            None => h,
        }
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
        o.extend_from_slice(&(self.routes.len() as u32).to_le_bytes());
        for r in &self.routes {
            o.push(r.genre);
            o.extend_from_slice(&r.largeur.to_le_bytes());
            o.extend_from_slice(&(r.points.len() as u32).to_le_bytes());
            for p in &r.points {
                for v in p {
                    o.extend_from_slice(&v.to_le_bytes());
                }
            }
        }
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
        let format = u32_(prendre(4)?);
        if !(1..=FORMAT).contains(&format) {
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
        let mut routes = Vec::new();
        if format >= 2 {
            let f32_ = |s: &[u8]| f32::from_le_bytes(s.try_into().unwrap());
            for _ in 0..u32_(prendre(4)?) {
                let genre = prendre(1)?[0];
                let largeur = f32_(prendre(4)?);
                let m = u32_(prendre(4)?) as usize;
                let o = prendre(m * 12)?;
                let points = o.chunks(12).map(|c| [f32_(&c[0..4]), f32_(&c[4..8]), f32_(&c[8..12])]).collect();
                routes.push(Route { genre, largeur, points });
            }
        }
        if k != o.len() {
            return None;
        }
        Some(Reel { nom, lat, lon, x0, y0, pas, cote: n, niveau, base, echelle, h, eau, routes, index: Default::default() }.indexe())
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
        let r = Reel::new("Essai", 49.0, 1.0, 1000.0, 2000.0, pas, 3.0, &h, &eau, Vec::new());
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

    #[test]
    fn une_route_aplanit_le_sol_et_se_relit() {
        let pas = 64.0;
        let n = cote(pas);
        // Un relief en pente vers l'est (10 cm par mètre), une route nord-sud à 5 m
        let h: Vec<f32> = (0..n * n).map(|i| (origine() + (i % n) as f64 * pas) as f32 * 0.1).collect();
        let route = Route { genre: ROUTE, largeur: 6.0, points: vec![[0.0, -200.0, 5.0], [0.0, 200.0, 5.0]] };
        let r = Reel::new("Essai", 49.0, 1.0, 0.0, 0.0, pas, -50.0, &h, &vec![false; n * n], vec![route]);
        // Sur la chaussée, sa hauteur ; au-delà des bas-côtés, le relief
        for x in [-2.9, 0.0, 2.9] {
            assert!((r.hauteur(x, 10.0) - 5.0).abs() < 1e-4, "{x}");
        }
        assert!((r.hauteur(7.0, 10.0) - r.relief(7.0, 10.0)).abs() < 1e-6);
        let mi = r.hauteur(4.5, 10.0);
        assert!(mi < 5.0 && mi > r.relief(4.5, 10.0), "{mi}");
        assert_eq!(r.route(0.0, 10.0).map(|p| p.genre), Some(ROUTE));
        assert!(r.route(30.0, 10.0).is_none());
        let o = r.octets();
        assert_eq!(Reel::depuis_octets(&o), Some(r));
    }
}
