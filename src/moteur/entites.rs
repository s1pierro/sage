//! Le monde : des entités, désignées par des poignées, et leurs composants rangés dans
//! une table par sorte.
//!
//! Une poignée porte une génération : quand une entité est détruite, son emplacement
//! peut resservir, mais les anciennes poignées ne désignent plus rien.

use super::ressources::MaillageId;
use super::tuiles::{Relief, TAILLE};
use crate::math::Mat4;
use crate::personnage::Modele;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Entite {
    pub indice: u32,
    pub generation: u32,
}

/// Une table de composants, indexée par l'indice des entités.
pub struct Stock<T> {
    v: Vec<Option<(u32, T)>>,
}

impl<T> Default for Stock<T> {
    fn default() -> Self {
        Stock { v: Vec::new() }
    }
}

impl<T> Stock<T> {
    pub fn mettre(&mut self, e: Entite, c: T) {
        let i = e.indice as usize;
        if self.v.len() <= i {
            self.v.resize_with(i + 1, || None);
        }
        self.v[i] = Some((e.generation, c));
    }

    pub fn ote(&mut self, e: Entite) -> Option<T> {
        let s = self.v.get_mut(e.indice as usize)?;
        if s.as_ref().is_some_and(|(g, _)| *g == e.generation) {
            s.take().map(|x| x.1)
        } else {
            None
        }
    }

    pub fn get(&self, e: Entite) -> Option<&T> {
        match self.v.get(e.indice as usize)? {
            Some((g, c)) if *g == e.generation => Some(c),
            _ => None,
        }
    }

    pub fn get_mut(&mut self, e: Entite) -> Option<&mut T> {
        match self.v.get_mut(e.indice as usize)? {
            Some((g, c)) if *g == e.generation => Some(c),
            _ => None,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (Entite, &T)> {
        self.v.iter().enumerate().filter_map(|(i, s)| s.as_ref().map(|(g, c)| (Entite { indice: i as u32, generation: *g }, c)))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Entite, &mut T)> {
        self.v.iter_mut().enumerate().filter_map(|(i, s)| s.as_mut().map(|(g, c)| (Entite { indice: i as u32, generation: *g }, c)))
    }
}

/// Où se tient une entité : position au sol, cap (0 : vers +Z), et la même chose au pas
/// précédent, pour interpoler entre deux pas de simulation.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Lieu {
    pub pos: [f32; 3],
    pub cap: f32,
    pub avant: ([f32; 3], f32),
}

impl Lieu {
    pub fn en(pos: [f32; 3], cap: f32) -> Lieu {
        Lieu { pos, cap, avant: (pos, cap) }
    }

    /// À appeler au début de chaque pas.
    pub fn retenir(&mut self) {
        self.avant = (self.pos, self.cap);
    }

    /// Position et cap entre le pas précédent (`u` = 0) et le dernier (`u` = 1).
    pub fn interpole(&self, u: f32) -> ([f32; 3], f32) {
        let (p, c) = self.avant;
        let mut e = self.cap - c;
        e -= std::f32::consts::TAU * (e / std::f32::consts::TAU).round();
        ([0, 1, 2].map(|k| p[k] + (self.pos[k] - p[k]) * u), c + e * u)
    }

    /// Repère de l'entité vers le monde.
    pub fn place(&self, u: f32) -> Mat4 {
        let (p, c) = self.interpole(u);
        let (s, co) = c.sin_cos();
        [co, 0., -s, 0., 0., 1., 0., 0., s, 0., co, 0., p[0], p[1], p[2], 1.]
    }
}

/// Ce qu'une entité dessine : un maillage, avec un repère local en plus (échelle,
/// décalage des pieds).
#[derive(Clone, Copy, Debug)]
pub struct Rendu {
    pub maillage: MaillageId,
    pub local: Mat4,
}

/// Un personnage animé : son modèle, ce qu'il sait jouer, où il en est, et les matrices
/// d'os de la dernière pose.
pub struct Squelette {
    pub modele: Arc<Modele>,
    pub locomotion: Arc<super::locomotion::Locomotion>,
    pub pas: super::locomotion::Pas,
    /// L'inclinaison du dos et le regard, posés sur l'animation.
    pub posture: super::posture::Posture,
    /// Le point que le personnage regarde, au monde.
    pub regard: Option<[f32; 3]>,
    /// Les pieds calés au sol, dernière couche de la pose.
    pub pieds: super::pieds::Pieds,
    pub os: Vec<Mat4>,
}

impl Squelette {
    pub fn nouveau(modele: Arc<Modele>) -> Squelette {
        let locomotion = Arc::new(super::locomotion::Locomotion::de(&modele.armature, modele.echelle as f64));
        let pieds = super::pieds::Pieds::de(&modele);
        let posture = super::posture::Posture::de(&modele);
        Squelette { modele, locomotion, pas: Default::default(), posture, regard: None, pieds, os: Vec::new() }
    }
}

/// Une boîte alignée sur les axes, dans le monde.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub centre: [f32; 3],
    pub demi: [f32; 3],
}

/// Ce qui se déplace de soi-même : vitesse au sol et direction du pas (unitaire).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mobile {
    pub vitesse: f32,
    pub elan: [f32; 2],
    /// Rayon du corps au sol.
    pub rayon: f32,
}

