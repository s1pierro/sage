//! Le relief lointain : toute la carte en un seul maillage, un point par coin de tuile
//! (64 m), dessiné au-delà des tuiles chargées. Il se voit jusqu'à la limite de la vue
//! (`reglages::VISIBILITE`) ; là où les tuiles sont chargées, le programme du terrain
//! l'efface (`Scene::trou`).
//!
//! Ses coins sont ceux des tuiles du générateur : au bord des tuiles chargées, les deux
//! se rejoignent (à leurs retouches près). Le fond de la mer au large n'y est pas : l'eau
//! le cache.

use crate::campagne::Campagne;
use crate::moteur::tuiles::DEMI_CARTE;
use crate::moteur::TAILLE;

/// L'identifiant de son maillage : « tuile/ », il se dessine en topologie avec le relief.
pub const ID: &str = "tuile/lointain";
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
    }
}
