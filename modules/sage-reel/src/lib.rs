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
pub const FORMAT: u32 = 7;

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


/// Le côté de la carte entière, en mètres.
pub fn cote_carte() -> f64 {
    (2 * DEMI_CARTE + 1) as f64 * TAILLE as f64
}

/// La grille d'un lieu : un rectangle de `nx` × `nz` points, découpé dans la grille de la
/// carte entière (`cote(pas)` points de côté) à partir du point (`a0`, `b0`). Les indices
/// (a, b) des méthodes de `Lieu` sont ceux de la carte entière ; hors du rectangle, le
/// relief prolonge ses bords, sans eau. `a0` et `b0` sont pairs : la grille du sol (au
/// double pas) tombe sur la même.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grille {
    pub a0: usize,
    pub b0: usize,
    pub nx: usize,
    pub nz: usize,
}

impl Grille {
    /// La carte entière.
    pub fn entiere(pas: f64) -> Grille {
        let n = cote(pas);
        Grille { a0: 0, b0: 0, nx: n, nz: n }
    }

    /// Un rectangle de `largeur` (est-ouest) × `longueur` (nord-sud) mètres, centré sur le
    /// départ, borné à la carte ; 0 (ou plus que la carte) : toute la carte dans ce sens.
    pub fn emprise(pas: f64, largeur: f64, longueur: f64) -> Grille {
        let n = cote(pas);
        let borne = |c: f64, l: f64| -> (usize, usize) {
            if l <= 0.0 || l >= cote_carte() {
                return (0, n);
            }
            let c = (c - origine()) / pas;
            let d = l / 2.0 / pas;
            // Le début pair ; au moins deux points
            let a0 = (((c - d).floor().max(0.0)) as usize) & !1;
            let a1 = ((c + d).ceil() as usize).min(n - 1).max(a0 + 1);
            (a0, a1 - a0 + 1)
        };
        let (a0, nx) = borne(DEPART[0] as f64, largeur);
        let (b0, nz) = borne(DEPART[1] as f64, longueur);
        Grille { a0, b0, nx, nz }
    }

    /// Toute la carte ?
    pub fn est_entiere(&self, pas: f64) -> bool {
        *self == Grille::entiere(pas)
    }

    /// Le nombre de points.
    pub fn points(&self) -> usize {
        self.nx * self.nz
    }

    /// L'indice dans les tableaux du lieu du point (a, b) de la carte, s'il est dedans.
    pub fn indice(&self, a: i64, b: i64) -> Option<usize> {
        let (a, b) = (a - self.a0 as i64, b - self.b0 as i64);
        (a >= 0 && b >= 0 && a < self.nx as i64 && b < self.nz as i64).then(|| b as usize * self.nx + a as usize)
    }

    /// L'indice du point de la grille le plus proche de (a, b).
    pub fn borner(&self, a: i64, b: i64) -> usize {
        let a = (a - self.a0 as i64).clamp(0, self.nx as i64 - 1) as usize;
        let b = (b - self.b0 as i64).clamp(0, self.nz as i64 - 1) as usize;
        b * self.nx + a
    }

    /// Le point (a, b) de la carte d'un indice.
    pub fn point(&self, i: usize) -> (i64, i64) {
        ((self.a0 + i % self.nx) as i64, (self.b0 + i / self.nx) as i64)
    }

    /// Les dimensions de la grille du sol (au double pas).
    pub fn sol(&self) -> (usize, usize) {
        (cote_sol(self.nx), cote_sol(self.nz))
    }

    /// L'indice dans le sol du point (a, b) de la grille du sol de la carte (au double
    /// pas), le plus proche dedans.
    pub fn borner_sol(&self, a: i64, b: i64) -> usize {
        let (mx, mz) = self.sol();
        let a = (a - (self.a0 / 2) as i64).clamp(0, mx as i64 - 1) as usize;
        let b = (b - (self.b0 / 2) as i64).clamp(0, mz as i64 - 1) as usize;
        b * mx + a
    }

