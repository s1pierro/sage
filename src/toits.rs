//! Les toits des bâtiments d'un lieu réel : leur forme, leur géométrie, et leur ajustement
//! sur les hauteurs mesurées (le MNH LiDAR HD de l'IGN, `examples/lieu_reel.rs`).
//!
//! Un toit se décrit par sa forme (`PLAT`, `DEUX_PANS`, `CROUPE`, `APPENTIS`), la
//! hauteur de l'égout et du faîtage (`Batiment::egout`, `Batiment::toit`) et un axe
//! (`Batiment::axe`, en radians dans le plan x, z) : le faîtage d'un toit à deux pans le
//! suit, un appentis monte en travers. Le cadre du toit est le rectangle orienté selon
//! l'axe qui enveloppe le contour.
//!
//! Sa surface est le plus bas de quelques plans (`plans`) : deux pour deux pans, quatre
//! pour une croupe, un pour un appentis ou un toit plat. Chaque pan est le morceau du
//! contour où son plan est le plus bas (découpé par demi-plans) ; les murs montent
//! jusqu'au toit, pignons compris.

use crate::reel::{aire, trianguler, Batiment};

pub const PLAT: u8 = 0;
pub const DEUX_PANS: u8 = 1;
pub const CROUPE: u8 = 2;
pub const APPENTIS: u8 = 3;

/// Le rectangle orienté selon `axe` qui enveloppe un contour : ses axes `u` (le long) et
/// `w` (en travers), son centre (`um`, `wm`) et ses demi-côtés (`a` le long, `b` en
/// travers).
#[derive(Clone, Copy, Debug)]
pub struct Cadre {
    pub u: [f32; 2],
    pub w: [f32; 2],
    pub um: f32,
    pub wm: f32,
    pub a: f32,
    pub b: f32,
}

