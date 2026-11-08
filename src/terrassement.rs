//! Le terrassement sous les routes du lieu réel, avant qu'elles aient leur propre chaussée.
//!
//! - Le profil en long : la hauteur de la chaussée se lisse sur tout le réseau à la fois,
//!   sur `Procedures::lissage` mètres environ (un écart type), sans s'arrêter aux bouts
//!   des voies : un carrefour (un point partagé par plusieurs routes) est un seul nœud,
//!   d'une seule hauteur, lissé avec toutes les routes qui y passent. Les bouts des ponts
//!   tiennent la hauteur du tablier. Les rues se lissent entre elles d'abord ; les voies
//!   piétonnes, chemins et sentiers ensuite, accrochés aux rues qu'ils rejoignent.
//! - La plateforme : la chaussée et, de chaque côté, la place du trottoir ou de
//!   l'accotement (`plateforme`), à plat à la hauteur de la chaussée. Où deux plateformes
//!   se recouvrent, la plus basse l'emporte.
//! - Les talus : au-delà, le sol rejoint le relief par une pente d'au plus `PENTE` (2
//!   pour 3), en déblai ou en remblai, sur `PORTEE` mètres au plus ; la route la plus
//!   basse passe la dernière. Dans l'eau, pas de talus : la berge reste un quai.
//! - Les voies piétonnes, chemins et sentiers cèdent aux rues : leurs plateformes ne
//!   passent qu'après celles des rues et des abords, et elles n'ont pas de travées.
//! - Les travées : où deux routes se croisent sans carrefour à plus de `SEUIL` d'écart,
//!   le sol est à la hauteur de la plus basse, et la plus haute la franchit sur un tablier
//!   (monté comme un pont), le temps de passer sa plateforme.
//! - Les abords : un bâtiment dont un coin est à moins de `Procedures::abords` du bord
//!   d'une rue (grande route, route, voie de service) a son abord, l'enveloppe de son
//!   contour et de ce bord ; le sol y est à la hauteur de la rue (celle de son point le
//!   plus proche), comme sur une plateforme, et un talus le rejoint au-delà. Le trottoir
//!   va donc à plat jusqu'à la façade.

use crate::reel::{Index, Route, SERVICE};
use std::collections::{HashMap, HashSet};

/// La pente des talus d'origine (`Procedures::pente`) : 2 de haut pour 3 de long.
pub const PENTE: f32 = 2.0 / 3.0;
/// Jusqu'où, au-delà de la plateforme, un talus peut aller d'origine (`Procedures::talus`, m).
pub const PORTEE: f32 = 20.0;
/// Sur ces derniers mètres de la portée, le talus s'efface dans le relief.
const FONDU: f32 = 5.0;
/// Un croisement sans carrefour plus haut que cela (m) a sa travée.
pub const SEUIL: f32 = 1.0;
/// Le lissage du profil en long d'origine (`Procedures::lissage`, m).
pub const LISSAGE: f32 = 20.0;
/// Les passes de lissage au plus.
const PASSES_MAX: usize = 400;
/// Une travée ne dépasse pas cette longueur (un croisement très biais n'en a pas).
const TRAVEE_MAX: f32 = 40.0;

/// La place à plat de chaque côté de la chaussée (m) : celle d'un trottoir ou d'un
/// accotement pour les rues et les routes, moins pour les chemins.
pub fn plateforme(genre: u8) -> f32 {
    plateforme_de(&crate::procedures::courantes(), genre)
}

fn plateforme_de(p: &crate::procedures::Procedures, genre: u8) -> f32 {
    if genre <= SERVICE {
        p.plateforme
    } else {
        0.5
    }
}

fn cle(p: [f32; 3]) -> (i64, i64) {
    ((p[0] * 100.0).round() as i64, (p[1] * 100.0).round() as i64)
}

/// Les longueurs cumulées le long de la route.
fn abscisses(r: &Route) -> Vec<f32> {
    let mut s = vec![0.0];
    for w in r.points.windows(2) {
        let l = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
        s.push(s.last().unwrap() + l);
    }
    s
}

/// Le profil en long des routes : lissé sur le réseau (`Procedures::lissage`), d'une
/// seule hauteur à chaque carrefour, et tenu aux bouts des ponts.
pub fn profiler(routes: &mut [Route], ponts: &[Route]) {
    profiler_avec(routes, ponts, crate::procedures::courantes().lissage);
}