    /// L'étendue au repère du jeu, pour un pas : [x0, z0, x1, z1].
    pub fn etendue(&self, pas: f64) -> [f64; 4] {
        let (x0, z0) = (origine() + self.a0 as f64 * pas, origine() + self.b0 as f64 * pas);
        [x0, z0, x0 + (self.nx - 1) as f64 * pas, z0 + (self.nz - 1) as f64 * pas]
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
    /// Le rectangle de la carte que couvre le lieu.
    pub grille: Grille,
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
    /// L'occupation du sol, une classe par point de la grille de `2 × pas`
    /// (`grille.sol()`) ; vide : rien de dit.
    pub sol: Vec<u8>,
}

/// L'échelle et la base qui encodent sur 16 bits des hauteurs de `lo` à `hi`.
fn quantum(lo: f32, hi: f32) -> f32 {
    ((hi - lo) / 65535.0).max(0.01)
}

impl Lieu {
    /// Les hauteurs `h` (en mètres) et l'eau, sur la carte entière (`cote(pas)` points de
    /// côté) ; les hauteurs s'encodent sur 16 bits.
    #[allow(clippy::too_many_arguments)]
    pub fn new(nom: &str, lat: f64, lon: f64, x0: f64, y0: f64, pas: f64, niveau: f32, h: &[f32], eau: &[bool], routes: Vec<Route>, batiments: Vec<Batiment>, ponts: Vec<Route>) -> Lieu {
        Lieu::sur(nom, lat, lon, x0, y0, pas, Grille::entiere(pas), niveau, h, eau, routes, batiments, ponts)
    }

    /// Pareil, sur le rectangle `grille` de la carte (`grille.points()` hauteurs).
    #[allow(clippy::too_many_arguments)]
    pub fn sur(nom: &str, lat: f64, lon: f64, x0: f64, y0: f64, pas: f64, grille: Grille, niveau: f32, h: &[f32], eau: &[bool], routes: Vec<Route>, batiments: Vec<Batiment>, ponts: Vec<Route>) -> Lieu {
        let n = grille.points();
        assert!(h.len() == n && eau.len() == n);
        let mut bits = vec![0u8; n.div_ceil(8)];
        for (i, _) in eau.iter().enumerate().filter(|(_, e)| **e) {
            bits[i / 8] |= 1 << (i % 8);
        }
        let mut l = Lieu { nom: nom.into(), lat, lon, x0, y0, pas, grille, niveau, base: 0.0, echelle: 1.0, h: Vec::new(), eau: bits, routes, batiments, ponts, sol: Vec::new() };
        l.fixer_hauteurs(h);
        l
    }

    /// Un lieu sans rien : un relief plat à `altitude`, sans eau (son niveau 10 m plus
    /// bas), centré sur (lat, lon).
    pub fn vierge(nom: &str, lat: f64, lon: f64, pas: f64, altitude: f32) -> Lieu {
        Lieu::vierge_sur(nom, lat, lon, pas, Grille::entiere(pas), altitude)
    }

    /// Pareil, sur le rectangle `grille` de la carte.
    pub fn vierge_sur(nom: &str, lat: f64, lon: f64, pas: f64, grille: Grille, altitude: f32) -> Lieu {
        let n = grille.points();
        let (xc, yc) = lambert93(lat, lon);
        let (x0, y0) = (xc - DEPART[0] as f64, yc + DEPART[1] as f64);
        let (mx, mz) = grille.sol();
        let mut l = Lieu::sur(nom, lat, lon, x0, y0, pas, grille, altitude - 10.0, &vec![altitude; n], &vec![false; n], Vec::new(), Vec::new(), Vec::new());
        l.sol = vec![0; mx * mz];
        l
    }

