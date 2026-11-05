//! La topologie à la main : un pinceau rond qui monte, creuse, aplanit, adoucit ou peint
//! le relief des tuiles chargées.
//!
//! Le pinceau agit sur les points de la grille sous son disque, d'autant plus fort qu'ils
//! sont près du centre. Un point du bord d'une tuile existe aussi dans sa voisine : les
//! deux reçoivent le même calcul, fait sur l'état d'avant le coup, et restent égaux.

use crate::moteur::{Maillages, Monde, Relief, Tuiles, TAILLE};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Outil {
    Monter,
    Creuser,
    Aplanir,
    Adoucir,
    Peindre,
}

impl Outil {
    pub const TOUS: [Outil; 5] = [Outil::Monter, Outil::Creuser, Outil::Aplanir, Outil::Adoucir, Outil::Peindre];

    pub fn nom(self) -> &'static str {
        match self {
            Outil::Monter => "Monter",
            Outil::Creuser => "Creuser",
            Outil::Aplanir => "Aplanir",
            Outil::Adoucir => "Adoucir",
            Outil::Peindre => "Peindre",
        }
    }
}

/// Les couleurs du sol proposées : celles de la campagne, et quelques autres.
pub const TEINTES: [[f32; 3]; 8] = [
    [0.50, 0.62, 0.40],
    [0.66, 0.66, 0.46],
    [0.42, 0.52, 0.32],
    [0.58, 0.56, 0.52],
    [0.55, 0.44, 0.32],
    [0.80, 0.74, 0.56],
    [0.45, 0.45, 0.47],
    [0.92, 0.93, 0.95],
];

/// Monter ou creuser à pleine force : tant de mètres par seconde au centre.
pub const VITESSE: f32 = 2.0;
/// Aplanir, adoucir, peindre à pleine force : la part du chemin faite par seconde au centre.
pub const PRISE: f32 = 4.0;
/// Les bornes du rayon, en mètres.
pub const RAYON_MIN: f32 = 2.0;
pub const RAYON_MAX: f32 = 24.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Pinceau {
    /// Rien : le doigt tourne la vue.
    pub outil: Option<Outil>,
    pub rayon: f32,
    /// De 0 à 1.
    pub force: f32,
    pub couleur: [f32; 3],
    /// La hauteur où « Aplanir » ramène le sol : celle du premier point du coup.
    pub niveau: Option<f32>,
}

impl Default for Pinceau {
    fn default() -> Pinceau {
        Pinceau { outil: None, rayon: 6.0, force: 0.5, couleur: TEINTES[0], niveau: None }
    }
}

/// Le poids du pinceau à `d` de son centre : 1 au centre, 0 au bord, en douceur.
pub fn poids(d: f32, rayon: f32) -> f32 {
    let t = (d / rayon.max(1e-3)).clamp(0.0, 1.0);
    let u = 1.0 - t * t;
    u * u
}

/// Les tuiles chargées que touche le disque de centre `c` et de rayon `r`.
fn touchees(monde: &Monde, tuiles: &Tuiles, c: [f32; 3], r: f32) -> Vec<(i32, i32)> {
    let mut v: Vec<(i32, i32)> = tuiles
        .chargees
        .keys()
        .copied()
        .filter(|&k| {
            let o = Tuiles::coin(monde, k);
            let (x, z) = (c[0].clamp(o[0], o[0] + TAILLE), c[2].clamp(o[2], o[2] + TAILLE));
            (x - c[0]).hypot(z - c[2]) < r
        })
        .collect();
    v.sort();
    v
}

/// Ce qu'un coup change : pour chaque tuile, son relief et ses couleurs d'avant.
pub type Avant = Vec<((i32, i32), Relief, Vec<[f32; 3]>)>;

/// Un coup de pinceau de `dt` secondes en `c` (au repère du monde). Rend les tuiles
/// touchées telles qu'elles étaient avant lui.
pub fn appliquer(monde: &mut Monde, maillages: &mut Maillages, tuiles: &mut Tuiles, p: &Pinceau, c: [f32; 3], dt: f32) -> Avant {
    let Some(outil) = p.outil else { return Vec::new() };
    let r = p.rayon;
    let ks = touchees(monde, tuiles, c, r);
    // Tout se calcule sur l'état d'avant le coup, pour toutes les tuiles
    let mut avant: Avant = Vec::new();
    let mut hauteurs: HashMap<(i64, i64), f32> = HashMap::new();
    for &k in &ks {
        let Some(t) = tuiles.chargees.get(&k) else { continue };
        let (Some(rel), Some(cl)) = (monde.relief.get(t.terrain), tuiles.couleurs(k)) else { continue };
        let o = Tuiles::coin(monde, k);
        let pas = rel.pas();
        for b in 0..=rel.n {
            for a in 0..=rel.n {
                let cle = ((((o[0] + a as f32 * pas) / pas).round()) as i64, (((o[2] + b as f32 * pas) / pas).round()) as i64);
                hauteurs.insert(cle, rel.sommet(a, b));
            }
        }
        avant.push((k, rel.clone(), cl.to_vec()));
    }
    for (k, rel, cl) in &avant {
        let o = Tuiles::coin(monde, *k);
        let pas = rel.pas();
        let (mut h, mut couleurs) = (rel.h.clone(), cl.clone());
        let mut change = false;
        for b in 0..=rel.n {
            for a in 0..=rel.n {
                let (x, z) = (o[0] + a as f32 * pas, o[2] + b as f32 * pas);
                let w = poids((x - c[0]).hypot(z - c[2]), r) * p.force;
                if w <= 0.0 {
                    continue;
                }
                let i = b * (rel.n + 1) + a;
                let prise = (w * PRISE * dt).min(1.0);
                match outil {
                    Outil::Monter => h[i] += w * VITESSE * dt,
                    Outil::Creuser => h[i] -= w * VITESSE * dt,
                    Outil::Aplanir => h[i] += (p.niveau.unwrap_or(c[1]) - h[i]) * prise,
                    Outil::Adoucir => {
                        // La moyenne des voisins, pris dans les tuiles d'à côté s'il le faut
                        let (ga, gb) = (((x / pas).round()) as i64, ((z / pas).round()) as i64);
                        let v: Vec<f32> = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().filter_map(|(da, db)| hauteurs.get(&(ga + da, gb + db)).copied()).collect();
                        if !v.is_empty() {
                            let m = v.iter().sum::<f32>() / v.len() as f32;
                            h[i] += (m - h[i]) * prise;
                        }
                    }
                    Outil::Peindre => {
                        let x = &mut couleurs[i];
                        *x = [0, 1, 2].map(|j| x[j] + (p.couleur[j] - x[j]) * prise);
                    }
                }
                change = true;
            }
        }
        if change {
            tuiles.retoucher(monde, maillages, *k, Relief { n: rel.n, h }, couleurs);
        }
    }
    avant
}