/// `profiler`, sur `lissage` mètres.
pub fn profiler_avec(routes: &mut [Route], ponts: &[Route], lissage: f32) {
    // Les nœuds : un par position, la moyenne des hauteurs qui s'y rencontrent
    let mut ids: HashMap<(i64, i64), usize> = HashMap::new();
    let (mut h, mut compte, mut lourd): (Vec<f32>, Vec<f32>, Vec<bool>) = (Vec::new(), Vec::new(), Vec::new());
    let mut noeuds: Vec<Vec<usize>> = Vec::with_capacity(routes.len());
    for r in routes.iter() {
        let mut l = Vec::with_capacity(r.points.len());
        for p in &r.points {
            let k = *ids.entry(cle(*p)).or_insert_with(|| {
                h.push(0.0);
                compte.push(0.0);
                lourd.push(false);
                h.len() - 1
            });
            h[k] += p[2];
            compte[k] += 1.0;
            lourd[k] |= r.genre <= SERVICE;
            l.push(k);
        }
        noeuds.push(l);
    }
    for (y, c) in h.iter_mut().zip(&compte) {
        *y /= c;
    }
    // Les bouts des ponts tiennent
    let mut fixe = vec![false; h.len()];
    for p in ponts {
        for q in [p.points.first(), p.points.last()].into_iter().flatten() {
            if let Some(&k) = ids.get(&cle(*q)) {
                h[k] = q[2];
                fixe[k] = true;
            }
        }
    }
    // Les voisins le long des routes, pesés à l'inverse de la distance, et la longueur
    // moyenne d'un segment (le lissage se compte en passes de ce pas)
    let mut voisins: Vec<Vec<(usize, f32, bool)>> = vec![Vec::new(); h.len()];
    let (mut somme, mut n) = (0.0f64, 0usize);
    for (r, l) in routes.iter().zip(&noeuds) {
        for (w, k) in r.points.windows(2).zip(l.windows(2)) {
            let d = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
            if k[0] == k[1] || d < 1e-3 {
                continue;
            }
            let (p, rue) = (1.0 / d.max(0.5), r.genre <= SERVICE);
            voisins[k[0]].push((k[1], p, rue));
            voisins[k[1]].push((k[0], p, rue));
            somme += d as f64;
            n += 1;
        }
    }
    if n > 0 && lissage > 0.0 {
        // Chaque passe rapproche un nœud de la moyenne de ses voisins de moitié : après k
        // passes, l'écart type vaut à peu près pas × √(k / 2)
        let pas = (somme / n as f64) as f32;
        let passes = ((2.0 * (lissage / pas).powi(2)).ceil() as usize).min(PASSES_MAX);
        // Les rues entre elles, puis les voies légères accrochées aux rues
        for rues in [true, false] {
            // Les nœuds qui bougent, et leurs voisins de ce genre aux poids normés, à plat
            let (mut libres, mut debut, mut liens): (Vec<usize>, Vec<usize>, Vec<(u32, f32)>) = (Vec::new(), vec![0], Vec::new());
            for k in (0..h.len()).filter(|&k| !fixe[k] && lourd[k] == rues) {
                let p: f32 = voisins[k].iter().filter(|x| x.2 == rues).map(|x| x.1).sum();
                if p > 0.0 {
                    libres.push(k);
                    liens.extend(voisins[k].iter().filter(|x| x.2 == rues).map(|x| (x.0 as u32, 0.5 * x.1 / p)));
                    debut.push(liens.len());
                }
            }
            let mut neuf = vec![0.0f32; libres.len()];
            for _ in 0..passes {
                for (q, &k) in libres.iter().enumerate() {
                    neuf[q] = 0.5 * h[k] + liens[debut[q]..debut[q + 1]].iter().map(|&(v, w)| h[v as usize] * w).sum::<f32>();
                }
                for (q, &k) in libres.iter().enumerate() {
                    h[k] = neuf[q];
                }
            }
        }
    }
    for (r, l) in routes.iter_mut().zip(&noeuds) {
        for (p, &k) in r.points.iter_mut().zip(l) {
            p[2] = h[k];
        }
    }
}

