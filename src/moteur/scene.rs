//! Ce que le moteur de rendu dessine, extrait du monde à chaque image. Le moteur de rendu
//! ne lit rien d'autre.

use super::entites::Monde;
use super::ressources::{Maillage3d, MaillageId, Maillages, Texture3d, TextureId, Textures};
use crate::math::{self, Mat4};

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub vue: Mat4,
    pub proj: Mat4,
    pub oeil: [f32; 3],
    /// Où dessiner sur l'écran, en pixels depuis le coin haut gauche : x, y, largeur,
    /// hauteur. Le reste de l'écran appartient à l'interface.
    pub fenetre: [f32; 4],
}

impl Camera {
    /// La direction, unitaire, du rayon qui part de l'œil et passe par le point (x, y) de
    /// l'écran.
    pub fn rayon(&self, x: f32, y: f32) -> [f32; 3] {
        let f = self.fenetre;
        let nx = (x - f[0]) / f[2].max(1.0) * 2.0 - 1.0;
        let ny = 1.0 - (y - f[1]) / f[3].max(1.0) * 2.0;
        let (dx, dy) = (nx / self.proj[0], ny / self.proj[5]);
        let v = &self.vue;
        let d = [0, 1, 2].map(|k| v[4 * k] * dx + v[4 * k + 1] * dy - v[4 * k + 2]);
        math::normalize(d)
    }

    /// Le point de l'écran où se voit `p`, s'il est devant l'œil.
    pub fn ecran(&self, p: [f32; 3]) -> Option<[f32; 2]> {
        let c = math::appliquer(&math::mul(&self.proj, &self.vue), p);
        if c[3] <= 1e-3 {
            return None;
        }
        let f = self.fenetre;
        Some([f[0] + (c[0] / c[3] + 1.0) / 2.0 * f[2], f[1] + (1.0 - c[1] / c[3]) / 2.0 * f[3]])
    }
}

/// Un objet à dessiner : son maillage, sa place dans le monde, ses os s'il a une peau.
#[derive(Clone, Debug)]
pub struct Dessin {
    pub maillage: MaillageId,
    pub place: Mat4,
    pub os: Vec<Mat4>,
    /// Choisi dans l'éditeur : dessiné en surbrillance.
    pub choisi: bool,
}

/// Le soleil : d'où vient sa lumière, et la zone, autour de `centre`, où il porte des ombres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Soleil {
    /// Vers le soleil, unitaire.
    pub direction: [f32; 3],
    pub centre: [f32; 3],
    /// Demi-côté de la zone des ombres, en mètres.
    pub rayon: f32,
}

/// Le soleil de la campagne : haut, un peu de côté.
pub const SOLEIL: [f32; 3] = [0.43968, 0.83050, 0.34197];
/// Les ombres portées couvrent un carré de deux fois cela autour du joueur.
pub const RAYON_OMBRES: f32 = 28.0;

impl Soleil {
    /// La vue du soleil, projection comprise, sur une carte de `cote` × `cote` texels : un
    /// carré de `2 × rayon` autour du centre, profond de 100 m. Le carré avance par texels
    /// entiers : les bords des ombres ne scintillent pas quand le joueur bouge.
    pub fn matrice(&self, cote: u32) -> Mat4 {
        let d = math::normalize(self.direction);
        let vue = math::look_at(d, [0.0; 3], if d[1].abs() > 0.99 { [0.0, 0.0, 1.0] } else { [0.0, 1.0, 0.0] });
        let c = math::appliquer(&vue, self.centre);
        let texel = 2.0 * self.rayon / cote as f32;
        let (x, y) = ((c[0] / texel).round() * texel, (c[1] / texel).round() * texel);
        let r = self.rayon;
        let p = math::ortho(x - r, x + r, y - r, y + r, -c[2] - 50.0, -c[2] + 50.0);
        math::mul(&p, &vue)
    }
}

pub struct Scene {
    pub camera: Camera,
    pub dessins: Vec<Dessin>,
    pub soleil: Soleil,
    /// Les maillages et les textures vivants : le moteur de rendu charge les nouveaux et
    /// libère les autres.
    pub maillages: Vec<(MaillageId, Maillage3d)>,
    pub textures: Vec<(TextureId, Texture3d)>,
    /// Là où le brouillard a tout recouvert, en mètres.
    pub brouillard: f32,
    /// L'écart des courbes de niveau tracées sur le relief, en mètres ; 0 : aucune.
    pub isolignes: f32,
    /// Le niveau de la mer, s'il y en a une : un plan d'eau jusqu'à l'horizon.
    pub mer: Option<f32>,
    /// Le temps qui passe, en secondes : la houle.
    pub temps: f32,
    /// Des maillages du terrain qui ne se dessinent pas dans un rectangle (x, z bas, x, z
    /// haut) : le relief et les bâtiments lointains, sous les tuiles chargées.
    pub trou: Option<(Vec<MaillageId>, [f32; 4])>,
}

