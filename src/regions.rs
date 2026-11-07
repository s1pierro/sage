//! Le monde matérialisé : les régions `.stuff` (voir `docs/format-stuff.md`), écrites une
//! fois à partir d'un lieu réel et de ses sources, et relues par le jeu à la place du
//! `.reel`.
//!
//! - Une région couvre 16 × 16 tuiles (1 024 m). Elle porte le relief à 2 m (routes déjà
//!   aplanies), l'eau, l'occupation du sol à 8 m, les routes (pour la couleur du sol), les
//!   bâtiments et les morceaux de pont en instances procédurales, les arbres en tableau, et
//!   sa silhouette lointaine (relief, blocs des bâtiments, canopée, fond de la carte).
//! - Le jeu charge les régions à la demande, et en garde quelques-unes (`CAPACITE`). Au
//!   lancement, il ne lit de chaque région que son manifeste et ses silhouettes.
//! - Tout ce qui se calcule ici reprend les calculs du lieu réel : le monde matérialisé
//!   se voit comme le lieu dont il vient (aux arrondis près).

use crate::campagne::{Campagne, Sol};
use crate::moteur::stuff::{Fichier, Stuff};
use crate::moteur::tuiles::DEMI_CARTE;
use crate::moteur::TAILLE;
use crate::reel::{Batiment, Index, Proche, Reel, Route};
use crate::vegetation::Arbre;
use gda::json::{self, J};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Tuiles par côté d'une région.
pub const TUILES: i32 = 16;
/// Le côté d'une région, en mètres.
pub const METRES: f64 = TUILES as f64 * TAILLE as f64;
/// Le relief : un point tous les 2 m, bords compris.
pub const PAS: f64 = 2.0;
pub const N: usize = 513;
/// L'occupation du sol : un point tous les 8 m.
pub const PAS_SOL: f64 = 8.0;
pub const N_SOL: usize = 129;
/// Le dossier des régions, dans celui du monde, et la fiche des sources.
pub const DOSSIER: &str = "regions";
pub const SOURCES: &str = "sources.json";
/// Le lieu réel source, dans le dossier du monde.
pub const LIEU: &str = "lieu.reel";
/// Les régions gardées en mémoire au plus.
const CAPACITE: usize = 24;

pub fn application() -> String {
    format!("S.A.G.E. {}", env!("CARGO_PKG_VERSION"))
}

/// La région de la tuile `i` (sur un axe).
pub fn de_tuile(i: i32) -> i32 {
    i.div_euclid(TUILES)
}

/// Les régions de la carte, sur un axe.
pub fn etendue() -> std::ops::RangeInclusive<i32> {
    de_tuile(-DEMI_CARTE)..=de_tuile(DEMI_CARTE)
}

/// Toutes les régions de la carte.
pub fn toutes() -> Vec<(i32, i32)> {
    etendue().flat_map(|rj| etendue().map(move |ri| (ri, rj))).collect()
}

pub fn nom_fichier(ri: i32, rj: i32) -> String {
    format!("{ri}_{rj}.stuff")
}

fn coin(r: i32) -> f64 {
    r as f64 * METRES
}

/// Les tuiles de la région sur un axe, rognées à la carte.
fn tuiles_dans(r: i32) -> std::ops::Range<i32> {
    (r * TUILES).max(-DEMI_CARTE)..((r + 1) * TUILES).min(DEMI_CARTE + 1)
}

// ---------------------------------------------------------------------------------------
// Petits outils JSON

fn num(x: f64) -> J {
    J::Num((x * 1e3).round() / 1e3)
}

fn n4(x: f32) -> J {
    J::Num(((x as f64) * 1e4).round() / 1e4)
}

fn liste(v: &[f32]) -> J {
    J::Liste(v.iter().map(|x| num(*x as f64)).collect())
}

fn txt(s: &str) -> J {
    J::Txt(s.into())
}

