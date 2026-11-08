//! Les rues maillées : chaussée, caniveau, bordure, trottoirs et accotements, tirés d'un
//! même champ (depuis la 0.49 ; avant, chaussées et trottoirs se faisaient à part).
//!
//! En chaque nœud d'une grille de `PAS` mètres, le champ donne :
//! - `e`, l'écart signé au bord de la chaussée (négatif dessus), pour toutes les rues à
//!   la fois (grandes routes, routes, voies de service) : le plus petit écart de chaque
//!   route, puis l'union des routes, arrondie aux angles (`Procedures::rayon`) quand
//!   elles se croisent franchement (pas entre deux morceaux d'une même rue) ;
//! - `y`, la hauteur de la chaussée : celle de la route la plus proche, fondue avec les
//!   routes voisines à moins de `MARCHE` d'elle ;
//! - `o`, la limite du trottoir (négatif dedans) : `Procedures::trottoir` mètres depuis la
//!   bordure, ou jusqu'à la façade d'un bâtiment à moins de `Procedures::portee` ;
//! - `b`, l'écart aux bâtiments (négatif dedans).
//!
//! Chaque maille se coupe en deux triangles ; chaque couche découpe le triangle par ses
//! conditions (le champ y est linéaire) : la chaussée (`e ≤ 0`), l'accotement là où il
//! n'y a pas de trottoir (`0 ≤ e ≤ accotement`, `o ≥ 0`), le caniveau pavé (`-RIGOLE ≤ e ≤ 0`, là où il y a un trottoir), le
//! rang de pavés du trottoir (`0 ≤ e ≤ BANDE`) et son bitume (`e ≥ BANDE`), tous deux dans
//! `o ≤ 0` et `b ≥ 0`. Chaussée et trottoir partagent ainsi la même bordure, la courbe
//! `e = 0`, et les trottoirs des rues qui se croisent ne font qu'une surface. Les bords
//! venus d'une condition se reconnaissent : la face de la bordure (`e = 0`), le chant du
//! trottoir côté façade, celui de l'accotement. Loin des bords, une couche qui couvre
//! toute une maille de 4 m (ou de 2 m) s'y pose en deux grands triangles.
//!
//! Le champ ne dépend que des positions absolues : deux tuiles voisines s'accordent à
//! leur bord. Au-delà du bout d'une route que nulle autre ne prolonge (vers un pont, une
//! impasse), la chaussée se coupe net.

use crate::moteur::{REVETEMENTS, TAILLE};
use crate::reel::{teinte, Index, Route, CASE, SERVICE};
use crate::revetements::{BITUME, METRES_BITUME, PAVES, PAVES_PAR_IMAGE, UNI};
use std::collections::{HashMap, HashSet};

/// Le pas de la grille (m).
pub const PAS: f32 = 1.0;
/// La chaussée au-dessus de la plateforme, et l'accotement (m).
const DESSUS: f32 = 0.03;
const DESSOUS: f32 = 0.01;
/// L'écart prêté à un nœud sans rue en vue.
const LOIN: f32 = 50.0;
/// Deux hauteurs plus éloignées que cela : deux niveaux (un croisement sans carrefour) ;
/// un triangle qui les touche reste vide, et elles ne se fondent pas.
const MARCHE: f32 = 0.5;
/// Les hauteurs des routes voisines se fondent sur tant de mètres d'écart.
const FONDU: f32 = 4.0;
/// Les seaux où l'on range les segments proches de la tuile (m).
const SEAU: f32 = 4.0;
/// Le rang de pavés le long de la bordure : sa largeur et la longueur d'un pavé.
pub const BANDE: f32 = 0.20;
pub const PAVE_TROTTOIR: f32 = 1.0;
/// Le caniveau pavé, sur la chaussée au pied de la bordure : sa largeur et la longueur
/// d'un pavé.
pub const RIGOLE: f32 = 0.15;
pub const PAVE_CANIVEAU: f32 = 0.90;
/// Le caniveau, juste au-dessus de la chaussée.
const SUR_CHAUSSEE: f32 = 0.04;
/// Le pied de la bordure, sous la chaussée.
const PIED: f32 = 0.02;

const ACCOTEMENT_CAMPAGNE: [f32; 3] = [0.50, 0.46, 0.37];
const ACCOTEMENT_VILLE: [f32; 3] = [0.47, 0.46, 0.44];
/// Les surfaces revêtues prennent la couleur de leur texture.
const BLANC: [f32; 3] = [1.0, 1.0, 1.0];
/// Le chant du trottoir, côté façade : du bitume.
const SOMBRE: [f32; 3] = [0.25, 0.25, 0.26];
/// Les pavés du caniveau, et ceux du trottoir (dessus et face de la bordure), plus ternes.
const CANIVEAU: [f32; 3] = [0.90, 0.90, 0.90];
const TERNE: [f32; 3] = [0.74, 0.74, 0.76];

/// Ce qu'il faut savoir du lieu autour des rues.
pub struct Lieu<'a> {
    pub routes: &'a [Route],
    pub index: &'a Index,
    /// Les contours (absolus) des bâtiments debout près de la tuile.
    pub batis: Vec<&'a [[f32; 2]]>,
    /// Un quartier (habité, d'activités) : les rues y ont des trottoirs.
    pub urbain: &'a dyn Fn(f32, f32) -> bool,
    /// La hauteur du sol (absolue).
    pub sol: &'a dyn Fn(f32, f32) -> f32,
    pub eau: &'a dyn Fn(f32, f32) -> bool,
}

/// Le plus proche d'un point : l'écart, la hauteur de la chaussée, la route.
#[derive(Clone, Copy)]
struct Proche {
    e: f32,
    y: f32,
    k: u32,
}

const AUCUNE: u32 = u32::MAX;

/// L'union de deux écarts, arrondie de `r` à l'angle.
fn union_ronde(a: f32, b: f32, r: f32) -> f32 {
    if r <= 0.0 {
        return a.min(b);
    }
    let (u, v) = ((r - a).max(0.0), (r - b).max(0.0));
    r.max(a.min(b)) - u.hypot(v)
}

/// Les segments des rues près d'une tuile, rangés par seaux, et ce qu'il faut pour les
/// interroger.
struct Champ<'a> {
    routes: &'a [Route],
    x0: f32,
    z0: f32,
    n: usize,
    seaux: Vec<Vec<(u32, u32)>>,
    /// Les bouts de route que prolonge une autre rue : au-delà, la chaussée s'arrondit ;
    /// sinon elle se coupe net.
    prolonges: HashSet<(u32, bool)>,
    /// Les longueurs cumulées le long de chaque route rangée.
    abscisses: HashMap<u32, Vec<f32>>,
    dmax: f32,
    rayon: f32,
}

fn cle(p: [f32; 3]) -> (i64, i64) {
    ((p[0] * 100.0).round() as i64, (p[1] * 100.0).round() as i64)
}

