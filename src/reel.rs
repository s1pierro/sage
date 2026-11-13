//! Un lieu réel dans le jeu : le `Lieu` de la crate partagée `sage-reel` (le format
//! `.reel`, préparé par `examples/lieu_reel.rs` et relu dans `monde/lieu.reel`), avec ce
//! que le jeu en calcule : les routes terrassées et indexées, les talus, les abords des
//! bâtiments, les bâtiments et les ponts rangés par tuile, montés en volumes
//! (`Reel::bati`).
//!
//! Le format, les constantes (genres, classes, couleurs), `lambert93` et les toits sont
//! dans `sage-reel` (`modules/sage-reel`), partagés avec Flow (le contexte Lieux, qui
//! explore, édite et génère les lieux). Un fichier préparé est dérivé d'OSM : il reste
//! sous ODbL, et ne va pas dans le dépôt.

use crate::moteur::TAILLE;
pub use sage_reel::{
    aire, cote, cote_sol, epaule, lambert93, lambert93_inverse, origine, teinte, teintes_bati, trianguler, Batiment, Lieu, Route, ANNEXE, BOIS, CHAMP, CHEMIN, CLASSES, EDIFICE, GRANDE, HABITE, IMMEUBLE, INDUSTRIEL, MAISON, PIETONNE, PRE, ROUTE, SENTIER, SERVICE, SOURCES, ZONE,
};

// Les mesures de la carte de la crate sont celles du moteur
const _: () = assert!(sage_reel::TAILLE == TAILLE && sage_reel::DEMI_CARTE == crate::moteur::tuiles::DEMI_CARTE);

/// Côté des cases de l'index des routes, en mètres.
pub const CASE: f32 = 32.0;

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

/// Un lieu réel tel que le jeu s'en sert. Ses données (`lieu`) se lisent directement
/// (`Deref`) ; `routes` et `ponts` sont ceux du jeu (terrassés, avec les travées), ceux
/// du fichier restent dans `lieu`.
#[derive(Clone, Debug, PartialEq)]
pub struct Reel {
    pub lieu: Lieu,
    /// Les routes terrassées (`terrassement::profiler`) : leur chaussée d'accord aux
    /// carrefours.
    pub routes: Vec<Route>,
    /// Les routes profilées, sans les retouches de la voirie : celles qui vont aux
    /// régions.
    pub routes_profilees: Vec<Route>,
    /// Pour chaque case, les segments (route, premier point) qui l'approchent.
    index: Index,
    /// Les mêmes, avec la place des talus.
    index_talus: Index,
    /// Les abords des bâtiments proches des rues, terrassés à leur hauteur.
    abords: crate::terrassement::Abords,
    /// Pour chaque tuile, les bâtiments dont le centre est chez elle.
    par_tuile: std::collections::HashMap<(i32, i32), Vec<u32>>,
    /// Les ponts : une ligne par pont, la hauteur du tablier ; puis les travées des
    /// croisements sans carrefour (`terrassement::travees`).
    pub ponts: Vec<Route>,
    /// Pour chaque tuile, les morceaux de pont (pont, premier point) dont le milieu est
    /// chez elle.
    ponts_par_tuile: std::collections::HashMap<(i32, i32), Vec<(u32, u32)>>,
}