fn objet(o: Vec<(&str, J)>) -> J {
    J::Objet(o.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn couleur(c: [f32; 3]) -> J {
    txt(&format!("#{}", crate::moteur::objets::hex(c)))
}

fn lire_couleur(j: Option<&J>) -> Option<[f32; 3]> {
    crate::moteur::objets::depuis_hex(j?.txt()?.trim_start_matches('#'))
}

fn f(j: Option<&J>) -> Option<f32> {
    j?.num().map(|x| x as f32)
}

const TOITS: [&str; 4] = ["plat", "deux_pans", "croupe", "appentis"];

// ---------------------------------------------------------------------------------------
// La région

/// La forme d'un bâtiment dans le repère de son instance : les paramètres du générateur
/// `batiment` (hauteurs depuis la position, contour autour d'elle).
#[derive(Clone, Debug, PartialEq)]
pub struct Forme {
    pub contour: Vec<[f32; 2]>,
    pub pied: f32,
    pub egout: f32,
    pub faitage: f32,
    /// `toits::PLAT`…
    pub toit: u8,
    /// L'axe du toit (radians), dans le repère de l'instance.
    pub axe: f32,
    pub murs: [f32; 3],
    pub couverture: [f32; 3],
}

/// Un bâtiment de la région : son instance (tuile, position dans la tuile, cap), sa forme,
/// d'où il vient, et le bâtiment monté (contour et hauteurs absolus).
#[derive(Clone, Debug, PartialEq)]
pub struct Bati {
    pub id: String,
    pub tuile: (i32, i32),
    pub position: [f32; 3],
    /// La rotation autour de la verticale (radians), comme `objets::tourner`.
    pub cap: f32,
    pub forme: Forme,
    pub provenance: String,
    pub retouche: bool,
    /// Démoli : il garde sa place pour qu'une régénération ne le fasse pas revenir.
    pub masquee: bool,
    pub b: Batiment,
    /// Enrichi : le modèle détaillé de sa définition propre (`enrichi`).
    pub detail: Option<Detail>,
}

/// Le niveau détaillé d'un bâtiment enrichi : sa définition dans la région, et son modèle
/// dans le repère de l'instance.
#[derive(Clone)]
pub struct Detail {
    /// L'identifiant de la définition.
    pub def: String,
    pub modele: Arc<crate::catalogue::Decor>,
    /// La clé du modèle dans le moteur : elle change avec sa géométrie.
    pub cle: String,
    /// Pas encore écrite dans la région.
    pub neuve: bool,
}

impl Detail {
    pub fn new(def: String, modele: Arc<crate::catalogue::Decor>, neuve: bool) -> Detail {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        modele.sommets.len().hash(&mut h);
        for x in &modele.sommets {
            x.to_bits().hash(&mut h);
        }
        modele.materiaux.iter().for_each(|m| m.source.as_ref().map(|s| s.len()).hash(&mut h));
        Detail { cle: format!("def/{def}/{:016x}", h.finish()), def, modele, neuve }
    }
}

impl PartialEq for Detail {
    fn eq(&self, o: &Detail) -> bool {
        self.def == o.def && self.cle == o.cle && self.neuve == o.neuve
    }
}

impl std::fmt::Debug for Detail {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Detail({}, {} triangles)", self.cle, self.modele.triangles.len())
    }
}

impl Bati {
    pub fn new(id: String, tuile: (i32, i32), position: [f32; 3], cap: f32, forme: Forme, provenance: String) -> Bati {
        let b = Batiment::plat(crate::reel::MAISON, 0.0, 0.0, Vec::new());
        let mut x = Bati { id, tuile, position, cap, forme, provenance, retouche: false, masquee: false, b, detail: None };
        x.refaire();
        x
    }

    /// Un bâtiment du lieu réel, sans rotation, sa position au centre du contour et au
    /// pied des murs.
    pub fn du_lieu(id: String, b: &Batiment) -> Bati {
        let c = b.centre();
        let tuile = ((c[0] / TAILLE).floor() as i32, (c[1] / TAILLE).floor() as i32);
        let (murs, couverture) = crate::reel::teintes_bati(b.genre);
        let forme = Forme { contour: b.contour.iter().map(|p| [p[0] - c[0], p[1] - c[1]]).collect(), pied: 0.0, egout: b.egout - b.pied, faitage: b.toit - b.pied, toit: b.forme, axe: b.axe, murs, couverture };
        Bati::new(id, tuile, [c[0] - tuile.0 as f32 * TAILLE, b.pied, c[1] - tuile.1 as f32 * TAILLE], 0.0, forme, "osm+lidar".into())
    }

    /// Un bâtiment neuf posé à la main, centré en (x, z) absolus, au cap `cap` : une
    /// maison de 10 × 8 m à deux pans, le pied un peu sous le sol le plus bas de son
    /// contour.
    pub fn neuf(m: &Regions, x: f64, z: f64, cap: f32) -> Bati {
        let contour = vec![[-5.0, -4.0], [5.0, -4.0], [5.0, 4.0], [-5.0, 4.0]];
        let sol = |p: &[f32; 2]| {
            let q = crate::moteur::objets::tourner([p[0], 0.0, p[1]], cap);
            m.hauteur(x + q[0] as f64, z + q[2] as f64) as f32
        };
        let bas = contour.iter().map(sol).fold(f32::MAX, f32::min);
        let centre = m.hauteur(x, z) as f32;
        let y = bas.min(centre) - 0.2;
        let (murs, couverture) = crate::reel::teintes_bati(crate::reel::MAISON);
        let egout = centre - y + 6.0;
        let forme = Forme { contour, pied: 0.0, egout, faitage: egout + 3.5, toit: crate::toits::DEUX_PANS, axe: 0.0, murs, couverture };
        let tuile = ((x / TAILLE as f64).floor() as i32, (z / TAILLE as f64).floor() as i32);
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        let mut b = Bati::new(format!("main-{n:x}"), tuile, [(x - tuile.0 as f64 * TAILLE as f64) as f32, y, (z - tuile.1 as f64 * TAILLE as f64) as f32], cap, forme, "main".into());
        b.retouche = true;
        b
    }

    /// Posé à la main : démoli, il n'a pas de place à garder et quitte le fichier.
    pub fn a_oter(&self) -> bool {
        self.masquee && self.provenance == "main"
    }

    /// La position, absolue.
    pub fn origine(&self) -> [f32; 3] {
        [self.tuile.0 as f32 * TAILLE + self.position[0], self.position[1], self.tuile.1 as f32 * TAILLE + self.position[2]]
    }

    /// Remonte le bâtiment absolu d'après l'instance et la forme.
    pub fn refaire(&mut self) {
        let o = self.origine();
        let f = &self.forme;
        let t = |p: [f32; 2]| {
            let q = crate::moteur::objets::tourner([p[0], 0.0, p[1]], self.cap);
            [o[0] + q[0], o[2] + q[2]]
        };
        let u = crate::moteur::objets::tourner([f.axe.cos(), 0.0, f.axe.sin()], self.cap);
        self.b = Batiment { genre: crate::reel::MAISON, pied: o[1] + f.pied, toit: o[1] + f.faitage.max(f.egout), egout: o[1] + f.egout, forme: f.toit, axe: u[2].atan2(u[0]), contour: f.contour.iter().map(|p| t(*p)).collect() };
    }

    /// Le point absolu (x, z) est-il dans le contour, ou à moins de `marge` d'un mur ?
    pub fn touche(&self, x: f32, z: f32, marge: f32) -> bool {
        let c = &self.b.contour;
        let mut dedans = false;
        for e in 0..c.len() {
            let (a, b) = (c[e], c[(e + 1) % c.len()]);
            let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
            let l2 = dx * dx + dz * dz;
            let t = if l2 > 0.0 { (((x - a[0]) * dx + (z - a[1]) * dz) / l2).clamp(0.0, 1.0) } else { 0.0 };
            if (x - a[0] - dx * t).hypot(z - a[1] - dz * t) < marge {
                return true;
            }
            if (a[1] > z) != (b[1] > z) && x < a[0] + (z - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
                dedans = !dedans;
            }
        }
        dedans
    }

    /// Ses triangles dans le repère de la tuile (i, j) : sommets (position, normale,
    /// couleur) et triangles de collision.
    pub fn monter(&self, i: i32, j: i32, v: &mut Vec<f32>, t: &mut Vec<[[f32; 3]; 3]>) {
        let (ox, oz) = (i as f32 * TAILLE, j as f32 * TAILLE);
        let mut tri = |p: [[f32; 3]; 3], n: [f32; 3], couleur: [f32; 3]| {
            let p = p.map(|q| [q[0] - ox, q[1], q[2] - oz]);
            for q in p {
                v.extend_from_slice(&q);
                v.extend_from_slice(&n);
                v.extend_from_slice(&couleur);
            }
            t.push(p);
        };
        crate::toits::monter(&self.b, self.forme.murs, self.forme.couverture, &mut tri);
    }

    /// Son instance en JSON. Partant de l'ancienne, les clés inconnues restent.
    pub fn instance(&self, ancienne: Option<&J>) -> J {
        let mut o = match ancienne {
            Some(J::Objet(o)) => o.clone(),
            _ => Vec::new(),
        };
        let params = self.params(ancienne.and_then(|a| a.get("procedural")).and_then(|p| p.get("params")));
        let mut proc = match ancienne.and_then(|a| a.get("procedural")) {
            Some(J::Objet(p)) => p.clone(),
            _ => Vec::new(),
        };
        mettre(&mut proc, "generateur", Some(txt("batiment")));
        mettre(&mut proc, "params", Some(params));
        mettre(&mut o, "id", Some(txt(&self.id)));
        // Enrichi, il renvoie à sa définition ; sinon il porte ses paramètres
        match &self.detail {
            Some(d) => {
                mettre(&mut o, "definition", Some(txt(&d.def)));
                mettre(&mut o, "procedural", None);
            }
            None => {
                mettre(&mut o, "definition", None);
                mettre(&mut o, "procedural", Some(J::Objet(proc)));
            }
        }
        mettre(&mut o, "tuile", Some(J::Liste(vec![J::Num(self.tuile.0 as f64), J::Num(self.tuile.1 as f64)])));
        mettre(&mut o, "position", Some(liste(&self.position)));
        let (s, c) = (self.cap / 2.0).sin_cos();
        mettre(&mut o, "rotation", (self.cap.abs() > 1e-6).then(|| J::Liste(vec![J::Num(0.0), n4(s), J::Num(0.0), n4(c)])));
        mettre(&mut o, "provenance", Some(txt(&self.provenance)));
        mettre(&mut o, "retouche", Some(J::Bool(self.retouche)));
        mettre(&mut o, "masquee", self.masquee.then_some(J::Bool(true)));
        J::Objet(o)
    }

    /// Les paramètres du générateur `batiment` ; partant des anciens, les clés inconnues
    /// restent.
    pub fn params(&self, anciens: Option<&J>) -> J {
        let f = &self.forme;
        let mut params = match anciens {
            Some(J::Objet(p)) => p.clone(),
            _ => Vec::new(),
        };
        mettre(&mut params, "contour", Some(J::Liste(f.contour.iter().map(|p| liste(p)).collect())));
        mettre(&mut params, "pied", Some(num(f.pied as f64)));
        mettre(&mut params, "egout", Some(num(f.egout as f64)));
        mettre(&mut params, "faitage", Some(num(f.faitage as f64)));
        mettre(&mut params, "toit", Some(txt(TOITS.get(f.toit as usize).unwrap_or(&"plat"))));
        mettre(&mut params, "axe", Some(n4(f.axe)));
        mettre(&mut params, "murs", Some(couleur(f.murs)));
        mettre(&mut params, "couverture", Some(couleur(f.couverture)));
        J::Objet(params)
    }

    /// Relu d'une instance `batiment` ; rien si ce n'en est pas une.
    pub fn depuis_instance(i: &J) -> Option<Bati> {
        let p = i.get("procedural")?;
        if p.get("generateur").and_then(J::txt) != Some("batiment") {
            return None;
        }
        Bati::depuis_params(i, p.get("params")?)
    }

    /// Relu d'une instance et des paramètres du générateur `batiment` (les siens, ou ceux
    /// du premier niveau de sa définition).
    pub fn depuis_params(i: &J, params: &J) -> Option<Bati> {
        let t = i.get("tuile")?.nombres::<2>()?;
        let pos = i.get("position")?.nombres::<3>()?;
        let contour: Vec<[f32; 2]> = params.get("contour").and_then(J::liste).unwrap_or(&[]).iter().filter_map(|q| q.nombres::<2>()).map(|q| [q[0] as f32, q[1] as f32]).collect();
        if contour.len() < 3 {
            return None;
        }
        let faitage = f(params.get("faitage")).unwrap_or(3.0);
        let (m, c) = crate::reel::teintes_bati(crate::reel::MAISON);
        let forme = Forme {
            contour,
            pied: f(params.get("pied")).unwrap_or(0.0),
            egout: f(params.get("egout")).unwrap_or(faitage),
            faitage,
            toit: params.get("toit").and_then(J::txt).and_then(|t| TOITS.iter().position(|x| *x == t)).unwrap_or(0) as u8,
            axe: f(params.get("axe")).unwrap_or(0.0),
            murs: lire_couleur(params.get("murs")).unwrap_or(m),
            couverture: lire_couleur(params.get("couverture")).unwrap_or(c),
        };
        let cap = i.get("rotation").and_then(|r| r.nombres::<4>()).map_or(0.0, |q| 2.0 * (q[1] as f32).atan2(q[3] as f32));
        let mut b = Bati::new(i.get("id").and_then(J::txt).unwrap_or("").to_string(), (t[0] as i32, t[1] as i32), pos.map(|x| x as f32), cap, forme, i.get("provenance").and_then(J::txt).unwrap_or("").to_string());
        b.retouche = i.get("retouche").and_then(J::bool) == Some(true);
        b.masquee = i.get("masquee").and_then(J::bool) == Some(true);
        Some(b)
    }
}

/// Une clé d'objet JSON : remplacée, ajoutée, ou ôtée (`None`).
fn mettre(o: &mut Vec<(String, J)>, cle: &str, v: Option<J>) {
    match (o.iter().position(|(c, _)| c == cle), v) {
        (Some(k), Some(v)) => o[k].1 = v,
        (Some(k), None) => {
            o.remove(k);
        }
        (None, Some(v)) => o.push((cle.into(), v)),
        (None, None) => {}
    }
}

/// Un morceau de pont, de `a` à `b` (x, z, hauteur du tablier, absolus).
#[derive(Clone, Debug, PartialEq)]
pub struct Pont {
    pub id: String,
    pub tuile: (i32, i32),
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub largeur: f32,
    pub teinte: [f32; 3],
}

/// La silhouette lointaine d'une région : sommets (position absolue, normale, couleur),
/// et le fond de la carte, un point (hauteur, couleur) par tuile de la carte.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Silhouette {
    pub relief: Vec<f32>,
    pub bati: Vec<f32>,
    pub bois: Vec<f32>,
    pub carte: Vec<crate::carte::Point>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub ri: i32,
    pub rj: i32,
    pub niveau: f32,
    pub base: f32,
    pub echelle: f32,
    /// N × N hauteurs (`base + v × echelle`), le sol des routes compris.
    pub h: Vec<u16>,
    /// Un bit par point du relief : de l'eau.
    pub eau: Vec<u8>,
    /// N_SOL × N_SOL classes d'occupation.
    pub sol: Vec<u8>,
    /// Les routes qui touchent la région, absolues.
    pub routes: Vec<Route>,
    index: Index,
    pub batiments: Vec<Bati>,
    pub ponts: Vec<Pont>,
    /// Les arbres, rangés par tuile ; pour chaque tuile (j puis i), son premier arbre et
    /// leur nombre.
    pub arbres: Vec<Arbre>,
    pub par_tuile: Vec<[u32; 2]>,
    pub silhouette: Silhouette,
}

/// La clé d'un arbre : sa cellule sur la carte, sur 32 bits.
fn cle_arbre(gi: i64, gj: i64) -> u32 {
    (((gi + 32768) as u32) << 16) | ((gj + 32768) as u32 & 0xFFFF)
}

fn cellule(cle: u32) -> (i64, i64) {
    ((cle >> 16) as i64 - 32768, (cle & 0xFFFF) as i64 - 32768)
}

impl Region {
    /// Le rang de la tuile (i, j) dans la région.
    fn rang(&self, i: i32, j: i32) -> Option<usize> {
        let (a, b) = (i - self.ri * TUILES, j - self.rj * TUILES);
        ((0..TUILES).contains(&a) && (0..TUILES).contains(&b)).then_some((b * TUILES + a) as usize)
    }

    fn point(&self, a: usize, b: usize) -> f64 {
        (self.base + self.h[b.min(N - 1) * N + a.min(N - 1)] as f32 * self.echelle) as f64
    }

    fn est_eau(&self, a: usize, b: usize) -> bool {
        let i = b.min(N - 1) * N + a.min(N - 1);
        self.eau[i / 8] & (1 << (i % 8)) != 0
    }

