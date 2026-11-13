//! La campagne : une île d'environ 15 km au milieu de la carte, toujours la même pour une
//! graine donnée, nue : le relief et ses couleurs, sans objets.
//!
//! - Le contour est un disque de `RAYON_ILE` déformé par un bruit à grande échelle : des
//!   caps, des baies. Au large, le fond descend vers `FOND`, sous la mer (`MER`).
//! - À l'intérieur : des plaines et des collines (bruit déformé, `domain warping`), et
//!   une chaîne de montagnes d'un côté de l'île, en crêtes (`ridged`), jusqu'à environ
//!   `SOMMET`.
//! - Près du rivage, le relief s'efface en pente douce : des plages.
//!
//! Tout se calcule en coordonnées absolues (double précision), point par point : deux
//! tuiles voisines s'accordent exactement sur leur bord, normales comprises.

use crate::moteur::{Contenu, Relief, Source, TAILLE};
use crate::erosion::Erosion;
use crate::reel::Reel;
use std::f64::consts::TAU;
use std::sync::Arc;

/// Points de grille par côté de tuile : un pas de 2 m.
pub const GRILLE: usize = 32;
/// Le départ, en absolu : le centre de la tuile (0, 0), au milieu de l'île.
pub const DEPART: [f32; 2] = [TAILLE / 2.0, TAILLE / 2.0];
/// Le niveau de la mer.
pub const MER: f32 = 0.0;
/// Le rayon moyen de l'île, en mètres (avant que son contour soit déformé).
pub const RAYON_ILE: f64 = 7900.0;
/// Les plus hauts sommets de la chaîne, à peu près.
pub const SOMMET: f64 = 900.0;
/// Le fond de la mer, au large.
pub const FOND: f64 = -35.0;
/// La chaîne passe à tant du centre de l'île.
const CHAINE_DECALAGE: f64 = 2800.0;
/// Sa demi-largeur et sa demi-longueur, à peu près.
const CHAINE_LARGE: f64 = 1700.0;
const CHAINE_LONG: f64 = 5200.0;

#[derive(Clone)]
pub struct Campagne {
    pub graine: u32,
    /// L'érosion de ce relief, calculée pour cette graine ; sans elle, le relief brut.
    pub erosion: Option<Arc<Erosion>>,
    /// Un lieu réel à la place de l'île : son relief, son eau.
    pub reel: Option<Arc<Reel>>,
    /// Le monde matérialisé (ses régions `.stuff`), qui prime sur tout le reste.
    pub monde: Option<Arc<crate::regions::Regions>>,
}

/// Ce qu'il faut d'un lieu pour teinter son sol : l'occupation, l'eau, les routes.
pub trait Sol {
    fn occupation(&self, x: f64, z: f64) -> [(u8, f64); 4];
    fn eau(&self, x: f64, z: f64) -> f64;
    fn niveau(&self) -> f32;
    fn route(&self, x: f64, z: f64) -> Option<crate::reel::Proche>;
}

impl Sol for Reel {
    fn occupation(&self, x: f64, z: f64) -> [(u8, f64); 4] {
        self.lieu.occupation(x, z)
    }
    fn eau(&self, x: f64, z: f64) -> f64 {
        self.lieu.eau(x, z)
    }
    fn niveau(&self) -> f32 {
        self.niveau
    }
    fn route(&self, x: f64, z: f64) -> Option<crate::reel::Proche> {
        Reel::route(self, x, z)
    }
}

