//! La végétation d'un lieu réel : des arbres, plantés tuile par tuile selon l'occupation
//! du sol (`Reel::occupation`), serrés dans les bois, épars dans les prés, les parcs, les
//! jardins des quartiers, rares dans les champs.
//!
//! La tuile se découpe en cellules de `CELLULE` mètres ; chacune tire au sort (un hachage
//! de sa place sur la carte : toujours le même arbre au même endroit) si elle porte un
//! arbre, où, de quelle essence et de quelle taille. Un arbre ne pousse ni dans l'eau, ni
//! sur une chaussée, un bâtiment ou un pont (`Reel::libre`).
//!
//! Deux essences en facettes : un feuillu (tronc et houppier en boule bosselée) et un
//! conifère (tronc et deux cônes). Seuls les troncs arrêtent ; on passe sous les
//! branches.
//!
//! Au loin, les bois sont une canopée (`lointain`) : une nappe sur une grille de 32 m,
//! à la hauteur des houppiers au-dessus du sol, qui retombe au sol en lisière et dans les
//! clairières, d'un vert tacheté. Elle s'efface sous les tuiles chargées, comme le relief
//! lointain.

use crate::campagne::{bruit, hache, Campagne};
use crate::moteur::tuiles::DEMI_CARTE;
use crate::moteur::TAILLE;
use crate::reel::{Reel, BOIS, CHAMP, HABITE, PRE, ZONE};

/// Le côté d'une cellule (m) : dix par tuile.
const CELLULE: f32 = TAILLE / 10.0;
/// Un tronc reste à tant de mètres d'une chaussée, d'un mur ou d'un pont.
const MARGE: f32 = 1.5;
const GRAINE: u32 = 0x7EE5;
const ECORCE: [f32; 3] = [0.34, 0.26, 0.18];
/// Le maillage de la canopée lointaine : « bati/ », il ne se dessine pas en topologie.
pub const ID_LOINTAIN: &str = "bati/bois-lointains";
/// Le pas de la canopée (m).
pub const PAS_CANOPEE: f32 = TAILLE / 2.0;

/// La densité des bois en (x, z) : moins dans les clairières.
fn densite_bois(x: f32, z: f32) -> f32 {
    0.85 * (0.55 + 0.6 * bruit(GRAINE, x as f64 / 90.0, z as f64 / 90.0) as f32).min(1.0)
}

/// Les points de la canopée par côté, sur toute la carte.
pub fn cote_canopee() -> usize {
    ((2 * DEMI_CARTE + 1) as f32 * TAILLE / PAS_CANOPEE) as usize + 1
}

/// Le point de canopée `a` (x ou z absolu).
pub fn coin_canopee(a: usize) -> f32 {
    -(DEMI_CARTE as f32) * TAILLE + a as f32 * PAS_CANOPEE
}

/// La canopée lointaine des bois d'un lieu réel (rien sinon), en triangles : position
/// absolue, normale, couleur.
pub fn lointain(c: &Campagne) -> Vec<f32> {
    if let Some(m) = &c.monde {
        return (*m.silhouettes().bois).clone();
    }
    let n = cote_canopee();
    lointain_zone(c, 0..n - 1, 0..n - 1)
}