    /// Une région tirée du lieu réel de `c`.
    pub fn materialiser(c: &Campagne, ri: i32, rj: i32) -> Region {
        let r = c.reel.as_ref().expect("un lieu réel à matérialiser");
        let (x0, z0) = (coin(ri), coin(rj));
        // Le relief, routes aplanies, et l'eau
        let mut hs = Vec::with_capacity(N * N);
        let mut eau = vec![0u8; (N * N).div_ceil(8)];
        for b in 0..N {
            for a in 0..N {
                let (x, z) = (x0 + a as f64 * PAS, z0 + b as f64 * PAS);
                hs.push(r.hauteur(x, z) as f32);
                if r.eau(x, z) >= 0.5 {
                    let i = b * N + a;
                    eau[i / 8] |= 1 << (i % 8);
                }
            }
        }
        let lo = hs.iter().cloned().fold(f32::MAX, f32::min);
        let hi = hs.iter().cloned().fold(f32::MIN, f32::max);
        let echelle = ((hi - lo) / 65535.0).max(0.001);
        let h = hs.iter().map(|v| ((v - lo) / echelle).round().clamp(0.0, 65535.0) as u16).collect();
        // L'occupation : la classe qui pèse le plus en chaque point
        let mut sol = Vec::with_capacity(N_SOL * N_SOL);
        for b in 0..N_SOL {
            for a in 0..N_SOL {
                let o = r.occupation(x0 + a as f64 * PAS_SOL, z0 + b as f64 * PAS_SOL);
                sol.push(o.iter().fold((0u8, -1.0), |m, &(k, p)| if p > m.1 { (k, p) } else { m }).0);
            }
        }
        // Les routes qui approchent la région
        let dans = |x0b: f32, x1b: f32, z0b: f32, z1b: f32| x1b >= x0 as f32 && x0b <= (x0 + METRES) as f32 && z1b >= z0 as f32 && z0b <= (z0 + METRES) as f32;
        let routes: Vec<Route> = r
            .routes
            .iter()
            .filter(|ro| {
                let m = ro.largeur / 2.0 + crate::reel::epaule(ro.genre) + 1.0;
                ro.points.windows(2).any(|s| dans(s[0][0].min(s[1][0]) - m, s[0][0].max(s[1][0]) + m, s[0][1].min(s[1][1]) - m, s[0][1].max(s[1][1]) + m))
            })
            .cloned()
            .collect();
        // Les bâtiments dont le centre est dans une tuile de la région
        let tuile = |x: f32, z: f32| ((x / TAILLE).floor() as i32, (z / TAILLE).floor() as i32);
        let chez_nous = |t: (i32, i32)| de_tuile(t.0) == ri && de_tuile(t.1) == rj;
        let mut batiments = Vec::new();
        for (k, b) in r.batiments.iter().enumerate() {
            let c = b.centre();
            let t = tuile(c[0], c[1]);
            if chez_nous(t) {
                batiments.push(Bati::du_lieu(format!("bat-{k}"), b));
            }
        }
        let mut ponts = Vec::new();
        for (k, p) in r.ponts.iter().enumerate() {
            for (s, w) in p.points.windows(2).enumerate() {
                let t = tuile((w[0][0] + w[1][0]) / 2.0, (w[0][1] + w[1][1]) / 2.0);
                if chez_nous(t) {
                    ponts.push(Pont { id: format!("pont-{k}-{s}"), tuile: t, a: w[0], b: w[1], largeur: p.largeur, teinte: crate::reel::teinte(p.genre) });
                }
            }
        }
        // Les arbres, tuile par tuile
        let (mut arbres, mut par_tuile) = (Vec::new(), Vec::with_capacity((TUILES * TUILES) as usize));
        for j in rj * TUILES..(rj + 1) * TUILES {
            for i in ri * TUILES..(ri + 1) * TUILES {
                let debut = arbres.len() as u32;
                if i.abs() <= DEMI_CARTE && j.abs() <= DEMI_CARTE {
                    arbres.extend(crate::vegetation::plantes(r, i, j));
                }
                par_tuile.push([debut, arbres.len() as u32 - debut]);
            }
        }
        // La silhouette
        let (ti, tj) = (tuiles_dans(ri), tuiles_dans(rj));
        let canopee = |r0: i32| {
            let n = crate::vegetation::cote_canopee();
            let par = (METRES / crate::vegetation::PAS_CANOPEE as f64) as i64;
            let debut = (r0 as i64 * par + DEMI_CARTE as i64 * 2).clamp(0, n as i64 - 1) as usize;
            let fin = ((r0 as i64 + 1) * par + DEMI_CARTE as i64 * 2).clamp(0, n as i64 - 1) as usize;
            debut..fin
        };
        let silhouette = Silhouette {
            relief: crate::lointain::sommets_zone(c, ti.clone(), tj.clone()),
            bati: blocs(&batiments),
            bois: crate::vegetation::lointain_zone(c, canopee(ri), canopee(rj)),
            carte: crate::carte::fond_zone(c, ti, tj),
        };
        let index = crate::reel::indexer(&routes);
        Region { ri, rj, niveau: r.niveau, base: lo, echelle, h, eau, sol, routes, index, batiments, ponts, arbres, par_tuile, silhouette }
    }

    /// Le fichier `.stuff` de la région.
    pub fn stuff(&self) -> Stuff {
        let (ri, rj) = (self.ri, self.rj);
        let (x0, z0) = (coin(ri) as f32, coin(rj) as f32);
        let mut s = Stuff::new(&format!("region/{ri}_{rj}"), "region", &application());
        // Les instances : bâtiments et morceaux de pont, leurs paramètres dans leur repère
        let mut instances = Vec::new();
        for b in &self.batiments {
            instances.push(b.instance(None));
        }
        for p in &self.ponts {
            let (ox, oz) = (p.tuile.0 as f32 * TAILLE, p.tuile.1 as f32 * TAILLE);
            let m = [(p.a[0] + p.b[0]) / 2.0, (p.a[1] + p.b[1]) / 2.0, (p.a[2] + p.b[2]) / 2.0];
            let rel = |q: [f32; 3]| liste(&[q[0] - m[0], q[1] - m[1], q[2] - m[2]]);
            let params = objet(vec![("ligne", J::Liste(vec![rel(p.a), rel(p.b)])), ("largeur", num(p.largeur as f64)), ("teinte", couleur(p.teinte))]);
            instances.push(objet(vec![
                ("id", txt(&p.id)),
                ("procedural", objet(vec![("generateur", txt("pont")), ("params", params)])),
                ("tuile", J::Liste(vec![J::Num(p.tuile.0 as f64), J::Num(p.tuile.1 as f64)])),
                ("position", liste(&[m[0] - ox, m[2], m[1] - oz])),
                ("provenance", txt("osm")),
                ("retouche", J::Bool(false)),
            ]));
        }
        s.mettre("instances", J::Liste(instances));
        // Les arbres, en colonnes
        let a = &self.arbres;
        let mut tuile = Vec::with_capacity(a.len() * 2);
        let (mut xs, mut ys, mut zs) = (Vec::with_capacity(a.len()), Vec::with_capacity(a.len()), Vec::with_capacity(a.len()));
        for (k, t) in self.par_tuile.iter().enumerate() {
            let (i, j) = (ri * TUILES + k as i32 % TUILES, rj * TUILES + k as i32 / TUILES);
            for x in &a[t[0] as usize..(t[0] + t[1]) as usize] {
                tuile.extend_from_slice(&[i as i16, j as i16]);
                xs.push(x.pos[0] - i as f32 * TAILLE);
                ys.push(x.pos[1]);
                zs.push(x.pos[2] - j as f32 * TAILLE);
            }
        }
        let ids: Vec<u32> = a.iter().map(|x| cle_arbre(x.gi, x.gj)).collect();
        let caps: Vec<f32> = a.iter().map(|x| x.cap * std::f32::consts::TAU / 6.0).collect();
        let zeros = vec![0u8; a.len()];
        let colonnes = objet(vec![
            ("id", s.tampon(&ids)),
            ("tuile", s.tampon(&tuile)),
            ("x", s.tampon(&xs)),
            ("y", s.tampon(&ys)),
            ("z", s.tampon(&zs)),
            ("cap", s.tampon(&caps)),
            ("echelle", s.tampon(&vec![1.0f32; a.len()])),
            (
                "params",
                objet(vec![
                    ("essence", s.tampon(&a.iter().map(|x| x.conifere as u8).collect::<Vec<u8>>())),
                    ("hauteur", s.tampon(&a.iter().map(|x| x.hauteur).collect::<Vec<f32>>())),
                    ("teinte", s.tampon(&a.iter().map(|x| (x.teinte * 255.0).round() as u8).collect::<Vec<u8>>())),
                ]),
            ),
            ("provenance", s.tampon(&zeros)),
            ("retouche", s.tampon(&zeros)),
            ("masquee", s.tampon(&zeros)),
        ]);
        let tableau = objet(vec![
            ("id", txt("arbres")),
            ("procedural", objet(vec![("generateur", txt("arbre")), ("essences", J::Liste(vec![txt("feuillu"), txt("conifere")]))])),
            ("nombre", J::Num(a.len() as f64)),
            ("colonnes", colonnes),
        ]);
        s.mettre("tableaux", J::Liste(vec![tableau]));
        // Le terrain
        let index: Vec<u32> = self.par_tuile.iter().flatten().copied().collect();
        let (mut genres, mut largeurs, mut debuts, mut points) = (Vec::new(), Vec::new(), vec![0u32], Vec::new());
        for r in &self.routes {
            genres.push(r.genre);
            largeurs.push(r.largeur);
            for p in &r.points {
                points.extend_from_slice(&[p[0] - x0, p[1] - z0, p[2]]);
            }
            debuts.push((points.len() / 3) as u32);
        }
        let relatif = |v: &[f32]| -> Vec<f32> {
            let mut v = v.to_vec();
            for s in v.chunks_mut(9) {
                s[0] -= x0;
                s[2] -= z0;
            }
            v
        };
        let carte: Vec<f32> = self.silhouette.carte.iter().flat_map(|(h, c)| [*h, c[0], c[1], c[2]]).collect();
        let region = objet(vec![
            ("ri", J::Num(ri as f64)),
            ("rj", J::Num(rj as f64)),
            ("relief", objet(vec![("pas", J::Num(PAS)), ("cote", J::Num(N as f64)), ("base", J::Num(self.base as f64)), ("echelle", J::Num(self.echelle as f64)), ("hauteurs", s.tampon(&self.h))])),
            ("eau", objet(vec![("niveau", J::Num(self.niveau as f64)), ("masque", s.tampon(&self.eau))])),
            ("occupation", objet(vec![("pas", J::Num(PAS_SOL)), ("cote", J::Num(N_SOL as f64)), ("classes", s.tampon(&self.sol))])),
            ("routes", objet(vec![("nombre", J::Num(self.routes.len() as f64)), ("genres", s.tampon(&genres)), ("largeurs", s.tampon(&largeurs)), ("debuts", s.tampon(&debuts)), ("points", s.tampon(&points))])),
            ("tuiles", objet(vec![("index", s.tampon(&index))])),
            (
                "silhouette",
                objet(vec![
                    ("relief", s.ranger(1, &relatif(&self.silhouette.relief)).json()),
                    ("bati", s.ranger(1, &relatif(&self.silhouette.bati)).json()),
                    ("bois", s.ranger(1, &relatif(&self.silhouette.bois)).json()),
                    ("carte", s.ranger(1, &carte).json()),
                ]),
            ),
        ]);
        s.mettre("region", region);
        s
    }