/// La couleur du sol d'un lieu réel : selon l'occupation du sol, de l'herbe plus ou
/// moins sèche, des bois, des prés, des champs, des quartiers ; puis la craie dans les
/// pentes raides (les coteaux de la Seine), la vase des berges, le fond de l'eau, les
/// chaussées.
pub fn couleur_lieu(r: &dyn Sol, x: f64, z: f64, h: f64, n: [f32; 3]) -> [f32; 3] {
    let melange = |a: [f32; 3], b: [f32; 3], t: f64| -> [f32; 3] { [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t as f32) };
    let s = bruit(0xA5A5, x / 60.0, z / 60.0);
    let mut c = [0.0f32; 3];
    for (classe, poids) in r.occupation(x, z) {
        if poids > 0.0 {
            let t = occupation(classe, x, z, s);
            c = [0, 1, 2].map(|k| c[k] + t[k] * poids as f32);
        }
    }
    c = melange(c, [0.52, 0.50, 0.44], ((0.85 - n[1] as f64) / 0.15).clamp(0.0, 1.0));
    c = melange(c, [0.86, 0.85, 0.80], ((0.62 - n[1] as f64) / 0.15).clamp(0.0, 1.0));
    let e = r.eau(x, z);
    let dessus = h - r.niveau() as f64;
    c = melange(c, [0.50, 0.47, 0.38], e.max(rampe(1.0, 0.2, dessus)));
    c = melange(c, [0.36, 0.38, 0.34], rampe(-0.5, -4.0, dessus));
    // La chaussée, pleine jusqu'à son bord (au pied de la bordure du trottoir), en
    // douceur sur un demi-mètre au-delà
    if let Some(p) = r.route(x, z) {
        c = melange(c, crate::reel::teinte(p.genre), rampe(0.5, 0.0, p.hors() as f64));
    }
    c
}

/// Un nombre de 0 à 1, toujours le même pour (graine, x, z).
pub(crate) fn hache(graine: u32, x: i64, z: i64) -> f64 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (z as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ graine as u64;
    h ^= h >> 31;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 29;
    (h >> 11) as f64 / (1u64 << 53) as f64
}

/// Bruit de valeurs, lissé (quintique), sur un réseau de pas 1 : de 0 à 1.
pub(crate) fn bruit(graine: u32, x: f64, z: f64) -> f64 {
    let (x0, z0) = (x.floor(), z.floor());
    let (fx, fz) = (x - x0, z - z0);
    let lisse = |t: f64| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let (sx, sz) = (lisse(fx), lisse(fz));
    let (i, k) = (x0 as i64, z0 as i64);
    let a = hache(graine, i, k) + (hache(graine, i + 1, k) - hache(graine, i, k)) * sx;
    let b = hache(graine, i, k + 1) + (hache(graine, i + 1, k + 1) - hache(graine, i, k + 1)) * sx;
    a + (b - a) * sz
}

/// Plusieurs octaves de bruit, de -1 à 1 environ. Chaque octave est tournée : les
/// réseaux ne s'alignent pas.
fn fbm(graine: u32, x: f64, z: f64, octaves: u32) -> f64 {
    let (mut s, mut a, mut total) = (0.0, 1.0, 0.0);
    let (mut x, mut z) = (x, z);
    for k in 0..octaves {
        s += (bruit(graine.wrapping_add(k * 7919), x, z) * 2.0 - 1.0) * a;
        total += a;
        a *= 0.5;
        (x, z) = (x * 1.6 - z * 1.2 + 17.3, x * 1.2 + z * 1.6 - 9.1);
    }
    s / total
}

/// Bruit en crêtes, de 0 à 1 : des arêtes fines, chaque octave plus forte sur les crêtes
/// de la précédente.
fn cretes(graine: u32, x: f64, z: f64, octaves: u32) -> f64 {
    let (mut s, mut a, mut poids, mut total) = (0.0, 1.0, 1.0, 0.0);
    let (mut x, mut z) = (x, z);
    for k in 0..octaves {
        let n = 1.0 - (bruit(graine.wrapping_add(k * 104_729), x, z) * 2.0 - 1.0).abs();
        let n = n * n * poids;
        poids = (n * 1.6).clamp(0.0, 1.0);
        s += n * a;
        total += a;
        a *= 0.5;
        (x, z) = (x * 1.6 - z * 1.2 + 31.7, x * 1.2 + z * 1.6 + 5.3);
    }
    s / total
}