/// Remet les tuiles comme elles étaient, celles encore chargées.
pub fn remettre(monde: &mut Monde, maillages: &mut Maillages, tuiles: &mut Tuiles, avant: &Avant) {
    for (k, rel, cl) in avant {
        tuiles.retoucher(monde, maillages, *k, rel.clone(), cl.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::moteur::{systemes, Contenu, Source};

    /// Un sol plat à 0 : 2 m de pas, comme la campagne.
    struct Plat;
    impl Source for Plat {
        fn tuile(&self, _: i32, _: i32) -> Contenu {
            let n = 32;
            let r = Relief { n, h: vec![0.0; (n + 1) * (n + 1)] };
            Contenu { normales: r.normales(), relief: r, couleurs: vec![[0.5; 3]; (n + 1) * (n + 1)], objets: vec![] }
        }
    }

    fn monde() -> (Monde, Maillages, Tuiles) {
        let mut t = Tuiles::new(Box::new(Plat), 1, 1);
        let (mut m, mut r) = (Monde::default(), Maillages::default());
        t.suivre(&mut m, &mut r, (0, 0), usize::MAX);
        (m, r, t)
    }

    fn sommet(m: &Monde, t: &Tuiles, k: (i32, i32), a: usize, b: usize) -> f32 {
        m.relief.get(t.chargees[&k].terrain).unwrap().sommet(a, b)
    }

    #[test]
    fn monter_sur_un_bord_garde_les_tuiles_jointes() {
        let (mut m, mut r, mut t) = monde();
        let p = Pinceau { outil: Some(Outil::Monter), force: 1.0, ..Default::default() };
        // Sur le bord entre (0, 0) et (1, 0)
        let avant = appliquer(&mut m, &mut r, &mut t, &p, [TAILLE, 0.0, 20.0], 0.5);
        assert_eq!(avant.len(), 2);
        let h = systemes::hauteur_sol(&m, TAILLE, 20.0).unwrap();
        assert!((h - VITESSE * 0.5).abs() < 1e-4, "{h}");
        for b in 0..=32 {
            assert_eq!(sommet(&m, &t, (0, 0), 32, b), sommet(&m, &t, (1, 0), 0, b), "bord {b}");
        }
        // Au bord du disque, rien
        assert_eq!(systemes::hauteur_sol(&m, TAILLE, 20.0 + p.rayon + 0.1), Some(0.0));
        assert_eq!(t.a_ecrire(), 2);
        // Défait
        remettre(&mut m, &mut r, &mut t, &avant);
        assert_eq!(systemes::hauteur_sol(&m, TAILLE, 20.0), Some(0.0));
    }

    #[test]
    fn aplanir_adoucir_et_peindre() {
        let (mut m, mut r, mut t) = monde();
        let mut p = Pinceau { outil: Some(Outil::Monter), force: 1.0, rayon: 8.0, ..Default::default() };
        appliquer(&mut m, &mut r, &mut t, &p, [32.0, 0.0, 32.0], 1.0);
        let bosse = systemes::hauteur_sol(&m, 32.0, 32.0).unwrap();
        // Adoucir : la bosse baisse, sans creuser
        p.outil = Some(Outil::Adoucir);
        appliquer(&mut m, &mut r, &mut t, &p, [32.0, 0.0, 32.0], 0.1);
        let doux = systemes::hauteur_sol(&m, 32.0, 32.0).unwrap();
        assert!(doux < bosse && doux > 0.0, "{bosse} → {doux}");
        // Aplanir vers 0.5 : longtemps, et l'on y est
        p.outil = Some(Outil::Aplanir);
        p.niveau = Some(0.5);
        for _ in 0..40 {
            appliquer(&mut m, &mut r, &mut t, &p, [32.0, 0.0, 32.0], 0.1);
        }
        assert!((systemes::hauteur_sol(&m, 32.0, 32.0).unwrap() - 0.5).abs() < 0.01);
        // Peindre
        p.outil = Some(Outil::Peindre);
        p.couleur = [1.0, 0.0, 0.0];
        for _ in 0..40 {
            appliquer(&mut m, &mut r, &mut t, &p, [32.0, 0.0, 32.0], 0.1);
        }
        let c = t.couleurs((0, 0)).unwrap()[16 * 33 + 16];
        assert!(c[0] > 0.95 && c[1] < 0.05, "{c:?}");
        // Sans outil, rien
        p.outil = None;
        assert!(appliquer(&mut m, &mut r, &mut t, &p, [32.0, 0.0, 32.0], 0.1).is_empty());
    }
}