impl<'a> Champ<'a> {
    /// Les rues à moins de `dmax` du carré (x0, z0)-(x1, z1) agrandi de `marge`.
    fn new(l: &Lieu<'a>, x0: f32, z0: f32, x1: f32, z1: f32, marge: f32, dmax: f32, rayon: f32) -> Champ<'a> {
        let m = marge + dmax + 1.0;
        let (ax, az) = (x0 - m, z0 - m);
        let n = ((x1 - x0 + 2.0 * m) / SEAU).ceil() as usize;
        let mut seaux = vec![Vec::new(); n * n];
        // Les segments rangés dans les cases autour (une rue large déborde de sa case)
        let marge = m + 16.0;
        let (c0, c1) = (((x0 - marge) / CASE).floor() as i32, ((x1 + marge) / CASE).floor() as i32);
        let (d0, d1) = (((z0 - marge) / CASE).floor() as i32, ((z1 + marge) / CASE).floor() as i32);
        let mut vus: HashSet<(u32, u32)> = HashSet::new();
        for cz in d0..=d1 {
            for cx in c0..=c1 {
                for &(k, i) in l.index.get(&(cx, cz)).map_or(&[][..], |v| v.as_slice()) {
                    let r = &l.routes[k as usize];
                    if r.genre <= SERVICE && r.points.len() >= 2 {
                        vus.insert((k, i));
                    }
                }
            }
        }
        let mut vus: Vec<(u32, u32)> = vus.into_iter().collect();
        vus.sort_unstable();
        let mut routes_vues: Vec<u32> = Vec::new();
        for &(k, i) in &vus {
            let r = &l.routes[k as usize];
            let (a, b) = (r.points[i as usize], r.points[i as usize + 1]);
            let w = r.largeur / 2.0 + dmax;
            let g = |v: f32, o: f32| ((v - o) / SEAU).floor();
            let (sx0, sx1) = (g(a[0].min(b[0]) - w, ax).max(0.0), g(a[0].max(b[0]) + w, ax).min(n as f32 - 1.0));
            let (sz0, sz1) = (g(a[1].min(b[1]) - w, az).max(0.0), g(a[1].max(b[1]) + w, az).min(n as f32 - 1.0));
            if sx0 > sx1 || sz0 > sz1 {
                continue;
            }
            for sz in sz0 as usize..=sz1 as usize {
                for sx in sx0 as usize..=sx1 as usize {
                    seaux[sz * n + sx].push((k, i));
                }
            }
            if routes_vues.last() != Some(&k) {
                routes_vues.push(k);
            }
        }
        // Les points des rues rangées, et combien de rues passent par chacun
        let mut passages: HashMap<(i64, i64), Vec<u32>> = HashMap::new();
        for &k in &routes_vues {
            for p in &l.routes[k as usize].points {
                let v = passages.entry(cle(*p)).or_default();
                if !v.contains(&k) {
                    v.push(k);
                }
            }
        }
        let mut prolonges = HashSet::new();
        let mut abscisses = HashMap::new();
        for &k in &routes_vues {
            let r = &l.routes[k as usize];
            for (fin, p) in [(false, r.points[0]), (true, r.points[r.points.len() - 1])] {
                if passages.get(&cle(p)).is_some_and(|v| v.len() >= 2) {
                    prolonges.insert((k, fin));
                }
            }
            let mut s = Vec::with_capacity(r.points.len());
            s.push(0.0f32);
            for w in r.points.windows(2) {
                s.push(s[s.len() - 1] + (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]));
            }
            abscisses.insert(k, s);
        }
        Champ { routes: l.routes, x0: ax, z0: az, n, seaux, prolonges, abscisses, dmax, rayon }
    }

    fn seau(&self, x: f32, z: f32) -> &[(u32, u32)] {
        let (sx, sz) = (((x - self.x0) / SEAU).floor(), ((z - self.z0) / SEAU).floor());
        if sx < 0.0 || sz < 0.0 || sx >= self.n as f32 || sz >= self.n as f32 {
            return &[];
        }
        &self.seaux[sz as usize * self.n + sx as usize]
    }

    /// Le point (i, t) de la route `k` le plus proche de (x, z) parmi les segments du seau,
    /// s'il y en a un : (distance à l'axe, i, t).
    fn sur(&self, k: u32, x: f32, z: f32, borne: bool) -> Option<(f32, u32, f32)> {
        let mut mieux: Option<(f32, u32, f32)> = None;
        for &(k2, i) in self.seau(x, z) {
            if k2 != k {
                continue;
            }
            if let Some((d, t)) = self.segment(k, i, x, z, borne) {
                if mieux.is_none_or(|m| d < m.0) {
                    mieux = Some((d, i, t));
                }
            }
        }
        mieux
    }

    /// La distance de (x, z) à l'axe du segment `i` de la route `k`, et le paramètre du
    /// point le plus proche ; rien au-delà d'un bout de route coupé net (si `borne`).
    fn segment(&self, k: u32, i: u32, x: f32, z: f32, borne: bool) -> Option<(f32, f32)> {
        let r = &self.routes[k as usize];
        let (a, b) = (r.points[i as usize], r.points[i as usize + 1]);
        let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
        let l2 = dx * dx + dz * dz;
        let t0 = if l2 > 0.0 { ((x - a[0]) * dx + (z - a[1]) * dz) / l2 } else { 0.5 };
        if borne && ((i == 0 && t0 < 0.0 && !self.prolonges.contains(&(k, false))) || (i as usize + 2 == r.points.len() && t0 > 1.0 && !self.prolonges.contains(&(k, true)))) {
            return None;
        }
        let t = t0.clamp(0.0, 1.0);
        Some(((x - a[0] - dx * t).hypot(z - a[1] - dz * t), t))
    }

    /// Le champ des rues en (x, z).
    fn proche(&self, x: f32, z: f32) -> Proche {
        // Le plus proche de chaque route : écart, hauteur, direction
        let mut par_route: [(u32, f32, f32, [f32; 2]); 16] = [(AUCUNE, 0.0, 0.0, [0.0; 2]); 16];
        let mut n = 0;
        for &(k, i) in self.seau(x, z) {
            let Some((d, t)) = self.segment(k, i, x, z, true) else { continue };
            let r = &self.routes[k as usize];
            let e = d - r.largeur / 2.0;
            if e > self.dmax {
                continue;
            }
            let (a, b) = (r.points[i as usize], r.points[i as usize + 1]);
            let y = a[2] + (b[2] - a[2]) * t;
            let lg = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-6);
            let dir = [(b[0] - a[0]) / lg, (b[1] - a[1]) / lg];
            match par_route[..n].iter().position(|x| x.0 == k) {
                Some(q) => {
                    if e < par_route[q].1 {
                        par_route[q] = (k, e, y, dir);
                    }
                }
                None if n < par_route.len() => {
                    par_route[n] = (k, e, y, dir);
                    n += 1;
                }
                None => {}
            }
        }
        if n == 0 {
            return Proche { e: LOIN, y: f32::NAN, k: AUCUNE };
        }
        let l = &mut par_route[..n];
        l.sort_by(|a, b| a.1.total_cmp(&b.1));
        let (k0, e0, y0, _) = l[0];
        // L'union, arrondie entre rues qui se croisent ; pas entre deux morceaux d'une
        // même rue, ni entre rues parallèles (une route presque parallèle à l'une de celles
        // déjà prises s'y joint sans arrondi)
        let mut e = e0;
        for (q, x) in l.iter().enumerate().skip(1) {
            let f = l[..q].iter().map(|y| ((0.95 - (y.3[0] * x.3[0] + y.3[1] * x.3[1]).abs()) / 0.15).clamp(0.0, 1.0)).fold(1.0f32, f32::min);
            e = union_ronde(e, x.1, self.rayon * f);
        }
        // La hauteur, fondue avec les routes voisines au même niveau
        let (mut s, mut p) = (0.0, 0.0);
        for x in l.iter() {
            if (x.2 - y0).abs() > MARCHE {
                continue;
            }
            let w = (1.0 - (x.1 - e0) / FONDU).max(0.0).powi(2);
            s += x.2 * w;
            p += w;
        }
        Proche { e, y: if p > 0.0 { s / p } else { y0 }, k: k0 }
    }

    /// L'abscisse de (x, z) le long de la route `k`.
    fn abscisse(&self, k: u32, x: f32, z: f32) -> Option<f32> {
        let (_, i, t) = self.sur(k, x, z, false)?;
        let s = self.abscisses.get(&k)?;
        Some(s[i as usize] + (s[i as usize + 1] - s[i as usize]) * t)
    }
}