impl Cadre {
    pub fn new(c: &[[f32; 2]], axe: f32) -> Cadre {
        let u = [axe.cos(), axe.sin()];
        let w = [-u[1], u[0]];
        let (mut u0, mut u1, mut w0, mut w1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for p in c {
            let (pu, pw) = (p[0] * u[0] + p[1] * u[1], p[0] * w[0] + p[1] * w[1]);
            (u0, u1, w0, w1) = (u0.min(pu), u1.max(pu), w0.min(pw), w1.max(pw));
        }
        Cadre { u, w, um: (u0 + u1) / 2.0, wm: (w0 + w1) / 2.0, a: ((u1 - u0) / 2.0).max(0.05), b: ((w1 - w0) / 2.0).max(0.05) }
    }

    /// Les coordonnées (u, w) d'un point, depuis le centre.
    pub fn local(&self, p: [f32; 2]) -> (f32, f32) {
        (p[0] * self.u[0] + p[1] * self.u[1] - self.um, p[0] * self.w[0] + p[1] * self.w[1] - self.wm)
    }

    /// Le plan h = k + su·u + sw·w (u, w depuis le centre), en (h0, dh/dx, dh/dz).
    fn plan(&self, k: f32, su: f32, sw: f32) -> [f32; 3] {
        let (px, pz) = (su * self.u[0] + sw * self.w[0], su * self.u[1] + sw * self.w[1]);
        [k - su * self.um - sw * self.wm, px, pz]
    }
}

/// L'angle du plus long côté d'un contour.
pub fn axe_long(c: &[[f32; 2]]) -> f32 {
    let (mut axe, mut long) = (0.0f32, 0.0f32);
    for i in 0..c.len() {
        let (p, q) = (c[i], c[(i + 1) % c.len()]);
        let (dx, dz) = (q[0] - p[0], q[1] - p[1]);
        let l = dx.hypot(dz);
        if l > long {
            (axe, long) = (dz.atan2(dx), l);
        }
    }
    axe
}

/// Les plans du toit d'un bâtiment (h = p[0] + p[1]·x + p[2]·z) ; sa surface est le plus
/// bas d'entre eux.
pub fn plans(b: &Batiment) -> Vec<[f32; 3]> {
    let k = Cadre::new(&b.contour, b.axe);
    let d = b.toit - b.egout;
    if b.forme == PLAT || d < 0.05 {
        return vec![[b.toit, 0.0, 0.0]];
    }
    match b.forme {
        DEUX_PANS => {
            let s = d / k.b;
            vec![k.plan(b.toit, 0.0, -s), k.plan(b.toit, 0.0, s)]
        }
        CROUPE => {
            let s = d / k.a.min(k.b);
            vec![k.plan(b.egout + s * k.b, 0.0, -s), k.plan(b.egout + s * k.b, 0.0, s), k.plan(b.egout + s * k.a, -s, 0.0), k.plan(b.egout + s * k.a, s, 0.0)]
        }
        _ => {
            let s = d / (2.0 * k.b);
            vec![k.plan(b.egout + s * k.b, 0.0, s)]
        }
    }
}

fn sur(p: &[f32; 3], x: f32, z: f32) -> f32 {
    p[0] + p[1] * x + p[2] * z
}

/// La hauteur du toit en (x, z).
pub fn hauteur(plans: &[[f32; 3]], x: f32, z: f32) -> f32 {
    plans.iter().map(|p| sur(p, x, z)).fold(f32::MAX, f32::min)
}

/// Le morceau d'un polygone où `g` ≤ 0 (`g` affine) : Sutherland-Hodgman.
fn couper(poly: &[[f32; 2]], g: impl Fn([f32; 2]) -> f32) -> Vec<[f32; 2]> {
    let mut o = Vec::with_capacity(poly.len() + 2);
    for k in 0..poly.len() {
        let (p, q) = (poly[k], poly[(k + 1) % poly.len()]);
        let (gp, gq) = (g(p), g(q));
        if gp <= 0.0 {
            o.push(p);
        }
        if (gp <= 0.0) != (gq <= 0.0) {
            let t = gp / (gp - gq);
            o.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
        }
    }
    o
}

/// Monte un bâtiment : ses murs jusqu'au toit et les pans du toit. `tri` reçoit chaque
/// triangle (positions absolues), sa normale et sa couleur, dans le sens de la normale.
pub fn monter(b: &Batiment, murs: [f32; 3], toit: [f32; 3], tri: &mut dyn FnMut([[f32; 3]; 3], [f32; 3], [f32; 3])) {
    let c = &b.contour;
    let pl = plans(b);
    let sens = aire(c).signum();
    // Les murs : chaque côté coupé là où le pan du dessus change (le haut d'un pignon)
    for s in 0..c.len() {
        let (a, d) = (c[s], c[(s + 1) % c.len()]);
        let (dx, dz) = (d[0] - a[0], d[1] - a[1]);
        let l = dx.hypot(dz);
        if l < 1e-3 {
            continue;
        }
        let mut ts = vec![0.0f32, 1.0];
        for i in 0..pl.len() {
            for j in i + 1..pl.len() {
                let (ia, id, ja, jd) = (sur(&pl[i], a[0], a[1]), sur(&pl[i], d[0], d[1]), sur(&pl[j], a[0], a[1]), sur(&pl[j], d[0], d[1]));
                let den = (id - ia) - (jd - ja);
                if den.abs() < 1e-6 {
                    continue;
                }
                let t = (ja - ia) / den;
                if t > 1e-4 && t < 1.0 - 1e-4 {
                    let (x, z) = (a[0] + dx * t, a[1] + dz * t);
                    if sur(&pl[i], x, z) <= hauteur(&pl, x, z) + 1e-3 {
                        ts.push(t);
                    }
                }
            }
        }
        ts.sort_by(|p, q| p.partial_cmp(q).unwrap());
        let n = [dz / l * sens, 0.0, -dx / l * sens];
        for w in ts.windows(2) {
            let p = [a[0] + dx * w[0], a[1] + dz * w[0]];
            let q = [a[0] + dx * w[1], a[1] + dz * w[1]];
            let (p0, p1) = ([p[0], b.pied, p[1]], [p[0], hauteur(&pl, p[0], p[1]), p[1]]);
            let (q0, q1) = ([q[0], b.pied, q[1]], [q[0], hauteur(&pl, q[0], q[1]), q[1]]);
            if sens > 0.0 {
                tri([p0, p1, q1], n, murs);
                tri([p0, q1, q0], n, murs);
            } else {
                tri([p0, q1, p1], n, murs);
                tri([p0, q0, q1], n, murs);
            }
        }
    }
    // Les pans : pour chaque plan, le morceau du contour où il est le plus bas
    for (i, p) in pl.iter().enumerate() {
        let mut morceau = c.clone();
        for (j, q) in pl.iter().enumerate() {
            if i != j && morceau.len() > 2 {
                morceau = couper(&morceau, |x| sur(p, x[0], x[1]) - sur(q, x[0], x[1]));
            }
        }
        if morceau.len() < 3 || aire(&morceau).abs() < 0.01 {
            continue;
        }
        let l = (p[1] * p[1] + p[2] * p[2] + 1.0).sqrt();
        let n = [-p[1] / l, 1.0 / l, -p[2] / l];
        let s = aire(&morceau).signum();
        for [x, y, z] in trianguler(&morceau) {
            let q = [x, y, z].map(|k| [morceau[k][0], sur(p, morceau[k][0], morceau[k][1]), morceau[k][1]]);
            if s > 0.0 {
                tri([q[0], q[2], q[1]], n, toit);
            } else {
                tri(q, n, toit);
            }
        }
    }
}

/// Le toit ajusté sur des mesures (x, z, hauteur au-dessus du sol) prises dans le
/// contour : (forme, égout, faîtage, axe), les hauteurs depuis le sol. Rien s'il y a trop
/// peu de mesures ou si le bâtiment n'y est pas (plus bas que 2 m : neuf, ou démoli).
pub fn ajuster(c: &[[f32; 2]], m: &[[f32; 3]]) -> Option<(u8, f32, f32, f32)> {
    if m.len() < 6 {
        return None;
    }
    let mediane = |mut v: Vec<f32>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };
    let m0 = mediane(m.iter().map(|p| p[2]).collect());
    if m0 < 2.0 {
        return None;
    }
    let e0 = mediane(m.iter().map(|p| (p[2] - m0).abs()).collect());
    // h ≈ k + s·f, robuste : un second ajustement sans les points trop loin du premier
    // (les cheminées, les arbres qui débordent) ; l'erreur, la médiane des écarts
    let droite = |f: &dyn Fn([f32; 3]) -> f32| -> (f32, f32, f32) {
        let mut garde: Vec<bool> = vec![true; m.len()];
        let mut r = (m0, 0.0);
        for _ in 0..2 {
            let (mut n, mut sf, mut sh, mut sff, mut sfh) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
            for (p, _) in m.iter().zip(&garde).filter(|(_, g)| **g) {
                let (x, h) = (f(*p) as f64, p[2] as f64);
                (n, sf, sh, sff, sfh) = (n + 1.0, sf + x, sh + h, sff + x * x, sfh + x * h);
            }
            let den = n * sff - sf * sf;
            if n < 4.0 || den.abs() < 1e-9 {
                break;
            }
            let s = (n * sfh - sf * sh) / den;
            r = (((sh - s * sf) / n) as f32, s as f32);
            let ecarts: Vec<f32> = m.iter().map(|p| (p[2] - r.0 - r.1 * f(*p)).abs()).collect();
            let mad = mediane(ecarts.clone());
            garde = ecarts.iter().map(|e| *e < (2.5 * mad).max(0.4)).collect();
        }
        let e = mediane(m.iter().map(|p| (p[2] - r.0 - r.1 * f(*p)).abs()).collect());
        (r.0, r.1, e)
    };
    let axe = axe_long(c);
    let mut mieux: Option<(f32, (u8, f32, f32, f32))> = None;
    let mut garder = |e: f32, t: (u8, f32, f32, f32)| {
        if t.2 - t.1 > 0.8 && mieux.is_none_or(|(em, _)| e < em) {
            mieux = Some((e, t));
        }
    };
    for axe in [axe, axe + std::f32::consts::FRAC_PI_2] {
        let k = Cadre::new(c, axe);
        // Deux pans, le faîtage le long de l'axe
        let (r, s, e) = droite(&|p| k.local([p[0], p[1]]).1.abs());
        if (-2.5..=-0.18).contains(&s) {
            garder(e, (DEUX_PANS, r + s * k.b, r, axe));
        }
        // Un appentis, qui monte en travers (dans un sens ou l'autre)
        let (r, s, e) = droite(&|p| k.local([p[0], p[1]]).1);
        if (0.1..=1.5).contains(&s.abs()) {
            let sens = if s > 0.0 { axe } else { axe + std::f32::consts::PI };
            garder(e * 1.25, (APPENTIS, r - s.abs() * k.b, r + s.abs() * k.b, sens));
        }
    }
    // Une croupe : la hauteur monte avec la distance au bord du cadre
    let k = Cadre::new(c, axe);
    let (r, s, e) = droite(&|p| {
        let (u, w) = k.local([p[0], p[1]]);
        (k.b - w.abs()).min(k.a - u.abs()).max(0.0)
    });
    if (0.18..=2.5).contains(&s) {
        garder(e * 1.05, (CROUPE, r, r + s * k.a.min(k.b), axe));
    }
    let t = match mieux {
        Some((e, t)) if e < 0.75 * e0 => t,
        _ => (PLAT, m0, m0, axe),
    };
    // L'égout au moins à 2 m
    let egout = t.1.max(2.0);
    Some((t.0, egout, t.2.max(egout), t.3))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reel::MAISON;