/// Les segments rangés par case, avec la place de leurs talus.
pub fn indexer(routes: &[Route]) -> Index {
    let p = crate::procedures::courantes();
    crate::reel::indexer_avec(routes, |r| r.largeur / 2.0 + plateforme_de(&p, r.genre) + p.talus)
}

/// La hauteur du sol en (x, z), terrassé sous les routes proches : `relief` est celle du
/// relief seul, `eau` dit si l'on est dans l'eau.
pub fn sol(routes: &[Route], index: &Index, abords: &Abords, x: f64, z: f64, relief: f64, eau: bool) -> f64 {
    let (x, z) = (x as f32, z as f32);
    let pr = crate::procedures::courantes();
    let case = crate::reel::CASE;
    let Some(l) = index.get(&((x / case).floor() as i32, (z / case).floor() as i32)) else {
        return relief;
    };
    // Pour chaque route, son point le plus proche : de combien on est hors de sa
    // plateforme, et la hauteur de sa chaussée
    // (clé, écart à la plateforme, hauteur, route où l'on roule ou abord)
    let mut proches: Vec<(u32, f32, f32, bool)> = Vec::new();
    for &(k, i) in l {
        let r = &routes[k as usize];
        let (a, b) = (r.points[i as usize], r.points[i as usize + 1]);
        let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
        let l2 = dx * dx + dz * dz;
        let t = if l2 > 0.0 { (((x - a[0]) * dx + (z - a[1]) * dz) / l2).clamp(0.0, 1.0) } else { 0.0 };
        let d = (x - a[0] - dx * t).hypot(z - a[1] - dz * t);
        let e = d - r.largeur / 2.0 - plateforme_de(&pr, r.genre);
        let y = a[2] + (b[2] - a[2]) * t;
        let lourd = r.genre <= SERVICE;
        match proches.iter_mut().find(|p| p.0 == k) {
            Some(p) if e < p.1 => *p = (k, e, y, lourd),
            Some(_) => {}
            None => proches.push((k, e, y, lourd)),
        }
    }
    // Les abords des bâtiments, comme des plateformes
    for &k in abords.index.get(&((x / case).floor() as i32, (z / case).floor() as i32)).map_or(&[][..], |l| l.as_slice()) {
        let a = &abords.liste[k as usize];
        let b = a.boite;
        if x < b[0] - pr.talus || x > b[2] + pr.talus || z < b[1] - pr.talus || z > b[3] + pr.talus {
            continue;
        }
        let (e, y) = a.ecart(routes, x, z);
        proches.push((routes.len() as u32 + k, e, y, true));
    }
    // Sur une plateforme, à plat ; la plus basse l'emporte, mais celles des rues et des
    // abords passent avant celles des voies piétonnes, chemins et sentiers (un trottoir
    // noté à part le long d'une rue ne creuse pas la sienne)
    let plat = |lourd: bool| proches.iter().filter(|p| p.1 <= 0.0 && p.3 == lourd).map(|p| p.2).reduce(f32::min);
    if let Some(y) = plat(true).or_else(|| plat(false)) {
        return y as f64;
    }
    if eau {
        return relief;
    }
    // Les talus, de la route la plus haute à la plus basse
    proches.sort_by(|a, b| b.2.total_cmp(&a.2));
    let mut h = relief as f32;
    for &(_, e, y, _) in &proches {
        if e >= pr.talus {
            continue;
        }
        let c = h.clamp(y - pr.pente * e, y + pr.pente * e);
        let f = ((pr.talus - e) / FONDU).clamp(0.0, 1.0);
        h += (c - h) * f;
    }
    h as f64
}

/// L'abord d'un bâtiment : l'enveloppe de son contour et du bord de la rue la plus
/// proche, et les segments de cette rue qui le bordent.
#[derive(Clone, Debug, PartialEq)]
pub struct Abord {
    route: u32,
    segments: Vec<u32>,
    contour: Vec<[f32; 2]>,
    /// x et z bas, x et z hauts.
    boite: [f32; 4],
}

/// Les abords des bâtiments, et pour chaque case ceux dont le talus l'approche.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Abords {
    pub liste: Vec<Abord>,
    index: HashMap<(i32, i32), Vec<u32>>,
}