/// 0 en `a`, 1 en `b`, en douceur (`a` peut être plus grand que `b`).
fn rampe(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Campagne {
    /// Les routes (absolues) dont un point est dans le rectangle (x, z bas, x, z hauts) :
    /// celles des régions du monde matérialisé (une seule fois chacune), sinon du lieu réel.
    pub fn routes_pres(&self, x0: f32, z0: f32, x1: f32, z1: f32) -> Vec<crate::reel::Route> {
        let touche = |r: &crate::reel::Route| r.points.iter().any(|p| p[0] >= x0 && p[0] <= x1 && p[1] >= z0 && p[1] <= z1);
        if let Some(m) = &self.monde {
            let mut vues = std::collections::HashSet::new();
            let mut l = Vec::new();
            let g = |v: f32| (v as f64 / crate::regions::METRES).floor() as i32;
            for rj in g(z0)..=g(z1) {
                for ri in g(x0)..=g(x1) {
                    let Some(r) = m.region(ri, rj) else { continue };
                    for ro in r.routes.iter().filter(|ro| touche(ro)) {
                        let (a, b) = (ro.points[0], ro.points[ro.points.len() - 1]);
                        if vues.insert((ro.genre, ro.points.len(), a[0].to_bits(), a[1].to_bits(), b[0].to_bits(), b[1].to_bits())) {
                            l.push(ro.clone());
                        }
                    }
                }
            }
            return l;
        }
        self.reel.as_ref().map_or_else(Vec::new, |r| r.routes.iter().filter(|ro| touche(ro)).cloned().collect())
    }

    /// La latitude du lieu, en degrés : celle du lieu réel, ou celle de Rouen pour l'île.
    pub fn latitude(&self) -> f64 {
        self.monde.as_ref().and_then(|m| m.lat).or(self.reel.as_ref().map(|r| r.lat)).unwrap_or(crate::moteur::scene::LATITUDE)
    }

    /// Le relief brut de `graine`, sans érosion.
    pub fn brute(graine: u32) -> Campagne {
        Campagne { graine, erosion: None, reel: None, monde: None }
    }

    /// Le relief de `graine`, érodé : le calcul prend quelques secondes.
    pub fn erodee(graine: u32) -> Campagne {
        let c = Campagne::brute(graine);
        let e = Erosion::calculer(graine, MER, &|x, z| c.relief(x, z));
        Campagne { graine, erosion: Some(Arc::new(e)), reel: None, monde: None }
    }

    /// Un lieu réel.
    pub fn reelle(r: Arc<Reel>) -> Campagne {
        Campagne { graine: 0, erosion: None, reel: Some(r), monde: None }
    }

    /// Un monde matérialisé.
    pub fn materialisee(m: Arc<crate::regions::Regions>) -> Campagne {
        Campagne { graine: 0, erosion: None, reel: None, monde: Some(m) }
    }

    /// Le nom du lieu réel, matérialisé ou non.
    pub fn nom_lieu(&self) -> Option<String> {
        self.monde.as_ref().map(|m| m.nom.clone()).or_else(|| self.reel.as_ref().map(|r| r.nom.clone()))
    }

    /// La surface de l'eau : la mer de l'île, ou celle du lieu réel.
    pub fn mer(&self) -> f32 {
        match (&self.monde, &self.reel) {
            (Some(m), _) => m.niveau,
            (None, Some(r)) => r.niveau,
            _ => MER,
        }
    }

    /// Le centre de l'île, en absolu.
    fn centre(&self) -> (f64, f64) {
        (DEPART[0] as f64, DEPART[1] as f64)
    }

    /// La part de terre en (x, z) : 1 dans l'île, 0 au large.
    pub fn terre(&self, x: f64, z: f64) -> f64 {
        let g = self.graine;
        let (cx, cz) = self.centre();
        let (wx, wz) = (fbm(g ^ 0x51, x / 3000.0, z / 3000.0, 3) * 1500.0, fbm(g ^ 0x52, x / 3000.0, z / 3000.0, 3) * 1500.0);
        let d = (x - cx + wx).hypot(z - cz + wz) / RAYON_ILE;
        rampe(1.08, 0.80, d)
    }

    /// Le poids de la chaîne de montagnes en (x, z), de 0 à 1.
    fn chaine(&self, x: f64, z: f64) -> f64 {
        let (cx, cz) = self.centre();
        let a = hache(self.graine, 77, 0) * TAU;
        let (dir, nrm) = ((a.cos(), a.sin()), (-a.sin(), a.cos()));
        let (px, pz) = (x - cx, z - cz);
        let le_long = px * dir.0 + pz * dir.1;
        // Elle serpente un peu
        let travers = px * nrm.0 + pz * nrm.1 - CHAINE_DECALAGE + fbm(self.graine ^ 0x53, le_long / 2500.0, 3.7, 2) * 700.0;
        (-(travers / CHAINE_LARGE).powi(2)).exp() * rampe(CHAINE_LONG, CHAINE_LONG * 0.45, le_long.abs())
    }

    /// La hauteur du sol en un point absolu : le relief, son érosion, le détail qui se
    /// voit à pied.
    pub fn hauteur(&self, x: f64, z: f64) -> f64 {
        if let Some(m) = &self.monde {
            return m.hauteur(x, z);
        }
        if let Some(r) = &self.reel {
            return r.hauteur(x, z);
        }
        let ecart = self.erosion.as_ref().map_or(0.0, |e| e.ecart(x, z));
        let detail = fbm(self.graine ^ 0x59, x / 40.0, z / 40.0, 2) * 1.2 * self.terre(x, z);
        self.relief(x, z) + ecart + detail
    }

    /// Le relief à grande échelle, avant l'érosion et sans le détail.
    pub fn relief(&self, x: f64, z: f64) -> f64 {
        let g = self.graine;
        let terre = self.terre(x, z);
        // Le fond de la mer, un peu inégal
        let fond = FOND + fbm(g ^ 0x61, x / 900.0, z / 900.0, 2) * 8.0;
        if terre <= 0.0 {
            return fond;
        }
        // Les coordonnées tordues : les formes perdent l'air de grille
        let (ux, uz) = (x + fbm(g ^ 0x54, x / 900.0, z / 900.0, 2) * 260.0, z + fbm(g ^ 0x55, x / 900.0, z / 900.0, 2) * 260.0);
        // Plaines ou collines, par grandes zones
        let zone = fbm(g ^ 0x56, x / 2600.0, z / 2600.0, 2) * 0.5 + 0.5;
        let collines = (fbm(g ^ 0x57, ux / 700.0, uz / 700.0, 5) * 0.5 + 0.5) * 70.0 * (0.15 + 0.85 * rampe(0.3, 0.75, zone));
        // La chaîne, en crêtes
        let w = self.chaine(x, z);
        let mont = if w > 1e-3 { SOMMET * 1.22 * w * cretes(g ^ 0x58, ux / 1500.0, uz / 1500.0, 6).powf(1.2) } else { 0.0 };
        // Vers le rivage, le relief s'efface : des plages en pente douce
        let t3 = terre * terre * terre;
        let sol = 2.5 + (collines + mont) * t3;
        fond + (sol - fond) * terre
    }

    /// Le sable des plages, l'herbe plus ou moins sèche, la lande en altitude, la roche
    /// dans les pentes et vers les sommets, le fond de la mer.
    pub fn couleur(&self, x: f64, z: f64, h: f64, n: [f32; 3]) -> [f32; 3] {
        let melange = |a: [f32; 3], b: [f32; 3], t: f64| -> [f32; 3] { [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t as f32) };
        if let Some(m) = &self.monde {
            return couleur_lieu(&**m, x, z, h, n);
        }
        if let Some(r) = &self.reel {
            return couleur_lieu(&**r, x, z, h, n);
        }
        let herbe = [0.47, 0.60, 0.36];
        let seche = [0.66, 0.65, 0.44];
        let lande = [0.48, 0.50, 0.36];
        let roche = [0.56, 0.54, 0.50];
        let claire = [0.78, 0.77, 0.74];
        let sable = [0.86, 0.80, 0.62];
        let mouille = [0.62, 0.58, 0.46];
        let s = bruit(self.graine ^ 0xA5A5, x / 60.0, z / 60.0);
        let mut c = melange(herbe, seche, s * 0.8);
        c = melange(c, lande, rampe(250.0, 450.0, h));
        c = melange(c, roche, rampe(520.0, 700.0, h));
        c = melange(c, claire, rampe(760.0, 880.0, h));
        // La roche dans les pentes
        c = melange(c, roche, ((0.90 - n[1] as f64) / 0.12).clamp(0.0, 1.0));
        // Ce qu'a laissé l'érosion : les fonds de vallée plus verts (sous la lande), la
        // roche à nu dans les ravines, la terre déposée plus brune
        if let Some(e) = &self.erosion {
            let (w, d) = (e.eau(x, z), e.ecart(x, z));
            c = melange(c, [0.36, 0.54, 0.29], rampe(0.62, 0.80, w) * rampe(500.0, 300.0, h) * 0.75);
            c = melange(c, [0.50, 0.48, 0.45], rampe(-3.0, -18.0, d) * 0.6);
            c = melange(c, [0.55, 0.50, 0.38], rampe(3.0, 14.0, d) * 0.35);
        }
        // Le sable au bord de l'eau, mouillé dessous, puis le fond
        c = melange(c, sable, rampe(4.0, 2.0, h) * rampe(0.80, 0.95, n[1] as f64));
        c = melange(c, mouille, rampe(0.3, -0.8, h));
        melange(c, [0.40, 0.44, 0.40], rampe(-2.0, -15.0, h))
    }
}

/// La teinte du sol d'une classe d'occupation (`reel::BOIS`…) en (x, z) ; `s`, le bruit
/// de l'herbe.
fn occupation(classe: u8, x: f64, z: f64, s: f64) -> [f32; 3] {
    use crate::reel::{BOIS, CHAMP, HABITE, PRE, ZONE};
    let melange = |a: [f32; 3], b: [f32; 3], t: f64| -> [f32; 3] { [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t as f32) };
    match classe {
        BOIS => melange([0.20, 0.31, 0.16], [0.27, 0.36, 0.19], s),
        PRE => melange([0.38, 0.58, 0.28], [0.46, 0.60, 0.32], s * 0.7),
        // Des parcelles : des blés, des labours, des cultures vertes, en taches de 150 m
        CHAMP => {
            let p = bruit(0x5EED, x / 150.0, z / 150.0);
            let c = melange([0.52, 0.62, 0.32], [0.74, 0.68, 0.42], rampe(0.35, 0.45, p));
            melange(c, [0.52, 0.44, 0.32], rampe(0.62, 0.70, p))
        }
        HABITE => melange([0.50, 0.53, 0.42], [0.56, 0.56, 0.50], s),
        ZONE => melange([0.55, 0.54, 0.50], [0.60, 0.58, 0.54], s),
        _ => melange([0.45, 0.58, 0.34], [0.60, 0.62, 0.42], s * 0.8),
    }
}

impl Source for Campagne {
    /// Les bâtiments enrichis du monde matérialisé.
    fn enrichis(&self, i: i32, j: i32) -> Vec<crate::moteur::tuiles::Enrichi> {
        let Some(m) = &self.monde else { return Vec::new() };
        m.enrichis(i, j)
            .into_iter()
            .filter_map(|b| {
                let d = b.detail?;
                Some(crate::moteur::tuiles::Enrichi { id: b.id, pos: b.position, cap: b.cap, cle: d.cle, modele: d.modele })
            })
            .collect()
    }

    /// Les bâtiments et les ponts de la tuile, et ses arbres.
    fn bati(&self, i: i32, j: i32) -> Option<crate::moteur::Bati3d> {
        if let Some(m) = &self.monde {
            return m.bati(i, j);
        }
        let mut b = self.reel.as_ref()?.bati(i, j).unwrap_or_default();
        let (va, ta) = crate::vegetation::arbres(self, i, j);
        b.sommets.extend(va);
        b.triangles.extend(ta);
        b.non_vide()
    }

    fn tuile(&self, i: i32, j: i32) -> Contenu {
        let (ox, oz) = (i as f64 * TAILLE as f64, j as f64 * TAILLE as f64);
        let n = GRILLE;
        let pas = TAILLE as f64 / n as f64;
        // Les hauteurs avec une marge d'un point : les normales, par différences centrées,
        // sont les mêmes des deux côtés d'un bord
        let m = n + 3;
        let mut g = Vec::with_capacity(m * m);
        for b in 0..m {
            for a in 0..m {
                g.push(self.hauteur(ox + (a as f64 - 1.0) * pas, oz + (b as f64 - 1.0) * pas));
            }
        }
        let at = |a: usize, b: usize| g[(b + 1) * m + a + 1];
        let k = (n + 1) * (n + 1);
        let (mut h, mut normales, mut couleurs) = (Vec::with_capacity(k), Vec::with_capacity(k), Vec::with_capacity(k));
        for b in 0..=n {
            for a in 0..=n {
                let y = at(a, b);
                let dx = (g[(b + 1) * m + a + 2] - g[(b + 1) * m + a]) / (2.0 * pas);
                let dz = (g[(b + 2) * m + a + 1] - g[b * m + a + 1]) / (2.0 * pas);
                let l = (dx * dx + dz * dz + 1.0).sqrt();
                let nm = [(-dx / l) as f32, (1.0 / l) as f32, (-dz / l) as f32];
                h.push(y as f32);
                normales.push(nm);
                couleurs.push(self.couleur(ox + a as f64 * pas, oz + b as f64 * pas, y, nm));
            }
        }
        Contenu { relief: Relief { n, h }, normales, couleurs, objets: Vec::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deux_tuiles_voisines_s_accordent_sur_leur_bord() {
        let c = Campagne::brute(7);
        for (i, j) in [(0, 0), (-3, 5), (40, -20), (40_000, -12_345)] {
            let (a, b, d) = (c.tuile(i, j), c.tuile(i + 1, j), c.tuile(i, j + 1));
            for k in 0..=GRILLE {
                assert_eq!(a.relief.sommet(GRILLE, k), b.relief.sommet(0, k), "bord est de ({i}, {j})");
                assert_eq!(a.relief.sommet(k, GRILLE), d.relief.sommet(k, 0), "bord sud de ({i}, {j})");
                assert_eq!(a.normales[k * (GRILLE + 1) + GRILLE], b.normales[k * (GRILLE + 1)], "normale au bord est de ({i}, {j})");
            }
        }
    }

    #[test]
    fn la_campagne_est_toujours_la_meme_et_nue() {
        let (a, b) = (Campagne::brute(3).tuile(2, -1), Campagne::brute(3).tuile(2, -1));
        assert_eq!(a.relief, b.relief);
        assert_ne!(Campagne::brute(4).tuile(2, -1).relief, a.relief);
        assert!(a.objets.is_empty());
    }

    #[test]
    fn une_ile_au_milieu_de_la_mer_avec_sa_chaine() {
        for graine in [1, 2, 3] {
            let c = Campagne::brute(graine);
            // Le départ est à terre, les coins de la carte au large
            assert!(c.hauteur(32.0, 32.0) > MER as f64 + 1.0, "graine {graine}");
            for (x, z) in [(-9900.0, -9900.0), (9900.0, 9900.0), (-9900.0, 9900.0), (9900.0, -9900.0), (0.0, 9900.0)] {
                assert!(c.hauteur(x, z) < -15.0, "graine {graine} : ({x}, {z}) {}", c.hauteur(x, z));
            }
            // L'île fait à peu près 15 km : sa terre, comptée sur une grille de 250 m
            let mut terre = 0;
            let mut sommet: f64 = 0.0;
            for b in -40..=40 {
                for a in -40..=40 {
                    let (x, z) = (a as f64 * 250.0, b as f64 * 250.0);
                    let h = c.hauteur(x, z);
                    if h > 0.0 {
                        terre += 1;
                    }
                    sommet = sommet.max(h);
                }
            }
            let km2 = terre as f64 * 0.0625;
            assert!((100.0..260.0).contains(&km2), "graine {graine} : {km2} km²");
            assert!((550.0..1100.0).contains(&sommet), "graine {graine} : sommet à {sommet} m");
        }
    }

    #[test]
    fn le_depart_reste_praticable() {
        let c = Campagne::brute(1);
        let t = c.tuile(0, 0);
        let p = t.relief.pas();
        let raide = (0..GRILLE).flat_map(|b| (0..GRILLE).map(move |a| (a, b))).map(|(a, b)| ((t.relief.sommet(a + 1, b) - t.relief.sommet(a, b)) / p).abs()).fold(0.0, f32::max);
        assert!(raide < 0.8, "pente au plus {raide}");
    }
}