    fn maison(forme: u8, axe: f32) -> Batiment {
        Batiment { genre: MAISON, pied: -1.0, toit: 9.0, egout: 5.0, forme, axe, contour: vec![[0.0, 0.0], [12.0, 0.0], [12.0, 8.0], [0.0, 8.0]] }
    }

    /// Les mesures d'un toit tous les 50 cm dans le contour, à 75 cm des murs.
    fn mesures(b: &Batiment) -> Vec<[f32; 3]> {
        let pl = plans(b);
        let mut m = Vec::new();
        for i in 0..48 {
            for j in 0..32 {
                let (x, z) = (0.25 + i as f32 * 0.25, 0.25 + j as f32 * 0.25);
                if x > 0.75 && x < 11.25 && z > 0.75 && z < 7.25 {
                    m.push([x, z, hauteur(&pl, x, z)]);
                }
            }
        }
        m
    }

    #[test]
    fn un_toit_a_deux_pans_a_son_faitage_au_milieu() {
        let b = maison(DEUX_PANS, 0.0);
        let pl = plans(&b);
        assert!((hauteur(&pl, 6.0, 4.0) - 9.0).abs() < 1e-4);
        assert!((hauteur(&pl, 6.0, 0.0) - 5.0).abs() < 1e-4);
        assert!((hauteur(&pl, 0.0, 8.0) - 5.0).abs() < 1e-4);
        // Une croupe : 5 au bord, le faîtage à 9 au milieu, plus bas vers les bouts
        let c = plans(&maison(CROUPE, 0.0));
        assert!((hauteur(&c, 6.0, 4.0) - 9.0).abs() < 1e-4);
        assert!((hauteur(&c, 0.0, 4.0) - 5.0).abs() < 1e-4);
        assert!(hauteur(&c, 2.0, 4.0) < 9.0);
    }