/// La distance signée de `p` au polygone `c` (négative dedans).
fn ecart_polygone(c: &[[f32; 2]], p: [f32; 2]) -> f32 {
    let mut d = f32::MAX;
    let mut dedans = false;
    for k in 0..c.len() {
        let (a, b) = (c[k], c[(k + 1) % c.len()]);
        let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
        let l2 = dx * dx + dz * dz;
        let t = if l2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dz) / l2).clamp(0.0, 1.0) } else { 0.0 };
        d = d.min((p[0] - a[0] - dx * t).hypot(p[1] - a[1] - dz * t));
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * dx {
            dedans = !dedans;
        }
    }
    if dedans {
        -d
    } else {
        d
    }
}

/// La distance de `o`, dans la direction `d` (unitaire), à la première façade des
/// bâtiments, jusqu'à `max`.
fn rayon(batis: &[&[[f32; 2]]], boites: &[[f32; 4]], o: [f32; 2], d: [f32; 2], max: f32) -> Option<f32> {
    let f = [o[0] + d[0] * max, o[1] + d[1] * max];
    let (x0, x1, z0, z1) = (o[0].min(f[0]), o[0].max(f[0]), o[1].min(f[1]), o[1].max(f[1]));
    let mut mieux: Option<f32> = None;
    for (c, bo) in batis.iter().zip(boites) {
        if bo[0] > x1 || bo[2] < x0 || bo[1] > z1 || bo[3] < z0 {
            continue;
        }
        for k in 0..c.len() {
            let (a, e) = (c[k], c[(k + 1) % c.len()]);
            let s = [e[0] - a[0], e[1] - a[1]];
            let den = d[0] * s[1] - d[1] * s[0];
            if den.abs() < 1e-9 {
                continue;
            }
            let w = [a[0] - o[0], a[1] - o[1]];
            let t = (w[0] * s[1] - w[1] * s[0]) / den;
            let u = (w[0] * d[1] - w[1] * d[0]) / den;
            if (0.0..=1.0).contains(&u) && t >= 0.0 && t <= max && mieux.is_none_or(|m| t < m) {
                mieux = Some(t);
            }
        }
    }
    mieux
}

/// Un sommet d'une couche : position, champ, hauteur de chaussée, couleur, et les
/// conditions dont il est sur la limite.
#[derive(Clone, Copy, Debug)]
struct S {
    x: f32,
    z: f32,
    e: f32,
    o: f32,
    b: f32,
    y: f32,
    c: [f32; 3],
    bords: u8,
}

fn milieu(p: &S, q: &S, t: f32) -> S {
    let f = |a: f32, b: f32| a + (b - a) * t;
    let y = if p.y.is_nan() {
        q.y
    } else if q.y.is_nan() {
        p.y
    } else {
        f(p.y, q.y)
    };
    S { x: f(p.x, q.x), z: f(p.z, q.z), e: f(p.e, q.e), o: f(p.o, q.o), b: f(p.b, q.b), y, c: [f(p.c[0], q.c[0]), f(p.c[1], q.c[1]), f(p.c[2], q.c[2])], bords: p.bords & q.bords }
}

/// Une condition d'une couche : `valeur(s) ≤ 0` dedans, et le bord qu'elle trace.
#[derive(Clone, Copy)]
enum Cond {
    /// e ≤ c
    EMax(f32),
    /// e ≥ c
    EMin(f32),
    /// o ≤ 0
    O,
    /// b ≥ 0
    B,
    /// o ≥ 0 (pas de trottoir)
    SansO,
}

/// Les bords que l'on reconnaît.
const BORDURE: u8 = 1;
const DEHORS: u8 = 2;
const CHANT: u8 = 4;

impl Cond {
    fn valeur(self, s: &S) -> f32 {
        let v = match self {
            Cond::EMax(c) => s.e - c,
            Cond::EMin(c) => c - s.e,
            Cond::O => s.o,
            Cond::B => -s.b,
            Cond::SansO => -s.o,
        };
        // Un sommet juste sur la limite compte dehors : un bord qui longe la grille n'est
        // pas pris pour une limite
        if v.abs() < 1e-5 {
            1e-5
        } else {
            v
        }
    }
}

/// Le triangle `t` découpé par les conditions ; `bord` dit quel bord trace chacune.
fn couper(t: &[S; 3], conds: &[(Cond, u8)]) -> Vec<S> {
    let mut p: Vec<S> = t.to_vec();
    let mut q: Vec<S> = Vec::with_capacity(8);
    for &(c, bord) in conds {
        q.clear();
        for k in 0..p.len() {
            let (a, b) = (&p[k], &p[(k + 1) % p.len()]);
            let (fa, fb) = (c.valeur(a), c.valeur(b));
            if fa <= 0.0 {
                q.push(*a);
            }
            if (fa <= 0.0) != (fb <= 0.0) {
                let mut m = milieu(a, b, fa / (fa - fb));
                m.bords |= bord;
                q.push(m);
            }
        }
        std::mem::swap(&mut p, &mut q);
        if p.len() < 3 {
            return Vec::new();
        }
    }
    p
}

