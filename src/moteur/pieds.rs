//! Les pieds calés au sol : la dernière couche de la pose, après toutes les autres
//! (étape 7 des couches).
//!
//! - **Le sol sous chaque pied.** La cheville animée est portée sur ce qu'il y a sous
//!   elle (relief, dalle, marche, rampe) : elle s'élève de ce que le sol y dépasse celui
//!   du corps, ou descend s'il est plus bas.
//! - **Le pied tenu.** Un pied posé (la cheville à moins de `POSE` de sa hauteur debout)
//!   reste là où il s'est posé, au monde, jusqu'à ce que l'animation le lève : il ne glisse
//!   plus. Il se libère s'il s'éloigne de plus de `LACHER` de l'animation (le corps tourne
//!   sur place, il est poussé), et rejoint l'animation en `T_LEVER`.
//! - **Le bassin.** Il descend de ce qu'il faut pour que le pied le plus bas atteigne son
//!   sol, et amortit les marches que le corps monte d'un coup.
//! - **La pente.** Un pied près du sol en prend la pente, dans les limites de la cheville.
//! - Les jambes plient par `gda::ik::deux_os`, qui respecte les limites du genou.

use super::entites::Monde;
use super::physique::{self, MARCHE};
use super::tuiles::TAILLE;
use crate::personnage::Modele;
use gda::armature::{Ecart, Pose};
use gda::ik::{self, Jambe};
use gda::quat::{qaxe, qmul};

/// Une cheville à moins de cette hauteur au-dessus de sa hauteur debout est posée (m).
pub const POSE: f64 = 0.03;
/// Un pied tenu plus loin que cela de l'animation se libère (m).
pub const LACHER: f32 = 0.25;
/// Le temps qu'un pied libéré met à rejoindre l'animation (s).
pub const T_LEVER: f32 = 0.15;
/// Le bassin ne descend pas plus bas que cela (m).
pub const BASSIN_MAX: f32 = 0.45;
/// Un pied plus haut que cela au-dessus du sol n'en prend plus la pente (m).
const PENTE_HAUT: f64 = 0.10;

/// Un pied : là où il est tenu (x, z, et le sol sous lui), et le poids de cette tenue.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Appui {
    pub tenu: Option<[f32; 3]>,
    pub poids: f32,
    /// Il s'est libéré : il attend que l'animation le lève avant d'être repris.
    lache: bool,
    /// Il est posé, dans l'animation.
    pub pose: bool,
}

/// Les pieds d'un personnage : ses jambes, la hauteur de ses chevilles debout, et ce qui
/// change d'une image à l'autre.
#[derive(Clone, Debug, Default)]
pub struct Pieds {
    pub jambes: Vec<Jambe>,
    /// Hauteur de chaque cheville au repos, au repère de l'armature.
    cheville: Vec<f64>,
    racine: Option<String>,
    pub appuis: Vec<Appui>,
    /// De combien le bassin est descendu (m).
    pub bassin: f32,
    /// La hauteur du corps vue, qui amortit les marches.
    y_vu: Option<f32>,
    origine: [i32; 2],
}

/// Le point `w` du monde au repère de l'armature d'un personnage aux pieds en `pos`,
/// tourné de `cap`.
pub fn vers_armature(m: &Modele, pos: [f32; 3], cap: f32, w: [f32; 3]) -> [f64; 3] {
    let (e, sol) = (m.echelle as f64, m.sol as f64);
    let (s, c) = (cap as f64).sin_cos();
    let (x, y, z) = ((w[0] - pos[0]) as f64 / e, (w[1] - pos[1]) as f64 / e, (w[2] - pos[2]) as f64 / e);
    [x * c - z * s, y + sol, x * s + z * c]
}

