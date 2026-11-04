//! La posture : deux couches calculées, posées sur l'animation avant les pieds (étape 7
//! des couches).
//!
//! - **L'inclinaison.** Le dos penche dans le sens de l'accélération, comme un corps qui
//!   garde l'équilibre : en avant quand il part, en arrière quand il freine, vers
//!   l'intérieur dans un virage. L'accélération vers l'avant vient de la vitesse, celle de
//!   côté de la vitesse et de la rotation du cap ; toutes deux varient en douceur. Les os
//!   du dos se partagent l'angle, chacun dans ses limites. En l'air, le dos se redresse.
//! - **Le regard.** Le cou et la tête tournent vers un point du monde (ce que vise la
//!   caméra), 40 % pour le cou et 60 % pour la tête, chacun dans ses limites. Le regard va
//!   jusqu'à `REGARD_COTE` de côté à l'arrêt, la moitié à 3 m/s ; un point plus en arrière
//!   que `REGARD_DOS`, le personnage regarde devant lui.
//!
//! Les angles sont lissés : rien ne saute d'une image à l'autre.

use crate::personnage::Modele;
use gda::armature::Pose;
use gda::ik::{self, Buste};
use gda::maillage::P3;
use gda::quat::{qappliquer, qaxe, qinv, qmul, Q};

const G: f64 = 9.81;
/// L'inclinaison la plus forte, vers l'avant ou l'arrière, puis de côté (degrés).
pub const PENCHE_AVANT: f64 = 8.0;
pub const PENCHE_COTE: f64 = 12.0;
/// Le regard tourne jusqu'à cet angle de côté, et lève ou baisse jusqu'à ceux-ci (degrés).
pub const REGARD_COTE: f64 = 70.0;
pub const REGARD_HAUT: f64 = 35.0;
pub const REGARD_BAS: f64 = 40.0;
/// Au-delà, le point est derrière : le regard revient devant (degrés).
pub const REGARD_DOS: f64 = 110.0;

/// La posture d'un personnage : ses os, et ce qui change d'une image à l'autre.
#[derive(Clone, Debug, Default)]
pub struct Posture {
    pub buste: Buste,
    /// Rotation de la tête au repos, et sa longueur.
    tete_repos: Q,
    tete_long: f64,
    precedent: Option<(f32, f32)>,
    /// Accélérations lissées, vers l'avant et vers la gauche (m/s²).
    acc: [f64; 2],
    /// Inclinaison du dos : vers l'avant, vers la gauche (radians).
    pub penche: [f64; 2],
    /// Regard : vers la gauche, vers le bas (radians), par rapport à l'animation.
    pub regard: [f64; 2],
}

