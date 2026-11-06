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
//! - Les bâtiments : un contour (x, z du jeu), un pied et un toit (hauteurs absolues) et
//!   un genre. Chaque tuile monte en volumes, toits plats, ceux dont le centre est chez
//!   elle (`Reel::bati`).
//! - Les ponts : des lignes comme les routes (la hauteur du tablier), montées en tabliers
//!   avec leurs parapets, par morceaux, dans la tuile du milieu de chaque morceau.
//! - L'occupation du sol (bois, prés et parcs, champs, quartiers habités, zones
//!   industrielles) : une classe par point d'une grille deux fois plus lâche que le
//!   relief, qui teinte le sol (`Reel::occupation`).
//!
//! Sources : relief © IGN (RGE ALTI, Licence Ouverte 2.0) ; eau © les contributeurs
//! OpenStreetMap (ODbL). Un fichier préparé est une base de données dérivée d'OSM : il
//! reste sous ODbL, et ne va pas dans le dépôt.

use crate::moteur::tuiles::DEMI_CARTE;
use crate::moteur::TAILLE;

const MAGIQUE: &[u8; 8] = b"SAGEREEL";
const FORMAT: u32 = 6;

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
    pub batiments: Vec<Batiment>,
    /// Pour chaque tuile, les bâtiments dont le centre est chez elle.
    par_tuile: std::collections::HashMap<(i32, i32), Vec<u32>>,
    /// Les ponts : une ligne par pont, la hauteur du tablier.
    pub ponts: Vec<Route>,
    /// Pour chaque tuile, les morceaux de pont (pont, premier point) dont le milieu est
    /// chez elle.
    ponts_par_tuile: std::collections::HashMap<(i32, i32), Vec<(u32, u32)>>,
    /// L'occupation du sol, une classe par point de la grille de `2 × pas` (`cote_sol`
    /// points de côté) ; vide : rien de dit.
    pub sol: Vec<u8>,
}

/// Le côté de la grille de l'occupation du sol pour un relief de `n` points de côté.
pub fn cote_sol(n: usize) -> usize {
    (n - 1) / 2 + 1
}

/// L'épaisseur du tablier, la hauteur et l'épaisseur des parapets (m).
const TABLIER: f32 = 0.8;
const PARAPET: f32 = 1.0;
const PARAPET_LARGE: f32 = 0.3;
const BETON: [f32; 3] = [0.62, 0.60, 0.56];
const GARDE_CORPS: [f32; 3] = [0.70, 0.68, 0.64];

/// Un pavé le long d'un morceau de pont, de `a` à `b` (x, z, hauteur du tablier) : en
/// travers de `u0` à `u1` (à droite positif), en hauteur de `v0` à `v1` au-dessus du
/// tablier ; le dessus d'une couleur, le reste d'une autre. Chaque face regarde dehors.
fn pave(v: &mut Vec<f32>, t: &mut Vec<[[f32; 3]; 3]>, a: [f32; 3], b: [f32; 3], (u0, u1): (f32, f32), (v0, v1): (f32, f32), dessus: [f32; 3], cotes: [f32; 3]) {
    let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
    let l = dx.hypot(dz);
    if l < 1e-3 {
        return;
    }
    let (rx, rz) = (-dz / l, dx / l);
    let p = |q: [f32; 3], u: f32, h: f32| [q[0] + rx * u, q[2] + h, q[1] + rz * u];
    let c = [[p(a, u0, v0), p(a, u1, v0), p(b, u1, v0), p(b, u0, v0)], [p(a, u0, v1), p(a, u1, v1), p(b, u1, v1), p(b, u0, v1)]];
    let centre = {
        let m = c.iter().flatten().fold([0.0; 3], |s, q| [s[0] + q[0], s[1] + q[1], s[2] + q[2]]);
        m.map(|x| x / 8.0)
    };
    let (bas, haut) = (c[0], c[1]);
    let faces = [
        (haut, dessus),
        (bas, cotes),
        ([bas[0], bas[1], haut[1], haut[0]], cotes),
        ([bas[1], bas[2], haut[2], haut[1]], cotes),
        ([bas[2], bas[3], haut[3], haut[2]], cotes),
        ([bas[3], bas[0], haut[0], haut[3]], cotes),
    ];
    for (mut q, couleur) in faces {
        let sub = |x: [f32; 3], y: [f32; 3]| [x[0] - y[0], x[1] - y[1], x[2] - y[2]];
        let croix = |x: [f32; 3], y: [f32; 3]| [x[1] * y[2] - x[2] * y[1], x[2] * y[0] - x[0] * y[2], x[0] * y[1] - x[1] * y[0]];
        let mut n = croix(sub(q[1], q[0]), sub(q[2], q[0]));
        let m = q.iter().fold([0.0; 3], |s, x| [s[0] + x[0] / 4.0, s[1] + x[1] / 4.0, s[2] + x[2] / 4.0]);
        let dehors = sub(m, centre);
        if n[0] * dehors[0] + n[1] * dehors[1] + n[2] * dehors[2] < 0.0 {
            q.reverse();
            n = n.map(|x| -x);
        }
        let ln = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-9);
        let n = n.map(|x| x / ln);
        for tri in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
            for x in tri {
                v.extend_from_slice(&x);
                v.extend_from_slice(&n);
                v.extend_from_slice(&couleur);
            }
            t.push(tri);
        }
    }
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