    /// Relue d'un `.stuff` de région.
    pub fn depuis(s: &Stuff) -> Result<Region, String> {
        let g = s.get("region").ok_or("pas de région")?;
        let e = |m: &str| format!("région illisible : {m}");
        let ri = g.get("ri").and_then(J::num).ok_or_else(|| e("ri"))? as i32;
        let rj = g.get("rj").and_then(J::num).ok_or_else(|| e("rj"))? as i32;
        let (x0, z0) = (coin(ri) as f32, coin(rj) as f32);
        let rel = g.get("relief").ok_or_else(|| e("relief"))?;
        if rel.get("cote").and_then(J::num) != Some(N as f64) || rel.get("pas").and_then(J::num) != Some(PAS) {
            return Err(e("grille du relief"));
        }
        let base = f(rel.get("base")).ok_or_else(|| e("base"))?;
        let echelle = f(rel.get("echelle")).ok_or_else(|| e("echelle"))?;
        let h: Vec<u16> = s.lire_ref(rel.get("hauteurs").ok_or_else(|| e("hauteurs"))?).filter(|v: &Vec<u16>| v.len() == N * N).ok_or_else(|| e("hauteurs"))?;
        let eau_j = g.get("eau").ok_or_else(|| e("eau"))?;
        let niveau = f(eau_j.get("niveau")).ok_or_else(|| e("niveau"))?;
        let eau: Vec<u8> = s.lire_ref(eau_j.get("masque").ok_or_else(|| e("masque"))?).filter(|v: &Vec<u8>| v.len() == (N * N).div_ceil(8)).ok_or_else(|| e("masque"))?;
        let sol: Vec<u8> = g.get("occupation").and_then(|o| s.lire_ref(o.get("classes")?)).filter(|v: &Vec<u8>| v.len() == N_SOL * N_SOL).ok_or_else(|| e("occupation"))?;
        // Les routes
        let mut routes = Vec::new();
        if let Some(ro) = g.get("routes") {
            let genres: Vec<u8> = ro.get("genres").and_then(|r| s.lire_ref(r)).unwrap_or_default();
            let largeurs: Vec<f32> = ro.get("largeurs").and_then(|r| s.lire_ref(r)).unwrap_or_default();
            let debuts: Vec<u32> = ro.get("debuts").and_then(|r| s.lire_ref(r)).unwrap_or_default();
            let points: Vec<f32> = ro.get("points").and_then(|r| s.lire_ref(r)).unwrap_or_default();
            for k in 0..genres.len().min(largeurs.len()).min(debuts.len().saturating_sub(1)) {
                let (d, fin) = (debuts[k] as usize * 3, debuts[k + 1] as usize * 3);
                let p = points.get(d..fin).ok_or_else(|| e("routes"))?;
                routes.push(Route { genre: genres[k], largeur: largeurs[k], points: p.chunks(3).map(|q| [q[0] + x0, q[1] + z0, q[2]]).collect() });
            }
        }
        // Les instances procédurales ; les autres attendent leur générateur
        let (mut batiments, mut ponts) = (Vec::new(), Vec::new());
        let definitions = s.get("definitions").and_then(J::liste).unwrap_or(&[]);
        for i in s.get("instances").and_then(J::liste).unwrap_or(&[]) {
            if i.get("masquee").and_then(J::bool) == Some(true) {
                continue;
            }
            // Un bâtiment enrichi : sa définition, procédurale de loin et détaillée de près
            if let Some(d) = i.get("definition").and_then(J::txt) {
                let lu = definitions.iter().find(|x| x.get("id").and_then(J::txt) == Some(d)).and_then(|x| crate::enrichi::lire(s, x));
                if let Some((params, modele)) = lu {
                    if let Some(mut b) = Bati::depuis_params(i, &params) {
                        b.detail = Some(Detail::new(d.to_string(), modele, false));
                        batiments.push(b);
                    }
                }
                continue;
            }
            let Some(p) = i.get("procedural") else { continue };
            let (Some(t), Some(pos), Some(params)) = (i.get("tuile").and_then(|t| t.nombres::<2>()), i.get("position").and_then(|t| t.nombres::<3>()), p.get("params")) else { continue };
            let tuile = (t[0] as i32, t[1] as i32);
            let (ox, oz) = (tuile.0 as f32 * TAILLE + pos[0] as f32, tuile.1 as f32 * TAILLE + pos[2] as f32);
            let y = pos[1] as f32;
            let id = i.get("id").and_then(J::txt).unwrap_or("").to_string();
            match p.get("generateur").and_then(J::txt) {
                Some("batiment") => batiments.extend(Bati::depuis_instance(i)),
                Some("pont") => {
                    let l: Vec<[f64; 3]> = params.get("ligne").and_then(J::liste).unwrap_or(&[]).iter().filter_map(|q| q.nombres::<3>()).collect();
                    let q = |v: [f64; 3]| [ox + v[0] as f32, oz + v[1] as f32, y + v[2] as f32];
                    for w in l.windows(2) {
                        ponts.push(Pont { id: id.clone(), tuile, a: q(w[0]), b: q(w[1]), largeur: f(params.get("largeur")).unwrap_or(5.0), teinte: lire_couleur(params.get("teinte")).unwrap_or([0.32, 0.32, 0.34]) });
                    }
                }
                _ => {}
            }
        }
        // Les arbres
        let mut arbres = Vec::new();
        let mut par_tuile = vec![[0u32, 0]; (TUILES * TUILES) as usize];
        if let Some(t) = s.get("tableaux").and_then(J::liste).and_then(|l| l.iter().find(|t| t.get("id").and_then(J::txt) == Some("arbres"))) {
            let c = t.get("colonnes").ok_or_else(|| e("arbres"))?;
            let col_f = |k: &str| -> Option<Vec<f32>> { s.lire_ref(c.get(k)?) };
            let p = c.get("params").ok_or_else(|| e("arbres"))?;
            let ids: Vec<u32> = c.get("id").and_then(|r| s.lire_ref(r)).ok_or_else(|| e("arbres"))?;
            let tu: Vec<i16> = c.get("tuile").and_then(|r| s.lire_ref(r)).ok_or_else(|| e("arbres"))?;
            let (xs, ys, zs, caps) = (col_f("x").ok_or_else(|| e("x"))?, col_f("y").ok_or_else(|| e("y"))?, col_f("z").ok_or_else(|| e("z"))?, col_f("cap").ok_or_else(|| e("cap"))?);
            let ech = col_f("echelle").unwrap_or_else(|| vec![1.0; ids.len()]);
            let ess: Vec<u8> = p.get("essence").and_then(|r| s.lire_ref(r)).ok_or_else(|| e("essence"))?;
            let haut: Vec<f32> = p.get("hauteur").and_then(|r| s.lire_ref(r)).ok_or_else(|| e("hauteur"))?;
            let tei: Vec<u8> = p.get("teinte").and_then(|r| s.lire_ref(r)).ok_or_else(|| e("teinte"))?;
            let masquee: Vec<u8> = c.get("masquee").and_then(|r| s.lire_ref(r)).unwrap_or_default();
            let n = ids.len();
            if [tu.len() / 2, xs.len(), ys.len(), zs.len(), caps.len(), ess.len(), haut.len(), tei.len(), ech.len()].iter().any(|&l| l != n) {
                return Err(e("colonnes des arbres"));
            }
            // Les lignes sont triées par tuile ; une ligne masquée ne pousse pas
            let mut courante = None;
            for k in 0..n {
                let (i, j) = (tu[2 * k] as i32, tu[2 * k + 1] as i32);
                let (a, b) = (i - ri * TUILES, j - rj * TUILES);
                if !(0..TUILES).contains(&a) || !(0..TUILES).contains(&b) {
                    continue;
                }
                let rg = (b * TUILES + a) as usize;
                if masquee.get(k) == Some(&1) {
                    continue;
                }
                if courante != Some(rg) {
                    par_tuile[rg][0] = arbres.len() as u32;
                    courante = Some(rg);
                }
                par_tuile[rg][1] += 1;
                let (gi, gj) = cellule(ids[k]);
                arbres.push(Arbre {
                    gi,
                    gj,
                    pos: [i as f32 * TAILLE + xs[k], ys[k], j as f32 * TAILLE + zs[k]],
                    conifere: ess[k] == 1,
                    hauteur: haut[k] * ech[k],
                    teinte: tei[k] as f32 / 255.0,
                    cap: caps[k] * 6.0 / std::f32::consts::TAU,
                });
            }
        }
        let index = crate::reel::indexer(&routes);
        Ok(Region { ri, rj, niveau, base, echelle, h, eau, sol, routes, index, batiments, ponts, arbres, par_tuile, silhouette: Silhouette::default() })
    }

