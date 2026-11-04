//! Le moniteur : cadence, temps passé par image, simulation, monde et joueur. Les mesures
//! s'accumulent à chaque image ; l'affichage se rafraîchit quatre fois par seconde pour
//! rester lisible.

use crate::moteur::Scene;
use std::collections::VecDeque;

/// Images gardées pour la courbe.
pub const HISTOIRE: usize = 120;
const RAFRAICHIR: f32 = 0.25;

/// Ce que le monde et le joueur montrent, relevé par le jeu.
#[derive(Clone, Default, Debug)]
pub struct Etat {
    pub entites: usize,
    pub pas_total: u64,
    pub pos: [f32; 3],
    /// La position absolue, en double précision.
    pub absolu: [f64; 3],
    pub cap: f32,
    pub vitesse: f32,
    /// 0 : repos, 1 : marche.
    pub melange: f32,
    /// Poids de la course, et ce que joue le personnage.
    pub course: f32,
    pub allures: String,
    /// Ce qui porte le joueur et la pente ; ce que vise le centre de la vue et sa distance.
    pub appui: Option<(String, f32)>,
    pub vise: Option<(String, f32)>,
    /// Les pieds : tenus ou levés, et la descente du bassin ; vide sans jambes.
    pub pieds: String,
    /// L'inclinaison du dos et le regard ; vides sans dos ni tête.
    pub buste: String,
    pub regard: String,
    /// Tuiles chargées, tuile du joueur, durée de la dernière génération (ms).
    pub tuiles: usize,
    pub tuile: [i32; 2],
    pub generation_ms: f32,
}

/// Les valeurs affichées : figées entre deux rafraîchissements.
#[derive(Clone, Default, Debug)]
pub struct Affiche {
    pub cadence: f32,
    pub intervalle: f32,
    pub pire: f32,
    /// Temps de la plateforme, s'il est mesuré : jeu, rendu, échange (ms).
    pub temps: Option<[f32; 3]>,
    pub pas_seconde: f32,
    pub pas_image: f32,
    pub dessins: usize,
    pub triangles: usize,
    pub maillages: usize,
    pub octets: usize,
    pub etat: Etat,
}

#[derive(Default)]
pub struct Moniteur {
    /// Intervalles entre images, en ms, les plus récents à la fin.
    pub images: VecDeque<f32>,
    /// Depuis le dernier rafraîchissement : images, pas, temps, et la somme des temps de
    /// la plateforme.
    n: u32,
    pas: u32,
    duree: f32,
    temps: Option<([f32; 3], u32)>,
    depuis: f32,
    pub affiche: Affiche,
}

impl Moniteur {
    /// Une image du jeu : son intervalle `dt` (s), les pas de simulation qu'elle a faits,
    /// la scène produite et l'état du monde.
    pub fn image(&mut self, dt: f32, pas: u32, scene: &Scene, etat: Etat) {
        self.n += 1;
        self.pas += pas;
        self.duree += dt;
        self.depuis += dt;
        if self.temps.is_none() {
            // Sans mesure de la plateforme (aperçu, tests), l'intervalle vient du jeu
            self.pousser(dt * 1000.0);
        }
        if self.depuis < RAFRAICHIR && self.affiche.cadence > 0.0 {
            return;
        }
        let a = &mut self.affiche;
        let s = self.duree.max(1e-6);
        a.cadence = self.n as f32 / s;
        a.intervalle = s * 1000.0 / self.n.max(1) as f32;
        let recent = self.images.len().saturating_sub(self.n as usize);
        a.pire = self.images.iter().skip(recent).copied().fold(0.0, f32::max);
        a.temps = self.temps.map(|(t, k)| t.map(|x| x / k.max(1) as f32));
        a.pas_seconde = self.pas as f32 / s;
        a.pas_image = self.pas as f32 / self.n.max(1) as f32;
        a.dessins = scene.dessins.len();
        a.triangles = scene
            .dessins
            .iter()
            .filter_map(|d| scene.maillages.iter().find(|(k, _)| *k == d.maillage))
            .map(|(_, m)| m.sommets.len() / m.format.flottants() / 3)
            .sum();
        a.maillages = scene.maillages.len();
        a.octets = scene.maillages.iter().map(|(_, m)| m.sommets.len() * 4).sum();
        a.etat = etat;
        self.n = 0;
        self.pas = 0;
        self.duree = 0.0;
        self.depuis = 0.0;
        if let Some(t) = &mut self.temps {
            *t = ([0.0; 3], 0);
        }
    }

