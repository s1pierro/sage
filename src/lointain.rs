//! Le relief lointain : toute la carte en un seul maillage, un point par coin de tuile
//! (64 m), dessiné au-delà des tuiles chargées. Il se voit jusqu'à la limite de la vue
//! (`reglages::VISIBILITE`) ; là où les tuiles sont chargées, le programme du terrain
//! l'efface (`Scene::trou`).
//!
//! Ses coins sont ceux des tuiles du générateur : au bord des tuiles chargées, les deux
//! se rejoignent (à leurs retouches près). Le fond de la mer au large n'y est pas : l'eau
//! le cache.
//!
//! Les bâtiments d'un lieu réel s'y ajoutent en blocs (`bati`) : les plus grands ou les
//! plus hauts, chacun en pavé droit le long de son plus long côté, à sa hauteur et ses
//! couleurs, effacés eux aussi sous les tuiles chargées.

use crate::campagne::Campagne;
use crate::moteur::tuiles::DEMI_CARTE;
use crate::moteur::TAILLE;

/// L'identifiant de son maillage : « tuile/ », il se dessine en topologie avec le relief.
pub const ID: &str = "tuile/lointain";
/// Celui des bâtiments lointains : « bati/ », il ne se dessine pas en topologie.
pub const ID_BATI: &str = "bati/lointain";
/// Les bâtiments lointains : au moins tant de m² au sol, ou tant de mètres de haut.
const BATI_AIRE: f32 = 100.0;
const BATI_HAUT: f32 = 15.0;
/// Un carré dont les quatre coins sont plus bas ne se dessine pas.
const FOND: f32 = -12.0;

/// Ses sommets (position absolue, normale, couleur), en triangles.
pub fn sommets(c: &Campagne) -> Vec<f32> {
    let d = DEMI_CARTE;
    let n = (2 * d + 2) as usize; // coins par côté
    let t = TAILLE as f64;
    let coin = |a: usize| (a as i32 - d) as f64 * t;
    let mut h = Vec::with_capacity(n * n);
    for b in 0..n {
        for a in 0..n {
            h.push(c.hauteur(coin(a), coin(b)));
        }
    }
    let at = |a: usize, b: usize| h[b.min(n - 1) * n + a.min(n - 1)];
    let mut points = Vec::with_capacity(n * n);
    for b in 0..n {
        for a in 0..n {
            let dx = (at(a + 1, b) - at(a.saturating_sub(1), b)) / (2.0 * t);
            let dz = (at(a, b + 1) - at(a, b.saturating_sub(1))) / (2.0 * t);
            let l = (dx * dx + dz * dz + 1.0).sqrt();
            let nm = [(-dx / l) as f32, (1.0 / l) as f32, (-dz / l) as f32];
            let y = at(a, b);
            points.push(([coin(a) as f32, y as f32, coin(b) as f32], nm, c.couleur(coin(a), coin(b), y, nm)));
        }
    }
    let mut v = Vec::new();
    for b in 0..n - 1 {
        for a in 0..n - 1 {
            let q = [b * n + a, b * n + a + 1, (b + 1) * n + a, (b + 1) * n + a + 1];
            if q.iter().all(|&i| points[i].0[1] < c.mer() + FOND) {
                continue;
            }
            for i in [q[0], q[3], q[1], q[0], q[2], q[3]] {
                let (p, nm, co) = points[i];
                v.extend_from_slice(&p);
                v.extend_from_slice(&nm);
                v.extend_from_slice(&co);
            }
        }
    }
    v
}