/// Le brouillard d'origine.
pub const BROUILLARD: f32 = 144.0;

impl Scene {
    /// La scène vue par `camera`, les entités placées entre leurs deux derniers pas (`u`).
    /// Les ombres sont autour de l'œil ; le jeu les centre sur le joueur.
    pub fn extraire(monde: &Monde, maillages: &Maillages, textures: &Textures, camera: Camera, u: f32) -> Scene {
        let mut dessins = Vec::new();
        for (e, r) in monde.rendu.iter() {
            if maillages.get(r.maillage).is_none() {
                continue;
            }
            let place = match monde.lieu.get(e) {
                Some(l) => math::mul(&l.place(u), &r.local),
                None => r.local,
            };
            let os = monde.squelette.get(e).map(|s| s.os.clone()).unwrap_or_default();
            dessins.push(Dessin { maillage: r.maillage, place, os, choisi: monde.choisie == Some(e) });
        }
        let soleil = Soleil { direction: SOLEIL, centre: camera.oeil, rayon: RAYON_OMBRES };
        Scene { camera, dessins, soleil, maillages: maillages.tous(), textures: textures.toutes(), brouillard: BROUILLARD, isolignes: 0.0, mer: None, temps: 0.0, trou: None }
    }
}

#[cfg(test)]
mod tests_soleil {
    use super::*;

    #[test]
    fn la_vue_du_soleil_couvre_le_carre_et_avance_par_texels() {
        let s = Soleil { direction: SOLEIL, centre: [10.0, 3.0, -4.0], rayon: 20.0 };
        let m = s.matrice(1024);
        let c = math::appliquer(&m, s.centre);
        assert!(c[0].abs() < 2.0 / 1024.0 && c[1].abs() < 2.0 / 1024.0 && c[2].abs() < 1e-3, "{c:?}");
        // Un point plus haut vers le soleil est plus près de lui
        let h = math::appliquer(&m, [10.0 + SOLEIL[0] * 5.0, 3.0 + SOLEIL[1] * 5.0, -4.0 + SOLEIL[2] * 5.0]);
        assert!(h[2] < c[2]);
        // Le centre bouge d'un rien : les texels restent sur la même grille
        let m2 = Soleil { centre: [10.003, 3.0, -4.002], ..s }.matrice(1024);
        let texel = 2.0 / 1024.0;
        let k = (m2[12] - m[12]) / texel;
        assert!((k - k.round()).abs() < 1e-2, "{k}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::moteur::{Format, Lieu, Rendu};

    #[test]
    fn le_rayon_passe_par_le_point_touche() {
        let (oeil, cible) = ([1.0, 2.0, 3.0], [4.0, 1.0, -2.0]);
        let proj = math::perspective(1.0, 2.0, 0.1, 100.0);
        let cam = Camera { vue: math::look_at(oeil, cible, [0.0, 1.0, 0.0]), proj, oeil, fenetre: [100.0, 0.0, 800.0, 400.0] };
        let d = cam.rayon(500.0, 200.0);
        let f = math::normalize([3.0, -1.0, -5.0]);
        assert!((0..3).all(|k| (d[k] - f[k]).abs() < 1e-5), "le centre : {d:?}");
        // Un point du monde, projeté, renvoie un rayon qui passe par lui
        let p = [2.0, 3.0, -4.0];
        let c = math::appliquer(&math::mul(&proj, &cam.vue), p);
        let (x, y) = (100.0 + (c[0] / c[3] + 1.0) / 2.0 * 800.0, (1.0 - c[1] / c[3]) / 2.0 * 400.0);
        let d = cam.rayon(x, y);
        let v = math::normalize([p[0] - oeil[0], p[1] - oeil[1], p[2] - oeil[2]]);
        assert!((0..3).all(|k| (d[k] - v[k]).abs() < 1e-4), "{d:?} {v:?}");
    }
    use std::sync::Arc;

    #[test]
    fn la_scene_ne_dessine_que_ce_qui_existe() {
        let mut m = Monde::default();
        let mut r = Maillages::default();
        let k = r.mettre("bloc", Format::Statique, Arc::new(vec![0.0; 27]));
        let a = m.creer("a");
        m.lieu.mettre(a, Lieu::en([3.0, 0.0, 0.0], 0.0));
        m.rendu.mettre(a, Rendu { maillage: k, local: math::scale(1.0) });
        let b = m.creer("b");
        m.rendu.mettre(b, Rendu { maillage: MaillageId(9), local: math::scale(1.0) });
        let cam = Camera { vue: math::scale(1.0), proj: math::scale(1.0), oeil: [0.0; 3], fenetre: [0.0, 0.0, 1.0, 1.0] };
        let s = Scene::extraire(&m, &r, &Default::default(), cam, 1.0);
        assert_eq!(s.dessins.len(), 1);
        assert_eq!(s.dessins[0].place[12], 3.0);
        assert_eq!(s.maillages.len(), 1);
    }
}
