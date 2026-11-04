//! La physique : des corps en capsule qui marchent, montent les marches, glissent sur les
//! pentes trop raides, tombent, et butent contre les obstacles ; et les rayons, pour
//! viser, désigner, regarder.
//!
//! - Un **corps** est une capsule verticale : `Mobile::rayon` et `Corps::hauteur`, posée
//!   par ses pieds sur `Lieu::pos`. Le corps n'est pas un objet physique libre : il se
//!   déplace comme on le commande, et la physique corrige.
//! - Un **obstacle** est une `Forme`, dans le repère du lieu de son entité (sans
//!   rotation) : une boîte, une capsule verticale, ou des triangles rangés dans une
//!   grille. Le relief des tuiles est le sol.
//! - **Un pas.** Le corps avance d'abord à l'horizontale, relevé de la hauteur d'une
//!   marche s'il est au sol : ce qui dépasse la marche l'arrête, et il glisse le long. Puis
//!   il cherche le sol sous lui : la surface la plus haute qui ne dépasse pas la marche.
//!   Au sol, il s'y pose, même un peu plus bas (il suit la descente) ; plus bas encore, il
//!   tombe.
//! - **Les pentes.** Une surface de plus de `PENTE_MAX` ne se gravit pas : la part du pas
//!   qui monte est retirée, et le corps y glisse vers le bas.

use super::entites::{Aabb, Entite, Monde};
use super::horloge::PAS_FIXE;
use super::tuiles::TAILLE;
use std::sync::Arc;

/// La plus haute marche qu'un corps franchit sans sauter.
pub const MARCHE: f32 = 0.35;
/// La pente la plus raide qui se gravit, en degrés.
pub const PENTE_MAX: f32 = 45.0;
pub const GRAVITE: f32 = 9.81;
/// Au sol, le corps suit une descente jusqu'à une marche plus cet écart par pas.
const ACCROCHE: f32 = 0.1;
/// Côté des cases de la grille des triangles.
const CASE: f32 = 2.0;

type V3 = [f32; 3];