    /// Les bâtiments, les ponts et les arbres de la tuile (i, j), dans son repère ; les
    /// bâtiments retouchés (`retouches`, par identifiant) à la place des leurs.
    pub fn bati(&self, i: i32, j: i32, retouches: Option<&HashMap<String, Bati>>) -> Option<(Vec<f32>, Vec<[[f32; 3]; 3]>)> {
        let rg = self.rang(i, j)?;
        let (ox, oz) = (i as f32 * TAILLE, j as f32 * TAILLE);
        let (mut v, mut t) = (Vec::new(), Vec::new());
        for p in self.ponts.iter().filter(|p| p.tuile == (i, j)) {
            let (a, b) = ([p.a[0] - ox, p.a[1] - oz, p.a[2]], [p.b[0] - ox, p.b[1] - oz, p.b[2]]);
            crate::reel::monter_pont(&mut v, &mut t, a, b, p.largeur, p.teinte);
        }
        let retouche = |id: &str| retouches.is_some_and(|r| r.contains_key(id));
        for b in self.batiments.iter().filter(|b| b.tuile == (i, j) && !retouche(&b.id) && b.detail.is_none()) {
            b.monter(i, j, &mut v, &mut t);
        }
        for b in retouches.into_iter().flat_map(|r| r.values()).filter(|b| b.tuile == (i, j) && !b.masquee && b.detail.is_none()) {
            b.monter(i, j, &mut v, &mut t);
        }
        let [d, n] = self.par_tuile[rg];
        let (va, ta) = crate::vegetation::monter(&self.arbres[d as usize..(d + n) as usize], i, j);
        v.extend(va);
        t.extend(ta);
        (!v.is_empty()).then_some((v, t))
    }

    /// Les bâtiments enrichis de la tuile (i, j), retouches comprises.
    pub fn enrichis(&self, i: i32, j: i32, retouches: Option<&HashMap<String, Bati>>) -> Vec<Bati> {
        let retouche = |id: &str| retouches.is_some_and(|r| r.contains_key(id));
        let a = self.batiments.iter().filter(|b| !retouche(&b.id));
        a.chain(retouches.into_iter().flat_map(|r| r.values())).filter(|b| b.tuile == (i, j) && !b.masquee && b.detail.is_some()).cloned().collect()
    }
}

/// Les blocs lointains de ces bâtiments (ceux qui ne sont pas démolis).
pub fn blocs(l: &[Bati]) -> Vec<f32> {
    crate::lointain::blocs(l.iter().filter(|b| !b.masquee).map(|b| (&b.b, b.forme.murs, b.forme.couverture)))
}

/// La silhouette d'une région, lue sans le reste du fichier.
fn lire_silhouette(f: &mut Fichier, ri: i32, rj: i32) -> Option<Silhouette> {
    let (x0, z0) = (coin(ri) as f32, coin(rj) as f32);
    let s = f.get("silhouette")?.clone();
    let mut absolu = |k: &str| -> Option<Vec<f32>> {
        let mut v: Vec<f32> = f.lire_ref(s.get(k)?)?;
        for q in v.chunks_mut(9) {
            q[0] += x0;
            q[2] += z0;
        }
        Some(v)
    };
    let (relief, bati, bois) = (absolu("relief")?, absolu("bati")?, absolu("bois")?);
    let carte: Vec<f32> = f.lire_ref(s.get("carte")?)?;
    Some(Silhouette { relief, bati, bois, carte: carte.chunks(4).map(|c| (c[0], [c[1], c[2], c[3]])).collect() })
}

// ---------------------------------------------------------------------------------------
// Le monde matérialisé, pour le jeu

/// Toutes les silhouettes du monde, pour le lointain et la carte.
#[derive(Clone, Default)]
pub struct Silhouettes {
    pub relief: Arc<Vec<f32>>,
    pub bati: Arc<Vec<f32>>,
    pub bois: Arc<Vec<f32>>,
    /// Le fond de la carte, `carte::COTE²` points.
    pub carte: Arc<Vec<crate::carte::Point>>,
    /// Les régions relues.
    pub lues: usize,
}

struct Cache {
    regions: HashMap<(i32, i32), (Option<Arc<Region>>, u64)>,
    horloge: u64,
}

/// Les bâtiments retouchés pas encore écrits, par région, et depuis quand.
#[derive(Default)]
struct Retouches {
    bati: HashMap<(i32, i32), HashMap<String, Bati>>,
    depuis: Option<std::time::Instant>,
}

/// Les retouches s'écrivent quand on n'a plus touché à rien depuis tant de secondes.
const CALME: f32 = 1.5;

/// Les régions d'un dossier de monde, lues à la demande.
pub struct Regions {
    dossier: PathBuf,
    pub nom: String,
    pub niveau: f32,
    cache: Mutex<Cache>,
    silhouettes: Mutex<Option<Silhouettes>>,
    /// Les blocs lointains de chaque région, pour refaire ceux d'une région retouchée.
    blocs: Mutex<HashMap<(i32, i32), Vec<f32>>>,
    retouches: Mutex<Retouches>,
    /// Compte les écritures de retouches : le lointain des bâtiments suit.
    pub version: std::sync::atomic::AtomicU64,
}

impl Regions {
    /// Le monde matérialisé de ce dossier, s'il l'est (sa fiche des sources et ses
    /// régions).
    pub fn ouvrir(dossier: &Path) -> Option<Regions> {
        let j = json::lire(&std::fs::read_to_string(dossier.join(SOURCES)).ok()?)?;
        let m = j.get("materialisation")?;
        if m.get("tuiles").and_then(J::num) != Some(TUILES as f64) || !dossier.join(DOSSIER).is_dir() {
            return None;
        }
        let lieu = j.get("lieu");
        Some(Regions {
            dossier: dossier.to_path_buf(),
            nom: lieu.and_then(|l| l.get("nom")).and_then(J::txt).unwrap_or("Monde").to_string(),
            niveau: lieu.and_then(|l| f(l.get("niveau"))).unwrap_or(0.0),
            cache: Mutex::new(Cache { regions: HashMap::new(), horloge: 0 }),
            silhouettes: Mutex::new(None),
            blocs: Mutex::new(HashMap::new()),
            retouches: Mutex::new(Retouches::default()),
            version: Default::default(),
        })
    }

    fn chemin(&self, ri: i32, rj: i32) -> PathBuf {
        self.dossier.join(DOSSIER).join(nom_fichier(ri, rj))
    }

    /// La région (ri, rj), relue au besoin ; rien si elle manque ou ne se lit pas.
    pub fn region(&self, ri: i32, rj: i32) -> Option<Arc<Region>> {
        let mut c = self.cache.lock().unwrap();
        c.horloge += 1;
        let h = c.horloge;
        if let Some(e) = c.regions.get_mut(&(ri, rj)) {
            e.1 = h;
            return e.0.clone();
        }
        let r = std::fs::read(self.chemin(ri, rj)).ok().and_then(|o| Stuff::lire(&o).ok()).and_then(|s| Region::depuis(&s).ok()).map(Arc::new);
        if c.regions.len() >= CAPACITE {
            if let Some(k) = c.regions.iter().min_by_key(|(_, e)| e.1).map(|(k, _)| *k) {
                c.regions.remove(&k);
            }
        }
        c.regions.insert((ri, rj), (r.clone(), h));
        r
    }

    fn ou(x: f64) -> (i32, f64) {
        let r = (x / METRES).floor() as i32;
        (r, x - coin(r))
    }

    /// La hauteur du sol en (x, z) : celle du point du relief, entre deux points en
    /// ligne droite.
    pub fn hauteur(&self, x: f64, z: f64) -> f64 {
        let ((ri, dx), (rj, dz)) = (Regions::ou(x), Regions::ou(z));
        let Some(r) = self.region(ri, rj) else { return self.niveau as f64 - 20.0 };
        let (gx, gz) = (dx / PAS, dz / PAS);
        let (a, b) = (gx.floor() as usize, gz.floor() as usize);
        let (u, v) = (gx - a as f64, gz - b as f64);
        let (h00, h10, h01, h11) = (r.point(a, b), r.point(a + 1, b), r.point(a, b + 1), r.point(a + 1, b + 1));
        if u == 0.0 && v == 0.0 {
            return h00;
        }
        (h00 * (1.0 - u) + h10 * u) * (1.0 - v) + (h01 * (1.0 - u) + h11 * u) * v
    }

    /// Les bâtiments retouchés de la région (ri, rj), pas encore écrits.
    fn retouches_de(&self, ri: i32, rj: i32) -> Option<HashMap<String, Bati>> {
        self.retouches.lock().unwrap().bati.get(&(ri, rj)).cloned()
    }

    /// Les bâtiments, ponts et arbres de la tuile (i, j).
    pub fn bati(&self, i: i32, j: i32) -> Option<(Vec<f32>, Vec<[[f32; 3]; 3]>)> {
        let (ri, rj) = (de_tuile(i), de_tuile(j));
        self.region(ri, rj)?.bati(i, j, self.retouches_de(ri, rj).as_ref())
    }

    /// Les bâtiments enrichis de la tuile (i, j).
    pub fn enrichis(&self, i: i32, j: i32) -> Vec<Bati> {
        let (ri, rj) = (de_tuile(i), de_tuile(j));
        self.region(ri, rj).map(|r| r.enrichis(i, j, self.retouches_de(ri, rj).as_ref())).unwrap_or_default()
    }

    /// Le bâtiment `id` de la région (ri, rj), retouches comprises.
    pub fn batiment(&self, ri: i32, rj: i32, id: &str) -> Option<Bati> {
        if let Some(b) = self.retouches.lock().unwrap().bati.get(&(ri, rj)).and_then(|r| r.get(id)) {
            return Some(b.clone());
        }
        self.region(ri, rj)?.batiments.iter().find(|b| b.id == id).cloned()
    }

    /// Le bâtiment de la tuile (i, j) au point absolu (x, z) : celui dont le contour le
    /// contient, sinon dont un mur passe à moins d'un mètre.
    pub fn bati_sous(&self, i: i32, j: i32, x: f64, z: f64) -> Option<Bati> {
        let (ri, rj) = (de_tuile(i), de_tuile(j));
        let r = self.region(ri, rj)?;
        let ret = self.retouches_de(ri, rj).unwrap_or_default();
        let l: Vec<&Bati> = r.batiments.iter().filter(|b| !ret.contains_key(&b.id)).chain(ret.values()).filter(|b| b.tuile == (i, j) && !b.masquee).collect();
        let (x, z) = (x as f32, z as f32);
        l.iter().find(|b| b.touche(x, z, 0.0)).or_else(|| l.iter().find(|b| b.touche(x, z, 1.0))).map(|b| (*b).clone())
    }

    /// Un bâtiment retouché : il se voit tout de suite, et s'écrira dans sa région quand
    /// on n'y touchera plus (`calme`, `ecrire`). Il reste dans la région de son fichier.
    pub fn retoucher(&self, b: Bati) {
        let mut r = self.retouches.lock().unwrap();
        r.bati.entry((de_tuile(b.tuile.0), de_tuile(b.tuile.1))).or_default().insert(b.id.clone(), b);
        r.depuis = Some(std::time::Instant::now());
    }