/// Les rues de la tuile (i, j) : leurs sommets (position dans le repère de la tuile,
/// normale, couleur, coordonnées de texture) ajoutés à `revetus` (les surfaces sans
/// texture à `UNI`), et les triangles pour les collisions à `t`.
pub fn monter(l: &Lieu, i: i32, j: i32, t: &mut Vec<[[f32; 3]; 3]>, revetus: &mut [Vec<f32>; REVETEMENTS]) {
    let pr = crate::procedures::courantes();
    if !pr.chaussees && !pr.trottoirs {
        return;
    }
    let (ox, oz) = (i as f32 * TAILLE, j as f32 * TAILLE);
    let accotement = if pr.chaussees && pr.accotements { pr.accotement } else { 0.0 };
    let trottoir = if pr.trottoirs { pr.trottoir.max(pr.portee) } else { 0.0 };
    let dmax = trottoir.max(accotement).max(pr.rayon) + 2.0;
    // (avec de la marge : les coins des bâtiments voisins y cherchent leur rue, et chaque
    // tuile doit trouver la même)
    let champ = Champ::new(l, ox, oz, ox + TAILLE, oz + TAILLE, crate::procedures::PORTEE_MAX + 10.0, dmax, pr.rayon);
    if champ.prolonges.is_empty() && champ.abscisses.is_empty() {
        return;
    }
    let n = (TAILLE / PAS).round() as usize;
    let m = n + 1;
    let pos = |a: usize, b: usize| (ox + a as f32 * PAS, oz + b as f32 * PAS);
    let mut g: Vec<Proche> = Vec::with_capacity(m * m);
    for b in 0..m {
        for a in 0..m {
            let (x, z) = pos(a, b);
            g.push(champ.proche(x, z));
        }
    }
    if g.iter().all(|p| p.e > dmax) {
        return;
    }
    // Les bâtiments : leur écart (négatif dedans), dans leur boîte
    let mut ecart_bati = vec![LOIN; m * m];
    let rang = |v: f32, o: f32| ((v - o) / PAS).clamp(-1.0, m as f32);
    let boites: Vec<[f32; 4]> = l
        .batis
        .iter()
        .map(|c| c.iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |s, p| [s[0].min(p[0]), s[1].min(p[1]), s[2].max(p[0]), s[3].max(p[1])]))
        .collect();
    if pr.trottoirs {
        for (c, bo) in l.batis.iter().zip(&boites) {
            if c.len() < 3 {
                continue;
            }
            let (a0, a1) = (rang(bo[0] - 1.0, ox).ceil().max(0.0) as usize, rang(bo[2] + 1.0, ox).floor().min(n as f32));
            let (b0, b1) = (rang(bo[1] - 1.0, oz).ceil().max(0.0) as usize, rang(bo[3] + 1.0, oz).floor().min(n as f32));
            if a1 < 0.0 || b1 < 0.0 {
                continue;
            }
            for b in b0..=b1 as usize {
                for a in a0..=a1 as usize {
                    let (x, z) = pos(a, b);
                    let d = ecart_polygone(c, [x, z]);
                    if d < ecart_bati[b * m + a] {
                        ecart_bati[b * m + a] = d;
                    }
                }
            }
        }
    }
    // La façade en face : depuis chaque point à moins de la portée, droit devant (en
    // s'éloignant de la chaussée, le long du gradient du champ, qui part en diagonale dans
    // les angles), l'écart à la bordure de la première façade à portée. Sur une grille
    // élargie de TROU : entre deux façades, la limite se tire de l'une à l'autre
    const TROU: usize = 10;
    let me = m + 2 * TROU;
    let pos_e = |a: usize, b: usize| (ox + (a as f32 - TROU as f32) * PAS, oz + (b as f32 - TROU as f32) * PAS);
    let mut facade: Vec<Option<f32>> = vec![None; me * me];
    let mut devant: Vec<[f32; 2]> = vec![[0.0; 2]; me * me];
    if pr.trottoirs && !l.batis.is_empty() {
        // L'écart sur la grille élargie d'un nœud de plus, pour le gradient
        let mx = me + 2;
        let ex: Vec<f32> = (0..mx * mx)
            .map(|k| {
                let (a, b) = (k % mx, k / mx);
                let dedans = a > TROU && b > TROU && a <= TROU + m && b <= TROU + m;
                if dedans {
                    g[(b - TROU - 1) * m + a - TROU - 1].e
                } else {
                    let (x, z) = pos_e(a, b);
                    champ.proche(x - PAS, z - PAS).e
                }
            })
            .collect();
        for b in 0..me {
            for a in 0..me {
                let (x, z) = pos_e(a, b);
                let e = ex[(b + 1) * mx + a + 1];
                if e < 0.0 || e >= pr.portee {
                    continue;
                }
                let (gx, gz) = (ex[(b + 1) * mx + a + 2] - ex[(b + 1) * mx + a], ex[(b + 2) * mx + a + 1] - ex[b * mx + a + 1]);
                let lg = gx.hypot(gz);
                if lg < 1e-6 {
                    continue;
                }
                let d = [gx / lg, gz / lg];
                devant[b * me + a] = d;
                facade[b * me + a] = rayon(&l.batis, &boites, [x, z], d, pr.portee - e).map(|t| e + t);
            }
        }
    }
    let limite = |a: usize, b: usize| -> Option<f32> {
        let k = (b + TROU) * me + a + TROU;
        if let Some(f) = facade[k] {
            return Some(f);
        }
        let d = devant[k];
        if d == [0.0; 2] {
            return None;
        }
        let t = [-d[1], d[0]];
        let cote = |s: f32| {
            (1..=TROU).find_map(|e| {
                let (qa, qb) = ((a + TROU) as f32 + s * e as f32 * t[0] / PAS, (b + TROU) as f32 + s * e as f32 * t[1] / PAS);
                let (qa, qb) = (qa.round(), qb.round());
                if qa < 0.0 || qb < 0.0 || qa >= me as f32 || qb >= me as f32 {
                    return None;
                }
                facade[qb as usize * me + qa as usize].map(|f| (e as f32, f))
            })
        };
        match (cote(1.0), cote(-1.0)) {
            (Some((ea, fa)), Some((eb, fb))) => Some(fa + (fb - fa) * ea / (ea + eb)),
            _ => None,
        }
    };
    // Le champ aux nœuds
    let mut sommets: Vec<S> = Vec::with_capacity(m * m);
    let mut eau = vec![false; m * m];
    for b in 0..m {
        for a in 0..m {
            let q = g[b * m + a];
            let (x, z) = pos(a, b);
            let mut o = LOIN;
            if pr.trottoirs && q.k != AUCUNE && q.e < dmax {
                let r = &l.routes[q.k as usize];
                let regle = crate::voirie::trottoir(r);
                let permis = match regle {
                    Some(w) => w > 0.0,
                    None => (l.urbain)(x, z),
                };
                if permis {
                    let w = regle.unwrap_or(pr.trottoir);
                    // Au-delà de la largeur, jusqu'à la façade en face, si rien ne les
                    // sépare de la bordure (pas d'arrière-cour derrière un bâtiment)
                    let f = limite(a, b).filter(|&f| f > w && (q.e <= w || rayon(&l.batis, &boites, [x, z], devant[(b + TROU) * me + a + TROU].map(|c| -c), q.e).is_none()));
                    o = q.e - f.unwrap_or(w).min(pr.portee).max(w);
                }
            }
            if o < 1.0 {
                eau[b * m + a] = (l.eau)(x, z);
            }
            let c = if q.k == AUCUNE { [0.0; 3] } else { teinte(l.routes[q.k as usize].genre) };
            sommets.push(S { x, z, e: q.e, o, b: ecart_bati[b * m + a], y: q.y, c, bords: 0 });
        }
    }
    // Le sol, sur une grille de 2 m, là où il sert (sous les trottoirs et les accotements)
    let (pas_sol, ms) = (2.0f32, (TAILLE / 2.0) as usize + 1);
    let mut sol = vec![f32::NAN; ms * ms];
    for b in 0..m {
        for a in 0..m {
            let s = &sommets[b * m + a];
            if s.o < 1.0 || (accotement > 0.0 && s.e < accotement + 1.0) {
                let (sa, sb) = ((a as f32 * PAS / pas_sol) as usize, (b as f32 * PAS / pas_sol) as usize);
                for (pa, pb) in [(sa, sb), (sa + 1, sb), (sa, sb + 1), (sa + 1, sb + 1)] {
                    if pa < ms && pb < ms && sol[pb * ms + pa].is_nan() {
                        sol[pb * ms + pa] = (l.sol)(ox + pa as f32 * pas_sol, oz + pb as f32 * pas_sol);
                    }
                }
            }
        }
    }
    let sol_en = |x: f32, z: f32| {
        let (gx, gz) = (((x - ox) / pas_sol).clamp(0.0, (ms - 1) as f32), ((z - oz) / pas_sol).clamp(0.0, (ms - 1) as f32));
        let (a, b) = ((gx.floor() as usize).min(ms - 2), (gz.floor() as usize).min(ms - 2));
        let (u, w) = (gx - a as f32, gz - b as f32);
        let h = [sol[b * ms + a], sol[b * ms + a + 1], sol[(b + 1) * ms + a], sol[(b + 1) * ms + a + 1]];
        if h.iter().any(|x| x.is_nan()) {
            return (l.sol)(x, z);
        }
        (h[0] * (1.0 - u) + h[1] * u) * (1.0 - w) + (h[2] * (1.0 - u) + h[3] * u) * w
    };
    let dessus_trottoir = |s: &S| sol_en(s.x, s.z).max(s.y) + pr.bordure;
    // Un triangle : sa normale (tournée vers `attendu`), ses sommets, et ses coordonnées
    // de texture s'il est revêtu
    let mut tri = |p: [[f32; 3]; 3], c: [[f32; 3]; 3], attendu: [f32; 3], uv: Option<(usize, [[f32; 2]; 3])>, heurte: bool| {
        let (u, w) = ([p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]], [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]]);
        let mut nm = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
        let lg = (nm[0] * nm[0] + nm[1] * nm[1] + nm[2] * nm[2]).sqrt();
        if lg < 1e-9 {
            return;
        }
        let (p, c, uv) = if nm[0] * attendu[0] + nm[1] * attendu[1] + nm[2] * attendu[2] < 0.0 {
            nm = nm.map(|x| -x);
            ([p[0], p[2], p[1]], [c[0], c[2], c[1]], uv.map(|(r, w)| (r, [w[0], w[2], w[1]])))
        } else {
            (p, c, uv)
        };
        let p = p.map(|q| [q[0] - ox, q[1], q[2] - oz]);
        let nm = nm.map(|x| x / lg);
        let (r, w) = uv.unwrap_or((UNI, [[0.0; 2]; 3]));
        for k in 0..3 {
            revetus[r].extend_from_slice(&p[k]);
            revetus[r].extend_from_slice(&nm);
            revetus[r].extend_from_slice(&c[k]);
            revetus[r].extend_from_slice(&w[k]);
        }
        if heurte {
            t.push(p);
        }
    };
    let haut = [0.0, 1.0, 0.0];
    // Le long de la bordure : u suit l'abscisse de la route `k`, ramenée près de zéro
    let le_long = |poly: &[S], k: u32, pave: f32| -> Vec<f32> {
        let mut u: Vec<f32> = poly.iter().map(|s| champ.abscisse(k, s.x, s.z).unwrap_or(0.0) / (pave * PAVES_PAR_IMAGE as f32)).collect();
        let base = u.iter().fold(f32::MAX, |a, &b| a.min(b)).floor();
        u.iter_mut().for_each(|x| *x -= base);
        u
    };
    let centre = |poly: &[S]| {
        let n = poly.len() as f32;
        [poly.iter().map(|s| s.x).sum::<f32>() / n, poly.iter().map(|s| s.z).sum::<f32>() / n]
    };
    // Les couches et leurs conditions
    const CHAUSSEE: usize = 0;
    const ACCOTEMENT: usize = 1;
    const RIGOLE_: usize = 2;
    const RANG: usize = 3;
    const BITUME_: usize = 4;
    let conds: [Vec<(Cond, u8)>; 5] = [
        vec![(Cond::EMax(0.0), 0)],
        vec![(Cond::EMin(0.0), 0), (Cond::EMax(accotement), CHANT), (Cond::SansO, 0)],
        vec![(Cond::EMax(0.0), 0), (Cond::EMin(-RIGOLE), 0), (Cond::O, 0), (Cond::B, 0)],
        vec![(Cond::EMin(0.0), BORDURE), (Cond::EMax(BANDE), 0), (Cond::O, DEHORS), (Cond::B, DEHORS)],
        vec![(Cond::EMin(BANDE), 0), (Cond::O, DEHORS), (Cond::B, DEHORS)],
    ];
    let actives: Vec<usize> = [(CHAUSSEE, pr.chaussees), (ACCOTEMENT, accotement > 0.0), (RIGOLE_, pr.trottoirs), (RANG, pr.trottoirs), (BITUME_, pr.trottoirs)].iter().filter(|x| x.1).map(|x| x.0).collect();
    // Une couche sur un triangle (déjà découpé ou non)
    let mut emettre = |couche: usize, p: &[S], k: u32| {
        if p.len() < 3 {
            return;
        }
        match couche {
            CHAUSSEE => {
                for q in 1..p.len() - 1 {
                    let s = [p[0], p[q], p[q + 1]];
                    tri(s.map(|s| [s.x, s.y + DESSUS, s.z]), s.map(|s| s.c), haut, None, true);
                }
            }
            ACCOTEMENT => {
                let c = if (l.urbain)(p[0].x, p[0].z) { ACCOTEMENT_VILLE } else { ACCOTEMENT_CAMPAGNE };
                for q in 1..p.len() - 1 {
                    let s = [p[0], p[q], p[q + 1]];
                    tri(s.map(|s| [s.x, s.y + DESSOUS, s.z]), [c; 3], haut, None, true);
                }
                let ce = centre(p);
                for q in 0..p.len() {
                    let (s0, s1) = (p[q], p[(q + 1) % p.len()]);
                    if s0.bords & s1.bords & CHANT == 0 {
                        continue;
                    }
                    let (a0, b0) = ([s0.x, s0.y + DESSOUS, s0.z], [s1.x, s1.y + DESSOUS, s1.z]);
                    let (a1, b1) = ([s0.x, sol_en(s0.x, s0.z).min(s0.y) - 0.3, s0.z], [s1.x, sol_en(s1.x, s1.z).min(s1.y) - 0.3, s1.z]);
                    let dehors = [(s0.x + s1.x) / 2.0 - ce[0], 0.0, (s0.z + s1.z) / 2.0 - ce[1]];
                    tri([a0, b0, b1], [c; 3], dehors, None, false);
                    tri([a0, b1, a1], [c; 3], dehors, None, false);
                }
            }
            RIGOLE_ => {
                let u = le_long(p, k, PAVE_CANIVEAU);
                for q in 1..p.len() - 1 {
                    let s = [0, q, q + 1];
                    let uv = s.map(|x| [u[x], -p[x].e / RIGOLE]);
                    tri(s.map(|x| [p[x].x, p[x].y + SUR_CHAUSSEE, p[x].z]), [CANIVEAU; 3], haut, Some((PAVES, uv)), false);
                }
            }
            _ => {
                let rang = couche == RANG;
                let h: Vec<f32> = p.iter().map(dessus_trottoir).collect();
                let u = if rang { le_long(p, k, PAVE_TROTTOIR) } else { Vec::new() };
                for q in 1..p.len() - 1 {
                    let s = [0, q, q + 1];
                    let pos = s.map(|x| [p[x].x, h[x], p[x].z]);
                    if rang {
                        tri(pos, [TERNE; 3], haut, Some((PAVES, s.map(|x| [u[x], p[x].e / BANDE]))), true);
                    } else {
                        tri(pos, [BLANC; 3], haut, Some((BITUME, s.map(|x| [(p[x].x - ox) / METRES_BITUME, (p[x].z - oz) / METRES_BITUME]))), true);
                    }
                }
                let ce = centre(p);
                for q in 0..p.len() {
                    let (r0, r1) = (q, (q + 1) % p.len());
                    let (s0, s1) = (p[r0], p[r1]);
                    let dehors = [(s0.x + s1.x) / 2.0 - ce[0], 0.0, (s0.z + s1.z) / 2.0 - ce[1]];
                    if rang && s0.bords & s1.bords & BORDURE != 0 {
                        // La face de la bordure, revêtue des mêmes pavés qui s'y replient :
                        // v part de l'arête
                        let (a0, b0) = ([s0.x, s0.y - PIED, s0.z], [s1.x, s1.y - PIED, s1.z]);
                        let (a, b) = ([s0.x, h[r0], s0.z], [s1.x, h[r1], s1.z]);
                        let uv = [[u[r0], (a[1] - a0[1]) / BANDE], [u[r1], (b[1] - b0[1]) / BANDE], [u[r1], 0.0], [u[r0], 0.0]];
                        tri([a0, b0, b], [TERNE; 3], dehors, Some((PAVES, [uv[0], uv[1], uv[2]])), true);
                        tri([a0, b, a], [TERNE; 3], dehors, Some((PAVES, [uv[0], uv[2], uv[3]])), true);
                    } else if s0.bords & s1.bords & DEHORS != 0 {
                        chant(&mut tri, &sol_en, s0, s1, h[r0], h[r1], dehors);
                    }
                }
            }
        }
    };
    // Une couche sur un triangle de la grille fine : découpé, s'il est d'un seul niveau et
    // (pour les trottoirs) hors de l'eau
    let fin = |couche: usize, coins: [usize; 3], emettre: &mut dyn FnMut(usize, &[S], u32)| {
        let ts = coins.map(|k| sommets[k]);
        if ts.iter().all(|s| s.e > dmax) {
            return;
        }
        let (lo, hi) = ts.iter().filter(|s| s.y.is_finite()).fold((f32::MAX, f32::MIN), |m, s| (m.0.min(s.y), m.1.max(s.y)));
        if lo > hi || hi - lo > MARCHE || (couche >= RIGOLE_ && coins.iter().any(|&c| eau[c])) {
            return;
        }
        let p = couper(&ts, &conds[couche]);
        emettre(couche, &p, g[coins[0]].k);
    };
    // Par mailles de GROS × GROS, divisées au besoin : une couche qui couvre toute une
    // maille (tous ses nœuds dedans, d'un seul niveau, hors de l'eau) s'y pose en deux
    // grands triangles ; une couche absente de toute la maille n'y fait rien ; sinon, la
    // maille se coupe en quatre, jusqu'à la grille fine
    const GROS: usize = 4;
    let mut pile: Vec<(usize, usize, usize)> = Vec::new();
    for gb in (0..n).step_by(GROS) {
        for ga in (0..n).step_by(GROS) {
            if (gb..=(gb + GROS).min(n)).all(|b| (ga..=(ga + GROS).min(n)).all(|a| sommets[b * m + a].e > dmax)) {
                continue;
            }
            for &couche in &actives {
                pile.push((ga, gb, GROS));
                while let Some((a0, b0, cote)) = pile.pop() {
                    let (a1, b1) = ((a0 + cote).min(n), (b0 + cote).min(n));
                    if cote == 1 {
                        let (i00, i10, i11, i01) = (b0 * m + a0, b0 * m + a1, b1 * m + a1, b1 * m + a0);
                        for coins in [[i00, i10, i11], [i00, i11, i01]] {
                            fin(couche, coins, &mut emettre);
                        }
                        continue;
                    }
                    let noeuds = (b0..=b1).flat_map(|b| (a0..=a1).map(move |a| b * m + a));
                    let (mut lo, mut hi, mut mouille, mut nb) = (f32::MAX, f32::MIN, false, 0usize);
                    let mut comptes = [0usize; 4];
                    for k in noeuds {
                        let s = &sommets[k];
                        lo = lo.min(s.y);
                        hi = hi.max(s.y);
                        mouille |= eau[k];
                        nb += 1;
                        for (c, x) in conds[couche].iter().zip(comptes.iter_mut()) {
                            if c.0.valeur(s) <= 0.0 {
                                *x += 1;
                            }
                        }
                    }
                    let comptes = &comptes[..conds[couche].len()];
                    if comptes.iter().any(|&x| x == 0) {
                        continue;
                    }
                    let uni = lo.is_finite() && hi.is_finite() && hi - lo <= MARCHE && (!mouille || couche < RIGOLE_);
                    if uni && comptes.iter().all(|&x| x == nb) {
                        let (i00, i10, i11, i01) = (b0 * m + a0, b0 * m + a1, b1 * m + a1, b1 * m + a0);
                        for coins in [[i00, i10, i11], [i00, i11, i01]] {
                            emettre(couche, &coins.map(|k| sommets[k]), g[coins[0]].k);
                        }
                        continue;
                    }
                    let h = cote / 2;
                    for (da, db) in [(0, 0), (h, 0), (0, h), (h, h)] {
                        if a0 + da < n && b0 + db < n {
                            pile.push((a0 + da, b0 + db, h));
                        }
                    }
                }
            }
        }
    }
}