    /// Mesures de la plateforme, en secondes : l'intervalle réel depuis l'image
    /// précédente, puis le temps du jeu, de l'envoi au processeur graphique, et de
    /// l'échange des tampons (l'attente du processeur graphique et de l'écran).
    pub fn plateforme(&mut self, intervalle: f32, jeu: f32, rendu: f32, echange: f32) {
        self.pousser(intervalle * 1000.0);
        let (t, k) = self.temps.get_or_insert(([0.0; 3], 0));
        for (x, v) in t.iter_mut().zip([jeu, rendu, echange]) {
            *x += v * 1000.0;
        }
        *k += 1;
    }

    fn pousser(&mut self, ms: f32) {
        if self.images.len() == HISTOIRE {
            self.images.pop_front();
        }
        self.images.push_back(ms);
    }
}

/// Des octets lisibles : « 812 Ko », « 1,4 Mo ».
pub fn octets(n: usize) -> String {
    if n < 1024 * 1024 {
        format!("{} Ko", (n + 512) / 1024)
    } else {
        format!("{:.1} Mo", n as f32 / (1024.0 * 1024.0)).replace('.', ",")
    }
}

/// Un nombre décimal à la française.
pub fn dec(v: f32, chiffres: usize) -> String {
    format!("{v:.chiffres$}").replace('.', ",")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math;
    use crate::moteur::{Camera, Format, Maillages, Monde, Rendu};
    use std::sync::Arc;

    fn scene() -> Scene {
        let mut m = Monde::default();
        let mut r = Maillages::default();
        let k = r.mettre("b", Format::Statique, Arc::new(vec![0.0; 9 * 6]));
        let e = m.creer("e");
        m.rendu.mettre(e, Rendu { maillage: k, local: math::scale(1.0) });
        let cam = Camera { vue: math::scale(1.0), proj: math::scale(1.0), oeil: [0.0; 3], fenetre: [0.0, 0.0, 1.0, 1.0] };
        Scene::extraire(&m, &r, &Default::default(), cam, 1.0)
    }

    #[test]
    fn le_moniteur_compte_images_pas_et_triangles() {
        let mut m = Moniteur::default();
        let s = scene();
        for _ in 0..30 {
            m.image(1.0 / 30.0, 2, &s, Etat::default());
        }
        let a = &m.affiche;
        assert!((a.cadence - 30.0).abs() < 0.5, "{}", a.cadence);
        assert!((a.pas_seconde - 60.0).abs() < 1.0);
        assert_eq!(a.pas_image, 2.0);
        assert_eq!((a.dessins, a.triangles, a.maillages, a.octets), (1, 2, 1, 9 * 6 * 4));
        assert!(a.temps.is_none());
        assert_eq!(m.images.len(), 30);
    }

    #[test]
    fn les_temps_de_la_plateforme_sont_moyennes() {
        let mut m = Moniteur::default();
        let s = scene();
        for i in 0..20 {
            m.plateforme(0.02, 0.002, 0.001 * i as f32, 0.01);
            m.image(0.02, 1, &s, Etat::default());
        }
        let t = m.affiche.temps.unwrap();
        assert!((t[0] - 2.0).abs() < 1e-3 && (t[2] - 10.0).abs() < 1e-3, "{t:?}");
        assert_eq!(m.images.len(), 20, "une seule mesure par image");
        for _ in 0..200 {
            m.plateforme(0.02, 0.0, 0.0, 0.0);
        }
        assert_eq!(m.images.len(), HISTOIRE);
    }

    #[test]
    fn les_nombres_se_lisent_a_la_francaise() {
        assert_eq!(octets(800 * 1024), "800 Ko");
        assert_eq!(octets(3 * 1024 * 1024 / 2), "1,5 Mo");
        assert_eq!(dec(16.666, 1), "16,7");
    }
}
