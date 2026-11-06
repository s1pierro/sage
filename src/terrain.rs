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

/// Un terrassement : une plateforme de niveau sous l'emprise d'un objet, en remblai,
/// et la fosse d'un sous-sol si l'on en veut un.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Terrasse {
    /// Le lieu de l'objet (au repère du monde) et son cap.
    pub centre: [f32; 3],
    pub cap: f32,
    /// Son emprise dans son repère : (x, z) bas et haut.
    pub lo: [f32; 2],
    pub hi: [f32; 2],
    /// La plateforme déborde de l'emprise de tant, de chaque côté.
    pub marge: f32,
    /// La pente du talus, en degrés.
    pub talus: f32,
    /// La profondeur de la fosse sous la plateforme, s'il y a un sous-sol.
    pub sous_sol: Option<f32>,
}

/// Le talus ne va pas plus loin que cela de la plateforme.
pub const TALUS_MAX: f32 = 80.0;
/// La terre remuée, sa couleur.
pub const TERRE: [f32; 3] = [0.55, 0.45, 0.33];

impl Terrasse {
    /// (x, z) du monde dans le repère de l'objet.
    fn local(&self, x: f32, z: f32) -> [f32; 2] {
        let p = crate::moteur::objets::tourner([x - self.centre[0], 0.0, z - self.centre[2]], -self.cap);
        [p[0], p[2]]
    }

    /// La distance de `l` (repère de l'objet) à l'emprise élargie de `m`.
    fn hors(&self, l: [f32; 2], m: f32) -> f32 {
        let dx = (self.lo[0] - m - l[0]).max(l[0] - self.hi[0] - m).max(0.0);
        let dz = (self.lo[1] - m - l[1]).max(l[1] - self.hi[1] - m).max(0.0);
        dx.hypot(dz)
    }

    /// Le rayon d'un cercle qui contient la plateforme.
    fn rayon(&self) -> f32 {
        let m = self.marge;
        [self.lo[0] - m, self.hi[0] + m].iter().flat_map(|x| [self.lo[1] - m, self.hi[1] + m].map(|z| x.hypot(z))).fold(0.0, f32::max)
    }
}