#[derive(Default)]
pub struct Monde {
    /// La tuile d'origine : toutes les positions lui sont relatives. La position absolue
    /// d'un point est `origine × TAILLE + pos`.
    pub origine: [i32; 2],
    generations: Vec<u32>,
    vivantes: Vec<bool>,
    libres: Vec<u32>,
    pub ident: Stock<String>,
    pub lieu: Stock<Lieu>,
    pub rendu: Stock<Rendu>,
    pub squelette: Stock<Squelette>,
    /// Formes à ne pas traverser, dans le repère du lieu de l'entité (sans rotation).
    pub obstacles: Stock<Vec<super::physique::Forme>>,
    pub mobile: Stock<Mobile>,
    /// Les mobiles qui ont un corps : la physique les déplace.
    pub corps: Stock<super::physique::Corps>,
    /// Le relief d'une tuile, dans le repère de son lieu.
    pub relief: Stock<Relief>,
    /// Les objets du décor : leur modèle et leurs réglages.
    pub objet: Stock<super::objets::Objet>,
    /// L'entité que l'éditeur a choisie : elle se dessine en surbrillance.
    pub choisie: Option<Entite>,
}

impl Monde {
    /// Une entité neuve, sous l'identifiant stable `id`.
    pub fn creer(&mut self, id: &str) -> Entite {
        let indice = match self.libres.pop() {
            Some(i) => i,
            None => {
                self.generations.push(0);
                self.vivantes.push(false);
                (self.generations.len() - 1) as u32
            }
        };
        self.vivantes[indice as usize] = true;
        let e = Entite { indice, generation: self.generations[indice as usize] };
        self.ident.mettre(e, id.to_string());
        e
    }

    pub fn vivante(&self, e: Entite) -> bool {
        let i = e.indice as usize;
        i < self.generations.len() && self.vivantes[i] && self.generations[i] == e.generation
    }

    /// Détruit l'entité et tous ses composants. Sans effet sur une poignée périmée.
    pub fn detruire(&mut self, e: Entite) {
        if !self.vivante(e) {
            return;
        }
        self.ident.ote(e);
        self.lieu.ote(e);
        self.rendu.ote(e);
        self.squelette.ote(e);
        self.obstacles.ote(e);
        self.mobile.ote(e);
        self.corps.ote(e);
        self.relief.ote(e);
        self.objet.ote(e);
        if self.choisie == Some(e) {
            self.choisie = None;
        }
        let i = e.indice as usize;
        self.vivantes[i] = false;
        self.generations[i] = self.generations[i].wrapping_add(1);
        self.libres.push(e.indice);
    }

    /// L'entité vivante qui porte l'identifiant `id`.
    pub fn trouver(&self, id: &str) -> Option<Entite> {
        self.ident.iter().find(|(_, i)| i.as_str() == id).map(|(e, _)| e)
    }

    /// Déplace l'origine de (di, dj) tuiles : tout le monde se décale d'autant, sans que
    /// rien ne bouge en absolu, ni ne saute dans l'interpolation.
    pub fn rebaser(&mut self, di: i32, dj: i32) {
        if (di, dj) == (0, 0) {
            return;
        }
        let (dx, dz) = (di as f32 * TAILLE, dj as f32 * TAILLE);
        for (_, l) in self.lieu.iter_mut() {
            l.pos[0] -= dx;
            l.pos[2] -= dz;
            l.avant.0[0] -= dx;
            l.avant.0[2] -= dz;
        }
        self.origine[0] += di;
        self.origine[1] += dj;
    }

    /// La position absolue d'un point relatif, en double précision.
    pub fn absolu(&self, p: [f32; 3]) -> [f64; 3] {
        let t = TAILLE as f64;
        [self.origine[0] as f64 * t + p[0] as f64, p[1] as f64, self.origine[1] as f64 * t + p[2] as f64]
    }

    pub fn nombre(&self) -> usize {
        self.vivantes.iter().filter(|v| **v).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_poignee_perimee_ne_designe_plus_rien() {
        let mut m = Monde::default();
        let a = m.creer("a");
        m.lieu.mettre(a, Lieu::en([1.0, 0.0, 0.0], 0.0));
        m.detruire(a);
        let b = m.creer("b");
        assert_eq!(a.indice, b.indice, "l'emplacement resert");
        assert!(!m.vivante(a) && m.vivante(b));
        assert!(m.lieu.get(a).is_none() && m.lieu.get(b).is_none(), "rien n'est hérité");
        m.lieu.mettre(b, Lieu::en([2.0, 0.0, 0.0], 0.0));
        assert!(m.lieu.get(a).is_none());
        m.detruire(a);
        assert!(m.vivante(b), "détruire une poignée périmée ne touche pas la nouvelle");
        assert_eq!(m.trouver("b"), Some(b));
        assert_eq!(m.trouver("a"), None);
        assert_eq!(m.nombre(), 1);
    }

    #[test]
    fn rebaser_ne_deplace_rien_en_absolu() {
        let mut m = Monde::default();
        let a = m.creer("a");
        m.lieu.mettre(a, Lieu::en([70.0, 2.0, -5.0], 0.0));
        let avant = m.absolu(m.lieu.get(a).unwrap().pos);
        m.rebaser(1, -1);
        let l = *m.lieu.get(a).unwrap();
        assert_eq!(l.pos, [70.0 - TAILLE, 2.0, -5.0 + TAILLE]);
        assert_eq!(l.avant.0, l.pos, "l'interpolation ne saute pas");
        assert_eq!(m.absolu(l.pos), avant);
        assert_eq!(m.origine, [1, -1]);
    }

    #[test]
    fn le_lieu_s_interpole_par_le_plus_court() {
        let mut l = Lieu::en([0.0; 3], 3.0);
        l.retenir();
        l.pos = [2.0, 0.0, 0.0];
        l.cap = -3.0; // de 3 à -3 en passant par π, pas par 0
        let (p, c) = l.interpole(0.5);
        assert!((p[0] - 1.0).abs() < 1e-6);
        assert!((c.abs() - std::f32::consts::PI).abs() < 0.01, "{c}");
    }
}