impl std::ops::Deref for Reel {
    type Target = Lieu;
    fn deref(&self) -> &Lieu {
        &self.lieu
    }
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

/// Un morceau de pont de `a` à `b` (x, z, hauteur du tablier, dans le repère voulu) : le
/// tablier, couleur `dessus`, et un parapet de chaque côté.
pub fn monter_pont(v: &mut Vec<f32>, t: &mut Vec<[[f32; 3]; 3]>, a: [f32; 3], b: [f32; 3], largeur: f32, dessus: [f32; 3]) {
    let w = largeur / 2.0;
    pave(v, t, a, b, (-w, w), (-TABLIER, 0.0), dessus, BETON);
    for cote in [-1.0, 1.0] {
        let (u0, u1) = if cote < 0.0 { (-w - PARAPET_LARGE, -w) } else { (w, w + PARAPET_LARGE) };
        pave(v, t, a, b, (u0, u1), (-TABLIER, PARAPET), GARDE_CORPS, GARDE_CORPS);
    }
}

/// Les segments (route, premier point) qui approchent chaque case de `CASE` mètres.
pub type Index = std::collections::HashMap<(i32, i32), Vec<(u32, u32)>>;

/// Range chaque segment de route dans les cases qu'il approche (chaussée et bas-côtés).
pub fn indexer(routes: &[Route]) -> Index {
    indexer_avec(routes, |r| r.largeur / 2.0 + epaule(r.genre))
}

/// Range chaque segment de route dans les cases qu'il approche à `marge(route)` près.
pub fn indexer_avec(routes: &[Route], marge: impl Fn(&Route) -> f32) -> Index {
    let mut index: Index = Default::default();
    for (k, r) in routes.iter().enumerate() {
        let m = marge(r);
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
    index
}

/// La route la plus proche de (x, z), si l'on est sur elle ou ses bas-côtés ; la plus
/// large l'emporte (on compte depuis son bord).
pub fn proche(routes: &[Route], index: &Index, x: f64, z: f64) -> Option<Proche> {
    let (x, z) = (x as f32, z as f32);
    let l = index.get(&((x / CASE).floor() as i32, (z / CASE).floor() as i32))?;
    let mut mieux: Option<Proche> = None;
    for &(k, i) in l {
        let r = &routes[k as usize];
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

impl Reel {
    /// Les hauteurs `h` (en mètres) et l'eau, d'une grille de `cote(pas)` points ; les
    /// hauteurs s'encodent sur 16 bits.
    #[allow(clippy::too_many_arguments)]
    pub fn new(nom: &str, lat: f64, lon: f64, x0: f64, y0: f64, pas: f64, niveau: f32, h: &[f32], eau: &[bool], routes: Vec<Route>, batiments: Vec<Batiment>, ponts: Vec<Route>) -> Reel {
        Reel::depuis(Lieu::new(nom, lat, lon, x0, y0, pas, niveau, h, eau, routes, batiments, ponts))
    }

    /// Le lieu, prêt pour le jeu (terrassé, indexé).
    pub fn depuis(lieu: Lieu) -> Reel {
        let (routes, ponts) = (lieu.routes.clone(), lieu.ponts.clone());
        Reel { lieu, routes, routes_profilees: Vec::new(), index: Default::default(), index_talus: Default::default(), abords: Default::default(), par_tuile: Default::default(), ponts, ponts_par_tuile: Default::default() }.indexe()
    }

    /// Avec son occupation du sol (`cote_sol(cote)²` classes).
    pub fn avec_sol(mut self, sol: Vec<u8>) -> Reel {
        self.lieu = self.lieu.avec_sol(sol);
        self
    }

    /// Range chaque segment de route dans les cases qu'il approche.
    fn indexe(mut self) -> Reel {
        // Le terrassement (les routes lues se gardent telles quelles dans `lieu`)
        crate::terrassement::profiler(&mut self.routes, &self.ponts);
        self.routes_profilees = self.routes.clone();
        crate::voirie::appliquer(&mut self.routes);
        self.index = indexer(&self.routes);
        // Les talus et les travées, avec le terrassement en talus seulement
        let pr = crate::procedures::courantes();
        if pr.terrassement == crate::procedures::TALUS {
            self.index_talus = crate::terrassement::indexer(&self.routes);
            let contours: Vec<&[[f32; 2]]> = self.batiments.iter().map(|b| b.contour.as_slice()).collect();
            self.abords = crate::terrassement::abords(&self.routes, &contours, pr.abords, pr.talus);
            let travees = crate::terrassement::travees(&self.routes, &self.index);
            self.ponts.extend(travees);
        }
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
    pub fn bati(&self, i: i32, j: i32) -> Option<crate::moteur::Bati3d> {
        let (ox, oz) = (i as f32 * TAILLE, j as f32 * TAILLE);
        let (mut v, mut t, mut r) = (Vec::new(), Vec::new(), <[Vec<f32>; crate::moteur::REVETEMENTS]>::default());
        // Les morceaux de pont : le tablier, et un parapet de chaque côté
        for &(k, s) in self.ponts_par_tuile.get(&(i, j)).map_or(&[][..], |l| l.as_slice()) {
            let p = &self.ponts[k as usize];
            let (a, b) = (p.points[s as usize], p.points[s as usize + 1]);
            let (a, b) = ([a[0] - ox, a[1] - oz, a[2]], [b[0] - ox, b[1] - oz, b[2]]);
            monter_pont(&mut v, &mut t, a, b, p.largeur, teinte(p.genre));
        }
        for &k in self.par_tuile.get(&(i, j)).map_or(&[][..], |l| l.as_slice()) {
            let b = &self.assis(&self.batiments[k as usize]);
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
        // Les trottoirs, comme ceux du monde matérialisé (mêmes grilles pour le quartier
        // et l'eau)
        let batis: Vec<&[[f32; 2]]> = (i - 1..=i + 1)
            .flat_map(|a| (j - 1..=j + 1).map(move |b| (a, b)))
            .flat_map(|k| self.par_tuile.get(&k).map_or(&[][..], |l| l.as_slice()))
            .map(|&k| self.batiments[k as usize].contour.as_slice())
            .collect();
        let urbain = |x: f32, z: f32| {
            let g = |v: f32| (v as f64 / 8.0).round() * 8.0;
            let o = self.occupation(g(x), g(z));
            let c = o.iter().fold((0u8, -1.0), |m, &(k, p)| if p > m.1 { (k, p) } else { m }).0;
            matches!(c, HABITE | ZONE)
        };
        let sol = |x: f32, z: f32| self.hauteur(x as f64, z as f64) as f32;
        let eau = |x: f32, z: f32| {
            let g = |v: f32| (v as f64 / 2.0).round() * 2.0;
            self.eau(g(x), g(z)) >= 0.5
        };
        let l = crate::rues::Lieu { routes: &self.routes, index: &self.index, batis, urbain: &urbain, sol: &sol, eau: &eau };
        crate::rues::monter(&l, i, j, &mut t, &mut r);
        crate::moteur::Bati3d { sommets: v, triangles: t, revetus: r }.non_vide()
    }

    /// La route la plus proche de (x, z), si l'on est sur elle ou ses bas-côtés ; la plus
    /// large l'emporte (on compte depuis son bord).
    pub fn route(&self, x: f64, z: f64) -> Option<Proche> {
        proche(&self.routes, &self.index, x, z)
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

    /// La hauteur du relief seul, sans les routes : `Lieu::relief`.

    /// La hauteur du sol en (x, z) du jeu : le relief, terrassé sous les routes
    /// (`terrassement::sol`).
    pub fn hauteur(&self, x: f64, z: f64) -> f64 {
        let h = self.relief(x, z);
        match crate::procedures::courantes().terrassement {
            crate::procedures::TALUS => {}
            // Comme avant la 0.35 : à la hauteur de la chaussée, fondu sur les bas-côtés
            crate::procedures::FONDU => {
                return match self.route(x, z) {
                    Some(p) => {
                        let t = (p.hors() / epaule(p.genre)).clamp(0.0, 1.0) as f64;
                        let t = t * t * (3.0 - 2.0 * t);
                        p.hauteur as f64 + (h - p.hauteur as f64) * t
                    }
                    None => h,
                }
            }
            _ => return h,
        }
        if !self.index_talus.contains_key(&((x as f32 / CASE).floor() as i32, (z as f32 / CASE).floor() as i32)) && !self.abords.pres(x as f32, z as f32) {
            return h;
        }
        crate::terrassement::sol(&self.routes, &self.index_talus, &self.abords, x, z, h, self.eau(x, z) >= 0.5)
    }

    /// Le bâtiment `b`, le pied sous le sol terrassé le plus bas de son contour : un talus
    /// ne le laisse pas en l'air ; et monté d'autant que le sol a monté sous lui.
    pub fn assis(&self, b: &Batiment) -> Batiment {
        let bas = b.contour.iter().map(|p| self.hauteur(p[0] as f64, p[1] as f64) as f32).fold(f32::MAX, f32::min);
        let mut b = b.clone();
        // Le sol remonté sous lui (un abord remblayé jusqu'à la rue) : le bâtiment monte
        // d'autant, il garde sa hauteur au-dessus du sol
        let avant = b.contour.iter().map(|p| self.relief(p[0] as f64, p[1] as f64) as f32).fold(f32::MAX, f32::min);
        if bas > avant + 0.05 {
            let d = bas - avant;
            b.pied += d;
            b.egout += d;
            b.toit += d;
        }
        b.pied = b.pied.min(bas - 0.5);
        b
    }

    /// Le fichier, en octets (les routes et les ponts tels que lus).
    pub fn octets(&self) -> Vec<u8> {
        self.lieu.octets()
    }

    /// Relu ; rien si ce n'est pas un lieu réel de ce format, ou pour une autre carte.
    pub fn depuis_octets(o: &[u8]) -> Option<Reel> {
        Lieu::depuis_octets(o).map(Reel::depuis)
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
            let crate::moteur::Bati3d { sommets: v, triangles: t, .. } = r.bati(0, 0).expect("dans la tuile (0, 0)");
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
        let crate::moteur::Bati3d { sommets: v, triangles: t, .. } = r.bati(0, 0).expect("le premier morceau");
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
        // Sur la chaussée et ses accotements, sa hauteur ; puis un talus (en remblai : le
        // relief est plus bas) jusqu'à le rejoindre
        for x in [-4.9, 0.0, 4.9] {
            assert!((r.hauteur(x, 10.0) - 5.0).abs() < 1e-4, "{x}");
        }
        let talus = 5.0 - 2.0 * crate::terrassement::PENTE as f64;
        assert!((r.hauteur(7.0, 10.0) - talus).abs() < 1e-4 && (r.hauteur(-7.0, 10.0) - talus).abs() < 1e-4);
        assert!((r.hauteur(22.0, 10.0) - r.relief(22.0, 10.0)).abs() < 1e-6);
        assert!((r.hauteur(40.0, 10.0) - r.relief(40.0, 10.0)).abs() < 1e-6);
        assert_eq!(r.route(0.0, 10.0).map(|p| p.genre), Some(ROUTE));
        assert!(r.route(30.0, 10.0).is_none());
        let o = r.octets();
        assert_eq!(Reel::depuis_octets(&o), Some(r));
    }
}