/// La canopée des carrés (a, b) de la grille de la canopée dans ces plages : morceau par
/// morceau, le même résultat que d'un seul tenant.
pub fn lointain_zone(c: &Campagne, qa: std::ops::Range<usize>, qb: std::ops::Range<usize>) -> Vec<f32> {
    let Some(r) = &c.reel else { return Vec::new() };
    if r.sol.is_empty() || qa.is_empty() || qb.is_empty() {
        return Vec::new();
    }
    let n = cote_canopee();
    // En chaque point de la fenêtre (les carrés et leurs voisins) : la part de bois
    // alentour (un carré de 32 m), la hauteur des houppiers et le sol
    let (a0, b0) = (qa.start.saturating_sub(1), qb.start.saturating_sub(1));
    let (a1, b1) = ((qa.end + 1).min(n - 1), (qb.end + 1).min(n - 1));
    let w = a1 - a0 + 1;
    let mut part = vec![0f32; w * (b1 - b0 + 1)];
    let mut haut = vec![0f32; part.len()];
    for b in b0..=b1 {
        for a in a0..=a1 {
            let (x, z) = (coin_canopee(a), coin_canopee(b));
            let f = r.part_sol(BOIS, x as f64, z as f64, PAS_CANOPEE as f64 / 2.0) as f32;
            let f = f * ((densite_bois(x, z) - 0.25) / 0.45).clamp(0.0, 1.0);
            let sol = c.hauteur(x as f64, z as f64) as f32;
            let h = 12.0 + 6.0 * bruit(GRAINE + 1, x as f64 / 120.0, z as f64 / 120.0) as f32;
            part[(b - b0) * w + a - a0] = f;
            haut[(b - b0) * w + a - a0] = sol + if f > 0.0 { 1.0 + h * f.sqrt() } else { 0.0 };
        }
    }
    let i = |a: usize, b: usize| (b.clamp(b0, b1) - b0) * w + a.clamp(a0, a1) - a0;
    let at = |a: usize, b: usize| haut[i(a.min(n - 1), b.min(n - 1))];
    let mut v = Vec::new();
    for b in qb {
        for a in qa.clone() {
            let q = [(a, b), (a + 1, b), (a, b + 1), (a + 1, b + 1)];
            if q.iter().all(|&(a, b)| part[i(a, b)] < 0.02) {
                continue;
            }
            for &(a, b) in &[q[0], q[3], q[1], q[0], q[2], q[3]] {
                let (x, z) = (coin_canopee(a), coin_canopee(b));
                let dx = (at(a + 1, b) - at(a.saturating_sub(1), b)) / (2.0 * PAS_CANOPEE);
                let dz = (at(a, b + 1) - at(a, b.saturating_sub(1))) / (2.0 * PAS_CANOPEE);
                let l = (dx * dx + dz * dz + 1.0).sqrt();
                let t = hache(GRAINE + 2, a as i64, b as i64) as f32;
                let s = bruit(GRAINE + 3, x as f64 / 200.0, z as f64 / 200.0) as f32;
                let vert = [0.16 + 0.07 * t, 0.27 + 0.08 * t - 0.04 * s, 0.13 + 0.03 * t];
                v.extend_from_slice(&[x, at(a, b), z, -dx / l, 1.0 / l, -dz / l]);
                v.extend_from_slice(&vert);
            }
        }
    }
    v
}

/// Un arbre planté : sa cellule (`gi`, `gj`, sur toute la carte), sa place (absolue), son
/// essence, sa hauteur, sa teinte (0–1) et son tour (`cap`, en sixièmes de tour, 0–1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arbre {
    pub gi: i64,
    pub gj: i64,
    pub pos: [f32; 3],
    pub conifere: bool,
    pub hauteur: f32,
    pub teinte: f32,
    pub cap: f32,
}

/// Les arbres de la tuile (i, j), comme `Reel::bati` : les sommets (position depuis le
/// coin de la tuile, normale, couleur) et les triangles qui arrêtent (les troncs).
pub fn arbres(c: &Campagne, i: i32, j: i32) -> (Vec<f32>, Vec<[[f32; 3]; 3]>) {
    match &c.reel {
        Some(r) => monter(&plantes(r, i, j), i, j),
        None => (Vec::new(), Vec::new()),
    }
}

/// Les arbres que porte la tuile (i, j) d'un lieu réel, dans l'ordre des cellules.
pub fn plantes(r: &Reel, i: i32, j: i32) -> Vec<Arbre> {
    let mut l = Vec::new();
    let (ox, oz) = (i as f32 * TAILLE, j as f32 * TAILLE);
    let n = (TAILLE / CELLULE).round() as i64;
    for b in 0..n {
        for a in 0..n {
            let (gi, gj) = (i as i64 * n + a, j as i64 * n + b);
            let h = |k: u32| hache(GRAINE + k, gi, gj) as f32;
            let x = ox + (a as f32 + 0.15 + 0.7 * h(1)) * CELLULE;
            let z = oz + (b as f32 + 0.15 + 0.7 * h(2)) * CELLULE;
            // La classe qui pèse le plus là, et sa densité (la part de cellules boisées)
            let classe = r.occupation(x as f64, z as f64).iter().fold((0u8, -1.0), |m, &(k, p)| if p > m.1 { (k, p) } else { m }).0;
            let (densite, taille) = match classe {
                // Des clairières dans les bois
                BOIS => (densite_bois(x, z), 1.0),
                PRE => (0.06, 0.85),
                HABITE => (0.08, 0.7),
                ZONE => (0.015, 0.7),
                CHAMP => (0.006, 1.0),
                _ => (0.04, 0.85),
            };
            if h(0) >= densite || !r.libre(x as f64, z as f64, MARGE) {
                continue;
            }
            let y = r.hauteur(x as f64, z as f64) as f32;
            let coniferes = if classe == BOIS { 0.3 } else { 0.12 };
            let conifere = h(3) < coniferes;
            let hauteur = if conifere { (12.0 + 10.0 * h(4)) * taille } else { (9.0 + 8.0 * h(4)) * taille };
            l.push(Arbre { gi, gj, pos: [x, y, z], conifere, hauteur, teinte: h(5), cap: h(6) });
        }
    }
    l
}