impl Abords {
    /// Un abord ou son talus approche-t-il cette case ?
    pub fn pres(&self, x: f32, z: f32) -> bool {
        self.index.contains_key(&((x / CASE).floor() as i32, (z / CASE).floor() as i32))
    }
}

use crate::reel::CASE;

/// Le point de `segments` de la route `r` le plus proche de (x, z) : la distance à
/// l'axe, le point de l'axe et sa hauteur.
fn sur_route(r: &Route, segments: &[u32], x: f32, z: f32) -> Option<(f32, [f32; 2], f32)> {
    let mut m: Option<(f32, [f32; 2], f32)> = None;
    for &i in segments {
        let (a, b) = (r.points[i as usize], r.points[i as usize + 1]);
        let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
        let l2 = dx * dx + dz * dz;
        let t = if l2 > 0.0 { (((x - a[0]) * dx + (z - a[1]) * dz) / l2).clamp(0.0, 1.0) } else { 0.0 };
        let p = [a[0] + dx * t, a[1] + dz * t];
        let d = (x - p[0]).hypot(z - p[1]);
        if m.is_none_or(|m| d < m.0) {
            m = Some((d, p, a[2] + (b[2] - a[2]) * t));
        }
    }
    m
}

impl Abord {
    /// De combien (x, z) est hors de l'abord (0 dedans), et la hauteur de la rue à son
    /// point le plus proche.
    fn ecart(&self, routes: &[Route], x: f32, z: f32) -> (f32, f32) {
        let y = sur_route(&routes[self.route as usize], &self.segments, x, z).map_or(0.0, |m| m.2);
        let c = &self.contour;
        let mut dedans = false;
        let mut d = f32::MAX;
        for k in 0..c.len() {
            let (a, b) = (c[k], c[(k + 1) % c.len()]);
            if (a[1] > z) != (b[1] > z) && x < a[0] + (z - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
                dedans = !dedans;
            }
            let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
            let l2 = dx * dx + dz * dz;
            let t = if l2 > 0.0 { (((x - a[0]) * dx + (z - a[1]) * dz) / l2).clamp(0.0, 1.0) } else { 0.0 };
            d = d.min((x - a[0] - dx * t).hypot(z - a[1] - dz * t));
        }
        (if dedans { 0.0 } else { d }, y)
    }
}

