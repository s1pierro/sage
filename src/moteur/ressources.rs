//! Les maillages et les textures à dessiner, sous leur identifiant. Chacun porte une
//! version : le moteur de rendu ne renvoie au processeur graphique que ce qui a changé.
//!
//! Un maillage peut être découpé en *parties*, des suites de triangles qui partagent un
//! matériau : une texture, et la façon dont il laisse voir à travers lui.

use crate::image::Image;
use std::sync::Arc;

/// La disposition des sommets, à plat, en flottants.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    /// Position, normale, couleur : 9 flottants.
    Statique,
    /// Position, normale, os (4), poids (4), couleur, coordonnées de texture : 19
    /// flottants.
    Peau,
    /// Comme `Statique`, dessiné avec le quadrillage et les ombres au sol.
    Terrain,
}

impl Format {
    pub fn attributs(self) -> &'static [i32] {
        match self {
            Format::Statique | Format::Terrain => &[3, 3, 3],
            Format::Peau => &[3, 3, 4, 4, 3, 2],
        }
    }

    pub fn flottants(self) -> usize {
        self.attributs().iter().sum::<i32>() as usize
    }
}

/// Poignée d'un maillage : ne change pas tant que l'identifiant reste enregistré.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct MaillageId(pub u32);

/// Comment un matériau laisse voir à travers lui.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash, PartialOrd, Ord)]
pub enum Alpha {
    #[default]
    Opaque,
    /// Coupé net là où sa texture est à moins de la moitié d'opacité.
    Masque,
    /// Mêlé à ce qui est derrière : dessiné après le reste, du plus loin au plus proche.
    Melange,
}

/// Un matériau : sa texture (multipliée par la couleur des sommets), son opacité et son
/// mode d'alpha.
#[derive(Clone, Debug, PartialEq)]
pub struct Materiau {
    pub texture: Option<TextureId>,
    pub opacite: f32,
    pub alpha: Alpha,
}

impl Default for Materiau {
    fn default() -> Materiau {
        Materiau { texture: None, opacite: 1.0, alpha: Alpha::Opaque }
    }
}

/// Une partie d'un maillage : `nombre` sommets à partir de `premier`, d'un matériau.
#[derive(Clone, Debug, PartialEq)]
pub struct Partie {
    pub premier: u32,
    pub nombre: u32,
    pub materiau: Materiau,
}

#[derive(Clone, Debug)]
pub struct Maillage3d {
    pub id: String,
    pub format: Format,
    pub sommets: Arc<Vec<f32>>,
    /// Ses parties ; vide, tout le maillage est d'un matériau opaque sans texture.
    pub parties: Arc<Vec<Partie>>,
    pub version: u64,
}

impl Maillage3d {
    /// Ses parties, ou une seule qui le couvre tout.
    pub fn parties(&self) -> Vec<Partie> {
        if self.parties.is_empty() {
            vec![Partie { premier: 0, nombre: (self.sommets.len() / self.format.flottants()) as u32, materiau: Materiau::default() }]
        } else {
            self.parties.to_vec()
        }
    }
}

#[derive(Default)]
pub struct Maillages {
    v: Vec<Option<Maillage3d>>,
    version: u64,
}

impl Maillages {
    /// Enregistre ou remplace le maillage `id`, d'un seul matériau.
    pub fn mettre(&mut self, id: &str, format: Format, sommets: Arc<Vec<f32>>) -> MaillageId {
        self.mettre_parties(id, format, sommets, Vec::new())
    }

    /// Enregistre ou remplace le maillage `id`, découpé en parties.
    pub fn mettre_parties(&mut self, id: &str, format: Format, sommets: Arc<Vec<f32>>, parties: Vec<Partie>) -> MaillageId {
        debug_assert_eq!(sommets.len() % format.flottants(), 0);
        self.version += 1;
        let m = Maillage3d { id: id.to_string(), format, sommets, parties: Arc::new(parties), version: self.version };
        if let Some(k) = self.chercher(id) {
            self.v[k.0 as usize] = Some(m);
            return k;
        }
        let k = match self.v.iter().position(Option::is_none) {
            Some(i) => i,
            None => {
                self.v.push(None);
                self.v.len() - 1
            }
        };
        self.v[k] = Some(m);
        MaillageId(k as u32)
    }