/// Le chant du trottoir côté façade, du dessus jusque sous le sol.
#[allow(clippy::too_many_arguments)]
fn chant(tri: &mut impl FnMut([[f32; 3]; 3], [[f32; 3]; 3], [f32; 3], Option<(usize, [[f32; 2]; 3])>, bool), sol: &impl Fn(f32, f32) -> f32, s0: S, s1: S, h0: f32, h1: f32, dehors: [f32; 3]) {
    let (a, b) = ([s0.x, h0, s0.z], [s1.x, h1, s1.z]);
    let (a2, b2) = ([s0.x, sol(s0.x, s0.z).min(h0) - 0.3, s0.z], [s1.x, sol(s1.x, s1.z).min(h1) - 0.3, s1.z]);
    tri([a, b, b2], [SOMBRE; 3], dehors, None, true);
    tri([a, b2, a2], [SOMBRE; 3], dehors, None, true);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reel::{indexer, ROUTE};
    use crate::regions::{Bati, Forme};

    fn rue() -> Vec<Route> {
        vec![Route { genre: ROUTE, largeur: 6.0, points: vec![[-100.0, 30.0, 10.0], [200.0, 30.0, 10.0]] }]
    }

    /// L'aire des triangles colorés de cette couleur, à plat à cette hauteur.
    fn aire(v: &[f32], c: [f32; 3], y: f32) -> f32 {
        v.chunks(27)
            .filter(|t| (0..3).all(|k| t[k * 9 + 6..k * 9 + 9] == c && (t[k * 9 + 1] - y).abs() < 1e-4))
            .map(|t| {
                let (u, w) = ([t[9] - t[0], t[11] - t[2]], [t[18] - t[0], t[20] - t[2]]);
                (u[0] * w[1] - u[1] * w[0]).abs() / 2.0
            })
            .sum()
    }

    /// Les triangles revêtus de `r` : leurs sommets (11 flottants chacun).
    fn revetus(r: &[f32]) -> Vec<&[f32]> {
        r.chunks(11).collect()
    }

    fn maison() -> Bati {
        // Une maison au sud de la rue, sa façade à 5 m de la bordure (la rue va de z 27 à 33)
        let forme =
            Forme { contour: vec![[-5.0, -4.0], [5.0, -4.0], [5.0, 4.0], [-5.0, 4.0]], pied: 0.0, egout: 6.0, faitage: 6.0, toit: crate::toits::PLAT, axe: 0.0, murs: [0.8; 3], couverture: [0.5; 3] };
        Bati::new("main-1".into(), (0, 0), [32.0, 10.0, 42.0], 0.0, forme, "main".into())
    }

    fn monter_ici(routes: &[Route], batis: Vec<&[[f32; 2]]>, urbain: bool, i: i32, j: i32) -> (Vec<f32>, Vec<[[f32; 3]; 3]>, [Vec<f32>; REVETEMENTS]) {
        let index = indexer(routes);
        let (u, sol, sec) = (move |_: f32, _: f32| urbain, |_: f32, _: f32| 10.0f32, |_: f32, _: f32| false);
        let l = Lieu { routes, index: &index, batis, urbain: &u, sol: &sol, eau: &sec };
        let (mut t, mut r): (_, [Vec<f32>; REVETEMENTS]) = (Vec::new(), Default::default());
        monter(&l, i, j, &mut t, &mut r);
        // Les surfaces sans texture, sans leurs coordonnées de texture
        let v = r[UNI].chunks(11).flat_map(|s| s[..9].to_vec()).collect();
        r[UNI].clear();
        (v, t, r)
    }

    #[test]
    fn la_chaussee_et_ses_accotements_couvrent_la_rue() {
        let (v, t, r) = monter_ici(&rue(), vec![], false, 0, 0);
        assert!(!v.is_empty() && v.len() % 27 == 0);
        assert!(r.iter().all(Vec::is_empty), "à la campagne, pas de trottoir");
        // Le dessus arrête qui marche ou roule, pas les chants
        assert!(!t.is_empty() && t.iter().all(|p| p.iter().all(|q| q[1] > 10.0)));
        // 6 × 64 m de chaussée, et 2 × 2 × 64 m d'accotement de part et d'autre
        let a = aire(&v, teinte(ROUTE), 10.0 + DESSUS);
        assert!((a - 6.0 * 64.0).abs() < 1.0, "{a}");
        let b = aire(&v, ACCOTEMENT_CAMPAGNE, 10.0 + DESSOUS);
        assert!((b - 4.0 * 64.0).abs() < 1.0, "{b}");
        // Les normales du dessus vers le haut ; les chants descendent sous le sol
        assert!(v.chunks(27).all(|t| t[4] > 0.99 || t[4].abs() < 1e-3));
        assert!(v.chunks(9).any(|s| s[1] < 10.0 - 0.29));
        // Une tuile loin de la rue : rien
        let (w, _, _) = monter_ici(&rue(), vec![], false, 0, 3);
        assert!(w.is_empty());
    }

    #[test]
    fn la_chaussee_se_coupe_au_bout_de_la_route() {
        let routes = vec![Route { genre: ROUTE, largeur: 6.0, points: vec![[-100.0, 30.0, 3.0], [32.0, 30.0, 3.0]] }];
        let (v, _, _) = monter_ici(&routes, vec![], false, 0, 0);
        let a = aire(&v, teinte(ROUTE), 3.0 + DESSUS);
        assert!((a - 6.0 * 32.0).abs() < 4.0, "{a}");
        assert!(v.chunks(9).all(|s| s[0] < 33.0));
    }

    #[test]
    fn le_trottoir_va_de_la_bordure_a_la_facade() {
        let b = maison();
        let (v, t, r) = monter_ici(&rue(), vec![b.b.contour.as_slice()], true, 0, 0);
        assert!(!t.is_empty());
        // Le dessus est à 15 cm, et ne passe pas la façade (z 38) devant la maison
        let tout: Vec<&[f32]> = r.iter().flat_map(|r| r.chunks(11)).collect();
        let dessus: Vec<[f32; 3]> = tout.iter().filter(|s| s[4] > 0.9 && s[1] > 10.1).map(|s| [s[0], s[1], s[2]]).collect();
        assert!(dessus.iter().all(|p| (p[1] - 10.15).abs() < 1e-3), "{:?}", dessus.iter().find(|p| (p[1] - 10.15).abs() >= 1e-3));
        assert!(dessus.iter().filter(|p| p[0] > 30.0 && p[0] < 34.0 && p[2] > 33.0).all(|p| p[2] <= 38.0 + 1e-3));
        assert!(dessus.iter().any(|p| p[0] > 30.0 && p[0] < 34.0 && (p[2] - 38.0).abs() < 0.05), "jusqu'à la façade");
        // De l'autre côté, sans façade : 2 m
        assert!(dessus.iter().filter(|p| p[2] < 30.0).all(|p| p[2] >= 27.0 - 2.0 - 1e-3));
        assert!(dessus.iter().any(|p| (p[2] - 25.0).abs() < 1e-3));
        // Les revêtus regardent en haut, sauf la face de la bordure, qui regarde la chaussée
        assert!(tout.iter().all(|s| s[4] > 0.9 || s[4].abs() < 1e-3));
        assert!(r[PAVES].chunks(11).any(|s| s[4].abs() < 1e-3 && s[2] > 30.0 && s[5] < -0.9), "la face de la bordure, vers la chaussée");
        assert!(v.chunks(9).filter(|s| s[4].abs() > 0.5).all(|s| s[4] > 0.0));
        // Le rang de pavés le long de la bordure (z 33 à 33,2 au sud), le bitume au-delà
        let paves = revetus(&r[PAVES]);
        let dessus_paves: Vec<&&[f32]> = paves.iter().filter(|s| s[4] > 0.9 && s[1] > 10.1).collect();
        assert!(!dessus_paves.is_empty() && !r[BITUME].is_empty());
        assert!(dessus_paves.iter().filter(|s| s[2] > 30.0).all(|s| s[2] >= 33.0 - 1e-3 && s[2] <= 33.0 + BANDE + 1e-3));
        assert!(r[BITUME].chunks(11).filter(|s| s[2] > 30.0).all(|s| s[2] >= 33.0 + BANDE - 1e-3));
        // Le caniveau, sur la chaussée (z 32,85 à 33), juste au-dessus
        let caniveau: Vec<&&[f32]> = paves.iter().filter(|s| s[2] > 30.0 && s[2] < 33.0 - 1e-3).collect();
        assert!(!caniveau.is_empty());
        assert!(caniveau.iter().all(|s| s[2] >= 33.0 - RIGOLE - 1e-3 && (s[1] - 10.0 - SUR_CHAUSSEE).abs() < 1e-3));
        assert!(paves.iter().all(|s| s[10] >= -1e-3 && s[10] <= 1.0 + 1e-3));
    }

    #[test]
    fn un_trottoir_note_a_part_n_arrete_pas_le_trottoir() {
        // Le long de la rue (bords à z 27 et 33), une voie piétonne de 2 m à z 36, et une
        // façade à z 38
        let mut routes = rue();
        routes.push(Route { genre: crate::reel::PIETONNE, largeur: 2.0, points: vec![[0.0, 36.0, 10.0], [64.0, 36.0, 10.0]] });
        let b = maison();
        let (_, _, r) = monter_ici(&routes, vec![b.b.contour.as_slice()], true, 0, 0);
        let dessus: Vec<[f32; 3]> = r.iter().flat_map(|r| r.chunks(11)).filter(|s| s[4] > 0.9 && s[1] > 10.1).map(|s| [s[0], s[1], s[2]]).collect();
        assert!(dessus.iter().any(|p| p[0] > 30.0 && p[0] < 34.0 && (p[2] - 38.0).abs() < 0.05), "jusqu'à la façade, par-dessus la voie piétonne");
    }

    #[test]
    fn au_carrefour_le_trottoir_tourne_l_angle() {
        // Deux rues de 6 m qui se croisent en (32, 32) : bords à 29 et 35 ; l'angle nord-est
        // du trottoir s'arrondit de 4 m autour de (35, 35)
        let routes = vec![
            Route { genre: ROUTE, largeur: 6.0, points: vec![[-100.0, 32.0, 10.0], [32.0, 32.0, 10.0]] },
            Route { genre: ROUTE, largeur: 6.0, points: vec![[32.0, 32.0, 10.0], [200.0, 32.0, 10.0]] },
            Route { genre: ROUTE, largeur: 6.0, points: vec![[32.0, -100.0, 10.0], [32.0, 32.0, 10.0], [32.0, 200.0, 10.0]] },
        ];
        let (v, _, r) = monter_ici(&routes, vec![], true, 0, 0);
        // Des triangles du dessus (chaussée et trottoir) qui contiennent un point
        fn couvre(tris: &mut dyn Iterator<Item = [[f32; 2]; 3]>, p: [f32; 2]) -> bool {
            for t in tris {
                let s = |a: [f32; 2], b: [f32; 2]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
                let (d0, d1, d2) = (s(t[0], t[1]), s(t[1], t[2]), s(t[2], t[0]));
                if (d0 >= -1e-4 && d1 >= -1e-4 && d2 >= -1e-4) || (d0 <= 1e-4 && d1 <= 1e-4 && d2 <= 1e-4) {
                    return true;
                }
            }
            false
        }
        let trottoir = || r.iter().flat_map(|r| r.chunks(33)).filter(|t| t[4] > 0.9 && t[1] > 10.1).map(|t| [[t[0], t[2]], [t[11], t[13]], [t[22], t[24]]]);
        let chaussee = || v.chunks(27).filter(|t| t[1] > 10.0 && t[6..9] == teinte(ROUTE)).map(|t| [[t[0], t[2]], [t[9], t[11]], [t[18], t[20]]]);
        let c = [35.0 + 4.0, 35.0 + 4.0];
        // Le long de la bordure arrondie : le cercle de rayon 4 autour de (39, 39), de
        // l'ouest au nord ; à 1 m dedans, du trottoir ; à 0,5 m dehors, de la chaussée
        for k in 0..=20 {
            let a = std::f32::consts::PI + std::f32::consts::FRAC_PI_2 * k as f32 / 20.0;
            let (ca, sa) = (a.cos(), a.sin());
            let dedans = [c[0] + ca * 3.0, c[1] + sa * 3.0];
            let dehors = [c[0] + ca * 4.5, c[1] + sa * 4.5];
            assert!(couvre(&mut trottoir(), dedans), "trottoir en {dedans:?}");
            assert!(couvre(&mut chaussee(), dehors), "chaussée en {dehors:?}");
        }
        // Le long des deux rues, loin de l'angle : trottoir à 1 m de la bordure
        assert!(couvre(&mut trottoir(), [50.0, 36.0]) && couvre(&mut trottoir(), [36.0, 50.0]));
        // Et nulle part sur la chaussée
        assert!(!couvre(&mut trottoir(), [32.0, 32.0]) && !couvre(&mut trottoir(), [33.0, 34.0]));
    }

    #[test]
    fn a_l_angle_le_trottoir_va_jusqu_au_batiment() {
        // Le même carrefour, et un bâtiment dans l'angle nord-est, ses façades à 6 m des
        // bordures (x et z de 41 à 55) : le trottoir remplit l'angle jusqu'à lui
        let routes = vec![
            Route { genre: ROUTE, largeur: 6.0, points: vec![[-100.0, 32.0, 10.0], [32.0, 32.0, 10.0]] },
            Route { genre: ROUTE, largeur: 6.0, points: vec![[32.0, 32.0, 10.0], [200.0, 32.0, 10.0]] },
            Route { genre: ROUTE, largeur: 6.0, points: vec![[32.0, -100.0, 10.0], [32.0, 32.0, 10.0], [32.0, 200.0, 10.0]] },
        ];
        let c: &[[f32; 2]] = &[[41.0, 41.0], [55.0, 41.0], [55.0, 55.0], [41.0, 55.0]];
        let (_, _, r) = monter_ici(&routes, vec![c], true, 0, 0);
        let dessus: Vec<[[f32; 2]; 3]> = r.iter().flat_map(|r| r.chunks(33)).filter(|t| t[4] > 0.9 && t[1] > 10.1).map(|t| [[t[0], t[2]], [t[11], t[13]], [t[22], t[24]]]).collect();
        let couvre = |p: [f32; 2]| {
            dessus.iter().any(|t| {
                let s = |a: [f32; 2], b: [f32; 2]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
                let (d0, d1, d2) = (s(t[0], t[1]), s(t[1], t[2]), s(t[2], t[0]));
                (d0 >= -1e-4 && d1 >= -1e-4 && d2 >= -1e-4) || (d0 <= 1e-4 && d1 <= 1e-4 && d2 <= 1e-4)
            })
        };
        for p in [[38.5, 38.5], [40.0, 38.0], [38.0, 40.0], [40.0, 40.0], [40.5, 40.5], [48.0, 40.0], [40.0, 48.0]] {
            assert!(couvre(p), "trottoir en {p:?}");
        }
        // Pas dans le bâtiment
        assert!(!couvre([42.0, 42.0]));
    }

    #[test]
    fn deux_morceaux_d_une_meme_rue_ne_bombent_pas() {
        // Une rue en deux morceaux bout à bout, coudée de 10° : le bord ne bouge pas au
        // raccord, et la chaussée ne s'y ouvre pas
        let routes = vec![
            Route { genre: ROUTE, largeur: 6.0, points: vec![[-100.0, 30.0, 10.0], [32.0, 30.0, 10.0]] },
            Route { genre: ROUTE, largeur: 6.0, points: vec![[32.0, 30.0, 10.0], [232.0, 30.0 + 200.0 * 0.176, 10.0]] },
        ];
        let index = indexer(&routes);
        let (u, sol, sec) = (|_: f32, _: f32| true, |_: f32, _: f32| 10.0f32, |_: f32, _: f32| false);
        let l = Lieu { routes: &routes, index: &index, batis: vec![], urbain: &u, sol: &sol, eau: &sec };
        let c = Champ::new(&l, 0.0, 0.0, 64.0, 64.0, 0.0, 14.0, 4.0);
        // Au-dessus du coude, à 1 m du bord nord (z 27 avant le coude)
        let e = c.proche(32.0, 26.0).e;
        assert!((e - 1.0).abs() < 0.1, "{e}");
        // Juste après le coude, des deux côtés : la chaussée continue
        assert!(c.proche(32.5, 32.9).e < 0.0 && c.proche(32.5, 27.2).e < 0.0);
    }

    #[test]
    fn les_outils() {
        assert_eq!(union_ronde(1.0, 2.0, 0.0), 1.0);
        assert!((union_ronde(10.0, 2.0, 4.0) - 2.0).abs() < 1e-6);
        // Deux demi-plans à angle droit : l'angle arrondi de 4
        assert!(union_ronde(0.0, 0.0, 4.0) < -1.0);
        let carre: &[[f32; 2]] = &[[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]];
        assert_eq!(rayon(&[carre], &[[0.0, 0.0, 2.0, 2.0]], [-3.0, 1.0], [1.0, 0.0], 5.0), Some(3.0));
        assert_eq!(rayon(&[carre], &[[0.0, 0.0, 2.0, 2.0]], [-3.0, 1.0], [1.0, 0.0], 2.0), None);
        let carre = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]];
        assert!((ecart_polygone(&carre, [1.0, 1.0]) + 1.0).abs() < 1e-6);
        assert!((ecart_polygone(&carre, [3.0, 1.0]) - 1.0).abs() < 1e-6);
    }
}