/// L'enveloppe convexe des points (Andrew), dans le sens trigonométrique.
fn enveloppe(mut p: Vec<[f32; 2]>) -> Vec<[f32; 2]> {
    p.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    p.dedup();
    if p.len() < 3 {
        return p;
    }
    let tour = |o: [f32; 2], a: [f32; 2], b: [f32; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut h: Vec<[f32; 2]> = Vec::new();
    for passe in 0..2 {
        let debut = h.len();
        let it: Box<dyn Iterator<Item = &[f32; 2]>> = if passe == 0 { Box::new(p.iter()) } else { Box::new(p.iter().rev()) };
        for &q in it {
            while h.len() >= debut + 2 && tour(h[h.len() - 2], h[h.len() - 1], q) <= 0.0 {
                h.pop();
            }
            h.push(q);
        }
        h.pop();
    }
    h
}

/// Les abords des bâtiments (leurs contours) dont un coin est à moins de `distance` du
/// bord d'une rue ; leurs talus vont jusqu'à `talus` au-delà.
pub fn abords(routes: &[Route], batiments: &[&[[f32; 2]]], distance: f32, talus: f32) -> Abords {
    let mut a = Abords::default();
    if distance <= 0.0 {
        return a;
    }
    let index = crate::reel::indexer_avec(routes, |r| r.largeur / 2.0 + distance);
    for c in batiments {
        if c.len() < 3 {
            continue;
        }
        // La rue la plus proche d'un coin, et de combien son bord en est loin
        let mut mieux: Option<(u32, f32)> = None;
        for p in c.iter() {
            for &(k, i) in index.get(&((p[0] / CASE).floor() as i32, (p[1] / CASE).floor() as i32)).map_or(&[][..], |l| l.as_slice()) {
                let r = &routes[k as usize];
                if r.genre > SERVICE {
                    continue;
                }
                if let Some((d, _, _)) = sur_route(r, &[i], p[0], p[1]) {
                    let hors = d - r.largeur / 2.0;
                    if mieux.is_none_or(|m| hors < m.1) {
                        mieux = Some((k, hors));
                    }
                }
            }
        }
        // Trop loin, ou sur la chaussée même (une donnée douteuse) : pas d'abord
        let Some((k, _)) = mieux.filter(|m| m.1 <= distance && m.1 >= 0.0) else { continue };
        let r = &routes[k as usize];
        let mut segments: Vec<u32> = c
            .iter()
            .flat_map(|p| index.get(&((p[0] / CASE).floor() as i32, (p[1] / CASE).floor() as i32)).map_or(&[][..], |l| l.as_slice()))
            .filter(|s| s.0 == k)
            .map(|s| s.1)
            .collect();
        segments.sort_unstable();
        segments.dedup();
        // Le contour, et le pied de chaque coin au bord de la rue
        let mut pts: Vec<[f32; 2]> = c.to_vec();
        for p in c.iter() {
            if let Some((d, q, _)) = sur_route(r, &segments, p[0], p[1]) {
                if d > r.largeur / 2.0 {
                    let w = r.largeur / 2.0 / d;
                    pts.push([q[0] + (p[0] - q[0]) * w, q[1] + (p[1] - q[1]) * w]);
                }
            }
        }
        let contour = enveloppe(pts);
        if contour.len() < 3 {
            continue;
        }
        let boite = contour.iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, p| [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])]);
        let n = a.liste.len() as u32;
        for cz in ((boite[1] - talus) / CASE).floor() as i32..=((boite[3] + talus) / CASE).floor() as i32 {
            for cx in ((boite[0] - talus) / CASE).floor() as i32..=((boite[2] + talus) / CASE).floor() as i32 {
                a.index.entry((cx, cz)).or_default().push(n);
            }
        }
        a.liste.push(Abord { route: k, segments, contour, boite });
    }
    a
}