/// Terrasse sous un objet : la plateforme, au niveau du point le plus haut du terrain
/// sous elle, est remblayée et rejoint le terrain par un talus ; avec un sous-sol, une
/// fosse au fond plat se creuse sous l'emprise. Le relief est une grille de 2 m : les
/// bords de la fosse sont en retrait d'un pas à l'intérieur de l'emprise, pour que sa
/// pente reste sous l'objet. Rend les tuiles d'avant et le niveau de la plateforme.
pub fn terrasser(monde: &mut Monde, maillages: &mut Maillages, tuiles: &mut Tuiles, t: &Terrasse) -> (Avant, f32) {
    let ks = touchees(monde, tuiles, t.centre, t.rayon() + TALUS_MAX);
    let mut avant: Avant = Vec::new();
    for &k in &ks {
        let Some(c) = tuiles.chargees.get(&k) else { continue };
        let (Some(rel), Some(cl)) = (monde.relief.get(c.terrain), tuiles.couleurs(k)) else { continue };
        avant.push((k, rel.clone(), cl.to_vec()));
    }
    // Le niveau : le plus haut sous la plateforme, ses coins compris
    let mut niveau = f32::MIN;
    for (k, rel, _) in &avant {
        let o = Tuiles::coin(monde, *k);
        let pas = rel.pas();
        for b in 0..=rel.n {
            for a in 0..=rel.n {
                if t.hors(t.local(o[0] + a as f32 * pas, o[2] + b as f32 * pas), t.marge) == 0.0 {
                    niveau = niveau.max(rel.sommet(a, b));
                }
            }
        }
    }
    let m = t.marge;
    for x in [t.lo[0] - m, 0.5 * (t.lo[0] + t.hi[0]), t.hi[0] + m] {
        for z in [t.lo[1] - m, 0.5 * (t.lo[1] + t.hi[1]), t.hi[1] + m] {
            let p = crate::moteur::objets::tourner([x, 0.0, z], t.cap);
            if let Some(h) = crate::moteur::systemes::hauteur_sol(monde, t.centre[0] + p[0], t.centre[2] + p[2]) {
                niveau = niveau.max(h);
            }
        }
    }
    if niveau == f32::MIN {
        return (Vec::new(), t.centre[1]);
    }
    let pente = t.talus.clamp(5.0, 85.0).to_radians().tan();
    for (k, rel, cl) in &avant {
        let o = Tuiles::coin(monde, *k);
        let pas = rel.pas();
        let (mut h, mut couleurs) = (rel.h.clone(), cl.clone());
        let mut change = false;
        for b in 0..=rel.n {
            for a in 0..=rel.n {
                let l = t.local(o[0] + a as f32 * pas, o[2] + b as f32 * pas);
                let i = b * (rel.n + 1) + a;
                let d = t.hors(l, t.marge);
                let mut y = if d == 0.0 { niveau } else { h[i].max(niveau - d * pente) };
                if let Some(p) = t.sous_sol {
                    if t.hors(l, -pas) == 0.0 {
                        y = niveau - p;
                    }
                }
                if (y - h[i]).abs() > 1e-4 {
                    let w = ((y - h[i]).abs() * 0.7).min(0.7);
                    let x = &mut couleurs[i];
                    *x = [0, 1, 2].map(|j| x[j] + (TERRE[j] - x[j]) * w);
                    h[i] = y;
                    change = true;
                }
            }
        }
        if change {
            tuiles.retoucher(monde, maillages, *k, Relief { n: rel.n, h }, couleurs);
        }
    }
    (avant, niveau)
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
    fn terrasser_en_remblai_avec_ou_sans_sous_sol() {
        let (mut m, mut r, mut t) = monde();
        // Une bosse de 2 m sous un coin de l'emprise
        let p = Pinceau { outil: Some(Outil::Monter), force: 1.0, rayon: 4.0, ..Default::default() };
        appliquer(&mut m, &mut r, &mut t, &p, [34.0, 0.0, 34.0], 1.0);
        let haut = systemes::hauteur_sol(&m, 34.0, 34.0).unwrap();
        let mut x = Terrasse { centre: [32.0, 0.0, 32.0], cap: 0.3, lo: [-5.0, -4.0], hi: [5.0, 4.0], marge: 1.0, talus: 35.0, sous_sol: None };
        let (avant, niveau) = terrasser(&mut m, &mut r, &mut t, &x);
        assert!(niveau >= haut - 1e-4, "{niveau} {haut}");
        // La plateforme est de niveau, le talus descend, au loin rien ne bouge
        for (px, pz) in [(32.0, 32.0), (28.0, 30.0), (35.0, 33.0)] {
            assert!((systemes::hauteur_sol(&m, px, pz).unwrap() - niveau).abs() < 1e-3);
        }
        let talus = systemes::hauteur_sol(&m, 32.0, 32.0 + 7.5).unwrap();
        assert!(talus > 0.0 && talus < niveau, "{talus}");
        assert_eq!(systemes::hauteur_sol(&m, 32.0, 32.0 + 7.0 + niveau / 0.7 + 4.0), Some(0.0));
        // Défait, puis avec un sous-sol de 3 m
        remettre(&mut m, &mut r, &mut t, &avant);
        x.sous_sol = Some(3.0);
        let (_, niveau) = terrasser(&mut m, &mut r, &mut t, &x);
        assert!((systemes::hauteur_sol(&m, 32.0, 32.0).unwrap() - (niveau - 3.0)).abs() < 1e-3);
        // Hors de l'emprise, la plateforme
        let bord = crate::moteur::objets::tourner([4.6, 0.0, 0.0], 0.3);
        let h = systemes::hauteur_sol(&m, 32.0 + bord[0], 32.0 + bord[2]).unwrap();
        assert!(h > niveau - 0.3 && h < niveau + 1e-3, "{h} {niveau}");
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