impl Pieds {
    pub fn de(m: &Modele) -> Pieds {
        let jambes = ik::jambes(&m.armature);
        let repos = m.armature.repos();
        let cheville = jambes.iter().map(|j| repos[&j.pied].0[1]).collect();
        let racine = m.armature.os.iter().find(|b| b.parent.is_none()).map(|b| b.id.clone());
        Pieds { appuis: vec![Appui::default(); jambes.len()], jambes, cheville, racine, ..Pieds::default() }
    }

    /// Cale les pieds de `pose` au monde : le corps a les pieds en `pos`, tourné de `cap`,
    /// au sol ou non.
    #[allow(clippy::too_many_arguments)]
    pub fn caler(&mut self, monde: &Monde, m: &Modele, pos: [f32; 3], cap: f32, au_sol: bool, mut pose: Pose, dt: f32) -> Pose {
        if self.jambes.is_empty() {
            return pose;
        }
        // Le monde s'est décalé d'une tuile : les pieds tenus aussi
        let (di, dj) = (monde.origine[0] - self.origine[0], monde.origine[1] - self.origine[1]);
        if (di, dj) != (0, 0) {
            for a in self.appuis.iter_mut().filter_map(|a| a.tenu.as_mut()) {
                a[0] -= di as f32 * TAILLE;
                a[1] -= dj as f32 * TAILLE;
            }
            self.origine = monde.origine;
        }
        let (e, sol) = (m.echelle as f64, m.sol as f64);
        let (s, c) = (cap as f64).sin_cos();
        let p = pos.map(|x| x as f64);
        let monde_de = |a: [f64; 3]| {
            let (x, y, z) = (a[0] * e, (a[1] - sol) * e, a[2] * e);
            [p[0] + x * c + z * s, p[1] + y, p[2] - x * s + z * c]
        };
        let armature_de = |w: [f64; 3]| {
            let (x, y, z) = ((w[0] - p[0]) / e, (w[1] - p[1]) / e, (w[2] - p[2]) / e);
            [x * c - z * s, y + sol, x * s + z * c]
        };
        let y = pos[1];

        // La hauteur vue suit le corps, en douceur, tant qu'il est au sol
        let vu = match self.y_vu {
            Some(v) if au_sol => (v + (y - v) * (1.0 - (-dt * 14.0).exp())).clamp(y - BASSIN_MAX, y + BASSIN_MAX),
            _ => y,
        };
        self.y_vu = Some(vu);

        let r = m.armature.reperes(&pose);
        let mut cibles = Vec::with_capacity(self.jambes.len());
        let mut voulu: f32 = 0.0;
        for (i, j) in self.jambes.iter().enumerate() {
            let a = r[&j.pied].0;
            let w = monde_de(a);
            let haut = (a[1] - self.cheville[i]) * e;
            // En l'air, les pieds jouent l'animation ; au sol, un vide plus bas que le
            // bassin ne peut descendre les laisse pendre
            let sous = |x: f32, z: f32, plafond: f32| if au_sol { physique::sol(monde, x, z, plafond).filter(|s| s.0 >= y - BASSIN_MAX) } else { None };
            let (g, n) = sous(w[0] as f32, w[2] as f32, y + MARCHE + haut.max(0.0) as f32).map_or((y, [0.0, 1.0, 0.0]), |(h, n, _)| (h, n));
            // La cheville animée, portée sur son sol
            let anime = [w[0], w[1] + (g - y) as f64, w[2]];
            let ap = &mut self.appuis[i];
            ap.pose = au_sol && haut < POSE;
            if ap.pose && !ap.lache {
                if ap.poids < 1.0 || ap.tenu.is_none() {
                    // Il se pose là où il est montré
                    let t = ap.tenu.unwrap_or([anime[0] as f32, anime[2] as f32, g]);
                    let k = ap.poids;
                    let x = anime[0] as f32 + (t[0] - anime[0] as f32) * k;
                    let z = anime[2] as f32 + (t[1] - anime[2] as f32) * k;
                    let h = sous(x, z, y + MARCHE).map_or(g, |s| s.0);
                    ap.tenu = Some([x, z, h]);
                    ap.poids = 1.0;
                }
                let t = ap.tenu.unwrap();
                if (t[0] - anime[0] as f32).hypot(t[1] - anime[2] as f32) > LACHER {
                    ap.lache = true;
                }
            } else if !ap.pose {
                ap.lache = false;
            }
            if !ap.pose || ap.lache {
                ap.poids = (ap.poids - dt / T_LEVER).max(0.0);
                if ap.poids == 0.0 {
                    ap.tenu = None;
                }
            }
            let cible = match ap.tenu {
                Some(t) => {
                    let k = ap.poids as f64;
                    let ty = t[2] as f64 + (w[1] - y as f64);
                    [anime[0] + (t[0] as f64 - anime[0]) * k, anime[1] + (ty - anime[1]) * k, anime[2] + (t[1] as f64 - anime[2]) * k]
                }
                None => anime,
            };
            // La pente, en douceur près du sol
            let prend: f64 = (1.0 - haut / PENTE_HAUT).clamp(0.0, 1.0);
            // Seul un pied près du sol, ou tenu, tire le bassin vers le bas
            voulu = voulu.min((cible[1] - w[1]) as f32 * prend.max(ap.poids as f64) as f32);
            cibles.push((cible, n, prend));
        }

        // Le bassin descend pour le pied le plus bas, et amortit les marches
        let voulu = voulu.max(-BASSIN_MAX);
        self.bassin += (voulu - self.bassin) * (1.0 - (-dt * 12.0).exp());
        let descente = (self.bassin + (vu - y)) as f64 / e;
        if let Some(id) = &self.racine {
            let x = pose.get(id).copied().unwrap_or(Ecart { r: [0.0, 0.0, 0.0, 1.0], t: [0.0; 3] });
            pose.insert(id.clone(), Ecart { t: [x.t[0], x.t[1] + descente, x.t[2]], ..x });
        }

        let r = m.armature.reperes(&pose);
        for (j, (cible, n, prend)) in self.jambes.iter().zip(cibles) {
            let tenu = r[&j.pied].1;
            ik::deux_os(&m.armature, &mut pose, j, armature_de(cible));
            // Le sol, au repère de l'armature
            let n = [n[0] as f64 * c - n[2] as f64 * s, n[1] as f64, n[0] as f64 * s + n[2] as f64 * c];
            let q = ik::entre([0.0, 1.0, 0.0], n);
            let angle = 2.0 * q[3].clamp(-1.0, 1.0).acos();
            let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt();
            let incline = if l > 1e-9 { qaxe([q[0] / l, q[1] / l, q[2] / l], angle * prend) } else { [0.0, 0.0, 0.0, 1.0] };
            ik::orienter(&m.armature, &mut pose, &j.pied, qmul(incline, tenu));
        }
        pose
    }