impl Reel {
    /// Les hauteurs `h` (en mètres) et l'eau, d'une grille de `cote(pas)` points ; les
    /// hauteurs s'encodent sur 16 bits.
    #[allow(clippy::too_many_arguments)]
    pub fn new(nom: &str, lat: f64, lon: f64, x0: f64, y0: f64, pas: f64, niveau: f32, h: &[f32], eau: &[bool], routes: Vec<Route>, batiments: Vec<Batiment>, ponts: Vec<Route>) -> Reel {
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
        Reel { nom: nom.into(), lat, lon, x0, y0, pas, cote: n, niveau, base: lo, echelle, h: hv, eau: bits, routes, index: Default::default(), batiments, par_tuile: Default::default(), ponts, ponts_par_tuile: Default::default(), sol: Vec::new() }.indexe()
    }

    /// Avec son occupation du sol (`cote_sol(cote)²` classes).
    pub fn avec_sol(mut self, sol: Vec<u8>) -> Reel {
        let m = cote_sol(self.cote);
        assert_eq!(sol.len(), m * m);
        self.sol = sol;
        self
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
        let mut par_tuile: std::collections::HashMap<(i32, i32), Vec<u32>> = Default::default();
        for (k, b) in self.batiments.iter().enumerate() {
            let c = b.centre();
            par_tuile.entry(((c[0] / TAILLE).floor() as i32, (c[1] / TAILLE).floor() as i32)).or_default().push(k as u32);
        }
        self.par_tuile = par_tuile;
        let mut ponts: std::collections::HashMap<(i32, i32), Vec<(u32, u32)>> = Default::default();
        for (k, p) in self.ponts.iter().enumerate() {
            for (i, s) in p.points.windows(2).enumerate() {
                let (x, z) = ((s[0][0] + s[1][0]) / 2.0, (s[0][1] + s[1][1]) / 2.0);
                ponts.entry(((x / TAILLE).floor() as i32, (z / TAILLE).floor() as i32)).or_default().push((k as u32, i as u32));
            }
        }
        self.ponts_par_tuile = ponts;
        self
    }