    /// Des retouches attendent, et l'on n'y touche plus depuis un moment.
    pub fn calme(&self) -> bool {
        let r = self.retouches.lock().unwrap();
        !r.bati.is_empty() && r.depuis.is_some_and(|d| d.elapsed().as_secs_f32() > CALME)
    }

    pub fn en_attente(&self) -> usize {
        self.retouches.lock().unwrap().bati.values().map(|r| r.len()).sum()
    }

    /// Écrit les retouches dans leurs régions : chaque instance réécrite (ses clés
    /// inconnues gardées), la silhouette des bâtiments refaite. Rend le nombre de régions
    /// écrites.
    pub fn ecrire(&self) -> Result<usize, String> {
        let a_ecrire = std::mem::take(&mut self.retouches.lock().unwrap().bati);
        let mut n = 0;
        let mut erreur = None;
        for (&(ri, rj), l) in &a_ecrire {
            match self.reecrire(ri, rj, l) {
                Ok(()) => n += 1,
                Err(e) => {
                    // Gardées pour la prochaine fois
                    let mut r = self.retouches.lock().unwrap();
                    let d = r.bati.entry((ri, rj)).or_default();
                    for (k, b) in l {
                        d.entry(k.clone()).or_insert_with(|| b.clone());
                    }
                    erreur = Some(e);
                }
            }
        }
        if n > 0 {
            self.version.fetch_add(1, Ordering::Relaxed);
        }
        erreur.map_or(Ok(n), Err)
    }

    fn reecrire(&self, ri: i32, rj: i32, l: &HashMap<String, Bati>) -> Result<(), String> {
        let chemin = self.chemin(ri, rj);
        let mut s = Stuff::lire(&std::fs::read(&chemin).map_err(|e| format!("{} : {e}", nom_fichier(ri, rj)))?)?;
        let mut instances = s.get("instances").and_then(J::liste).map(|l| l.to_vec()).unwrap_or_default();
        // Les définitions : écrite pour un bâtiment qu'on vient d'enrichir, ôtée pour celui
        // qui revient au procédural. Chacune a son tampon (à partir du 2).
        let mut defs = s.get("definitions").and_then(J::liste).map(|l| l.to_vec()).unwrap_or_default();
        let mut changees = false;
        for b in l.values() {
            let avant = instances.iter().find(|i| i.get("id").and_then(J::txt) == Some(&b.id)).and_then(|i| i.get("definition")).and_then(J::txt).map(String::from);
            let oter = match &b.detail {
                _ if b.a_oter() => avant,
                Some(d) if d.neuve => Some(d.def.clone()),
                None => avant,
                _ => None,
            };
            if let Some(x) = oter {
                if let Some(k) = defs.iter().position(|d| d.get("id").and_then(J::txt) == Some(&x)) {
                    if let Some(t) = crate::enrichi::tampon(&defs[k]).filter(|&t| t >= 2).and_then(|t| s.tampons.get_mut(t)) {
                        t.clear();
                    }
                    defs.remove(k);
                    changees = true;
                }
            }
            if let Some(d) = b.detail.as_ref().filter(|d| d.neuve && !b.a_oter()) {
                let k = (2..s.tampons.len()).find(|&k| s.tampons[k].is_empty()).unwrap_or(s.tampons.len().max(2));
                let anciens = instances.iter().find(|i| i.get("id").and_then(J::txt) == Some(&b.id)).and_then(|i| i.get("procedural")).and_then(|p| p.get("params"));
                defs.push(crate::enrichi::ecrire(&mut s, k, &d.def, &b.id, b.params(anciens), &d.modele));
                changees = true;
            }
        }
        if changees {
            s.mettre("definitions", J::Liste(defs));
        }
        for b in l.values() {
            if b.a_oter() {
                instances.retain(|i| i.get("id").and_then(J::txt) != Some(&b.id));
                continue;
            }
            match instances.iter_mut().find(|i| i.get("id").and_then(J::txt) == Some(&b.id)) {
                Some(i) => *i = b.instance(Some(i)),
                None => instances.push(b.instance(None)),
            }
        }
        s.mettre("instances", J::Liste(instances));
        // La silhouette : les blocs refaits, le reste repris
        let region = Region::depuis(&s)?;
        let (x0, z0) = (coin(ri) as f32, coin(rj) as f32);
        let mut bati = blocs(&region.batiments);
        let relatif = |v: &mut Vec<f32>, sens: f32| {
            for q in v.chunks_mut(9) {
                q[0] += x0 * sens;
                q[2] += z0 * sens;
            }
        };
        let mut g = s.get("region").cloned().ok_or("pas de région")?;
        let sil = g.get("silhouette").cloned().ok_or("pas de silhouette")?;
        let lire = |k: &str| -> Result<Vec<f32>, String> { sil.get(k).and_then(|r| s.lire_ref(r)).ok_or_else(|| format!("silhouette illisible : {k}")) };
        let (relief, bois, carte) = (lire("relief")?, lire("bois")?, lire("carte")?);
        let mut b_rel = bati.clone();
        relatif(&mut b_rel, -1.0);
        if let Some(t) = s.tampons.get_mut(1) {
            t.clear();
        }
        let nouvelle = objet(vec![("relief", s.ranger(1, &relief).json()), ("bati", s.ranger(1, &b_rel).json()), ("bois", s.ranger(1, &bois).json()), ("carte", s.ranger(1, &carte).json())]);
        if let J::Objet(o) = &mut g {
            mettre(o, "silhouette", Some(nouvelle));
        }
        s.mettre("region", g);
        let tmp = chemin.with_extension("tmp");
        std::fs::write(&tmp, s.octets()).map_err(|e| format!("{} : {e}", nom_fichier(ri, rj)))?;
        std::fs::rename(&tmp, &chemin).map_err(|e| format!("{} : {e}", nom_fichier(ri, rj)))?;
        // La région se relira ; le lointain des bâtiments se refait
        self.cache.lock().unwrap().regions.remove(&(ri, rj));
        let mut blocs = self.blocs.lock().unwrap();
        blocs.insert((ri, rj), std::mem::take(&mut bati));
        let tout: Vec<f32> = toutes().iter().filter_map(|k| blocs.get(k)).flatten().copied().collect();
        drop(blocs);
        if let Some(sil) = self.silhouettes.lock().unwrap().as_mut() {
            sil.bati = Arc::new(tout);
        }
        Ok(())
    }

    /// Les silhouettes de toutes les régions, lues une fois.
    pub fn silhouettes(&self) -> Silhouettes {
        let mut garde = self.silhouettes.lock().unwrap();
        if let Some(s) = garde.as_ref() {
            return s.clone();
        }
        let s = self.lire_silhouettes();
        *garde = Some(s.clone());
        s
    }

    /// Relit toutes les silhouettes (après une régénération), sans bloquer le jeu qui
    /// garde les anciennes jusque-là.
    pub fn relire_silhouettes(&self) {
        let s = self.lire_silhouettes();
        *self.silhouettes.lock().unwrap() = Some(s);
    }

    fn lire_silhouettes(&self) -> Silhouettes {
        let cote = crate::carte::COTE;
        let mut carte = vec![(self.niveau - 20.0, [0.36, 0.38, 0.34]); cote * cote];
        let (mut relief, mut bati, mut bois, mut lues) = (Vec::new(), Vec::new(), Vec::new(), 0);
        let mut blocs = self.blocs.lock().unwrap();
        for (ri, rj) in toutes() {
            let Some(r) = Fichier::ouvrir_partiel(&self.chemin(ri, rj), &["silhouette"]).ok().and_then(|mut f| lire_silhouette(&mut f, ri, rj)) else { continue };
            lues += 1;
            relief.extend(r.relief);
            bati.extend_from_slice(&r.bati);
            blocs.insert((ri, rj), r.bati);
            bois.extend(r.bois);
            let (ti, tj) = (tuiles_dans(ri), tuiles_dans(rj));
            let mut k = 0;
            for j in tj {
                for i in ti.clone() {
                    if let Some(p) = r.carte.get(k) {
                        carte[(j + DEMI_CARTE) as usize * cote + (i + DEMI_CARTE) as usize] = *p;
                    }
                    k += 1;
                }
            }
        }
        Silhouettes { relief: Arc::new(relief), bati: Arc::new(bati), bois: Arc::new(bois), carte: Arc::new(carte), lues }
    }

    /// Régénère les régions `liste` à partir du lieu réel du monde (`lieu.reel`), sur
    /// `fils` fils : tout ce que les sources donnent est refait, sauf les instances
    /// retouchées ou démolies, gardées telles quelles avec leurs définitions. Un bâtiment
    /// refait qui tomberait dans l'un d'eux ne revient pas, ni un arbre dans un bâtiment
    /// gardé. Rend (bâtiments refaits, instances gardées).
    pub fn regenerer(&self, liste: &[(i32, i32)], av: &Avancement, fils: usize) -> Result<(usize, usize), String> {
        av.total.store(liste.len(), Ordering::Relaxed);
        av.faites.store(0, Ordering::Relaxed);
        let chemin = self.dossier.join(LIEU);
        let o = std::fs::read(&chemin).map_err(|e| format!("{LIEU} : {e}"))?;
        let r = Reel::depuis_octets(&o).ok_or_else(|| format!("{LIEU} illisible"))?;
        drop(o);
        let c = Campagne::reelle(Arc::new(r));
        let suivant = AtomicUsize::new(0);
        let bilan: Mutex<Result<(usize, usize), String>> = Mutex::new(Ok((0, 0)));
        std::thread::scope(|sc| {
            for _ in 0..fils.max(1).min(liste.len().max(1)) {
                sc.spawn(|| loop {
                    if av.arret.load(Ordering::Relaxed) || bilan.lock().unwrap().is_err() {
                        return;
                    }
                    let k = suivant.fetch_add(1, Ordering::Relaxed);
                    let Some(&(ri, rj)) = liste.get(k) else { return };
                    let r = regenerer_region(&c, &self.chemin(ri, rj), ri, rj);
                    let mut b = bilan.lock().unwrap();
                    match (r, &mut *b) {
                        (Ok((x, y)), Ok(t)) => {
                            t.0 += x;
                            t.1 += y;
                        }
                        (Err(e), _) => *b = Err(e),
                        _ => {}
                    }
                    av.faites.fetch_add(1, Ordering::Relaxed);
                });
            }
        });
        // Les régions se reliront, les silhouettes aussi
        {
            let mut cache = self.cache.lock().unwrap();
            for k in liste {
                cache.regions.remove(k);
            }
        }
        self.relire_silhouettes();
        self.version.fetch_add(1, Ordering::Relaxed);
        bilan.into_inner().unwrap()
    }
}