    #[test]
    fn une_maison_se_monte_fermee() {
        for forme in [PLAT, DEUX_PANS, CROUPE, APPENTIS] {
            let b = maison(forme, 0.0);
            let mut t = Vec::new();
            monter(&b, [1.0; 3], [0.5; 3], &mut |p, n, _| t.push((p, n)));
            // Chaque triangle tourne dans le sens de sa normale
            for (p, n) in &t {
                let (u, w) = ([p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]], [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]]);
                let x = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
                assert!(x[0] * n[0] + x[1] * n[1] + x[2] * n[2] > 0.0, "{forme}");
            }
            // Le toit couvre l'emprise : 96 m² vus d'en haut
            let dessus: f32 = t.iter().filter(|(_, n)| n[1] > 0.1).map(|(p, _)| ((p[1][0] - p[0][0]) * (p[2][2] - p[0][2]) - (p[2][0] - p[0][0]) * (p[1][2] - p[0][2])).abs() / 2.0).sum();
            assert!((dessus - 96.0).abs() < 0.1, "{forme} : {dessus}");
            // Les murs montent jusqu'au toit : le plus haut d'un mur est à 9 m sauf à plat
            let haut = t.iter().filter(|(_, n)| n[1] == 0.0).flat_map(|(p, _)| p.iter().map(|q| q[1])).fold(f32::MIN, f32::max);
            assert!((haut - if forme == DEUX_PANS || forme == APPENTIS { 9.0 } else if forme == PLAT { 9.0 } else { 5.0 }).abs() < 1e-3, "{forme} : {haut}");
        }
    }

    #[test]
    fn les_mesures_retrouvent_le_toit() {
        for (forme, axe) in [(DEUX_PANS, 0.0), (DEUX_PANS, std::f32::consts::FRAC_PI_2), (CROUPE, 0.0), (APPENTIS, 0.0), (PLAT, 0.0)] {
            let b = maison(forme, axe);
            let mut m = mesures(&b);
            // Une cheminée
            for p in m.iter_mut().take(6) {
                p[2] += 1.5;
            }
            let (f, e, t, a) = ajuster(&b.contour, &m).expect("des mesures");
            assert_eq!(f, forme, "{forme} {axe}");
            assert!((t - b.toit).abs() < 0.3, "{forme} : faîtage {t}");
            if forme != PLAT {
                assert!((e - b.egout).abs() < 0.4, "{forme} : égout {e}");
                let d = (a - axe).rem_euclid(std::f32::consts::PI);
                assert!(d < 0.01 || d > std::f32::consts::PI - 0.01 || forme == APPENTIS, "{forme} : axe {a}");
            }
        }
        // Un bâtiment absent des mesures (au sol)
        let m: Vec<[f32; 3]> = mesures(&maison(PLAT, 0.0)).iter().map(|p| [p[0], p[1], 0.2]).collect();
        assert!(ajuster(&maison(PLAT, 0.0).contour, &m).is_none());
    }
}