    /// Les bâtiments de la tuile (i, j), en volumes, dans son repère : les sommets
    /// (position, normale, couleur) et les triangles pour les collisions. Rien s'il n'y en
    /// a pas.
    pub fn bati(&self, i: i32, j: i32) -> Option<(Vec<f32>, Vec<[[f32; 3]; 3]>)> {
        let (ox, oz) = (i as f32 * TAILLE, j as f32 * TAILLE);
        let (mut v, mut t) = (Vec::new(), Vec::new());
        // Les morceaux de pont : le tablier, et un parapet de chaque côté
        for &(k, s) in self.ponts_par_tuile.get(&(i, j)).map_or(&[][..], |l| l.as_slice()) {
            let p = &self.ponts[k as usize];
            let (a, b) = (p.points[s as usize], p.points[s as usize + 1]);
            let (a, b) = ([a[0] - ox, a[1] - oz, a[2]], [b[0] - ox, b[1] - oz, b[2]]);
            let w = p.largeur / 2.0;
            pave(&mut v, &mut t, a, b, (-w, w), (-TABLIER, 0.0), teinte(p.genre), BETON);
            for cote in [-1.0, 1.0] {
                let (u0, u1) = if cote < 0.0 { (-w - PARAPET_LARGE, -w) } else { (w, w + PARAPET_LARGE) };
                pave(&mut v, &mut t, a, b, (u0, u1), (-TABLIER, PARAPET), GARDE_CORPS, GARDE_CORPS);
            }
        }
        for &k in self.par_tuile.get(&(i, j)).map_or(&[][..], |l| l.as_slice()) {
            let b = &self.batiments[k as usize];
            let (murs, toit) = teintes_bati(b.genre);
            let mut tri = |p: [[f32; 3]; 3], n: [f32; 3], couleur: [f32; 3]| {
                let p = p.map(|q| [q[0] - ox, q[1], q[2] - oz]);
                for q in p {
                    v.extend_from_slice(&q);
                    v.extend_from_slice(&n);
                    v.extend_from_slice(&couleur);
                }
                t.push(p);
            };
            crate::toits::monter(b, murs, toit, &mut tri);
        }
        (!v.is_empty()).then_some((v, t))
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
            let (a, b) = (((x - origine()) / p).round() as i64, ((z - origine()) / p).round() as i64);
            return (self.sol[(b.clamp(0, m - 1) * m + a.clamp(0, m - 1)) as usize] == classe) as i32 as f64;
        }
        oui as f64 / tous as f64
    }

    /// Le point (x, z) est-il libre, à `marge` mètres près : ni eau, ni chaussée, ni
    /// bâtiment, ni pont ? (Un arbre y pousse.)
    pub fn libre(&self, x: f64, z: f64, marge: f32) -> bool {
        if self.eau(x, z) > 0.0 || self.relief(x, z) < self.niveau as f64 + 0.3 {
            return false;
        }
        if self.route(x, z).is_some_and(|p| p.hors() < marge) {
            return false;
        }
        let (x, z) = (x as f32, z as f32);
        let segment = |a: [f32; 2], b: [f32; 2]| {
            let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
            let l2 = dx * dx + dz * dz;
            let t = if l2 > 0.0 { (((x - a[0]) * dx + (z - a[1]) * dz) / l2).clamp(0.0, 1.0) } else { 0.0 };
            (x - a[0] - dx * t).hypot(z - a[1] - dz * t)
        };
        let (ti, tj) = ((x / TAILLE).floor() as i32, (z / TAILLE).floor() as i32);
        for (i, j) in (tj - 1..=tj + 1).flat_map(|j| (ti - 1..=ti + 1).map(move |i| (i, j))) {
            for &k in self.par_tuile.get(&(i, j)).map_or(&[][..], |l| l.as_slice()) {
                let c = &self.batiments[k as usize].contour;
                let mut dedans = false;
                for e in 0..c.len() {
                    let (a, b) = (c[e], c[(e + 1) % c.len()]);
                    if segment(a, b) < marge {
                        return false;
                    }
                    if (a[1] > z) != (b[1] > z) && x < a[0] + (z - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
                        dedans = !dedans;
                    }
                }
                if dedans {
                    return false;
                }
            }
            for &(k, s) in self.ponts_par_tuile.get(&(i, j)).map_or(&[][..], |l| l.as_slice()) {
                let p = &self.ponts[k as usize];
                let (a, b) = (p.points[s as usize], p.points[s as usize + 1]);
                if segment([a[0], a[1]], [b[0], b[1]]) < p.largeur / 2.0 + PARAPET_LARGE + marge {
                    return false;
                }
            }
        }
        true
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
                let (forme, egout, axe) = if format >= 6 { (prendre(1)?[0], f32_(prendre(4)?), f32_(prendre(4)?)) } else { (crate::toits::PLAT, toit, 0.0) };
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
        Some(Reel { nom, lat, lon, x0, y0, pas, cote: n, niveau, base, echelle, h, eau, routes, index: Default::default(), batiments, par_tuile: Default::default(), ponts, ponts_par_tuile: Default::default(), sol }.indexe())
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
        let r = Reel::new("Essai", 49.0, 1.0, 1000.0, 2000.0, pas, 3.0, &h, &eau, Vec::new(), Vec::new(), Vec::new());
        let o = r.octets();
        assert_eq!(Reel::depuis_octets(&o), Some(r.clone()));
        assert!(Reel::depuis_octets(&o[..o.len() - 1]).is_none());
        // Sur un point, la hauteur du point ; entre deux, entre les deux
        let x = origine() + 10.0 * pas;
        assert!((r.hauteur(x, 0.0) - 20.0).abs() < 0.02);
        assert!((r.hauteur(x + pas / 2.0, 0.0) - 21.0).abs() < 0.02);
        assert_eq!(r.eau(origine() + pas, 0.0), 1.0);
        assert_eq!(r.eau(-origine() - pas, 0.0), 0.0);
        // L'occupation du sol : des bois au nord, des champs au sud, relue telle quelle
        let m = cote_sol(n);
        let sol: Vec<u8> = (0..m * m).map(|i| if i / m < m / 2 { BOIS } else { CHAMP }).collect();
        let r = r.avec_sol(sol);
        assert_eq!(Reel::depuis_octets(&r.octets()), Some(r.clone()));
        assert_eq!(r.occupation(0.0, origine() + 2.0 * pas)[0], (BOIS, 1.0));
        assert_eq!(r.occupation(0.0, -origine() - 2.0 * pas)[0], (CHAMP, 1.0));
    }

    #[test]
    fn un_batiment_se_monte_dans_sa_tuile() {
        // Un L, dans les deux sens : 6 points, 4 triangles de toit, 6 murs
        let l = vec![[10.0, 10.0], [30.0, 10.0], [30.0, 20.0], [20.0, 20.0], [20.0, 40.0], [10.0, 40.0]];
        assert_eq!(trianguler(&l).len(), 4);
        let pas = 64.0;
        let n = cote(pas);
        for c in [l.clone(), l.iter().rev().cloned().collect()] {
            let b = Batiment::plat(MAISON, -1.0, 7.0, c);
            let r = Reel::new("Essai", 49.0, 1.0, 0.0, 0.0, pas, -50.0, &vec![0.0; n * n], &vec![false; n * n], Vec::new(), vec![b], Vec::new());
            assert!(r.bati(1, 0).is_none());
            let (v, t) = r.bati(0, 0).expect("dans la tuile (0, 0)");
            assert_eq!(t.len(), 6 * 2 + 4);
            assert_eq!(v.len(), t.len() * 3 * 9);
            // Les normales des murs regardent dehors, le toit vers le haut
            for (k, tri) in t.iter().enumerate() {
                let nm = &v[k * 27 + 3..k * 27 + 6];
                let m = [(tri[0][0] + tri[1][0] + tri[2][0]) / 3.0, (tri[0][2] + tri[1][2] + tri[2][2]) / 3.0];
                if nm[1] > 0.5 {
                    assert!(tri.iter().all(|p| p[1] == 7.0));
                    // Le sens des triangles suit leur normale
                    let (a, b, c) = (tri[0], tri[1], tri[2]);
                    let y = (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]);
                    assert!(y > 0.0, "toit {k}");
                } else {
                    let dehors = [m[0] + nm[0] * 0.5, m[1] + nm[2] * 0.5];
                    let dedans = |p: [f32; 2]| (p[0] > 10.0 && p[0] < 20.0 && p[1] > 10.0 && p[1] < 40.0) || (p[0] > 10.0 && p[0] < 30.0 && p[1] > 10.0 && p[1] < 20.0);
                    assert!(!dedans(dehors), "mur {k}");
                }
            }
            let o = r.octets();
            assert_eq!(Reel::depuis_octets(&o), Some(r));
        }
    }

    #[test]
    fn un_pont_se_monte_et_se_franchit() {
        let pas = 64.0;
        let n = cote(pas);
        // Un pont de 5 m sur deux tuiles, à 8 m de haut, au-dessus d'un sol à 0
        let pont = Route { genre: ROUTE, largeur: 5.0, points: vec![[30.0, 20.0, 8.0], [60.0, 20.0, 8.0], [90.0, 20.0, 8.0]] };
        let r = Reel::new("Essai", 49.0, 1.0, 0.0, 0.0, pas, -50.0, &vec![0.0; n * n], &vec![false; n * n], Vec::new(), Vec::new(), vec![pont]);
        let (v, t) = r.bati(0, 0).expect("le premier morceau");
        assert!(r.bati(1, 0).is_some() && r.bati(2, 0).is_none());
        // Un tablier et deux parapets : trois pavés de douze triangles
        assert_eq!(t.len(), 36);
        assert_eq!(v.len(), t.len() * 27);
        // Le dessus du tablier est à 8 m, normale en haut, et sens des triangles d'accord
        let dessus: Vec<usize> = (0..t.len()).filter(|&k| v[k * 27 + 4] > 0.99 && t[k].iter().all(|p| (p[1] - 8.0).abs() < 1e-4)).collect();
        assert_eq!(dessus.len(), 2);
        for k in 0..t.len() {
            let (a, b, c) = (t[k][0], t[k][1], t[k][2]);
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let w = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let x = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
            let n = &v[k * 27 + 3..k * 27 + 6];
            assert!(x[0] * n[0] + x[1] * n[1] + x[2] * n[2] > 0.0, "triangle {k}");
        }
        assert_eq!(Reel::depuis_octets(&r.octets()), Some(r));
    }

    #[test]
    fn une_route_aplanit_le_sol_et_se_relit() {
        let pas = 64.0;
        let n = cote(pas);
        // Un relief en pente vers l'est (10 cm par mètre), une route nord-sud à 5 m
        let h: Vec<f32> = (0..n * n).map(|i| (origine() + (i % n) as f64 * pas) as f32 * 0.1).collect();
        let route = Route { genre: ROUTE, largeur: 6.0, points: vec![[0.0, -200.0, 5.0], [0.0, 200.0, 5.0]] };
        let r = Reel::new("Essai", 49.0, 1.0, 0.0, 0.0, pas, -50.0, &h, &vec![false; n * n], vec![route], Vec::new(), Vec::new());
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