    /// Le lieu réduit au rectangle `g` (pris dans le sien) : le relief, l'eau et le sol
    /// découpés ; les bâtiments gardés s'ils sont entiers dedans, les routes et les ponts
    /// coupés à son bord.
    pub fn recadrer(&self, g: Grille) -> Lieu {
        let (a1, b1) = ((g.a0 + g.nx).min(self.grille.a0 + self.grille.nx), (g.b0 + g.nz).min(self.grille.b0 + self.grille.nz));
        let a0 = g.a0.max(self.grille.a0).next_multiple_of(2);
        let b0 = g.b0.max(self.grille.b0).next_multiple_of(2);
        let g = Grille { a0, b0, nx: a1.saturating_sub(a0).max(2), nz: b1.saturating_sub(b0).max(2) };
        let n = g.points();
        let h: Vec<f32> = (0..n).map(|i| { let (a, b) = g.point(i); self.h_point(a, b) }).collect();
        let eau: Vec<bool> = (0..n).map(|i| { let (a, b) = g.point(i); self.est_eau(a, b) }).collect();
        let [x0, z0, x1, z1] = g.etendue(self.pas);
        let dans = |p: &[f32]| (p[0] as f64) >= x0 && (p[0] as f64) <= x1 && (p[1] as f64) >= z0 && (p[1] as f64) <= z1;
        let couper = |l: &[Route]| -> Vec<Route> {
            let mut v = Vec::new();
            for r in l {
                let mut m: Vec<[f32; 3]> = Vec::new();
                for p in &r.points {
                    if dans(p) {
                        m.push(*p);
                    } else if m.len() > 1 {
                        v.push(Route { genre: r.genre, largeur: r.largeur, points: std::mem::take(&mut m) });
                    } else {
                        m.clear();
                    }
                }
                if m.len() > 1 {
                    v.push(Route { genre: r.genre, largeur: r.largeur, points: m });
                }
            }
            v
        };
        let batiments = self.batiments.iter().filter(|b| b.contour.iter().all(|p| dans(p))).cloned().collect();
        let mut l = Lieu::sur(&self.nom, self.lat, self.lon, self.x0, self.y0, self.pas, g, self.niveau, &h, &eau, couper(&self.routes), batiments, couper(&self.ponts));
        if !self.sol.is_empty() {
            let (mx, mz) = g.sol();
            l.sol = (0..mx * mz).map(|i| self.sol[self.grille.borner_sol((g.a0 / 2 + i % mx) as i64, (g.b0 / 2 + i / mx) as i64)]).collect();
        }
        l
    }

    /// L'étendue du lieu au repère du jeu : [x0, z0, x1, z1].
    pub fn etendue(&self) -> [f64; 4] {
        self.grille.etendue(self.pas)
    }