impl Sol for Regions {
    fn occupation(&self, x: f64, z: f64) -> [(u8, f64); 4] {
        let ((ri, dx), (rj, dz)) = (Regions::ou(x), Regions::ou(z));
        let Some(r) = self.region(ri, rj) else { return [(0, 1.0), (0, 0.0), (0, 0.0), (0, 0.0)] };
        let (gx, gz) = (dx / PAS_SOL, dz / PAS_SOL);
        let (a, b) = (gx.floor() as usize, gz.floor() as usize);
        let (u, v) = (gx - a as f64, gz - b as f64);
        let c = |a: usize, b: usize| r.sol[b.min(N_SOL - 1) * N_SOL + a.min(N_SOL - 1)];
        [(c(a, b), (1.0 - u) * (1.0 - v)), (c(a + 1, b), u * (1.0 - v)), (c(a, b + 1), (1.0 - u) * v), (c(a + 1, b + 1), u * v)]
    }

    fn eau(&self, x: f64, z: f64) -> f64 {
        let ((ri, dx), (rj, dz)) = (Regions::ou(x), Regions::ou(z));
        let Some(r) = self.region(ri, rj) else { return 1.0 };
        let (gx, gz) = (dx / PAS, dz / PAS);
        let (a, b) = (gx.floor() as usize, gz.floor() as usize);
        let (u, v) = (gx - a as f64, gz - b as f64);
        let e = |a: usize, b: usize| if r.est_eau(a, b) { 1.0 } else { 0.0 };
        (e(a, b) * (1.0 - u) + e(a + 1, b) * u) * (1.0 - v) + (e(a, b + 1) * (1.0 - u) + e(a + 1, b + 1) * u) * v
    }

    fn niveau(&self) -> f32 {
        self.niveau
    }

    fn route(&self, x: f64, z: f64) -> Option<Proche> {
        let r = self.region((x / METRES).floor() as i32, (z / METRES).floor() as i32)?;
        crate::reel::proche(&r.routes, &r.index, x, z)
    }
}

// ---------------------------------------------------------------------------------------
// La matérialisation

/// Où en est une matérialisation : les régions faites, sur combien ; `arret` l'interrompt.
#[derive(Default)]
pub struct Avancement {
    pub faites: AtomicUsize,
    pub total: AtomicUsize,
    pub arret: AtomicBool,
}

/// Refait la région (ri, rj) de `c` dans `chemin`, en gardant de l'ancienne ses instances
/// retouchées ou démolies et leurs définitions. Rend (bâtiments refaits, gardées).
fn regenerer_region(c: &Campagne, chemin: &Path, ri: i32, rj: i32) -> Result<(usize, usize), String> {
    let mut region = Region::materialiser(c, ri, rj);
    let ancien = std::fs::read(chemin).ok().and_then(|o| Stuff::lire(&o).ok());
    // Ce que l'on garde : les instances, leurs bâtiments, les définitions des enrichis
    let (mut gardees, mut bati_garde, mut defs) = (Vec::new(), Vec::new(), Vec::new());
    if let Some(a) = &ancien {
        let definitions = a.get("definitions").and_then(J::liste).unwrap_or(&[]);
        for i in a.get("instances").and_then(J::liste).unwrap_or(&[]) {
            let oui = |k: &str| i.get(k).and_then(J::bool) == Some(true);
            if !oui("retouche") && !oui("masquee") {
                continue;
            }
            gardees.push(i.clone());
            let b = match i.get("definition").and_then(J::txt) {
                Some(d) => definitions.iter().find(|x| x.get("id").and_then(J::txt) == Some(d)).and_then(|x| crate::enrichi::lire(a, x)).and_then(|(params, modele)| {
                    let b = Bati::depuis_params(i, &params);
                    defs.push((d.to_string(), params, modele));
                    b
                }),
                None => Bati::depuis_instance(i),
            };
            bati_garde.extend(b);
        }
    }
    let ids: std::collections::HashSet<String> = gardees.iter().filter_map(|i| i.get("id").and_then(J::txt).map(String::from)).collect();
    // Les bâtiments refaits, hors de la place des gardés
    region.batiments.retain(|b| {
        let c = b.b.centre();
        !ids.contains(&b.id) && !bati_garde.iter().any(|g| g.touche(c[0], c[1], 0.0))
    });
    region.ponts.retain(|p| !ids.contains(&p.id));
    // Les arbres, hors des bâtiments gardés
    let debout: Vec<&Bati> = bati_garde.iter().filter(|b| !b.masquee).collect();
    if !debout.is_empty() {
        let (mut arbres, mut par_tuile) = (Vec::with_capacity(region.arbres.len()), Vec::with_capacity(region.par_tuile.len()));
        for &[d, n] in &region.par_tuile {
            let debut = arbres.len() as u32;
            arbres.extend(region.arbres[d as usize..(d + n) as usize].iter().filter(|a| !debout.iter().any(|b| b.touche(a.pos[0], a.pos[2], 0.5))).cloned());
            par_tuile.push([debut, arbres.len() as u32 - debut]);
        }
        region.arbres = arbres;
        region.par_tuile = par_tuile;
    }
    let refaits = region.batiments.len();
    let tous: Vec<Bati> = region.batiments.iter().chain(bati_garde.iter()).cloned().collect();
    region.silhouette.bati = blocs(&tous);
    let mut s = region.stuff();
    let mut instances: Vec<J> = s.get("instances").and_then(J::liste).map(|l| l.to_vec()).unwrap_or_default();
    instances.retain(|i| !i.get("id").and_then(J::txt).is_some_and(|x| ids.contains(x)));
    let n = gardees.len();
    instances.extend(gardees);
    s.mettre("instances", J::Liste(instances));
    if !defs.is_empty() {
        let mut l = Vec::new();
        for (d, params, modele) in defs {
            let k = s.tampons.len().max(2);
            let nom = modele.nom.clone().unwrap_or_else(|| d.clone());
            l.push(crate::enrichi::ecrire(&mut s, k, &d, &nom, params, &modele));
        }
        s.mettre("definitions", J::Liste(l));
    }
    let tmp = chemin.with_extension("tmp");
    std::fs::write(&tmp, s.octets()).map_err(|e| format!("{} : {e}", nom_fichier(ri, rj)))?;
    std::fs::rename(&tmp, chemin).map_err(|e| format!("{} : {e}", nom_fichier(ri, rj)))?;
    Ok((refaits, n))
}

/// Écrit toutes les régions du lieu réel `r` dans `dossier/regions/`, puis la fiche des
/// sources, sur `fils` fils. Les régions s'écrivent d'abord à part : le monde n'est
/// matérialisé qu'une fois tout écrit. Rend le nombre d'octets écrits.
pub fn materialiser(r: Arc<Reel>, dossier: &Path, av: &Avancement, fils: usize) -> Result<u64, String> {
    let c = Campagne::reelle(r.clone());
    let liste = toutes();
    av.total.store(liste.len(), Ordering::Relaxed);
    av.faites.store(0, Ordering::Relaxed);
    let tmp = dossier.join(format!("{DOSSIER}.tmp"));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| format!("{} : {e}", tmp.display()))?;
    let suivant = AtomicUsize::new(0);
    let octets = std::sync::atomic::AtomicU64::new(0);
    let erreur: Mutex<Option<String>> = Mutex::new(None);
    std::thread::scope(|sc| {
        for _ in 0..fils.max(1) {
            sc.spawn(|| loop {
                if av.arret.load(Ordering::Relaxed) || erreur.lock().unwrap().is_some() {
                    return;
                }
                let k = suivant.fetch_add(1, Ordering::Relaxed);
                let Some(&(ri, rj)) = liste.get(k) else { return };
                let o = Region::materialiser(&c, ri, rj).stuff().octets();
                if let Err(e) = std::fs::write(tmp.join(nom_fichier(ri, rj)), &o) {
                    *erreur.lock().unwrap() = Some(format!("{} : {e}", nom_fichier(ri, rj)));
                    return;
                }
                octets.fetch_add(o.len() as u64, Ordering::Relaxed);
                av.faites.fetch_add(1, Ordering::Relaxed);
            });
        }
    });
    if let Some(e) = erreur.into_inner().unwrap() {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e);
    }
    if av.arret.load(Ordering::Relaxed) {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err("matérialisation interrompue".into());
    }
    let fin = dossier.join(DOSSIER);
    let _ = std::fs::remove_dir_all(&fin);
    std::fs::rename(&tmp, &fin).map_err(|e| format!("{} : {e}", fin.display()))?;
    let (a, b) = (*etendue().start(), *etendue().end());
    let sources = objet(vec![
        ("format", J::Num(1.0)),
        ("application", txt(&application())),
        (
            "lieu",
            objet(vec![
                ("fichier", txt(LIEU)),
                ("nom", txt(&r.nom)),
                ("lat", J::Num(r.lat)),
                ("lon", J::Num(r.lon)),
                ("x0", J::Num(r.x0)),
                ("y0", J::Num(r.y0)),
                ("pas", J::Num(r.pas)),
                ("niveau", J::Num(r.niveau as f64)),
                ("sources", txt(crate::reel::SOURCES)),
            ]),
        ),
        ("materialisation", objet(vec![("dossier", txt(DOSSIER)), ("tuiles", J::Num(TUILES as f64)), ("de", J::Liste(vec![J::Num(a as f64); 2])), ("a", J::Liste(vec![J::Num(b as f64); 2]))])),
    ]);
    let mut t = String::new();
    json::indente(&sources, 0, &mut t);
    t.push('\n');
    std::fs::write(dossier.join(SOURCES), t).map_err(|e| format!("{SOURCES} : {e}"))?;
    Ok(octets.into_inner())
}