fn moins(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn lacet(v: P3) -> f64 {
    v[0].atan2(v[2])
}

fn plongee(v: P3) -> f64 {
    (-v[1]).atan2(v[0].hypot(v[2]))
}

fn lisser(x: f64, voulu: f64, vitesse: f64, dt: f64) -> f64 {
    x + (voulu - x) * (1.0 - (-dt * vitesse).exp())
}

/// La rotation qui tourne de `lacet` autour de la verticale, puis plonge de `plongee`
/// autour de l'axe de côté ainsi tourné.
fn vers(lacet: f64, plongee: f64) -> Q {
    let y = qaxe([0.0, 1.0, 0.0], lacet);
    qmul(qaxe(qappliquer(y, [1.0, 0.0, 0.0]), plongee), y)
}

impl Posture {
    pub fn de(m: &Modele) -> Posture {
        let buste = ik::buste(&m.armature);
        let repos = m.armature.repos();
        let (tete_repos, tete_long) = buste.tete.as_ref().map_or(([0.0, 0.0, 0.0, 1.0], 0.0), |t| (repos[t].1, m.armature.trouver(t).map_or(0.0, |b| b.longueur)));
        Posture { buste, tete_repos, tete_long, ..Posture::default() }
    }

    /// La direction du visage et la place des yeux, au repère de l'armature.
    pub fn visage(&self, m: &Modele, pose: &Pose) -> Option<(P3, P3)> {
        let t = self.buste.tete.as_ref()?;
        let (h, q) = m.armature.reperes(pose)[t];
        let devant = qappliquer(qmul(q, qinv(self.tete_repos)), [0.0, 0.0, 1.0]);
        let d = qappliquer(q, [0.0, self.tete_long * 0.5, 0.0]);
        Some((devant, [h[0] + d[0], h[1] + d[1], h[2] + d[2]]))
    }

    /// Pose le dos et la tête sur `pose`. Le corps va à `vitesse`, tourné de `cap` ; `cible`
    /// est le point à regarder, au repère de l'armature.
    #[allow(clippy::too_many_arguments)]
    pub fn poser(&mut self, m: &Modele, mut pose: Pose, vitesse: f32, cap: f32, au_sol: bool, cible: Option<P3>, dt: f32) -> Pose {
        let dt = dt as f64;
        if dt <= 0.0 {
            return pose;
        }
        // L'accélération : vers l'avant, la vitesse qui change ; vers la gauche, la
        // vitesse fois la rotation du cap
        let (av, gauche) = match self.precedent {
            Some((v0, c0)) => {
                let mut dc = (cap - c0) as f64;
                dc -= std::f64::consts::TAU * (dc / std::f64::consts::TAU).round();
                ((vitesse - v0) as f64 / dt, vitesse as f64 * dc / dt)
            }
            None => (0.0, 0.0),
        };
        self.precedent = Some((vitesse, cap));
        let borne = 1.5 * G;
        self.acc = [lisser(self.acc[0], av.clamp(-borne, borne), 10.0, dt), lisser(self.acc[1], gauche.clamp(-borne, borne), 10.0, dt)];
        let voulu = if au_sol {
            [(0.6 * (self.acc[0] / G).atan()).clamp(-PENCHE_AVANT.to_radians(), PENCHE_AVANT.to_radians()), (0.8 * (self.acc[1] / G).atan()).clamp(-PENCHE_COTE.to_radians(), PENCHE_COTE.to_radians())]
        } else {
            [0.0, 0.0]
        };
        self.penche = [lisser(self.penche[0], voulu[0], 8.0, dt), lisser(self.penche[1], voulu[1], 8.0, dt)];

        // Le dos : vers l'avant autour de l'axe de côté (+x), vers la gauche autour de
        // l'axe avant (+z) : un angle négatif y penche le haut vers +x, la gauche
        let n = self.buste.dos.len();
        if n > 0 {
            let q = qmul(qaxe([0.0, 0.0, 1.0], -self.penche[1] / n as f64), qaxe([1.0, 0.0, 0.0], self.penche[0] / n as f64));
            for id in &self.buste.dos {
                let depart = pose.clone();
                ik::tourner(&m.armature, &mut pose, id, q, &depart);
            }
        }

        // Le regard
        let voulu = match (cible, self.visage(m, &pose)) {
            (Some(c), Some((devant, yeux))) => {
                let d = moins(c, yeux);
                let mut l = lacet(d) - lacet(devant);
                l -= std::f64::consts::TAU * (l / std::f64::consts::TAU).round();
                if l.abs() > REGARD_DOS.to_radians() || d.iter().map(|x| x * x).sum::<f64>() < 0.25 {
                    [0.0, 0.0]
                } else {
                    // En courant, le regard reste plus près de la route : la moitié à 3 m/s
                    let cote = REGARD_COTE.to_radians() * (1.0 - 0.5 * (vitesse as f64 / 3.0).clamp(0.0, 1.0));
                    [l.clamp(-cote, cote), (plongee(d) - plongee(devant)).clamp(-REGARD_HAUT.to_radians(), REGARD_BAS.to_radians())]
                }
            }
            _ => [0.0, 0.0],
        };
        self.regard = [lisser(self.regard[0], voulu[0], 6.0, dt), lisser(self.regard[1], voulu[1], 6.0, dt)];
        for (id, k) in [(&self.buste.cou, 0.4), (&self.buste.tete, 0.6)] {
            if let Some(id) = id {
                let depart = pose.clone();
                ik::tourner(&m.armature, &mut pose, id, vers(self.regard[0] * k, self.regard[1] * k), &depart);
            }
        }
        pose
    }

    /// Pour le moniteur : l'inclinaison du dos et le regard.
    pub fn resume(&self) -> (String, String) {
        let deg = |x: f64| x.to_degrees().abs().round() as i32;
        let sens = |x: f64, plus: &str, moins: &str| if deg(x) == 0 { None } else { Some(format!("{}° {}", deg(x), if x > 0.0 { plus } else { moins })) };
        let dire = |l: Vec<Option<String>>, rien: &str| {
            let l: Vec<String> = l.into_iter().flatten().collect();
            if l.is_empty() { rien.to_string() } else { l.join(" · ") }
        };
        (
            dire(vec![sens(self.penche[0], "en avant", "en arrière"), sens(self.penche[1], "à gauche", "à droite")], "droit"),
            dire(vec![sens(self.regard[0], "à gauche", "à droite"), sens(self.regard[1], "en bas", "en haut")], "devant"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gda::armature::Armature;
    use gda::json::{self, J};

    fn hb01() -> Modele {
        let t = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../gda/tests/donnees/hb-01-repos.gda")).unwrap();
        let j = json::lire(&t).unwrap();
        let o = j.get("scene").and_then(|s| s.get("objets")).and_then(J::liste).unwrap().iter().find(|o| o.get("armature").is_some()).unwrap();
        Modele::depuis_armature("hb-01", Armature::depuis_donnees(o.get("armature").unwrap()))
    }

    /// Le haut du thorax : où sa queue penche, au repère de l'armature.
    fn haut(m: &Modele, p: &Pose) -> P3 {
        let r = m.armature.reperes(p);
        let (t, q) = r["o3"];
        let d = qappliquer(q, [0.0, m.armature.trouver("o3").unwrap().longueur, 0.0]);
        let b = r["o2"].0;
        [t[0] + d[0] - b[0], t[1] + d[1] - b[1], t[2] + d[2] - b[2]]
    }

    fn jambes(p: &Pose) -> Vec<Option<gda::armature::Ecart>> {
        ["o1", "o10", "o11", "o12", "o17", "o18", "o19"].iter().map(|i| p.get(*i).copied()).collect()
    }

    #[test]
    fn le_buste_se_trouve() {
        let b = ik::buste(&hb01().armature);
        assert_eq!((b.dos, b.cou, b.tete), (vec!["o2".to_string(), "o3".into()], Some("o4".into()), Some("o5".into())));
    }

    #[test]
    fn le_dos_penche_avec_l_acceleration_et_dans_les_virages() {
        let m = hb01();
        let dt = 1.0 / 60.0;
        let droit = haut(&m, &Pose::new());
        let angle = |v: P3, k: usize| (v[k]).atan2(v[1]).to_degrees() - droit[k].atan2(droit[1]).to_degrees();
        // Il part : de 0 à 3 m/s en un quart de seconde
        let mut p = Posture::de(&m);
        let mut v = 0.0;
        let mut pire: f64 = 0.0;
        for _ in 0..15 {
            v += 0.2;
            let pose = p.poser(&m, Pose::new(), v, 0.0, true, None, dt);
            pire = pire.max(angle(haut(&m, &pose), 2));
            assert_eq!(jambes(&pose), jambes(&Pose::new()), "le dos ne touche pas aux jambes");
        }
        assert!(pire > 3.0 && pire <= PENCHE_AVANT + 0.5, "en avant au départ : {pire}°");
        // À vitesse constante, il se redresse
        let mut pose = Pose::new();
        for _ in 0..120 {
            pose = p.poser(&m, Pose::new(), v, 0.0, true, None, dt);
        }
        assert!(angle(haut(&m, &pose), 2).abs() < 0.3, "redressé : {}°", angle(haut(&m, &pose), 2));
        // Il tourne vers la gauche (le cap augmente) à 3 m/s, 1 rad/s : il penche à gauche
        let mut cap = 0.0;
        for _ in 0..90 {
            cap += dt;
            pose = p.poser(&m, Pose::new(), v, cap, true, None, dt);
        }
        let c = angle(haut(&m, &pose), 0);
        assert!(c > 5.0 && c <= PENCHE_COTE + 0.5, "à gauche dans le virage : {c}°");
        // En l'air, il se redresse
        for _ in 0..120 {
            cap += dt;
            pose = p.poser(&m, Pose::new(), v, cap, false, None, dt);
        }
        assert!(angle(haut(&m, &pose), 0).abs() < 0.3);
    }

    #[test]
    fn la_tete_regarde_le_point_vise_dans_ses_limites() {
        let m = hb01();
        let dt = 1.0 / 60.0;
        let regarde = |c: P3| {
            let mut p = Posture::de(&m);
            let mut pose = Pose::new();
            for _ in 0..120 {
                pose = p.poser(&m, Pose::new(), 0.0, 0.0, true, Some(c), dt);
            }
            let (devant, _) = p.visage(&m, &pose).unwrap();
            assert_eq!(jambes(&pose), jambes(&Pose::new()));
            (lacet(devant).to_degrees(), plongee(devant).to_degrees())
        };
        let (_, yeux) = Posture::de(&m).visage(&m, &Pose::new()).unwrap();
        let a = 45f64.to_radians();
        // À 45° sur sa gauche (+x), à hauteur des yeux
        let (l, b) = regarde([yeux[0] + 5.0 * a.sin(), yeux[1], yeux[2] + 5.0 * a.cos()]);
        assert!((l - 45.0).abs() < 3.0 && b.abs() < 3.0, "à gauche : {l}°, {b}°");
        // À droite et en bas
        let (l, b) = regarde([yeux[0] - 3.0, yeux[1] - 2.0, yeux[2] + 3.0]);
        assert!(l < -40.0 && b > 15.0, "à droite en bas : {l}°, {b}°");
        // Très haut : le regard s'arrête à sa limite
        let (_, b) = regarde([yeux[0], yeux[1] + 10.0, yeux[2] + 1.0]);
        assert!(b < -20.0 && b >= -REGARD_HAUT - 1.0, "en haut : {b}°");
        // Derrière lui : il regarde devant
        let (l, b) = regarde([yeux[0] + 0.5, yeux[1], yeux[2] - 5.0]);
        assert!(l.abs() < 1.0 && b.abs() < 1.0, "derrière : {l}°, {b}°");
    }
}