/// Les bâtiments lointains d'un lieu réel (rien sinon), en triangles : position absolue,
/// normale, couleur.
pub fn bati(c: &Campagne) -> Vec<f32> {
    let Some(r) = &c.reel else { return Vec::new() };
    let mut v = Vec::new();
    for b in &r.batiments {
        let k = &b.contour;
        let aire = crate::reel::aire(k).abs();
        if aire < BATI_AIRE && b.toit - b.pied < BATI_HAUT + 1.0 {
            continue;
        }
        // Le plus long côté donne l'axe du pavé
        let (mut ax, mut long) = ([1.0f32, 0.0], 0.0f32);
        for i in 0..k.len() {
            let (p, q) = (k[i], k[(i + 1) % k.len()]);
            let d = [q[0] - p[0], q[1] - p[1]];
            let l = d[0].hypot(d[1]);
            if l > long {
                (ax, long) = ([d[0] / l, d[1] / l], l);
            }
        }
        let tr = [-ax[1], ax[0]];
        let (mut u0, mut u1, mut w0, mut w1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for p in k {
            let (u, w) = (p[0] * ax[0] + p[1] * ax[1], p[0] * tr[0] + p[1] * tr[1]);
            (u0, u1, w0, w1) = (u0.min(u), u1.max(u), w0.min(w), w1.max(w));
        }
        // Le pavé couvre plus que le contour : il se rétrécit à la même aire
        let s = (aire / ((u1 - u0) * (w1 - w0)).max(1e-3)).sqrt().min(1.0);
        let (um, wm) = ((u0 + u1) / 2.0, (w0 + w1) / 2.0);
        let (du, dw) = ((u1 - u0) / 2.0 * s, (w1 - w0) / 2.0 * s);
        let coin = |u: f32, w: f32, y: f32| [ax[0] * u + tr[0] * w, y, ax[1] * u + tr[1] * w];
        let pied = [(um - du, wm - dw), (um + du, wm - dw), (um + du, wm + dw), (um - du, wm + dw)];
        let (murs, toit) = crate::reel::teintes_bati(b.genre);
        let mut quad = |q: [[f32; 3]; 4], n: [f32; 3], co: [f32; 3]| {
            for i in [0, 1, 2, 0, 2, 3] {
                v.extend_from_slice(&q[i]);
                v.extend_from_slice(&n);
                v.extend_from_slice(&co);
            }
        };
        // Un toit en pente : le bloc à mi-hauteur du toit
        let sommet = (b.toit + b.egout) / 2.0;
        let haut = pied.map(|(u, w)| coin(u, w, sommet));
        // Le toit, vers le haut (sens des triangles d'après sa normale)
        let e = |a: [f32; 3], b: [f32; 3], c: [f32; 3]| (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]);
        let t = if e(haut[0], haut[1], haut[2]) > 0.0 { haut } else { [haut[0], haut[3], haut[2], haut[1]] };
        quad(t, [0.0, 1.0, 0.0], toit);
        // Les quatre murs, chacun sa normale vers dehors
        for i in 0..4 {
            let (a, b2) = (pied[i], pied[(i + 1) % 4]);
            let (p, q) = (coin(a.0, a.1, b.pied), coin(b2.0, b2.1, b.pied));
            let (pp, qq) = (coin(a.0, a.1, sommet), coin(b2.0, b2.1, sommet));
            let m = coin(um, wm, 0.0);
            let mut n = [q[2] - p[2], 0.0, p[0] - q[0]];
            let l = n[0].hypot(n[2]).max(1e-6);
            n = [n[0] / l, 0.0, n[2] / l];
            let dehors = (p[0] + q[0]) / 2.0 - m[0];
            let dehors_z = (p[2] + q[2]) / 2.0 - m[2];
            if n[0] * dehors + n[2] * dehors_z < 0.0 {
                quad([q, p, pp, qq], [-n[0], 0.0, -n[2]], murs);
            } else {
                quad([p, q, qq, pp], n, murs);
            }
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_lointain_couvre_l_ile_et_rejoint_les_tuiles() {
        use crate::moteur::Source;
        let c = Campagne::brute(1);
        let v = sommets(&c);
        assert_eq!(v.len() % (9 * 3), 0);
        // L'île y est : des sommets au-dessus de 100 m, et pas toute la mer
        assert!(v.chunks(9).any(|s| s[1] > 100.0));
        let carres = v.len() / 9 / 6;
        assert!(carres < 313 * 313 * 3 / 4, "{carres}");
        // Un coin du lointain a la hauteur du coin de la tuile
        let t = c.tuile(0, 0);
        let s = v.chunks(9).find(|s| s[0] == 0.0 && s[2] == 0.0).expect("le coin (0, 0)");
        assert_eq!(s[1], t.relief.sommet(0, 0));
        assert!(bati(&c).is_empty());
    }

    #[test]
    fn les_grands_batiments_se_voient_de_loin() {
        use crate::reel::{cote, Batiment, Reel, MAISON};
        let pas = 64.0;
        let n = cote(pas);
        let carre = |x: f32, l: f32| vec![[x, 0.0], [x + l, 0.0], [x + l, l], [x, l]];
        let b = vec![
            Batiment::plat(MAISON, -1.0, 6.0, carre(0.0, 8.0)),
            Batiment::plat(MAISON, -1.0, 9.0, carre(100.0, 20.0)),
        ];
        let r = Reel::new("Essai", 49.0, 1.0, 0.0, 0.0, pas, -50.0, &vec![0.0; n * n], &vec![false; n * n], Vec::new(), b, Vec::new());
        let v = bati(&Campagne::reelle(std::sync::Arc::new(r)));
        // Le petit n'y est pas, le grand en cinq quadrilatères
        assert_eq!(v.len(), 5 * 6 * 9);
        assert!(v.chunks(9).all(|s| s[0] >= 99.9 && s[0] <= 120.1));
        // Le toit à 9 m, les murs regardent dehors (du centre (110, 10))
        for s in v.chunks(9) {
            if s[4] > 0.5 {
                assert_eq!(s[1], 9.0);
            } else {
                assert!((s[0] - 110.0) * s[3] + (s[2] - 10.0) * s[5] > 0.0);
            }
        }
        // Les triangles du toit vus d'en haut tournent dans le sens de la normale
        let t: Vec<&[f32]> = v.chunks(9).take(3).collect();
        let (a, b, c) = (t[0], t[1], t[2]);
        let y = (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]);
        assert!(y > 0.0);
    }
}