/// Efface le monde matérialisé du dossier (ses régions et sa fiche des sources).
pub fn effacer(dossier: &Path) -> std::io::Result<()> {
    for d in [DOSSIER.to_string(), format!("{DOSSIER}.tmp")] {
        match std::fs::remove_dir_all(dossier.join(d)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    match std::fs::remove_file(dossier.join(SOURCES)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Le registre des catalogues du monde : l'identifiant de chaque `catalogue/*.stuff` et
/// son chemin.
pub fn catalogues(dossier: &Path) -> Vec<(String, String)> {
    let mut l: Vec<(String, String)> = std::fs::read_dir(dossier.join("catalogue"))
        .map(|d| {
            d.flatten()
                .filter_map(|e| {
                    let nom = e.file_name().to_str()?.to_string();
                    nom.ends_with(".stuff").then_some(())?;
                    let f = Fichier::ouvrir(&e.path()).ok()?;
                    Some((f.get("id")?.txt()?.to_string(), format!("catalogue/{nom}")))
                })
                .collect()
        })
        .unwrap_or_default();
    l.sort();
    l
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::moteur::Source;
    use crate::reel::{cote, cote_sol, Batiment, BOIS, MAISON, ROUTE};

    fn lieu() -> Arc<Reel> {
        let pas = 64.0;
        let n = cote(pas);
        let h: Vec<f32> = (0..n * n).map(|i| ((i % n) as f32 * 0.7).sin() * 4.0 + (i / n) as f32 * 0.1).collect();
        let eau: Vec<bool> = (0..n * n).map(|i| i % n < 3).collect();
        let route = Route { genre: ROUTE, largeur: 6.0, points: vec![[-300.0, 40.0, 5.0], [700.0, 60.0, 6.0]] };
        let maison = Batiment::plat(MAISON, 0.0, 7.0, vec![[100.0, 100.0], [112.0, 100.0], [112.0, 110.0], [100.0, 110.0]]);
        let pont = Route { genre: ROUTE, largeur: 5.0, points: vec![[300.0, 300.0, 9.0], [360.0, 300.0, 9.0]] };
        let m = cote_sol(n);
        Arc::new(Reel::new("Essai", 49.0, 1.0, 0.0, 0.0, pas, -50.0, &h, &eau, vec![route], vec![maison], vec![pont]).avec_sol(vec![BOIS; m * m]))
    }

    #[test]
    fn regenerer_une_region_garde_le_batiment_enrichi() {
        let c = Campagne::reelle(lieu());
        let reg = Region::materialiser(&c, 0, 0);
        // Le bâtiment enrichi de son propre modèle, tourné : une retouche écrite
        let mut b = reg.batiments[0].clone();
        let d = crate::catalogue::decoder_glb(&crate::enrichi::glb(&b), Some(false)).unwrap();
        let n = d.triangles.len();
        b.cap = 0.5;
        b.retouche = true;
        b.refaire();
        b.detail = Some(Detail::new(format!("def-{}", b.id), Arc::new(d), true));
        let mut s = reg.stuff();
        let def = crate::enrichi::ecrire(&mut s, 2, &format!("def-{}", b.id), &b.id, b.params(None), &b.detail.as_ref().unwrap().modele);
        s.mettre("definitions", J::Liste(vec![def]));
        let autres: Vec<J> = s.get("instances").and_then(J::liste).unwrap().iter().filter(|i| i.get("id").and_then(J::txt) != Some(&b.id)).cloned().collect();
        s.mettre("instances", J::Liste(autres.into_iter().chain([b.instance(None)]).collect()));
        let f = std::env::temp_dir().join(format!("sage-fragment-{}.stuff", std::process::id()));
        std::fs::write(&f, s.octets()).unwrap();
        // Régénérée : le bâtiment n'est pas refait, le gardé revient tel quel
        let (refaits, gardes) = regenerer_region(&c, &f, 0, 0).unwrap();
        assert_eq!((refaits, gardes), (0, 1));
        let lu = Region::depuis(&Stuff::lire(&std::fs::read(&f).unwrap()).unwrap()).unwrap();
        std::fs::remove_file(&f).unwrap();
        assert_eq!(lu.batiments.len(), 1);
        let x = &lu.batiments[0];
        assert!(x.retouche && (x.cap - 0.5).abs() < 1e-3);
        assert_eq!(x.detail.as_ref().map(|d| d.modele.triangles.len()), Some(n));
        assert!(lu.arbres.len() <= reg.arbres.len());
        assert_eq!(lu.ponts.len(), reg.ponts.len());
    }

    #[test]
    fn une_region_se_materialise_se_relit_et_se_voit_pareil() {
        let r = lieu();
        let c = Campagne::reelle(r.clone());
        let reg = Region::materialiser(&c, 0, 0);
        assert!(!reg.arbres.is_empty() && reg.batiments.len() == 1 && reg.ponts.len() == 1 && reg.routes.len() == 1);
        let s = reg.stuff();
        let o = s.octets();
        let lu = Region::depuis(&Stuff::lire(&o).unwrap()).unwrap();
        assert_eq!(lu.h, reg.h);
        assert_eq!(lu.sol, reg.sol);
        assert_eq!(lu.par_tuile, reg.par_tuile);
        assert_eq!(lu.arbres.len(), reg.arbres.len());
        // Le relief, au centimètre près, routes comprises
        for (x, z) in [(0.0, 0.0), (64.0, 40.0), (130.0, 502.0), (1022.0, 1022.0)] {
            let (ri, rj) = (0, 0);
            let (a, b) = (((x - coin(ri)) / PAS) as usize, ((z - coin(rj)) / PAS) as usize);
            assert!((lu.point(a, b) - r.hauteur(x, z)).abs() < 0.01, "({x}, {z})");
        }
        // Les bâtiments, ponts et arbres de chaque tuile : autant de triangles qu'avant
        for (i, j) in [(1, 1), (4, 4), (5, 4), (0, 0), (7, 3)] {
            let avant = c.bati(i, j).map_or(0, |(_, t)| t.len());
            let apres = lu.bati(i, j, None).map_or(0, |(_, t)| t.len());
            assert_eq!(avant, apres, "tuile ({i}, {j})");
        }
        // Le bâtiment revient au même endroit, à la même hauteur
        let b = &lu.batiments[0].b;
        assert!((b.toit - 7.0).abs() < 1e-3 && (b.centre()[0] - 106.0).abs() < 1e-3);
        // La silhouette se relit sans le reste
        let f = std::env::temp_dir().join(format!("sage-region-{}.stuff", std::process::id()));
        std::fs::write(&f, &o).unwrap();
        let sil = lire_silhouette(&mut Fichier::ouvrir_partiel(&f, &["silhouette"]).unwrap(), 0, 0).unwrap();
        let _ = std::fs::remove_file(&f);
        assert_eq!(sil.relief.len(), reg.silhouette.relief.len());
        assert_eq!(sil.carte.len(), 256);
        assert!((sil.relief[0] - reg.silhouette.relief[0]).abs() < 1e-3);
    }

    #[test]
    fn un_monde_se_materialise_et_le_jeu_le_lit() {
        let r = lieu();
        let d = std::env::temp_dir().join(format!("sage-monde-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        // Toute la carte d'un lieu à 64 m : quatre cents régions
        let av = Avancement::default();
        materialiser(r.clone(), &d, &av, 4).unwrap();
        assert_eq!(av.faites.load(Ordering::Relaxed), toutes().len());
        let m = Arc::new(Regions::ouvrir(&d).expect("matérialisé"));
        assert_eq!(m.nom, "Essai");
        let (avant, apres) = (Campagne::reelle(r.clone()), Campagne::materialisee(m.clone()));
        for (i, j) in [(0, 0), (-3, 2), (5, 4), (40, -60)] {
            let (a, b) = (avant.tuile(i, j), apres.tuile(i, j));
            for k in 0..a.relief.h.len() {
                assert!((a.relief.h[k] - b.relief.h[k]).abs() < 0.01, "tuile ({i}, {j}), point {k}");
            }
            assert_eq!(avant.bati(i, j).map(|x| x.1.len()), apres.bati(i, j).map(|x| x.1.len()), "tuile ({i}, {j})");
        }
        // Le lointain et la carte, d'un tenant ou par régions : autant de sommets
        assert_eq!(crate::lointain::sommets(&avant).len(), crate::lointain::sommets(&apres).len());
        assert_eq!(crate::vegetation::lointain(&avant).len(), crate::vegetation::lointain(&apres).len());
        assert_eq!(m.silhouettes().carte.len(), crate::carte::COTE * crate::carte::COTE);
        // Un bâtiment retouché se voit tout de suite, puis s'écrit dans sa région, ses clés
        // inconnues gardées ; démoli, il ne se voit plus mais reste dans le fichier
        let b = m.bati_sous(1, 1, 106.0, 105.0).expect("la maison");
        assert!(m.bati_sous(1, 1, 90.0, 90.0).is_none());
        let tri = |m: &Regions| m.bati(1, 1).map_or(0, |x| x.1.len());
        let avant_tri = tri(&m);
        let mut x = b.clone();
        x.forme.toit = crate::toits::DEUX_PANS;
        x.forme.faitage = x.forme.egout + 4.0;
        x.cap = 0.5;
        x.retouche = true;
        x.refaire();
        m.retoucher(x.clone());
        assert_eq!(m.en_attente(), 1);
        assert_ne!(tri(&m), avant_tri);
        let f = m.chemin(0, 0);
        let mut s = Stuff::lire(&std::fs::read(&f).unwrap()).unwrap();
        let mut l = s.get("instances").and_then(J::liste).unwrap().to_vec();
        if let Some(J::Objet(o)) = l.iter_mut().find(|i| i.get("id").and_then(J::txt) == Some(&b.id)) {
            o.push(("note".into(), J::Txt("gardée".into())));
        }
        s.mettre("instances", J::Liste(l));
        std::fs::write(&f, s.octets()).unwrap();
        assert_eq!(m.ecrire(), Ok(1));
        assert_eq!(m.en_attente(), 0);
        let relu = m.batiment(0, 0, &b.id).unwrap();
        assert!(relu.retouche && relu.forme.toit == crate::toits::DEUX_PANS && (relu.cap - 0.5).abs() < 1e-3);
        let s = Stuff::lire(&std::fs::read(&f).unwrap()).unwrap();
        let i = s.get("instances").and_then(J::liste).unwrap().iter().find(|i| i.get("id").and_then(J::txt) == Some(&b.id)).unwrap().clone();
        assert_eq!(i.get("note").and_then(J::txt), Some("gardée"));
        let mut y = relu.clone();
        y.masquee = true;
        m.retoucher(y);
        m.ecrire().unwrap();
        assert!(m.batiment(0, 0, &b.id).is_none() && m.bati_sous(1, 1, 106.0, 105.0).is_none());
        effacer(&d).unwrap();
        assert!(Regions::ouvrir(&d).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