/// Les maillages d'arbres de la tuile (i, j) : sommets depuis le coin de la tuile, et les
/// troncs qui arrêtent.
pub fn monter(arbres: &[Arbre], i: i32, j: i32) -> (Vec<f32>, Vec<[[f32; 3]; 3]>) {
    let (mut v, mut t) = (Vec::new(), Vec::new());
    let (ox, oz) = (i as f32 * TAILLE, j as f32 * TAILLE);
    for a in arbres {
        let p = [a.pos[0] - ox, a.pos[1], a.pos[2] - oz];
        if a.conifere {
            conifere(&mut v, &mut t, p, a.hauteur, a.cap, a.teinte);
        } else {
            let (gi, gj) = (a.gi, a.gj);
            feuillu(&mut v, &mut t, p, a.hauteur, a.cap, a.teinte, |k| hache(GRAINE + 20 + k, gi, gj) as f32);
        }
    }
    (v, t)
}

/// Un triangle à facette : sa normale tournée à l'opposé du point `dedans`.
fn facette(v: &mut Vec<f32>, mut q: [[f32; 3]; 3], dedans: [f32; 3], couleur: [f32; 3]) {
    let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let (u, w) = (sub(q[1], q[0]), sub(q[2], q[0]));
    let mut n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
    let m = [(q[0][0] + q[1][0] + q[2][0]) / 3.0, (q[0][1] + q[1][1] + q[2][1]) / 3.0, (q[0][2] + q[1][2] + q[2][2]) / 3.0];
    let d = sub(m, dedans);
    if n[0] * d[0] + n[1] * d[1] + n[2] * d[2] < 0.0 {
        q.swap(1, 2);
        n = n.map(|x| -x);
    }
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if l < 1e-9 {
        return;
    }
    let n = n.map(|x| x / l);
    for p in q {
        v.extend_from_slice(&p);
        v.extend_from_slice(&n);
        v.extend_from_slice(&couleur);
    }
}

/// Un anneau de six points de rayon `r` à la hauteur `y` autour de `p`, tourné de `a`
/// (en sixièmes de tour).
fn anneau(p: [f32; 3], y: f32, r: f32, a: f32) -> [[f32; 3]; 6] {
    std::array::from_fn(|k| {
        let t = (k as f32 + a) * std::f32::consts::TAU / 6.0;
        [p[0] + r * t.cos(), y, p[2] + r * t.sin()]
    })
}

/// Le tronc, du sol (un demi-mètre dessous) à `haut` : ses six faces, qui arrêtent.
fn tronc(v: &mut Vec<f32>, t: &mut Vec<[[f32; 3]; 3]>, p: [f32; 3], haut: f32, r: f32, a: f32) {
    let (bas, dessus) = (anneau(p, p[1] - 0.5, r, a), anneau(p, p[1] + haut, r * 0.7, a));
    for k in 0..6 {
        let l = (k + 1) % 6;
        for q in [[bas[k], bas[l], dessus[l]], [bas[k], dessus[l], dessus[k]]] {
            let dedans = [p[0], (q[0][1] + q[1][1] + q[2][1]) / 3.0, p[2]];
            facette(v, q, dedans, ECORCE);
            t.push(q);
        }
    }
}