    /// Pour le moniteur : chaque pied, posé ou levé, et la descente du bassin.
    pub fn resume(&self) -> String {
        let pieds: Vec<&str> = self.appuis.iter().map(|a| if a.tenu.is_some() && a.poids >= 1.0 { "tenu" } else if a.tenu.is_some() { "lâché" } else { "levé" }).collect();
        format!("{} · bassin {} cm", pieds.join(" / "), (self.bassin * 100.0).round() as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::moteur::locomotion::{Locomotion, Pas};
    use crate::moteur::{Aabb, Forme, Lieu, Relief, Triangles};
    use gda::armature::Armature;
    use gda::json::{self, J};
    use std::sync::Arc;

    fn hb01() -> Modele {
        let t = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../gda/tests/donnees/hb-01-repos.gda")).unwrap();
        let j = json::lire(&t).unwrap();
        let o = j.get("scene").and_then(|s| s.get("objets")).and_then(J::liste).unwrap().iter().find(|o| o.get("armature").is_some()).unwrap();
        Modele::depuis_armature("hb-01", Armature::depuis_donnees(o.get("armature").unwrap()))
    }

    /// Un sol plat à 0, et des obstacles.
    fn monde(obstacles: Vec<Forme>) -> Monde {
        let mut m = Monde::default();
        let t = m.creer("tuile");
        m.lieu.mettre(t, Lieu::en([0.0; 3], 0.0));
        m.relief.mettre(t, Relief { n: 1, h: vec![0.0; 4] });
        let d = m.creer("decor");
        m.lieu.mettre(d, Lieu::en([0.0; 3], 0.0));
        m.obstacles.mettre(d, obstacles);
        m
    }

    /// Les chevilles et la pointe des orteils au monde, pour la pose.
    fn chevilles(m: &Modele, pose: &Pose, pos: [f32; 3]) -> Vec<[f64; 3]> {
        let r = m.armature.reperes(pose);
        let a = |p: [f64; 3]| [p[0] + pos[0] as f64, p[1] - m.sol as f64 + pos[1] as f64, p[2] + pos[2] as f64];
        ["o12", "o19"].iter().map(|i| a(r[*i].0)).collect()
    }

    #[test]
    fn un_pied_tenu_ne_glisse_plus() {
        let m = hb01();
        let l = Locomotion::de(&m.armature, 1.0);
        let monde = monde(Vec::new());
        for v in [1.0, 1.44, 3.0] {
            let mut pieds = Pieds::de(&m);
            let mut pas = Pas { marche: 1.0, ..Pas::default() };
            let dt = 1.0 / 60.0;
            let mut pos = [10.0, 0.0, 5.0];
            let mut avant: Option<(Vec<[f64; 3]>, Vec<Appui>)> = None;
            let (mut pire, mut tenus, mut n) = (0.0f64, 0, 0);
            for _ in 0..240 {
                pos[2] += v * dt;
                l.avancer(&mut pas, v as f64, dt as f64);
                let pose = pieds.caler(&monde, &m, pos, 0.0, true, l.pose(&pas), dt);
                let c = chevilles(&m, &pose, pos);
                if let Some((c0, a0)) = &avant {
                    for k in 0..2 {
                        let (a, b) = (a0[k], pieds.appuis[k]);
                        n += 1;
                        if a.poids >= 1.0 && b.poids >= 1.0 && a.tenu == b.tenu {
                            tenus += 1;
                            pire = pire.max((c[k][0] - c0[k][0]).hypot(c[k][2] - c0[k][2]));
                        }
                    }
                }
                avant = Some((c, pieds.appuis.clone()));
            }
            eprintln!("{v} m/s : pieds tenus {} % du temps, glissement {:.1} mm par image au plus", 100 * tenus / n, pire * 1000.0);
            // La course n'a qu'un appui court
            assert!(tenus * 6 > n, "à {v} m/s, les pieds ne sont tenus que {tenus} fois sur {n}");
            assert!(pire < 0.002, "à {v} m/s, un pied tenu glisse de {pire} m par image");
        }
    }

    #[test]
    fn une_dalle_porte_un_pied_et_le_bassin_descend_pour_l_autre() {
        let m = hb01();
        let debout = m.armature.repos()["o12"].0[1] - m.sol as f64;
        let dalle = |y: f32| Forme::Boite(Aabb { centre: [10.3, y / 2.0, 5.0], demi: [0.3, y / 2.0, 0.5] });
        // Le pied gauche (+x) sur une dalle de 15 cm : il y monte, le bassin ne bouge pas
        let monde1 = monde(vec![dalle(0.15)]);
        let mut pieds = Pieds::de(&m);
        let mut pose = Pose::new();
        for _ in 0..60 {
            pose = pieds.caler(&monde1, &m, [10.0, 0.0, 5.0], 0.0, true, Pose::new(), 1.0 / 60.0);
        }
        let c = chevilles(&m, &pose, [10.0, 0.0, 5.0]);
        assert!((c[0][1] - (0.15 + debout)).abs() < 0.005, "gauche sur la dalle : {:?}", c[0]);
        assert!((c[1][1] - debout).abs() < 0.005, "droit au sol : {:?}", c[1]);
        assert!(pieds.bassin.abs() < 1e-3);
        // Le corps sur une dalle de 20 cm sous le pied droit : le gauche descend au sol,
        // le bassin avec lui
        let monde2 = monde(vec![Forme::Boite(Aabb { centre: [9.7, 0.1, 5.0], demi: [0.3, 0.1, 0.5] })]);
        let mut pieds = Pieds::de(&m);
        for _ in 0..90 {
            pose = pieds.caler(&monde2, &m, [10.0, 0.2, 5.0], 0.0, true, Pose::new(), 1.0 / 60.0);
        }
        let c = chevilles(&m, &pose, [10.0, 0.2, 5.0]);
        assert!((c[0][1] - debout).abs() < 0.005, "gauche au sol : {:?}", c[0]);
        assert!((c[1][1] - (0.2 + debout)).abs() < 0.005, "droit sur la dalle : {:?}", c[1]);
        assert!((pieds.bassin + 0.2).abs() < 0.01, "bassin {}", pieds.bassin);
    }

    #[test]
    fn en_l_air_ou_au_bord_d_un_vide_le_bassin_ne_tombe_pas() {
        let m = hb01();
        // Le corps sur une plate-forme de 2 m, au bord : le pied gauche au-dessus du vide
        let monde = monde(vec![Forme::Boite(Aabb { centre: [9.0, 1.0, 5.0], demi: [1.0, 1.0, 1.0] })]);
        for au_sol in [true, false] {
            let mut pieds = Pieds::de(&m);
            for _ in 0..60 {
                pieds.caler(&monde, &m, [10.0, 2.0, 5.0], 0.0, au_sol, Pose::new(), 1.0 / 60.0);
            }
            assert!(pieds.bassin.abs() < 1e-3, "au sol {au_sol} : bassin {}", pieds.bassin);
        }
    }

    #[test]
    fn sur_une_pente_le_pied_la_suit() {
        let m = hb01();
        let a = 12f32.to_radians();
        // Une pente qui monte vers +z, à 5 cm au-dessus du relief en z = 5
        let (z0, z1) = (3.0, 7.0);
        let h = |z: f32| 0.05 + (z - 5.0) * a.tan();
        let t = Arc::new(Triangles::new(vec![
            [[8.0, h(z0), z0], [12.0, h(z0), z0], [12.0, h(z1), z1]],
            [[8.0, h(z0), z0], [12.0, h(z1), z1], [8.0, h(z1), z1]],
        ]));
        let monde = monde(vec![Forme::Triangles(t)]);
        let mut pieds = Pieds::de(&m);
        let mut pose = Pose::new();
        // Le corps est posé sur la pente, comme le pose la physique
        for _ in 0..60 {
            pose = pieds.caler(&monde, &m, [10.0, 0.05, 5.0], 0.0, true, Pose::new(), 1.0 / 60.0);
        }
        // L'axe de l'orteil, du pied à sa pointe, monte comme la pente
        let r = m.armature.reperes(&pose);
        let r0 = m.armature.repos();
        for (pied, orteil) in [("o12", "o20"), ("o19", "o21")] {
            let d = |r: &std::collections::HashMap<String, ([f64; 3], [f64; 4])>| {
                let (p, q) = (r[orteil].0, r[pied].0);
                ((p[1] - q[1]).atan2(p[2] - q[2])).to_degrees()
            };
            let ecart = d(&r) - d(&r0);
            assert!((ecart - 12.0).abs() < 1.5, "{pied} incliné de {ecart}° pour 12°");
        }
    }
}
