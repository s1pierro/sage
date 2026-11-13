//! Les lieux réels de S.A.G.E. : un morceau de France de 20 km de côté, préparé d'après
//! le relief de l'IGN et OpenStreetMap, dans le format `.reel` (« SAGEREEL »).
//!
//! Cette crate est partagée : S.A.G.E. y lit ses lieux (`sage::reel` les monte en
//! tuiles), son outil `lieu_reel` et Flow (le contexte Lieux) les génèrent
//! (`generer`), Flow les explore et les édite.
//!
//! - La carte se cale en Lambert-93 (EPSG:2154, en mètres) : le point (x, z) du jeu est
//!   en X = `x0 + x`, Y = `y0 - z` (x vers l'est, z vers le sud).
//! - Le relief est une grille de `pas` mètres qui couvre la carte, les hauteurs sur 16
//!   bits (`base + v × echelle`) ; il se lit en douceur (Catmull-Rom).
//! - L'eau est un masque d'un bit par point ; sa surface est à `niveau`, et son fond déjà
//!   creusé dans le relief, plus profond loin des berges.
//! - Les routes sont des lignes (x, z du jeu, et la hauteur de la chaussée) avec un
//!   genre et une largeur ; les ponts, des lignes à la hauteur du tablier.
//! - Les bâtiments : un contour (x, z du jeu), un pied, un égout et un faîtage (hauteurs
//!   absolues), la forme du toit et son axe (`toits`), un genre.
//! - L'occupation du sol (bois, prés et parcs, champs, quartiers habités, zones
//!   d'activité) : une classe par point d'une grille deux fois plus lâche que le relief.
//!
//! Sources : relief © IGN (RGE ALTI, LiDAR HD, Licence Ouverte 2.0) ; eau, routes,
//! bâtiments et occupation © les contributeurs OpenStreetMap (ODbL). Un fichier préparé
//! est une base de données dérivée d'OSM : il reste sous ODbL, et ne va dans aucun dépôt.

pub mod generer;
pub mod json;
pub mod toits;

/// Le côté d'une tuile du jeu (m) et le nombre de tuiles de part et d'autre du centre :
/// la carte fait `2 × DEMI_CARTE + 1` tuiles de côté (20 km). Ceux du moteur de S.A.G.E.
pub const TAILLE: f32 = 64.0;
pub const DEMI_CARTE: i32 = 156;
/// Le point de départ du jeu, où tombe le centre demandé d'un lieu.
pub const DEPART: [f32; 2] = [TAILLE / 2.0, TAILLE / 2.0];

pub const MAGIQUE: &[u8; 8] = b"SAGEREEL";
pub const FORMAT: u32 = 6;

/// Le texte à citer.
pub const SOURCES: &str = "Relief et hauteurs © IGN (RGE ALTI, LiDAR HD) · eau, routes, bâtiments et occupation du sol © les contributeurs OpenStreetMap";

/// Les classes d'occupation du sol (0 : rien de dit, de l'herbe).
pub const BOIS: u8 = 1;
pub const PRE: u8 = 2;
pub const CHAMP: u8 = 3;
pub const HABITE: u8 = 4;
pub const ZONE: u8 = 5;
pub const CLASSES: u8 = 6;

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

/// Les genres de bâtiments.
pub const MAISON: u8 = 0;
pub const IMMEUBLE: u8 = 1;
pub const INDUSTRIEL: u8 = 2;
pub const EDIFICE: u8 = 3;
pub const ANNEXE: u8 = 4;

