//! Les modèles importés : des décors et des bâtiments venus d'un `.glb`, chacun sous son
//! identifiant. Le moteur n'en garde que la géométrie : le maillage à dessiner, découpé
//! par matériau, et les triangles de sa collision, dans le repère du modèle (la base en
//! (0, 0, 0), l'avant vers +z).
//!
//! Un objet qui désigne un modèle encore inconnu (pas importé, ou retiré) le garde sous
//! son identifiant : il se dessine en boîte grise jusqu'à ce que le modèle arrive.

use super::ressources::Partie;
use std::sync::Arc;

/// Poignée d'un modèle : son rang dans le registre, stable tant que le monde vit.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct ModeleId(pub u32);

/// La géométrie d'un modèle.
#[derive(Clone, Debug)]
pub struct Gabarit {
    /// Au format `Format::Texture`.
    pub sommets: Arc<Vec<f32>>,
    pub parties: Vec<Partie>,
    pub triangles: Arc<Vec<[[f32; 3]; 3]>>,
    /// La boîte englobante, au repère du modèle.
    pub boite: ([f32; 3], [f32; 3]),
}

/// Un modèle : son identifiant, son nom affiché, sa géométrie s'il est là.
#[derive(Clone, Debug)]
pub struct Modele {
    pub id: String,
    pub nom: String,
    pub gabarit: Option<Gabarit>,
    /// Change à chaque nouvelle géométrie : les objets savent qu'ils sont à refaire.
    pub version: u32,
}

#[derive(Default, Clone, Debug)]
pub struct Modeles {
    v: Vec<Modele>,
}

/// La boîte d'un modèle absent : un mètre de côté, posée au sol.
pub const BOITE_ABSENTE: ([f32; 3], [f32; 3]) = ([-0.5, 0.0, -0.5], [0.5, 1.0, 0.5]);

impl Modeles {
    /// La poignée de `id`, inscrit au besoin sans géométrie.
    pub fn inscrire(&mut self, id: &str) -> ModeleId {
        if let Some(k) = self.chercher(id) {
            return k;
        }
        self.v.push(Modele { id: id.to_string(), nom: id.to_string(), gabarit: None, version: 0 });
        ModeleId(self.v.len() as u32 - 1)
    }

    /// Donne sa géométrie et son nom à `id`.
    pub fn mettre(&mut self, id: &str, nom: &str, g: Gabarit) -> ModeleId {
        let k = self.inscrire(id);
        let m = &mut self.v[k.0 as usize];
        m.nom = nom.to_string();
        m.gabarit = Some(g);
        m.version += 1;
        k
    }

    /// Le modèle `id` n'est plus là : ses objets restent, en boîte grise.
    pub fn retirer(&mut self, k: ModeleId) {
        if let Some(m) = self.v.get_mut(k.0 as usize) {
            m.gabarit = None;
            m.version += 1;
        }
    }

    pub fn chercher(&self, id: &str) -> Option<ModeleId> {
        self.v.iter().position(|m| m.id == id).map(|i| ModeleId(i as u32))
    }

    pub fn get(&self, k: ModeleId) -> Option<&Modele> {
        self.v.get(k.0 as usize)
    }

    pub fn id(&self, k: ModeleId) -> &str {
        self.get(k).map_or("", |m| m.id.as_str())
    }

    /// Les modèles présents, dans l'ordre d'arrivée.
    pub fn presents(&self) -> Vec<ModeleId> {
        (0..self.v.len()).filter(|&i| self.v[i].gabarit.is_some()).map(|i| ModeleId(i as u32)).collect()
    }
}