fn moins(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn plus(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn fois(a: V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn scal(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn vect(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn unitaire(a: V3) -> V3 {
    let l = scal(a, a).sqrt();
    if l > 1e-12 { fois(a, 1.0 / l) } else { [0.0, 1.0, 0.0] }
}

/// Des triangles, et une grille au sol de cases de `CASE` mètres qui dit lesquels passent
/// dans chacune.
#[derive(Debug)]
pub struct Triangles {
    pub t: Vec<[V3; 3]>,
    min: [f32; 2],
    n: [usize; 2],
    cases: Vec<Vec<u32>>,
}

impl Triangles {
    pub fn new(t: Vec<[V3; 3]>) -> Triangles {
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for p in t.iter().flatten() {
            for (k, c) in [0, 2].into_iter().enumerate() {
                lo[k] = lo[k].min(p[c]);
                hi[k] = hi[k].max(p[c]);
            }
        }
        if t.is_empty() {
            (lo, hi) = ([0.0; 2], [0.0; 2]);
        }
        let n = [0, 1].map(|k| (((hi[k] - lo[k]) / CASE).floor() as usize + 1).min(1024));
        let mut cases = vec![Vec::new(); n[0] * n[1]];
        for (i, tri) in t.iter().enumerate() {
            let (a, b) = Self::boite(tri);
            let (c0, c1) = (Self::case_de(lo, n, a), Self::case_de(lo, n, b));
            for z in c0[1]..=c1[1] {
                for x in c0[0]..=c1[0] {
                    cases[z * n[0] + x].push(i as u32);
                }
            }
        }
        Triangles { t, min: lo, n, cases }
    }

    fn boite(tri: &[V3; 3]) -> ([f32; 2], [f32; 2]) {
        let xs = tri.map(|p| p[0]);
        let zs = tri.map(|p| p[2]);
        ([xs.iter().copied().fold(f32::MAX, f32::min), zs.iter().copied().fold(f32::MAX, f32::min)], [xs.iter().copied().fold(f32::MIN, f32::max), zs.iter().copied().fold(f32::MIN, f32::max)])
    }

    fn case_de(lo: [f32; 2], n: [usize; 2], p: [f32; 2]) -> [usize; 2] {
        [0, 1].map(|k| (((p[k] - lo[k]) / CASE).floor().max(0.0) as usize).min(n[k] - 1))
    }

    /// Les triangles dont la case touche le rectangle `a`–`b` (x, z), chacun une fois.
    pub fn pres(&self, a: [f32; 2], b: [f32; 2]) -> Vec<usize> {
        if self.t.is_empty() {
            return Vec::new();
        }
        let hi = [0, 1].map(|k| self.min[k] + self.n[k] as f32 * CASE);
        if b[0] < self.min[0] || b[1] < self.min[1] || a[0] > hi[0] || a[1] > hi[1] {
            return Vec::new();
        }
        let (c0, c1) = (Self::case_de(self.min, self.n, a), Self::case_de(self.min, self.n, b));
        let mut l: Vec<usize> = Vec::new();
        for z in c0[1]..=c1[1] {
            for x in c0[0]..=c1[0] {
                l.extend(self.cases[z * self.n[0] + x].iter().map(|i| *i as usize));
            }
        }
        l.sort_unstable();
        l.dedup();
        l
    }
}

/// Une forme d'obstacle, dans le repère du lieu de son entité.
#[derive(Clone, Debug)]
pub enum Forme {
    Boite(Aabb),
    /// Capsule verticale : posée en `base`, de `hauteur` en tout.
    Capsule { base: V3, hauteur: f32, rayon: f32 },
    Triangles(Arc<Triangles>),
}

impl Forme {
    /// Déplacée de `o` : la forme au repère de l'origine du monde.
    fn placee(&self, o: V3) -> Forme {
        match self {
            Forme::Boite(b) => Forme::Boite(Aabb { centre: plus(b.centre, o), demi: b.demi }),
            Forme::Capsule { base, hauteur, rayon } => Forme::Capsule { base: plus(*base, o), hauteur: *hauteur, rayon: *rayon },
            Forme::Triangles(t) => Forme::Triangles(t.clone()),
        }
    }
}

/// Ce qu'un corps porte en plus d'un mobile : sa hauteur, sa vitesse de chute, s'il est
/// au sol, la normale de ce sol et la vitesse de sa glissade.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Corps {
    pub hauteur: f32,
    pub vy: f32,
    pub au_sol: bool,
    pub normale: V3,
    pub glisse: [f32; 2],
}

impl Corps {
    pub fn new(hauteur: f32) -> Corps {
        Corps { hauteur, vy: 0.0, au_sol: true, normale: [0.0, 1.0, 0.0], glisse: [0.0; 2] }
    }
}

/// Un obstacle près du corps : sa forme, au repère du monde, et son entité. Pour des
/// triangles, `o` est le lieu de l'entité, où leurs coordonnées se rapportent.
struct Proche {
    forme: Forme,
    o: V3,
    entite: Entite,
}

/// Les obstacles dont la boîte englobante approche le rectangle (x, z) `a`–`b`.
fn proches(monde: &Monde, a: [f32; 2], b: [f32; 2]) -> Vec<Proche> {
    let mut l = Vec::new();
    for (e, formes) in monde.obstacles.iter() {
        let o = monde.lieu.get(e).map_or([0.0; 3], |l| l.pos);
        for f in formes {
            let (lo, hi) = match f {
                Forme::Boite(x) => ([x.centre[0] - x.demi[0], x.centre[2] - x.demi[2]], [x.centre[0] + x.demi[0], x.centre[2] + x.demi[2]]),
                Forme::Capsule { base, rayon, .. } => ([base[0] - rayon, base[2] - rayon], [base[0] + rayon, base[2] + rayon]),
                Forme::Triangles(t) => (t.min, [t.min[0] + t.n[0] as f32 * CASE, t.min[1] + t.n[1] as f32 * CASE]),
            };
            let (lo, hi) = ([lo[0] + o[0], lo[1] + o[2]], [hi[0] + o[0], hi[1] + o[2]]);
            if hi[0] >= a[0] && hi[1] >= a[1] && lo[0] <= b[0] && lo[1] <= b[1] {
                l.push(Proche { forme: f.placee(o), o, entite: e });
            }
        }
    }
    l
}

/// Le point du triangle le plus proche de `p` (Ericson, *Real-Time Collision Detection*).
fn plus_proche(p: V3, [a, b, c]: [V3; 3]) -> V3 {
    let (ab, ac, ap) = (moins(b, a), moins(c, a), moins(p, a));
    let (d1, d2) = (scal(ab, ap), scal(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = moins(p, b);
    let (d3, d4) = (scal(ab, bp), scal(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return plus(a, fois(ab, d1 / (d1 - d3)));
    }
    let cp = moins(p, c);
    let (d5, d6) = (scal(ab, cp), scal(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return plus(a, fois(ac, d2 / (d2 - d6)));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return plus(b, fois(moins(c, b), (d4 - d3) / ((d4 - d3) + (d5 - d6))));
    }
    let d = 1.0 / (va + vb + vc);
    plus(a, plus(fois(ab, vb * d), fois(ac, vc * d)))
}

fn normale_tri([a, b, c]: [V3; 3]) -> V3 {
    let n = unitaire(vect(moins(b, a), moins(c, a)));
    if n[1] < 0.0 { fois(n, -1.0) } else { n }
}

/// Le cosinus de `PENTE_MAX` : une normale moins verticale est trop raide.
fn raide(n: V3) -> bool {
    n[1] < PENTE_MAX.to_radians().cos()
}

/// Le sol sous (x, z) : la surface la plus haute à `plafond` au plus, sa normale et ce qui
/// la porte (`None` : le relief).
pub fn sol(monde: &Monde, x: f32, z: f32, plafond: f32) -> Option<(f32, V3, Option<Entite>)> {
    let mut mieux: Option<(f32, V3, Option<Entite>)> = None;
    let mut prendre = |h: f32, n: V3, e: Option<Entite>| {
        if h <= plafond + 1e-4 && mieux.is_none_or(|m| h > m.0) {
            mieux = Some((h, n, e));
        }
    };
    for (e, r) in monde.relief.iter() {
        let Some(o) = monde.lieu.get(e).map(|l| l.pos) else { continue };
        let (u, v) = (x - o[0], z - o[2]);
        if (0.0..=TAILLE).contains(&u) && (0.0..=TAILLE).contains(&v) {
            prendre(o[1] + r.hauteur(u, v), r.normale(u, v), None);
        }
    }
    for p in proches(monde, [x, z], [x, z]) {
        match &p.forme {
            Forme::Boite(b) => {
                if (x - b.centre[0]).abs() <= b.demi[0] && (z - b.centre[2]).abs() <= b.demi[2] {
                    prendre(b.centre[1] + b.demi[1], [0.0, 1.0, 0.0], Some(p.entite));
                }
            }
            Forme::Capsule { base, hauteur, rayon } => {
                let d = (x - base[0]).hypot(z - base[2]);
                if d < *rayon {
                    let haut = base[1] + hauteur - rayon;
                    let y = haut + (rayon * rayon - d * d).sqrt();
                    prendre(y, unitaire([x - base[0], y - haut, z - base[2]]), Some(p.entite));
                }
            }
            Forme::Triangles(t) => {
                let (lx, lz) = (x - p.o[0], z - p.o[2]);
                for i in t.pres([lx, lz], [lx, lz]) {
                    let tri = t.t[i].map(|q| plus(q, p.o));
                    if let Some(y) = hauteur_tri(tri, x, z) {
                        prendre(y, normale_tri(tri), Some(p.entite));
                    }
                }
            }
        }
    }
    mieux
}

/// La hauteur du triangle à la verticale de (x, z), s'il y passe.
fn hauteur_tri([a, b, c]: [V3; 3], x: f32, z: f32) -> Option<f32> {
    let d = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
    if d.abs() < 1e-9 {
        return None;
    }
    let l1 = ((b[2] - c[2]) * (x - c[0]) + (c[0] - b[0]) * (z - c[2])) / d;
    let l2 = ((c[2] - a[2]) * (x - c[0]) + (a[0] - c[0]) * (z - c[2])) / d;
    let l3 = 1.0 - l1 - l2;
    let e = -1e-5;
    (l1 >= e && l2 >= e && l3 >= e).then(|| l1 * a[1] + l2 * b[1] + l3 * c[1])
}

/// Repousse le corps (pieds en `p`, rayon `r`, du dessus de `bas` à `haut`) hors des murs,
/// à l'horizontale. Ce qui est sous `bas` est une marche, et ne compte pas.
fn repousser(p: &mut V3, r: f32, bas: f32, haut: f32, obstacles: &[Proche]) {
    for _ in 0..3 {
        let mut bouge = false;
        for o in obstacles {
            match &o.forme {
                Forme::Boite(b) => {
                    if b.centre[1] + b.demi[1] <= bas || b.centre[1] - b.demi[1] >= haut {
                        continue;
                    }
                    let (dx, dz) = (p[0] - b.centre[0], p[2] - b.centre[2]);
                    let (px, pz) = (dx.clamp(-b.demi[0], b.demi[0]), dz.clamp(-b.demi[2], b.demi[2]));
                    let (ex, ez) = (dx - px, dz - pz);
                    let d = ex.hypot(ez);
                    if d >= r {
                        continue;
                    }
                    bouge = true;
                    if d > 1e-6 {
                        p[0] += ex / d * (r - d);
                        p[2] += ez / d * (r - d);
                    } else {
                        // Au-dedans : sortie par le côté le plus proche
                        let (ox, oz) = (b.demi[0] - dx.abs(), b.demi[2] - dz.abs());
                        if ox < oz {
                            p[0] = b.centre[0] + dx.signum() * (b.demi[0] + r);
                        } else {
                            p[2] = b.centre[2] + dz.signum() * (b.demi[2] + r);
                        }
                    }
                }
                Forme::Capsule { base, hauteur, rayon } => {
                    if base[1] + hauteur <= bas || base[1] >= haut {
                        continue;
                    }
                    let (dx, dz) = (p[0] - base[0], p[2] - base[2]);
                    let (d, min) = (dx.hypot(dz), r + rayon);
                    if d >= min {
                        continue;
                    }
                    bouge = true;
                    let (ux, uz) = if d > 1e-6 { (dx / d, dz / d) } else { (0.0, 1.0) };
                    p[0] = base[0] + ux * min;
                    p[2] = base[2] + uz * min;
                }
                Forme::Triangles(t) => {
                    let (lx, lz) = (p[0] - o.o[0], p[2] - o.o[2]);
                    for i in t.pres([lx - r, lz - r], [lx + r, lz + r]) {
                        let tri = t.t[i].map(|q| plus(q, o.o));
                        let n = normale_tri(tri);
                        // Les surfaces praticables sont du sol
                        if !raide(n) {
                            continue;
                        }
                        // Des sphères le long du corps, au-dessus de la marche
                        let mut y = bas + r;
                        while y <= haut - r + 1e-4 {
                            let c = [p[0], y, p[2]];
                            let q = plus_proche(c, tri);
                            let (ex, ez) = (c[0] - q[0], c[2] - q[2]);
                            let d = scal(moins(c, q), moins(c, q)).sqrt();
                            if d < r && q[1] > bas {
                                let h = ex.hypot(ez);
                                let (ux, uz) = if h > 1e-6 { (ex / h, ez / h) } else { let l = n[0].hypot(n[2]).max(1e-6); (n[0] / l, n[2] / l) };
                                // Assez pour que la sphère ne touche plus, à l'horizontale
                                let dy = c[1] - q[1];
                                let voulu = (r * r - dy * dy).max(0.0).sqrt();
                                let pousse = voulu - h.min(voulu);
                                if pousse > 1e-6 {
                                    p[0] += ux * pousse;
                                    p[2] += uz * pousse;
                                    bouge = true;
                                }
                            }
                            y += r;
                        }
                    }
                }
            }
        }
        if !bouge {
            break;
        }
    }
}

/// Un pas de physique pour tous les corps : ils avancent de ce que leur mobile demande, à
/// l'horizontale, puis trouvent le sol ou tombent.
pub fn deplacer(monde: &mut Monde) {
    let dt = PAS_FIXE;
    let corps: Vec<Entite> = monde.corps.iter().map(|(e, _)| e).collect();
    for e in corps {
        let (Some(m), Some(c), Some(l)) = (monde.mobile.get(e).copied(), monde.corps.get(e).copied(), monde.lieu.get(e).copied()) else { continue };
        let (p, c) = pas(monde, l.pos, [m.elan[0] * m.vitesse * dt, m.elan[1] * m.vitesse * dt], m.rayon, c, dt);
        monde.lieu.get_mut(e).unwrap().pos = p;
        *monde.corps.get_mut(e).unwrap() = c;
    }
}

/// Un pas d'un corps aux pieds en `p` qui veut avancer de `d` (x, z).
pub fn pas(monde: &Monde, p: V3, d: [f32; 2], r: f32, mut c: Corps, dt: f32) -> (V3, Corps) {
    let mut d = [d[0] + c.glisse[0] * dt, d[1] + c.glisse[1] * dt];
    // Une pente trop raide ne se gravit pas : la part du pas qui monte est retirée
    if c.au_sol && raide(c.normale) {
        let (nx, nz) = (c.normale[0], c.normale[2]);
        let l = nx.hypot(nz).max(1e-6);
        let (ux, uz) = (nx / l, nz / l);
        let monte = d[0] * ux + d[1] * uz;
        if monte < 0.0 {
            d = [d[0] - monte * ux, d[1] - monte * uz];
        }
    }
    let marche = if c.au_sol { MARCHE } else { 0.0 };
    let mut q = [p[0] + d[0], p[1], p[2] + d[1]];
    let portee = r + d[0].hypot(d[1]) + 0.5;
    let obstacles = proches(monde, [q[0] - portee, q[2] - portee], [q[0] + portee, q[2] + portee]);
    let (bas, haut) = (q[1] + marche, q[1] + c.hauteur);
    repousser(&mut q, r, bas, haut, &obstacles);

    // Le sol sous le corps, une marche au-dessus des pieds au plus
    let s = sol(monde, q[0], q[2], q[1] + marche.max(0.05));
    // Une marche trop raide pour s'y poser : on n'y monte pas
    if let Some((h, n, _)) = s {
        if raide(n) && h > p[1] + 0.02 && c.au_sol {
            q = [p[0], p[1], p[2]];
            repousser(&mut q, r, bas, haut, &obstacles);
        }
    }
    let s = sol(monde, q[0], q[2], q[1] + marche.max(0.05));
    if c.au_sol {
        match s {
            Some((h, n, _)) if h >= q[1] - MARCHE - ACCROCHE => {
                q[1] = h;
                (c.vy, c.normale) = (0.0, n);
            }
            _ => c.au_sol = false,
        }
    }
    if !c.au_sol {
        c.vy -= GRAVITE * dt;
        let y = q[1] + c.vy * dt;
        match s {
            Some((h, n, _)) if h >= y => {
                q[1] = h;
                (c.vy, c.au_sol, c.normale) = (0.0, true, n);
            }
            _ => q[1] = y,
        }
    }
    // La glissade : sur une pente trop raide, la pesanteur l'emporte ; ailleurs, elle
    // s'éteint vite
    if c.au_sol && raide(c.normale) {
        let n = c.normale;
        c.glisse = [c.glisse[0] + n[0] * n[1] * GRAVITE * dt, c.glisse[1] + n[2] * n[1] * GRAVITE * dt];
    } else {
        let k = (-dt * 10.0).exp();
        c.glisse = [c.glisse[0] * k, c.glisse[1] * k];
    }
    (q, c)
}

/// Pose chaque corps sur ce qu'il y a de plus haut à sa verticale : au départ, ou quand
/// le monde a changé sous lui.
pub fn poser(monde: &mut Monde) {
    let corps: Vec<Entite> = monde.corps.iter().map(|(e, _)| e).collect();
    for e in corps {
        let (Some(c), Some(p)) = (monde.corps.get(e).copied(), monde.lieu.get(e).map(|l| l.pos)) else { continue };
        if let Some((h, n, _)) = sol(monde, p[0], p[2], f32::MAX) {
            monde.lieu.get_mut(e).unwrap().pos[1] = h;
            *monde.corps.get_mut(e).unwrap() = Corps { au_sol: true, vy: 0.0, normale: n, ..c };
        }
    }
}

/// Ce qu'un rayon touche : à quelle distance, où, la normale, et ce qui le porte
/// (`None` : le relief).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Impact {
    pub distance: f32,
    pub point: V3,
    pub normale: V3,
    pub entite: Option<Entite>,
}

fn rayon_boite(o: V3, d: V3, b: &Aabb) -> Option<(f32, V3)> {
    let (mut t0, mut t1, mut n) = (0.0f32, f32::MAX, [0.0; 3]);
    for k in 0..3 {
        let (lo, hi) = (b.centre[k] - b.demi[k], b.centre[k] + b.demi[k]);
        if d[k].abs() < 1e-9 {
            if o[k] < lo || o[k] > hi {
                return None;
            }
            continue;
        }
        let (mut a, mut z) = ((lo - o[k]) / d[k], (hi - o[k]) / d[k]);
        if a > z {
            std::mem::swap(&mut a, &mut z);
        }
        // La face d'entrée regarde vers l'origine du rayon
        let s = -d[k].signum();
        if a > t0 {
            t0 = a;
            n = [0.0; 3];
            n[k] = s;
        }
        t1 = t1.min(z);
        if t0 > t1 {
            return None;
        }
    }
    (t0 > 0.0).then_some((t0, n))
}

fn rayon_sphere(o: V3, d: V3, c: V3, r: f32) -> Option<f32> {
    let m = moins(o, c);
    let b = scal(m, d);
    let k = scal(m, m) - r * r;
    if k > 0.0 && b > 0.0 {
        return None;
    }
    let disc = b * b - k;
    if disc < 0.0 {
        return None;
    }
    Some((-b - disc.sqrt()).max(0.0))
}

fn rayon_capsule(o: V3, d: V3, base: V3, hauteur: f32, r: f32) -> Option<f32> {
    let (y0, y1) = (base[1] + r, base[1] + hauteur - r);
    let mut mieux: Option<f32> = None;
    let mut garder = |t: f32| {
        if t >= 0.0 && mieux.is_none_or(|m| t < m) {
            mieux = Some(t);
        }
    };
    // Le cylindre, entre les deux demi-sphères
    let (mx, mz) = (o[0] - base[0], o[2] - base[2]);
    let a = d[0] * d[0] + d[2] * d[2];
    if a > 1e-12 {
        let b = mx * d[0] + mz * d[2];
        let k = mx * mx + mz * mz - r * r;
        let disc = b * b - a * k;
        if disc >= 0.0 {
            let t = (-b - disc.sqrt()) / a;
            let y = o[1] + d[1] * t;
            if (y0..=y1).contains(&y) {
                garder(t);
            }
        }
    }
    for y in [y0, y1.max(y0)] {
        if let Some(t) = rayon_sphere(o, d, [base[0], y, base[2]], r) {
            garder(t);
        }
    }
    mieux
}

/// Möller et Trumbore.
fn rayon_tri(o: V3, d: V3, [a, b, c]: [V3; 3]) -> Option<f32> {
    let (e1, e2) = (moins(b, a), moins(c, a));
    let p = vect(d, e2);
    let det = scal(e1, p);
    if det.abs() < 1e-9 {
        return None;
    }
    let inv = 1.0 / det;
    let s = moins(o, a);
    let u = scal(s, p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = vect(s, e1);
    let v = scal(d, q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = scal(e2, q) * inv;
    (t > 1e-5).then_some(t)
}

/// Le premier obstacle ou le relief que touche le rayon parti de `o` vers `dir`, jusqu'à
/// `max` mètres.
pub fn lancer(monde: &Monde, o: V3, dir: V3, max: f32) -> Option<Impact> {
    let d = unitaire(dir);
    let fin = plus(o, fois(d, max));
    let mut mieux: Option<Impact> = None;
    let garder = |mieux: &mut Option<Impact>, t: f32, n: V3, e: Option<Entite>| {
        if t <= max && mieux.is_none_or(|m| t < m.distance) {
            *mieux = Some(Impact { distance: t, point: plus(o, fois(d, t)), normale: n, entite: e });
        }
    };
    let (a, b) = ([o[0].min(fin[0]), o[2].min(fin[2])], [o[0].max(fin[0]), o[2].max(fin[2])]);
    for p in proches(monde, a, b) {
        match &p.forme {
            Forme::Boite(x) => {
                if let Some((t, n)) = rayon_boite(o, d, x) {
                    garder(&mut mieux, t, n, Some(p.entite));
                }
            }
            Forme::Capsule { base, hauteur, rayon } => {
                if let Some(t) = rayon_capsule(o, d, *base, *hauteur, *rayon) {
                    let q = plus(o, fois(d, t));
                    let y = q[1].clamp(base[1] + rayon, base[1] + hauteur - rayon);
                    garder(&mut mieux, t, unitaire(moins(q, [base[0], y, base[2]])), Some(p.entite));
                }
            }
            Forme::Triangles(t) => {
                let (la, lb) = ([a[0] - p.o[0], a[1] - p.o[2]], [b[0] - p.o[0], b[1] - p.o[2]]);
                for i in t.pres(la, lb) {
                    let tri = t.t[i].map(|q| plus(q, p.o));
                    if let Some(s) = rayon_tri(o, d, tri) {
                        let mut n = unitaire(vect(moins(tri[1], tri[0]), moins(tri[2], tri[0])));
                        if scal(n, d) > 0.0 {
                            n = fois(n, -1.0);
                        }
                        garder(&mut mieux, s, n, Some(p.entite));
                    }
                }
            }
        }
    }
    // Le relief : pas à pas, puis par dichotomie au passage sous le sol
    let dessus = |t: f32| {
        let q = plus(o, fois(d, t));
        super::systemes::hauteur_sol(monde, q[0], q[2]).map(|h| q[1] - h)
    };
    let pas = 0.5;
    let limite = mieux.map_or(max, |m| m.distance);
    let mut t = 0.0;
    let mut avant = dessus(0.0);
    while t < limite {
        let t2 = (t + pas).min(limite);
        let apres = dessus(t2);
        if let (Some(x), Some(y)) = (avant, apres) {
            if x > 0.0 && y <= 0.0 {
                let (mut lo, mut hi) = (t, t2);
                for _ in 0..16 {
                    let m = (lo + hi) / 2.0;
                    if dessus(m).is_some_and(|h| h > 0.0) { lo = m } else { hi = m }
                }
                let q = plus(o, fois(d, hi));
                let n = sol(monde, q[0], q[2], f32::MAX).map_or([0.0, 1.0, 0.0], |s| s.1);
                // Le relief, pas ce qui le recouvre
                let n = monde.relief.iter().find_map(|(e, r)| {
                    let w = monde.lieu.get(e)?.pos;
                    let (u, v) = (q[0] - w[0], q[2] - w[2]);
                    ((0.0..=TAILLE).contains(&u) && (0.0..=TAILLE).contains(&v)).then(|| r.normale(u, v))
                }).unwrap_or(n);
                garder(&mut mieux, hi, n, None);
                break;
            }
        }
        avant = apres;
        t = t2;
    }
    mieux
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::moteur::{Lieu, Mobile, Relief};

    /// Un sol plat à 0, sur neuf tuiles autour de l'origine, un corps aux pieds en `p`,
    /// et des obstacles au repère du monde.
    fn monde(p: V3, formes: Vec<Forme>) -> (Monde, Entite) {
        let mut m = Monde::default();
        for j in -1..=1 {
            for i in -1..=1 {
                let t = m.creer(&format!("t{i}/{j}"));
                m.lieu.mettre(t, Lieu::en([i as f32 * TAILLE, 0.0, j as f32 * TAILLE], 0.0));
                m.relief.mettre(t, Relief { n: 1, h: vec![0.0; 4] });
            }
        }
        let d = m.creer("decor");
        m.lieu.mettre(d, Lieu::en([0.0; 3], 0.0));
        m.obstacles.mettre(d, formes);
        let e = m.creer("corps");
        m.lieu.mettre(e, Lieu::en(p, 0.0));
        m.mobile.mettre(e, Mobile { vitesse: 0.0, elan: [0.0, 1.0], rayon: 0.3 });
        m.corps.mettre(e, Corps::new(1.8));
        poser(&mut m);
        (m, e)
    }

    /// Avance `n` pas vers `dir` à `v` m/s.
    fn marcher(m: &mut Monde, e: Entite, dir: [f32; 2], v: f32, n: usize) {
        for _ in 0..n {
            *m.mobile.get_mut(e).unwrap() = Mobile { vitesse: v, elan: dir, rayon: 0.3 };
            deplacer(m);
        }
    }

    fn pos(m: &Monde, e: Entite) -> V3 {
        m.lieu.get(e).unwrap().pos
    }

    fn boite(c: V3, d: V3) -> Forme {
        Forme::Boite(Aabb { centre: c, demi: d })
    }

    #[test]
    fn une_marche_se_monte_un_mur_arrete() {
        // Une dalle de 30 cm, puis un muret de 50 cm sur la dalle
        let (mut m, e) = monde([0.0, 0.0, 0.0], vec![boite([0.0, 0.15, 3.0], [2.0, 0.15, 1.0]), boite([0.0, 0.55, 6.0], [2.0, 0.25, 1.0])]);
        marcher(&mut m, e, [0.0, 1.0], 1.5, 120);
        let p = pos(&m, e);
        assert!((p[1] - 0.3).abs() < 1e-4 && p[2] > 2.5, "sur la dalle : {p:?}");
        marcher(&mut m, e, [0.0, 1.0], 1.5, 240);
        let p = pos(&m, e);
        assert!((p[2] - (5.0 - 0.3)).abs() < 1e-3, "contre le muret, qui dépasse la marche : {p:?}");
        // De biais, il glisse le long du muret
        marcher(&mut m, e, [0.6, 0.8], 1.5, 60);
        let q = pos(&m, e);
        assert!(q[0] > p[0] + 0.5 && (q[2] - p[2]).abs() < 1e-3, "{q:?}");
        // Et redescend de la dalle en la suivant
        marcher(&mut m, e, [0.0, -1.0], 1.5, 240);
        let q = pos(&m, e);
        assert!(q[1].abs() < 1e-4 && m.corps.get(e).unwrap().au_sol, "{q:?}");
    }

    #[test]
    fn un_bord_fait_tomber_et_la_chute_suit_la_pesanteur() {
        // Une plate-forme de 2 m de haut, le corps dessus
        let (mut m, e) = monde([0.0, 3.0, 0.0], vec![boite([0.0, 1.0, 0.0], [1.0, 1.0, 1.0])]);
        assert_eq!(pos(&m, e)[1], 2.0);
        // Il quitte le bord, puis s'arrête et tombe
        marcher(&mut m, e, [1.0, 0.0], 3.0, 30);
        let mut n = 0;
        while !m.corps.get(e).unwrap().au_sol || n == 0 {
            marcher(&mut m, e, [1.0, 0.0], 0.0, 1);
            n += 1;
            assert!(n < 200);
        }
        assert_eq!(pos(&m, e)[1], 0.0);
        // Une chute de 2 m dure √(2h/g) ≈ 0,64 s, moins ce qui est déjà tombé en marchant
        assert!(n < 50, "{n} pas");
    }

    #[test]
    fn une_rampe_en_triangles_se_monte_et_un_poteau_arrete() {
        // Une rampe de 4 m qui monte de 1 m vers +z, puis le dessus d'une plate-forme
        let t = Arc::new(Triangles::new(vec![
            [[-1.0, 0.0, 2.0], [1.0, 0.0, 2.0], [1.0, 1.0, 6.0]],
            [[-1.0, 0.0, 2.0], [1.0, 1.0, 6.0], [-1.0, 1.0, 6.0]],
        ]));
        let (mut m, e) = monde([0.0, 0.0, 0.0], vec![Forme::Triangles(t), boite([0.0, 0.5, 7.0], [1.0, 0.5, 1.0])]);
        marcher(&mut m, e, [0.0, 1.0], 1.5, 300);
        let p = pos(&m, e);
        assert!(p[2] > 6.0 && (p[1] - 1.0).abs() < 1e-3, "sur la plate-forme : {p:?}");
        // Un poteau
        let (mut m, e) = monde([0.0, 0.0, 0.0], vec![Forme::Capsule { base: [0.0, 0.0, 3.0], hauteur: 3.0, rayon: 0.2 }]);
        marcher(&mut m, e, [0.0, 1.0], 1.5, 240);
        assert!((pos(&m, e)[2] - (3.0 - 0.5)).abs() < 1e-3, "{:?}", pos(&m, e));
    }

    #[test]
    fn une_pente_trop_raide_ne_se_gravit_pas_et_fait_glisser() {
        // Un talus à 60° vers +z
        let h = 3.0;
        let l = h / 60f32.to_radians().tan();
        let t = Arc::new(Triangles::new(vec![
            [[-3.0, 0.0, 2.0], [3.0, 0.0, 2.0], [3.0, h, 2.0 + l]],
            [[-3.0, 0.0, 2.0], [3.0, h, 2.0 + l], [-3.0, h, 2.0 + l]],
        ]));
        let (mut m, e) = monde([0.0, 0.0, 0.0], vec![Forme::Triangles(t.clone())]);
        marcher(&mut m, e, [0.0, 1.0], 2.0, 240);
        let p = pos(&m, e);
        assert!(p[1] < MARCHE + 0.05, "il ne monte pas : {p:?}");
        // Posé dessus, il glisse en bas
        let (mut m, e) = monde([0.0, 2.0, 2.0 + l * 0.66], vec![Forme::Triangles(t)]);
        assert!(pos(&m, e)[1] > 1.5);
        marcher(&mut m, e, [0.0, 1.0], 0.0, 240);
        let p = pos(&m, e);
        assert!(p[1] < 0.05 && p[2] < 2.0 + 0.2, "en bas du talus : {p:?}");
        // Une pente de 30° se gravit
        let l = h / 30f32.to_radians().tan();
        let t = Arc::new(Triangles::new(vec![
            [[-3.0, 0.0, 2.0], [3.0, 0.0, 2.0], [3.0, h, 2.0 + l]],
            [[-3.0, 0.0, 2.0], [3.0, h, 2.0 + l], [-3.0, h, 2.0 + l]],
        ]));
        let (mut m, e) = monde([0.0, 0.0, 0.0], vec![Forme::Triangles(t)]);
        marcher(&mut m, e, [0.0, 1.0], 1.5, 300);
        assert!(pos(&m, e)[1] > 2.0, "{:?}", pos(&m, e));
    }

    #[test]
    fn les_rayons_touchent_le_premier_obstacle() {
        let t = Arc::new(Triangles::new(vec![[[-1.0, 0.0, 8.0], [1.0, 0.0, 8.0], [0.0, 3.0, 8.0]]]));
        let (m, _) = monde([0.0, 0.0, -20.0], vec![boite([0.0, 1.0, 5.0], [1.0, 1.0, 1.0]), Forme::Capsule { base: [3.0, 0.0, 0.0], hauteur: 2.0, rayon: 0.5 }, Forme::Triangles(t)]);
        let decor = m.trouver("decor");
        // La boîte, de face
        let i = lancer(&m, [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], 20.0).unwrap();
        assert!((i.distance - 4.0).abs() < 1e-4 && i.normale == [0.0, 0.0, -1.0] && i.entite == decor, "{i:?}");
        // Au-dessus de la boîte : le triangle
        let i = lancer(&m, [0.0, 2.5, 0.0], [0.0, 0.0, 1.0], 20.0).unwrap();
        assert!((i.distance - 8.0).abs() < 1e-4, "{i:?}");
        // La capsule, de côté
        let i = lancer(&m, [0.0, 1.0, 0.0], [1.0, 0.0, 0.0], 20.0).unwrap();
        assert!((i.distance - 2.5).abs() < 1e-4 && (i.normale[0] + 1.0).abs() < 1e-4, "{i:?}");
        // Le sol, en biais
        let i = lancer(&m, [0.0, 2.0, 0.0], [0.0, -1.0, -1.0], 20.0).unwrap();
        assert!((i.distance - 2.0 * 2f32.sqrt()).abs() < 1e-3 && i.entite.is_none(), "{i:?}");
        // Rien vers le ciel, ni au-delà de la portée
        assert!(lancer(&m, [0.0, 1.0, 0.0], [0.0, 1.0, 0.0], 20.0).is_none());
        assert!(lancer(&m, [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], 3.0).is_none());
    }

    #[test]
    fn les_triangles_se_rangent_dans_leur_grille() {
        let t = Triangles::new(vec![[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]], [[10.0, 0.0, 10.0], [11.0, 0.0, 10.0], [10.0, 0.0, 11.0]]]);
        assert_eq!(t.pres([0.5, 0.5], [0.5, 0.5]), [0]);
        assert_eq!(t.pres([10.5, 10.5], [10.6, 10.6]), [1]);
        assert_eq!(t.pres([-1.0, -1.0], [20.0, 20.0]), [0, 1]);
        assert!(t.pres([30.0, 30.0], [31.0, 31.0]).is_empty());
    }
}