    pub fn chercher(&self, id: &str) -> Option<MaillageId> {
        self.v.iter().position(|m| m.as_ref().is_some_and(|m| m.id == id)).map(|i| MaillageId(i as u32))
    }

    pub fn ote(&mut self, id: &str) {
        if let Some(k) = self.chercher(id) {
            self.v[k.0 as usize] = None;
        }
    }

    pub fn get(&self, k: MaillageId) -> Option<&Maillage3d> {
        self.v.get(k.0 as usize)?.as_ref()
    }

    /// Tous les maillages enregistrés, avec leur poignée.
    pub fn tous(&self) -> Vec<(MaillageId, Maillage3d)> {
        self.v.iter().enumerate().filter_map(|(i, m)| m.clone().map(|m| (MaillageId(i as u32), m))).collect()
    }
}

/// Poignée d'une texture.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct TextureId(pub u32);

#[derive(Clone, Debug)]
pub struct Texture3d {
    pub id: String,
    pub image: Arc<Image>,
    pub version: u64,
}

/// Les textures, comme les maillages : sous leur identifiant, versionnées.
#[derive(Default)]
pub struct Textures {
    v: Vec<Option<Texture3d>>,
    version: u64,
}

impl Textures {
    pub fn mettre(&mut self, id: &str, image: Arc<Image>) -> TextureId {
        self.version += 1;
        let t = Texture3d { id: id.to_string(), image, version: self.version };
        let k = match self.chercher(id) {
            Some(k) => k.0 as usize,
            None => match self.v.iter().position(Option::is_none) {
                Some(i) => i,
                None => {
                    self.v.push(None);
                    self.v.len() - 1
                }
            },
        };
        self.v[k] = Some(t);
        TextureId(k as u32)
    }

    pub fn chercher(&self, id: &str) -> Option<TextureId> {
        self.v.iter().position(|t| t.as_ref().is_some_and(|t| t.id == id)).map(|i| TextureId(i as u32))
    }

    /// Ôte les textures dont l'identifiant commence par `prefixe`.
    pub fn oter_sous(&mut self, prefixe: &str) {
        for t in self.v.iter_mut() {
            if t.as_ref().is_some_and(|t| t.id.starts_with(prefixe)) {
                *t = None;
            }
        }
    }

    pub fn toutes(&self) -> Vec<(TextureId, Texture3d)> {
        self.v.iter().enumerate().filter_map(|(i, t)| t.clone().map(|t| (TextureId(i as u32), t))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remplacer_garde_la_poignee_et_change_la_version() {
        let mut m = Maillages::default();
        let a = m.mettre("a", Format::Statique, Arc::new(vec![0.0; 9]));
        let va = m.get(a).unwrap().version;
        let b = m.mettre("b", Format::Peau, Arc::new(vec![0.0; 19]));
        assert_ne!(a, b);
        assert_eq!(m.mettre("a", Format::Statique, Arc::new(vec![0.0; 18])), a);
        assert!(m.get(a).unwrap().version > va);
        m.ote("a");
        assert!(m.get(a).is_none());
        assert_eq!(m.tous().len(), 1);
        assert_eq!(m.mettre("c", Format::Statique, Arc::new(vec![])), a, "l'emplacement resert");
    }

    #[test]
    fn un_maillage_sans_parties_en_a_une_et_les_textures_se_remplacent() {
        let mut m = Maillages::default();
        let a = m.mettre("a", Format::Statique, Arc::new(vec![0.0; 27]));
        assert_eq!(m.get(a).unwrap().parties(), vec![Partie { premier: 0, nombre: 3, materiau: Materiau::default() }]);
        let mut t = Textures::default();
        let i = Arc::new(Image { l: 1, h: 1, rgba: vec![0; 4] });
        let x = t.mettre("perso/a/0", i.clone());
        let v = t.toutes()[0].1.version;
        assert_eq!(t.mettre("perso/a/0", i.clone()), x);
        assert!(t.toutes()[0].1.version > v);
        t.mettre("autre", i);
        t.oter_sous("perso/a/");
        assert_eq!(t.toutes().len(), 1);
    }
}