/// Un feuillu de `haut` mètres : un tronc, un houppier en boule à facettes, bosselé
/// (`h` : un hasard par sommet).
fn feuillu(v: &mut Vec<f32>, t: &mut Vec<[[f32; 3]; 3]>, p: [f32; 3], haut: f32, a: f32, teinte: f32, h: impl Fn(u32) -> f32) {
    let r = haut * 0.3;
    let cy = p[1] + haut - r * 0.95;
    tronc(v, t, p, cy - p[1], 0.12 + haut * 0.018, a);
    let vert = [0.20 + 0.08 * teinte, 0.34 + 0.08 * teinte, 0.13 + 0.03 * teinte];
    let mut bas = anneau(p, cy - r * 0.3, r, a);
    let mut haut_ = anneau(p, cy + r * 0.4, r * 0.8, a + 0.5);
    for (k, q) in bas.iter_mut().chain(haut_.iter_mut()).enumerate() {
        let s = 0.85 + 0.3 * h(k as u32);
        q[0] = p[0] + (q[0] - p[0]) * s;
        q[2] = p[2] + (q[2] - p[2]) * s;
    }
    let (sous, sommet) = ([p[0], cy - r * 0.8, p[2]], [p[0], cy + r * 0.95, p[2]]);
    let centre = [p[0], cy, p[2]];
    for k in 0..6 {
        let l = (k + 1) % 6;
        facette(v, [sous, bas[l], bas[k]], centre, vert);
        facette(v, [bas[k], bas[l], haut_[k]], centre, vert);
        facette(v, [bas[l], haut_[l], haut_[k]], centre, vert);
        facette(v, [haut_[k], haut_[l], sommet], centre, vert);
    }
}

/// Un conifère de `haut` mètres : un tronc et deux cônes.
fn conifere(v: &mut Vec<f32>, t: &mut Vec<[[f32; 3]; 3]>, p: [f32; 3], haut: f32, a: f32, teinte: f32) {
    let r = haut * 0.2;
    tronc(v, t, p, haut * 0.35, 0.12 + haut * 0.015, a);
    let vert = [0.12 + 0.05 * teinte, 0.24 + 0.06 * teinte, 0.14 + 0.03 * teinte];
    for (base, rayon, pointe, tour) in [(0.22, r, 0.72, a), (0.5, r * 0.72, 1.0, a + 0.5)] {
        let y = p[1] + haut * base;
        let bord = anneau(p, y, rayon, tour);
        let sommet = [p[0], p[1] + haut * pointe, p[2]];
        let dedans = [p[0], y + (sommet[1] - y) * 0.3, p[2]];
        for k in 0..6 {
            let l = (k + 1) % 6;
            facette(v, [bord[k], bord[l], sommet], dedans, vert);
            facette(v, [bord[k], bord[l], [p[0], y, p[2]]], [p[0], y + 1.0, p[2]], vert);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reel::{cote, cote_sol, Reel};
    use std::sync::Arc;

    fn lieu(classe: u8, eau: bool) -> Campagne {
        let pas = 64.0;
        let n = cote(pas);
        let m = cote_sol(n);
        let r = Reel::new("Essai", 49.0, 1.0, 0.0, 0.0, pas, if eau { 5.0 } else { -50.0 }, &vec![0.0; n * n], &vec![eau; n * n], Vec::new(), Vec::new(), Vec::new()).avec_sol(vec![classe; m * m]);
        Campagne::reelle(Arc::new(r))
    }

    #[test]
    fn un_bois_est_serre_un_champ_presque_nu() {
        let bois = lieu(BOIS, false);
        let (v, t) = arbres(&bois, 0, 0);
        assert_eq!(v.len() % 27, 0);
        // Douze faces de tronc par arbre, qui arrêtent
        let n = t.len() / 12;
        assert!(n > 25, "{n} arbres");
        assert_eq!(arbres(&bois, 0, 0), (v.clone(), t.clone()), "toujours les mêmes");
        let champs: usize = (0..4).map(|i| arbres(&lieu(CHAMP, false), i, 0).1.len() / 12).sum();
        assert!(champs < 5, "{champs}");
        // Rien dans l'eau, rien hors d'un lieu réel
        assert!(arbres(&lieu(BOIS, true), 0, 0).0.is_empty());
        assert!(arbres(&Campagne::brute(1), 0, 0).0.is_empty());
        // Au loin, une canopée au-dessus du sol, sauf au-dessus d'un champ
        let l = lointain(&bois);
        assert!(!l.is_empty() && l.len() % 27 == 0);
        assert!(l.chunks(9).any(|s| s[1] > 10.0) && l.chunks(9).all(|s| s[4] > 0.0));
        assert!(lointain(&lieu(CHAMP, false)).is_empty());
        assert!(lointain(&Campagne::brute(1)).is_empty());
        // Les normales regardent dehors : vers le haut au sommet des houppiers
        let haut = v.chunks(9).map(|s| s[1]).fold(f32::MIN, f32::max);
        assert!(haut > 8.0 && haut < 23.0, "{haut}");
        for s in v.chunks(9).filter(|s| s[1] == haut) {
            assert!(s[4] > 0.0);
        }
    }
}