/// Le point de la route `r` à l'abscisse `s` (bornée à la route), et sa hauteur.
fn point_a(r: &Route, abs: &[f32], s: f32) -> [f32; 3] {
    let s = s.clamp(0.0, *abs.last().unwrap());
    let i = abs.partition_point(|&v| v <= s).clamp(1, r.points.len() - 1);
    let (a, b) = (r.points[i - 1], r.points[i]);
    let l = abs[i] - abs[i - 1];
    let t = if l > 0.0 { (s - abs[i - 1]) / l } else { 0.0 };
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Les travées : où deux routes se croisent sans carrefour, à plus de `SEUIL` d'écart, la
/// plus haute passe sur un tablier le long de la plateforme de l'autre.
pub fn travees(routes: &[Route], index: &Index) -> Vec<Route> {
    // Les croisements : chaque paire de segments de routes différentes qui partagent une
    // case, s'ils se coupent hors de leurs bouts (un bout commun est un carrefour)
    let mut vus: HashSet<(u32, u32, u32, u32)> = HashSet::new();
    let mut cases: Vec<_> = index.iter().collect();
    cases.sort_by_key(|(c, _)| **c);
    let mut abs: HashMap<u32, Vec<f32>> = HashMap::new();
    let mut sortie = Vec::new();
    for (_, l) in cases {
        for (n, &(k1, i1)) in l.iter().enumerate() {
            for &(k2, i2) in &l[n + 1..] {
                if k1 == k2 {
                    continue;
                }
                let (r1, r2) = (&routes[k1 as usize], &routes[k2 as usize]);
                // Entre voies où l'on roule seulement : un chemin piéton dont le profil
                // s'écarte au croisement ne creuse pas la rue
                if r1.genre > SERVICE || r2.genre > SERVICE {
                    continue;
                }
                let (a, b) = (r1.points[i1 as usize], r1.points[i1 as usize + 1]);
                let (c, d) = (r2.points[i2 as usize], r2.points[i2 as usize + 1]);
                let (u, v) = ([b[0] - a[0], b[1] - a[1]], [d[0] - c[0], d[1] - c[1]]);
                let den = u[0] * v[1] - u[1] * v[0];
                if den.abs() < 1e-9 {
                    continue;
                }
                let w = [c[0] - a[0], c[1] - a[1]];
                let (t, s) = ((w[0] * v[1] - w[1] * v[0]) / den, (w[0] * u[1] - w[1] * u[0]) / den);
                if !(1e-4..=1.0 - 1e-4).contains(&t) || !(1e-4..=1.0 - 1e-4).contains(&s) {
                    continue;
                }
                if !vus.insert(if k1 < k2 { (k1, i1, k2, i2) } else { (k2, i2, k1, i1) }) {
                    continue;
                }
                let (y1, y2) = (a[2] + (b[2] - a[2]) * t, c[2] + (d[2] - c[2]) * s);
                if (y1 - y2).abs() <= SEUIL {
                    continue;
                }
                // La plus haute (h), la plus basse (b)
                let (kh, ih, th, rh, rb, uh, ub) = if y1 > y2 { (k1, i1, t, r1, r2, u, v) } else { (k2, i2, s, r2, r1, v, u) };
                let (lh, lb) = (uh[0].hypot(uh[1]), ub[0].hypot(ub[1]));
                let sin = (uh[0] * ub[1] - uh[1] * ub[0]).abs() / (lh * lb);
                let cos = (uh[0] * ub[0] + uh[1] * ub[1]).abs() / (lh * lb);
                if sin < 0.25 {
                    continue;
                }
                let demi = (rb.largeur / 2.0 + plateforme(rb.genre)) / sin + rh.largeur / 2.0 * cos / sin + 1.0;
                if 2.0 * demi > TRAVEE_MAX {
                    continue;
                }
                let s_h = abs.entry(kh).or_insert_with(|| abscisses(rh));
                let centre = s_h[ih as usize] + th * lh;
                let n = ((2.0 * demi / 4.0).ceil() as usize).max(1);
                let points = (0..=n).map(|j| point_a(rh, s_h, centre - demi + 2.0 * demi * j as f32 / n as f32)).collect();
                sortie.push(Route { genre: rh.genre, largeur: rh.largeur, points });
            }
        }
    }
    sortie
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reel::ROUTE;

    #[test]
    fn les_carrefours_s_accordent_et_les_ponts_tiennent() {
        // Deux routes qui se rejoignent en (0, 0) à des hauteurs différentes, et une qui
        // finit au bout d'un pont
        let mut routes = vec![
            Route { genre: ROUTE, largeur: 6.0, points: (0..=10).map(|i| [-(i as f32) * 8.0, 0.0, 4.0]).collect() },
            Route { genre: ROUTE, largeur: 6.0, points: (0..=10).map(|i| [0.0, i as f32 * 8.0, 6.0]).collect() },
            Route { genre: ROUTE, largeur: 6.0, points: (0..=10).map(|i| [100.0 + i as f32 * 8.0, 0.0, 2.0]).collect() },
        ];
        let pont = Route { genre: ROUTE, largeur: 6.0, points: vec![[90.0, 0.0, 3.0], [100.0, 0.0, 3.0]] };
        profiler_avec(&mut routes, &[pont], LISSAGE);
        assert!((routes[0].points[0][2] - 5.0).abs() < 1e-4 && (routes[1].points[0][2] - 5.0).abs() < 1e-4);
        // L'écart s'efface vers le bout libre
        assert!((routes[0].points[10][2] - 4.0).abs() < 0.05);
        assert!((routes[0].points[1][2] - 5.0).abs() < 0.6);
        assert!((routes[2].points[0][2] - 3.0).abs() < 1e-4 && (routes[2].points[10][2] - 2.0).abs() < 0.05);
    }

    #[test]
    fn le_profil_se_lisse_d_une_voie_a_l_autre() {
        // Une rue en deux voies bout à bout, au profil heurté (±1 m d'un point à l'autre),
        // et une voie piétonne bien plus haute qui la rejoint au milieu
        let y = |i: i32| 10.0 + if i % 2 == 0 { 1.0 } else { -1.0 };
        let mut routes = vec![
            Route { genre: ROUTE, largeur: 6.0, points: (0..=20).map(|i| [i as f32 * 7.0, 0.0, y(i)]).collect() },
            Route { genre: ROUTE, largeur: 6.0, points: (20..=40).map(|i| [i as f32 * 7.0, 0.0, y(i)]).collect() },
            Route { genre: crate::reel::PIETONNE, largeur: 2.0, points: (0..=10).map(|i| [140.0, i as f32 * 7.0, if i == 0 { y(20) } else { 30.0 }]).collect() },
        ];
        profiler_avec(&mut routes, &[], LISSAGE);
        // Plus de heurt sur la rue, au carrefour des deux voies non plus ; la voie
        // piétonne ne la tire pas vers le haut
        let rue: Vec<f32> = routes[0].points.iter().chain(&routes[1].points[1..]).map(|p| p[2]).collect();
        assert!(rue.windows(2).all(|w| (w[1] - w[0]).abs() < 0.2), "{rue:?}");
        assert!(rue.iter().all(|&h| (h - 10.0).abs() < 0.5));
        assert_eq!(routes[0].points[20][2], routes[1].points[0][2]);
        assert_eq!(routes[2].points[0][2], routes[0].points[20][2]);
    }

    #[test]
    fn un_croisement_sans_carrefour_a_sa_travee() {
        // Une route à 0 m d'est en ouest, une autre à 6 m du nord au sud, sans point commun
        let routes =
            vec![Route { genre: ROUTE, largeur: 6.0, points: vec![[-50.0, 1.0, 0.0], [50.0, 1.0, 0.0]] }, Route { genre: ROUTE, largeur: 4.0, points: vec![[2.0, -50.0, 6.0], [2.0, 50.0, 6.0]] }];
        let index = indexer(&routes);
        // Le sol est à la plus basse au croisement, à la plus haute sur sa plateforme
        // hors de celle de l'autre, et un talus les relie
        assert_eq!(sol(&routes, &index, &Abords::default(), 2.0, 1.0, 3.0, false), 0.0);
        assert_eq!(sol(&routes, &index, &Abords::default(), 2.0, 10.0, 3.0, false), 6.0);
        assert_eq!(sol(&routes, &index, &Abords::default(), 30.0, 9.0, 3.0, false), (PENTE * 3.0) as f64);
        let t = travees(&routes, &crate::reel::indexer(&routes));
        assert_eq!(abords(&routes, &[], 12.0, 20.0), Abords::default());
        assert_eq!(t.len(), 1);
        let p = &t[0].points;
        // Le long de la route haute, à sa hauteur, de part et d'autre de la plateforme
        assert!(p.iter().all(|q| q[0] == 2.0 && q[2] == 6.0));
        assert!(p[0][1] < 1.0 - 5.0 && p[p.len() - 1][1] > 1.0 + 5.0);
    }

    #[test]
    fn le_sol_va_a_plat_de_la_rue_au_batiment() {
        // Une rue à 5 m d'est en ouest (bords à z ±3), une maison de z 10 à 20 au nord,
        // sur un relief à 12 m
        let routes = vec![Route { genre: ROUTE, largeur: 6.0, points: vec![[-60.0, 0.0, 5.0], [60.0, 0.0, 5.0]] }];
        let index = indexer(&routes);
        let maison: &[[f32; 2]] = &[[-5.0, 10.0], [5.0, 10.0], [5.0, 20.0], [-5.0, 20.0]];
        let a = abords(&routes, &[maison], 12.0, PORTEE);
        assert_eq!(a.liste.len(), 1);
        // Sous la maison et entre elle et la rue : à la hauteur de la rue
        assert_eq!(sol(&routes, &index, &a, 0.0, 15.0, 12.0, false), 5.0);
        assert_eq!(sol(&routes, &index, &a, 0.0, 8.0, 12.0, false), 5.0);
        // Derrière elle, un talus ; sans abord, le talus de la rue seule
        assert!((sol(&routes, &index, &a, 0.0, 23.0, 12.0, false) - (5.0 + 3.0 * PENTE as f64)).abs() < 1e-4);
        assert!((sol(&routes, &index, &Abords::default(), 0.0, 8.0, 12.0, false) - (5.0 + 3.0 * PENTE as f64)).abs() < 1e-4);
        // Trop loin de la rue, ou à distance nulle : pas d'abord
        assert!(abords(&routes, &[maison], 6.0, PORTEE).liste.is_empty());
        assert!(abords(&routes, &[maison], 0.0, PORTEE).liste.is_empty());
    }
}