/// Les couleurs d'un bâtiment de ce genre : ses murs, son toit.
pub fn teintes_bati(genre: u8) -> ([f32; 3], [f32; 3]) {
    match genre {
        MAISON => ([0.84, 0.80, 0.72], [0.34, 0.35, 0.40]),
        IMMEUBLE => ([0.78, 0.76, 0.72], [0.40, 0.40, 0.42]),
        INDUSTRIEL => ([0.64, 0.66, 0.68], [0.50, 0.52, 0.54]),
        EDIFICE => ([0.76, 0.72, 0.64], [0.36, 0.38, 0.40]),
        _ => ([0.70, 0.66, 0.60], [0.45, 0.40, 0.36]),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Batiment {
    pub genre: u8,
    /// Le pied des murs (un peu sous le sol), le haut du toit (le faîtage) et l'égout, en
    /// hauteurs absolues.
    pub pied: f32,
    pub toit: f32,
    pub egout: f32,
    /// La forme du toit (`toits::PLAT`…) et son axe (radians, dans le plan x, z).
    pub forme: u8,
    pub axe: f32,
    /// Le contour, x, z du jeu, sans répéter le premier point.
    pub contour: Vec<[f32; 2]>,
}

impl Batiment {
    /// Un bâtiment à toit plat.
    pub fn plat(genre: u8, pied: f32, toit: f32, contour: Vec<[f32; 2]>) -> Batiment {
        Batiment { genre, pied, toit, egout: toit, forme: crate::toits::PLAT, axe: 0.0, contour }
    }

    /// Le centre du contour (la moyenne de ses points).
    pub fn centre(&self) -> [f32; 2] {
        let n = self.contour.len().max(1) as f32;
        let s = self.contour.iter().fold([0.0, 0.0], |a, p| [a[0] + p[0], a[1] + p[1]]);
        [s[0] / n, s[1] / n]
    }
}

/// L'aire signée d'un contour (positive : sens inverse des aiguilles d'une montre, x vers
/// la droite et z vers le bas, vu d'en haut).
pub fn aire(c: &[[f32; 2]]) -> f32 {
    let mut s = 0.0;
    for i in 0..c.len() {
        let (a, b) = (c[i], c[(i + 1) % c.len()]);
        s += a[0] * b[1] - b[0] * a[1];
    }
    s / 2.0
}

/// Les triangles d'un polygone simple (oreilles), en indices, dans le sens de `c`.
pub fn trianguler(c: &[[f32; 2]]) -> Vec<[usize; 3]> {
    let n = c.len();
    if n < 3 {
        return Vec::new();
    }
    let sens = aire(c).signum();
    let mut reste: Vec<usize> = (0..n).collect();
    let mut t = Vec::with_capacity(n - 2);
    let croix = |a: [f32; 2], b: [f32; 2], p: [f32; 2]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
    let mut garde = 0;
    while reste.len() > 3 && garde < reste.len() {
        let m = reste.len();
        let mut coupe = false;
        for k in 0..m {
            let (i0, i1, i2) = (reste[(k + m - 1) % m], reste[k], reste[(k + 1) % m]);
            let (a, b, d) = (c[i0], c[i1], c[i2]);
            if croix(a, b, d) * sens <= 0.0 {
                continue;
            }
            let dedans = reste.iter().any(|&j| {
                if j == i0 || j == i1 || j == i2 {
                    return false;
                }
                let p = c[j];
                croix(a, b, p) * sens > 0.0 && croix(b, d, p) * sens > 0.0 && croix(d, a, p) * sens > 0.0
            });
            if !dedans {
                t.push([i0, i1, i2]);
                reste.remove(k);
                coupe = true;
                break;
            }
        }
        garde = if coupe { 0 } else { garde + 1 };
        if !coupe {
            // Un contour croisé : le reste en éventail
            break;
        }
    }
    for k in 1..reste.len().saturating_sub(1) {
        t.push([reste[0], reste[k], reste[k + 1]]);
    }
    t
}

#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub genre: u8,
    pub largeur: f32,
    /// x, z du jeu, et la hauteur de la chaussée.
    pub points: Vec<[f32; 3]>,
}

/// Le côté de la grille de l'occupation du sol pour un relief de `n` points de côté.
pub fn cote_sol(n: usize) -> usize {
    (n - 1) / 2 + 1
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

fn ecrire_routes(o: &mut Vec<u8>, routes: &[Route]) {
    o.extend_from_slice(&(routes.len() as u32).to_le_bytes());
    for r in routes {
        o.push(r.genre);
        o.extend_from_slice(&r.largeur.to_le_bytes());
        o.extend_from_slice(&(r.points.len() as u32).to_le_bytes());
        for p in &r.points {
            for v in p {
                o.extend_from_slice(&v.to_le_bytes());
            }
        }
    }
}

fn cr(a: f64, b: f64, c: f64, d: f64, t: f64) -> f64 {
    b + 0.5 * t * (c - a + t * (2.0 * a - 5.0 * b + 4.0 * c - d + t * (3.0 * (b - c) + d - a)))
}

/// Le point (latitude, longitude) en degrés d'un point Lambert-93 (X, Y) : l'inverse de
/// `lambert93`, par itération sur la latitude isométrique (formules de l'IGN).
pub fn lambert93_inverse(x: f64, y: f64) -> (f64, f64) {
    const E: f64 = 0.081_819_191_042_815_8;
    const N: f64 = 0.725_607_765_053_267;
    const C: f64 = 11_754_255.426_096;
    const XS: f64 = 700_000.0;
    const YS: f64 = 12_655_612.049_876;
    let (dx, dy) = (x - XS, y - YS);
    let r = dx.hypot(dy);
    let g = (dx / -dy).atan();
    let lon = 3.0 + (g / N).to_degrees();
    let l = -(r / C).ln() / N;
    let mut p = 2.0 * l.exp().atan() - std::f64::consts::FRAC_PI_2;
    for _ in 0..12 {
        let s = p.sin();
        let q = 2.0 * (l.exp() * ((1.0 + E * s) / (1.0 - E * s)).powf(E / 2.0)).atan() - std::f64::consts::FRAC_PI_2;
        if (q - p).abs() < 1e-12 {
            p = q;
            break;
        }
        p = q;
    }
    (p.to_degrees(), lon)
}

/// Un lieu réel tel qu'il s'écrit : ses données, sans rien de calculé pour le jeu. Les
/// routes et les ponts sont ceux du fichier (le jeu les terrasse à part).
#[derive(Clone, Debug, PartialEq)]
pub struct Lieu {
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
    pub batiments: Vec<Batiment>,
    pub ponts: Vec<Route>,
    /// L'occupation du sol, une classe par point de la grille de `2 × pas` (`cote_sol`
    /// points de côté) ; vide : rien de dit.
    pub sol: Vec<u8>,
}

/// L'échelle et la base qui encodent sur 16 bits des hauteurs de `lo` à `hi`.
fn quantum(lo: f32, hi: f32) -> f32 {
    ((hi - lo) / 65535.0).max(0.01)
}

impl Lieu {
    /// Les hauteurs `h` (en mètres) et l'eau, d'une grille de `cote(pas)` points ; les
    /// hauteurs s'encodent sur 16 bits.
    #[allow(clippy::too_many_arguments)]
    pub fn new(nom: &str, lat: f64, lon: f64, x0: f64, y0: f64, pas: f64, niveau: f32, h: &[f32], eau: &[bool], routes: Vec<Route>, batiments: Vec<Batiment>, ponts: Vec<Route>) -> Lieu {
        let n = cote(pas);
        assert!(h.len() == n * n && eau.len() == n * n);
        let mut bits = vec![0u8; (n * n).div_ceil(8)];
        for (i, _) in eau.iter().enumerate().filter(|(_, e)| **e) {
            bits[i / 8] |= 1 << (i % 8);
        }
        let mut l = Lieu { nom: nom.into(), lat, lon, x0, y0, pas, cote: n, niveau, base: 0.0, echelle: 1.0, h: Vec::new(), eau: bits, routes, batiments, ponts, sol: Vec::new() };
        l.fixer_hauteurs(h);
        l
    }

    /// Un lieu sans rien : un relief plat à `altitude`, sans eau (son niveau 10 m plus
    /// bas), centré sur (lat, lon).
    pub fn vierge(nom: &str, lat: f64, lon: f64, pas: f64, altitude: f32) -> Lieu {
        let n = cote(pas);
        let (xc, yc) = lambert93(lat, lon);
        let (x0, y0) = (xc - DEPART[0] as f64, yc + DEPART[1] as f64);
        let m = cote_sol(n);
        let mut l = Lieu::new(nom, lat, lon, x0, y0, pas, altitude - 10.0, &vec![altitude; n * n], &vec![false; n * n], Vec::new(), Vec::new(), Vec::new());
        l.sol = vec![0; m * m];
        l
    }

    /// Avec son occupation du sol (`cote_sol(cote)²` classes).
    pub fn avec_sol(mut self, sol: Vec<u8>) -> Lieu {
        let m = cote_sol(self.cote);
        assert_eq!(sol.len(), m * m);
        self.sol = sol;
        self
    }

    /// Toutes les hauteurs, en mètres.
    pub fn hauteurs(&self) -> Vec<f32> {
        self.h.iter().map(|v| self.base + *v as f32 * self.echelle).collect()
    }

    /// Remplace toutes les hauteurs (en mètres), réencodées sur 16 bits.
    pub fn fixer_hauteurs(&mut self, h: &[f32]) {
        let lo = h.iter().cloned().fold(f32::MAX, f32::min);
        let hi = h.iter().cloned().fold(f32::MIN, f32::max);
        let echelle = quantum(lo, hi);
        self.base = lo;
        self.echelle = echelle;
        self.h = h.iter().map(|v| ((v - lo) / echelle).round().clamp(0.0, 65535.0) as u16).collect();
    }

    /// La hauteur du point (a, b) de la grille, en mètres (bornée à la grille).
    pub fn h_point(&self, a: i64, b: i64) -> f32 {
        let n = self.cote as i64;
        let i = (b.clamp(0, n - 1) * n + a.clamp(0, n - 1)) as usize;
        self.base + self.h[i] as f32 * self.echelle
    }

    /// Change des hauteurs de la grille : `f` reçoit chaque point (a, b) du carré
    /// `a0..=a1`, `b0..=b1` et sa hauteur, et rend la nouvelle. Si elles sortent de ce que
    /// les 16 bits couvrent, tout est réencodé.
    pub fn modifier_relief(&mut self, (a0, b0): (i64, i64), (a1, b1): (i64, i64), mut f: impl FnMut(i64, i64, f32) -> f32) {
        let n = self.cote as i64;
        let (a0, a1, b0, b1) = (a0.max(0), a1.min(n - 1), b0.max(0), b1.min(n - 1));
        let mut changes = Vec::new();
        for b in b0..=b1 {
            for a in a0..=a1 {
                let v = f(a, b, self.h_point(a, b));
                if v.is_finite() {
                    changes.push(((b * n + a) as usize, v));
                }
            }
        }
        self.poser_hauteurs(&changes);
    }

    /// Pose des hauteurs (indice dans la grille, mètres). Si elles sortent de ce que les
    /// 16 bits couvrent, tout est réencodé.
    pub fn poser_hauteurs(&mut self, changes: &[(usize, f32)]) {
        let haut = self.base + 65535.0 * self.echelle;
        if changes.iter().any(|(_, v)| *v < self.base || *v > haut) {
            let mut h = self.hauteurs();
            for &(i, v) in changes {
                if let Some(x) = h.get_mut(i) {
                    *x = v;
                }
            }
            self.fixer_hauteurs(&h);
            return;
        }
        for &(i, v) in changes {
            if let Some(x) = self.h.get_mut(i) {
                *x = ((v - self.base) / self.echelle).round().clamp(0.0, 65535.0) as u16;
            }
        }
    }

    /// La hauteur du relief en (x, z) du jeu, lue en douceur.
    pub fn relief(&self, x: f64, z: f64) -> f64 {
        let (gx, gz) = ((x - origine()) / self.pas, (z - origine()) / self.pas);
        let (a, b) = (gx.floor(), gz.floor());
        let (u, v) = (gx - a, gz - b);
        let (a, b) = (a as i64, b as i64);
        let p = |a: i64, b: i64| self.h_point(a, b) as f64;
        let l = |j: i64| cr(p(a - 1, j), p(a, j), p(a + 1, j), p(a + 2, j), u);
        cr(l(b - 1), l(b), l(b + 1), l(b + 2), v)
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

    /// Met ou retire l'eau au point (a, b) de la grille.
    pub fn poser_eau(&mut self, a: i64, b: i64, oui: bool) {
        let n = self.cote as i64;
        if a < 0 || b < 0 || a >= n || b >= n {
            return;
        }
        let i = (b * n + a) as usize;
        if oui {
            self.eau[i / 8] |= 1 << (i % 8);
        } else {
            self.eau[i / 8] &= !(1 << (i % 8));
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

    /// L'occupation du sol en (x, z) : les classes des quatre points autour et leurs
    /// poids.
    pub fn occupation(&self, x: f64, z: f64) -> [(u8, f64); 4] {
        if self.sol.is_empty() {
            return [(0, 1.0), (0, 0.0), (0, 0.0), (0, 0.0)];
        }
        let m = cote_sol(self.cote) as i64;
        let p = self.pas * 2.0;
        let (gx, gz) = ((x - origine()) / p, (z - origine()) / p);
        let (a, b) = (gx.floor(), gz.floor());
        let (u, v) = (gx - a, gz - b);
        let (a, b) = (a as i64, b as i64);
        let c = |a: i64, b: i64| self.sol[(b.clamp(0, m - 1) * m + a.clamp(0, m - 1)) as usize];
        [(c(a, b), (1.0 - u) * (1.0 - v)), (c(a + 1, b), u * (1.0 - v)), (c(a, b + 1), (1.0 - u) * v), (c(a + 1, b + 1), u * v)]
    }

    /// La classe du point le plus proche de (x, z) (0 sans occupation).
    pub fn classe(&self, x: f64, z: f64) -> u8 {
        if self.sol.is_empty() {
            return 0;
        }
        let m = cote_sol(self.cote) as i64;
        let p = self.pas * 2.0;
        let (a, b) = (((x - origine()) / p).round() as i64, ((z - origine()) / p).round() as i64);
        self.sol[(b.clamp(0, m - 1) * m + a.clamp(0, m - 1)) as usize]
    }

    /// La part (de 0 à 1) des points d'occupation de la `classe` dans le carré de demi-côté
    /// `r` mètres autour de (x, z).
    pub fn part_sol(&self, classe: u8, x: f64, z: f64, r: f64) -> f64 {
        if self.sol.is_empty() {
            return 0.0;
        }
        let m = cote_sol(self.cote) as i64;
        let p = self.pas * 2.0;
        let (a0, a1) = (((x - r - origine()) / p).ceil() as i64, ((x + r - origine()) / p).floor() as i64);
        let (b0, b1) = (((z - r - origine()) / p).ceil() as i64, ((z + r - origine()) / p).floor() as i64);
        let (mut oui, mut tous) = (0, 0);
        for b in b0.max(0)..=b1.min(m - 1) {
            for a in a0.max(0)..=a1.min(m - 1) {
                tous += 1;
                oui += (self.sol[(b * m + a) as usize] == classe) as i32;
            }
        }
        // Un carré plus petit que la grille : le point le plus proche
        if tous == 0 {
            return (self.classe(x, z) == classe) as i32 as f64;
        }
        oui as f64 / tous as f64
    }

    /// (x, z) du jeu d'un point en degrés, et l'inverse.
    pub fn vers_jeu(&self, lat: f64, lon: f64) -> (f64, f64) {
        let (x, y) = lambert93(lat, lon);
        (x - self.x0, self.y0 - y)
    }

    pub fn vers_degres(&self, x: f64, z: f64) -> (f64, f64) {
        lambert93_inverse(self.x0 + x, self.y0 - z)
    }

    /// Le fichier, en octets.
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
        ecrire_routes(&mut o, &self.routes);
        o.extend_from_slice(&(self.batiments.len() as u32).to_le_bytes());
        for b in &self.batiments {
            o.push(b.genre);
            o.extend_from_slice(&b.pied.to_le_bytes());
            o.extend_from_slice(&b.toit.to_le_bytes());
            o.push(b.forme);
            o.extend_from_slice(&b.egout.to_le_bytes());
            o.extend_from_slice(&b.axe.to_le_bytes());
            o.extend_from_slice(&(b.contour.len() as u32).to_le_bytes());
            for p in &b.contour {
                o.extend_from_slice(&p[0].to_le_bytes());
                o.extend_from_slice(&p[1].to_le_bytes());
            }
        }
        ecrire_routes(&mut o, &self.ponts);
        // L'occupation du sol, par plages (classe, longueur de 1 à 255)
        let mut plages = Vec::new();
        let mut it = self.sol.iter().peekable();
        while let Some(&c) = it.next() {
            let mut l = 1u8;
            while l < 255 && it.peek() == Some(&&c) {
                it.next();
                l += 1;
            }
            plages.push(c);
            plages.push(l);
        }
        o.extend_from_slice(&(plages.len() as u32 / 2).to_le_bytes());
        o.extend_from_slice(&plages);
        o
    }

    /// Écrit le fichier d'un coup : d'abord à côté, puis renommé.
    pub fn ecrire(&self, f: &std::path::Path) -> std::io::Result<()> {
        let tmp = f.with_extension("reel~");
        if let Some(d) = f.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(&tmp, self.octets())?;
        std::fs::rename(&tmp, f)
    }

    /// Lit un fichier ; une erreur dite en clair.
    pub fn lire(f: &std::path::Path) -> Result<Lieu, String> {
        let o = std::fs::read(f).map_err(|e| e.to_string())?;
        Lieu::depuis_octets(&o).ok_or_else(|| "ce n'est pas un lieu réel lisible (format SAGEREEL 1 à 6)".to_string())
    }

    /// Relu ; rien si ce n'est pas un lieu réel de ce format, ou pour une autre carte.
    pub fn depuis_octets(o: &[u8]) -> Option<Lieu> {
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
        let f32_ = |s: &[u8]| f32::from_le_bytes(s.try_into().unwrap());
        macro_rules! lire_routes { () => {{
            let mut routes = Vec::new();
            for _ in 0..u32_(prendre(4)?) {
                let genre = prendre(1)?[0];
                let largeur = f32_(prendre(4)?);
                let m = u32_(prendre(4)?) as usize;
                let o = prendre(m * 12)?;
                let points = o.chunks(12).map(|c| [f32_(&c[0..4]), f32_(&c[4..8]), f32_(&c[8..12])]).collect();
                routes.push(Route { genre, largeur, points });
            }
            routes
        }}}
        let routes = if format >= 2 { lire_routes!() } else { Vec::new() };
        let mut batiments = Vec::new();
        if format >= 3 {
            for _ in 0..u32_(prendre(4)?) {
                let genre = prendre(1)?[0];
                let pied = f32_(prendre(4)?);
                let toit = f32_(prendre(4)?);
                let (forme, egout, axe) = if format >= 6 { (prendre(1)?[0], f32_(prendre(4)?), f32_(prendre(4)?)) } else { (toits::PLAT, toit, 0.0) };
                let m = u32_(prendre(4)?) as usize;
                let contour = prendre(m * 8)?.chunks(8).map(|c| [f32_(&c[0..4]), f32_(&c[4..8])]).collect();
                batiments.push(Batiment { genre, pied, toit, egout, forme, axe, contour });
            }
        }
        let ponts = if format >= 4 { lire_routes!() } else { Vec::new() };
        let mut sol = Vec::new();
        if format >= 5 {
            let l = u32_(prendre(4)?) as usize;
            for c in prendre(l * 2)?.chunks(2) {
                sol.extend(std::iter::repeat_n(c[0], c[1] as usize));
            }
            let m = cote_sol(n);
            if !sol.is_empty() && sol.len() != m * m {
                return None;
            }
        }
        if k != o.len() {
            return None;
        }
        Some(Lieu { nom, lat, lon, x0, y0, pas, cote: n, niveau, base, echelle, h, eau, routes, batiments, ponts, sol })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lambert93_tombe_juste_et_s_inverse() {
        // L'origine de la projection, et Rouen (la cathédrale)
        let (x, y) = lambert93(46.5, 3.0);
        assert!((x - 700_000.0).abs() < 1e-6 && (y - 6_600_000.0).abs() < 1e-6);
        let (x, y) = lambert93(49.4402, 1.0951);
        assert!((x - 561_810.0).abs() < 5.0 && (y - 6_928_419.0).abs() < 5.0, "{x} {y}");
        let (la, lo) = lambert93_inverse(x, y);
        assert!((la - 49.4402).abs() < 1e-8 && (lo - 1.0951).abs() < 1e-8, "{la} {lo}");
    }

    #[test]
    fn un_lieu_se_relit_et_s_edite() {
        let pas = 64.0;
        let n = cote(pas);
        let h: Vec<f32> = (0..n * n).map(|i| (i % n) as f32 * 2.0).collect();
        let eau: Vec<bool> = (0..n * n).map(|i| i % n < n / 2).collect();
        let m = cote_sol(n);
        let b = Batiment::plat(MAISON, -1.0, 7.0, vec![[10.0, 10.0], [30.0, 10.0], [30.0, 20.0]]);
        let r = Route { genre: ROUTE, largeur: 6.0, points: vec![[0.0, -200.0, 5.0], [0.0, 200.0, 5.0]] };
        let l = Lieu::new("Essai", 49.0, 1.0, 1000.0, 2000.0, pas, 3.0, &h, &eau, vec![r.clone()], vec![b], vec![r]).avec_sol(vec![BOIS; m * m]);
        let o = l.octets();
        assert_eq!(Lieu::depuis_octets(&o), Some(l.clone()));
        assert!(Lieu::depuis_octets(&o[..o.len() - 1]).is_none());
        let x = origine() + 10.0 * pas;
        assert!((l.relief(x, 0.0) - 20.0).abs() < 0.02);
        assert!((l.relief(x + pas / 2.0, 0.0) - 21.0).abs() < 0.02);
        assert_eq!(l.eau(origine() + pas, 0.0), 1.0);
        // Creuser sous la base : tout se réencode, le reste ne bouge pas
        let mut e = l.clone();
        e.modifier_relief((10, 10), (11, 11), |_, _, v| v - 500.0);
        assert!((e.h_point(10, 10) - (20.0 - 500.0)).abs() < 0.02);
        assert!((e.h_point(40, 3) - 80.0).abs() < 0.02);
        e.poser_eau(0, 0, false);
        assert!(!e.est_eau(0, 0) && e.est_eau(1, 0));
        // Un lieu vierge se relit aussi
        let v = Lieu::vierge("Vierge", 49.0, 1.0, 64.0, 100.0);
        assert_eq!(Lieu::depuis_octets(&v.octets()), Some(v.clone()));
        assert!((v.relief(0.0, 0.0) - 100.0).abs() < 0.02);
        let (la, lo) = v.vers_degres(DEPART[0] as f64, DEPART[1] as f64);
        assert!((la - 49.0).abs() < 1e-7 && (lo - 1.0).abs() < 1e-7);
    }
}