    /// Avec son occupation du sol (`grille.sol()` classes).
    pub fn avec_sol(mut self, sol: Vec<u8>) -> Lieu {
        let (mx, mz) = self.grille.sol();
        assert_eq!(sol.len(), mx * mz);
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

    /// La hauteur du point (a, b) de la carte, en mètres (bornée à la grille du lieu).
    pub fn h_point(&self, a: i64, b: i64) -> f32 {
        let i = self.grille.borner(a, b);
        self.base + self.h[i] as f32 * self.echelle
    }

    /// Change des hauteurs : `f` reçoit chaque point (a, b) de la carte du carré
    /// `a0..=a1`, `b0..=b1` (borné à la grille du lieu) et sa hauteur, et rend la nouvelle.
    /// Si elles sortent de ce que les 16 bits couvrent, tout est réencodé.
    pub fn modifier_relief(&mut self, (a0, b0): (i64, i64), (a1, b1): (i64, i64), mut f: impl FnMut(i64, i64, f32) -> f32) {
        let g = self.grille;
        let (a0, a1) = (a0.max(g.a0 as i64), a1.min((g.a0 + g.nx) as i64 - 1));
        let (b0, b1) = (b0.max(g.b0 as i64), b1.min((g.b0 + g.nz) as i64 - 1));
        let mut changes = Vec::new();
        for b in b0..=b1 {
            for a in a0..=a1 {
                let v = f(a, b, self.h_point(a, b));
                if let (true, Some(i)) = (v.is_finite(), g.indice(a, b)) {
                    changes.push((i, v));
                }
            }
        }
        self.poser_hauteurs(&changes);
    }

    /// Pose des hauteurs (indice dans les tableaux du lieu, mètres). Si elles sortent de ce que les
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

    /// Le point (a, b) de la carte est-il dans l'eau ? (Jamais hors du lieu.)
    pub fn est_eau(&self, a: i64, b: i64) -> bool {
        let Some(i) = self.grille.indice(a, b) else { return false };
        self.eau[i / 8] & (1 << (i % 8)) != 0
    }

    /// Met ou retire l'eau au point (a, b) de la carte (rien hors du lieu).
    pub fn poser_eau(&mut self, a: i64, b: i64, oui: bool) {
        let Some(i) = self.grille.indice(a, b) else { return };
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
        let p = self.pas * 2.0;
        let (gx, gz) = ((x - origine()) / p, (z - origine()) / p);
        let (a, b) = (gx.floor(), gz.floor());
        let (u, v) = (gx - a, gz - b);
        let (a, b) = (a as i64, b as i64);
        let c = |a: i64, b: i64| self.sol[self.grille.borner_sol(a, b)];
        [(c(a, b), (1.0 - u) * (1.0 - v)), (c(a + 1, b), u * (1.0 - v)), (c(a, b + 1), (1.0 - u) * v), (c(a + 1, b + 1), u * v)]
    }

    /// La classe du point le plus proche de (x, z) (0 sans occupation).
    pub fn classe(&self, x: f64, z: f64) -> u8 {
        if self.sol.is_empty() {
            return 0;
        }
        let p = self.pas * 2.0;
        let (a, b) = (((x - origine()) / p).round() as i64, ((z - origine()) / p).round() as i64);
        self.sol[self.grille.borner_sol(a, b)]
    }

    /// La part (de 0 à 1) des points d'occupation de la `classe` dans le carré de demi-côté
    /// `r` mètres autour de (x, z).
    pub fn part_sol(&self, classe: u8, x: f64, z: f64, r: f64) -> f64 {
        if self.sol.is_empty() {
            return 0.0;
        }
        let (mx, mz) = self.grille.sol();
        let (sa, sb) = ((self.grille.a0 / 2) as i64, (self.grille.b0 / 2) as i64);
        let p = self.pas * 2.0;
        let (a0, a1) = (((x - r - origine()) / p).ceil() as i64 - sa, ((x + r - origine()) / p).floor() as i64 - sa);
        let (b0, b1) = (((z - r - origine()) / p).ceil() as i64 - sb, ((z + r - origine()) / p).floor() as i64 - sb);
        let (mut oui, mut tous) = (0, 0);
        for b in b0.max(0)..=b1.min(mz as i64 - 1) {
            for a in a0.max(0)..=a1.min(mx as i64 - 1) {
                tous += 1;
                oui += (self.sol[b as usize * mx + a as usize] == classe) as i32;
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
        let g = self.grille;
        for v in [g.a0, g.b0, g.nx, g.nz] {
            o.extend_from_slice(&(v as u32).to_le_bytes());
        }
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
        Lieu::depuis_octets(&o).ok_or_else(|| "ce n'est pas un lieu réel lisible (format SAGEREEL 1 à 7)".to_string())
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
        if !(pas > 0.0) {
            return None;
        }
        // Jusqu'au format 6, la carte entière ; au 7, un rectangle de la carte
        let grille = if format >= 7 {
            let mut v = [0usize; 4];
            for x in &mut v {
                *x = u32_(prendre(4)?) as usize;
            }
            let g = Grille { a0: v[0], b0: v[1], nx: v[2], nz: v[3] };
            let c = cote(pas);
            if g.nx < 2 || g.nz < 2 || g.a0 % 2 != 0 || g.b0 % 2 != 0 || g.a0 + g.nx > c || g.b0 + g.nz > c {
                return None;
            }
            g
        } else {
            let n = u32_(prendre(4)?) as usize;
            if n != cote(pas) {
                return None;
            }
            Grille::entiere(pas)
        };
        let n = grille.points();
        let mut g = [0f32; 3];
        for v in &mut g {
            *v = f32::from_le_bytes(prendre(4)?.try_into().ok()?);
        }
        let [niveau, base, echelle] = g;
        let h = prendre(n * 2)?.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let eau = prendre(n.div_ceil(8))?.to_vec();
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
            let (mx, mz) = grille.sol();
            if !sol.is_empty() && sol.len() != mx * mz {
                return None;
            }
        }
        if k != o.len() {
            return None;
        }
        Some(Lieu { nom, lat, lon, x0, y0, pas, grille, niveau, base, echelle, h, eau, routes, batiments, ponts, sol })
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

    #[test]
    fn l_emprise_se_centre_sur_le_depart_et_se_borne_a_la_carte() {
        let n = cote(4.0);
        assert_eq!(Grille::emprise(4.0, 0.0, 0.0), Grille::entiere(4.0));
        assert_eq!(Grille::emprise(4.0, 50_000.0, 50_000.0), Grille { a0: 0, b0: 0, nx: n, nz: n });
        let g = Grille::emprise(4.0, 2000.0, 1000.0);
        assert!(g.a0 % 2 == 0 && g.b0 % 2 == 0);
        // Le départ (en points) est au milieu de l'emprise
        let c = (DEPART[0] as f64 - origine()) / 4.0;
        assert!(((g.a0 as f64 + (g.nx - 1) as f64 / 2.0) - c).abs() <= 1.5);
        assert!(((g.nx - 1) as f64 * 4.0 - 2000.0).abs() <= 12.0);
        assert!(((g.nz - 1) as f64 * 4.0 - 1000.0).abs() <= 12.0);
    }

    #[test]
    fn un_lieu_rectangulaire_se_relit_et_prolonge_ses_bords() {
        let pas = 16.0;
        let g = Grille::emprise(pas, 1000.0, 600.0);
        let n = g.points();
        // Le relief monte vers l'est : 1 m par point de la carte
        let h: Vec<f32> = (0..n).map(|i| g.point(i).0 as f32).collect();
        let eau: Vec<bool> = (0..n).map(|i| i % g.nx < 3).collect();
        let (mx, mz) = g.sol();
        let sol: Vec<u8> = (0..mx * mz).map(|i| if i % mx < mx / 2 { BOIS } else { PRE }).collect();
        let l = Lieu::sur("Rect", 49.0, 1.0, 0.0, 0.0, pas, g, 0.0, &h, &eau, Vec::new(), Vec::new(), Vec::new()).avec_sol(sol);
        let o = l.octets();
        let r = Lieu::depuis_octets(&o).unwrap();
        assert_eq!(r, l);
        assert!(o.len() < 100_000);
        let [x0, z0, x1, z1] = l.etendue();
        assert!((l.relief(x0 + 3.0 * pas, z0) - (g.a0 + 3) as f64).abs() < 0.02);
        // Hors du lieu : le bord, sans eau
        assert!((l.relief(x1 + 500.0, z1 + 500.0) - (g.a0 + g.nx - 1) as f64).abs() < 0.02);
        assert!((l.relief(x0 - 500.0, 0.0) - g.a0 as f64).abs() < 0.02);
        assert!(l.est_eau(g.a0 as i64, g.b0 as i64) && !l.est_eau(g.a0 as i64 - 1, g.b0 as i64));
        assert_eq!(l.classe(x0, z0), BOIS);
        assert_eq!(l.classe(x1 + 1000.0, z0), PRE);
        assert_eq!(l.part_sol(BOIS, x0, z0, 20.0), 1.0);
        // Une retouche hors du lieu ne change rien
        let mut e = l.clone();
        e.modifier_relief((0, 0), (3, 3), |_, _, v| v + 50.0);
        assert_eq!(e.h, l.h);
        // Recadré : le même relief au même endroit
        let c = l.recadrer(Grille { a0: g.a0 + 4, b0: g.b0 + 2, nx: 10, nz: 6 });
        assert_eq!((c.grille.nx, c.grille.nz), (10, 6));
        assert!((c.relief(x0 + 6.0 * pas, z0 + 3.0 * pas) - l.relief(x0 + 6.0 * pas, z0 + 3.0 * pas)).abs() < 0.02);
        assert_eq!(Lieu::depuis_octets(&c.octets()), Some(c));
        // Un vierge rectangulaire
        let v = Lieu::vierge_sur("V", 49.0, 1.0, pas, g, 100.0);
        assert_eq!(Lieu::depuis_octets(&v.octets()), Some(v));
    }
}
