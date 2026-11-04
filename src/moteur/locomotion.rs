//! La locomotion : marche et course jouées à une **phase commune**, l'attente dessous,
//! et les couches additives du mélange d'attente par-dessus.
//!
//! - La phase φ ∈ [0, 1[ avance de `dt × v / foulée(w)`, où `v` est la vitesse de la
//!   racine et `foulée(w)` la longueur d'un cycle, marche et course mêlées au poids `w`.
//!   La racine avance donc d'une foulée par cycle : les pieds ne glissent pas. À
//!   l'allure naturelle d'une animation, c'est son propre rythme.
//! - Marche et course sont échantillonnées à la même phase : leurs contacts coïncident
//!   (contact G à φ = 0, contact D à φ = 0,5, comme le disent leurs `marqueurs`).
//! - Le poids de la course vient de la vitesse : marche pure jusqu'à 1,9 m/s, course pure
//!   au-delà de 2,4 m/s. Hors de cette zone, seule la cadence varie.
//! - L'attente est le mélange d'attente de l'armature, s'il y en a un, réduit à ses
//!   couches normales ; ses couches additives (respiration…) se posent ensuite sur tout,
//!   avec leur masque. Sans mélange, c'est l'animation de repos.

use crate::personnage::melanger;
use gda::animation::{Animation, Mode};
use gda::armature::{Armature, Pose};
use gda::couches::{Lecteur, Melange};

/// Début et fin du passage de la marche à la course, en m/s à l'échelle 1.
pub const COURSE_DEBUT: f64 = 1.9;
pub const COURSE_PLEINE: f64 = 2.4;
/// Vitesse naturelle d'une allure qui ne la donne pas (`vitesse` dans ses extras).
pub const VITESSE_PAR_DEFAUT: f64 = 1.4;
/// En deçà, l'allure garde ce rythme le temps que le corps s'arrête.
const VITESSE_MIN: f64 = 0.6;

/// Une allure en boucle : son animation, sa durée et sa foulée, la distance que parcourt
/// la racine en un cycle.
#[derive(Clone, Debug, PartialEq)]
pub struct Allure {
    pub animation: usize,
    pub duree: f64,
    pub foulee: f64,
}

impl Allure {
    fn de(l: &[Animation], k: usize, echelle: f64) -> Allure {
        let a = &l[k];
        let duree = a.duree.max(1e-3);
        Allure { animation: k, duree, foulee: a.vitesse().unwrap_or(VITESSE_PAR_DEFAUT) * echelle * duree }
    }
}

/// Où en est un personnage : sa phase, le temps de son attente, et les poids du
/// mouvement (0 : à l'arrêt, 1 : en mouvement) et de la course.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pas {
    pub phase: f64,
    pub t_attente: f64,
    pub marche: f32,
    pub course: f32,
}

/// Ce qu'un personnage sait jouer, lu une fois de son armature.
pub struct Locomotion {
    pub lecteur: Lecteur,
    pub marche: Option<Allure>,
    pub course: Option<Allure>,
    /// L'attente : les couches normales d'un mélange, ou une animation.
    attente: Option<Melange>,
    repos: Option<usize>,
    /// Les couches additives du mélange d'attente.
    dessus: Option<Melange>,
    echelle: f64,
}

/// L'animation normale dont le nom contient l'un des mots, sans tenir compte de la casse.
fn trouver(l: &[Animation], mots: &[&str], sauf: Option<usize>) -> Option<usize> {
    l.iter().enumerate().position(|(k, a)| {
        let n = a.nom.to_lowercase();
        Some(k) != sauf && a.mode == Mode::Normal && a.reste.iter().all(|x| x.0 != "melange") && mots.iter().any(|m| n.contains(m))
    })
}

/// 0 jusqu'à `a`, 1 à partir de `b`, en douceur entre les deux.
fn transition(v: f64, a: f64, b: f64) -> f64 {
    let t = ((v - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Locomotion {
    pub fn de(a: &Armature, echelle: f64) -> Locomotion {
        let lecteur = Lecteur::de(a);
        let l = &lecteur.animations;
        let marche = trouver(l, &["march", "walk"], None).or_else(|| trouver(l, &["cour", "run"], None));
        let course = trouver(l, &["cour", "run"], marche);
        let mut repos = trouver(l, &["repos", "idle", "attente", "respir"], None);
        // Une seule animation, sans nom connu : c'est la marche
        let marche = marche.or_else(|| (l.len() == 1 && repos.is_none()).then_some(0));
        let melanges = a.melanges();
        let m = melanges
            .iter()
            .find(|m| ["repos", "idle", "attente"].iter().any(|x| m.nom.to_lowercase().contains(x)))
            .or(melanges.first());
        let (mut attente, mut dessus) = (None, None);
        if let Some(m) = m {
            let additive = |c: &gda::couches::Couche| l.iter().find(|x| x.id == c.animation).is_some_and(|x| x.mode == Mode::Additif);
            let (sur, sous): (Vec<_>, Vec<_>) = m.couches.iter().cloned().partition(|c| additive(c));
            if !sous.is_empty() {
                attente = Some(Melange { couches: sous, ..m.clone() });
                repos = None;
            }
            if !sur.is_empty() {
                dessus = Some(Melange { couches: sur, ..m.clone() });
            }
        }
        Locomotion {
            marche: marche.map(|k| Allure::de(l, k, echelle)),
            course: course.map(|k| Allure::de(l, k, echelle)),
            attente,
            repos,
            dessus,
            echelle,
            lecteur,
        }
    }

    /// Le poids de la course à la vitesse `v`.
    pub fn poids_course(&self, v: f64) -> f64 {
        if self.course.is_none() || self.marche.is_none() {
            return 0.0;
        }
        transition(v, COURSE_DEBUT * self.echelle, COURSE_PLEINE * self.echelle)
    }

    /// La foulée, marche et course mêlées au poids `w`.
    pub fn foulee(&self, w: f64) -> f64 {
        match (&self.marche, &self.course) {
            (Some(m), Some(c)) => m.foulee + (c.foulee - m.foulee) * w,
            (Some(m), None) => m.foulee,
            _ => 1.0,
        }
    }

    /// Avance d'un pas de `dt` à la vitesse `v` de la racine.
    pub fn avancer(&self, p: &mut Pas, v: f64, dt: f64) {
        let voulu = (v / (VITESSE_MIN * self.echelle)).clamp(0.0, 1.0) as f32;
        p.marche += (voulu - p.marche) * (1.0 - (-dt as f32 * 10.0).exp());
        let w = self.poids_course(v);
        p.course = w as f32;
        // Une foulée par cycle ; à l'arrêt, le temps que le pas se finisse
        let v = v.max(VITESSE_MIN * self.echelle * p.marche as f64);
        p.phase = (p.phase + dt * v / self.foulee(w).max(1e-6)).rem_euclid(1.0);
        p.t_attente += dt;
    }

    fn jouer(&self, k: usize, t: f64) -> Pose {
        let a = &self.lecteur.animations[k];
        a.pose_a(if a.boucle { t.rem_euclid(a.duree.max(1e-6)) } else { t.min(a.duree) })
    }

    /// La pose de la locomotion seule, à la phase et au poids de course donnés.
    pub fn allure(&self, phase: f64, w: f64) -> Option<Pose> {
        let m = self.marche.as_ref()?;
        let pm = self.jouer(m.animation, phase * m.duree);
        Some(match &self.course {
            Some(c) if w > 0.0 => melanger(&pm, &self.jouer(c.animation, phase * c.duree), w),
            _ => pm,
        })
    }

    /// La pose du personnage.
    pub fn pose(&self, p: &Pas) -> Pose {
        let attente = match (&self.attente, self.repos) {
            (Some(m), _) => self.lecteur.pose(m, p.t_attente),
            (None, Some(k)) => self.jouer(k, p.t_attente),
            _ => Pose::new(),
        };
        let base = match self.allure(p.phase, p.course as f64) {
            Some(l) => melanger(&attente, &l, p.marche as f64),
            None => attente,
        };
        match &self.dessus {
            Some(m) => self.lecteur.poser_sur(base, m, p.t_attente),
            None => base,
        }
    }

    /// Les noms de ce qui est joué, pour le moniteur.
    pub fn resume(&self) -> String {
        let nom = |k: usize| self.lecteur.animations[k].nom.clone();
        let mut l = Vec::new();
        if let Some(m) = self.attente.as_ref().or(self.dessus.as_ref()) {
            l.push(m.nom.clone());
        } else if let Some(k) = self.repos {
            l.push(nom(k));
        }
        l.extend(self.marche.iter().chain(&self.course).map(|a| nom(a.animation)));
        l.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gda::json::{self, J};

    fn hb01() -> Armature {
        let t = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../gda/tests/donnees/hb-01-repos.gda")).unwrap();
        let j = json::lire(&t).unwrap();
        let o = j.get("scene").and_then(|s| s.get("objets")).and_then(J::liste).unwrap().iter().find(|o| o.get("armature").is_some()).unwrap();
        Armature::depuis_donnees(o.get("armature").unwrap())
    }

    /// Le « Repos vivant » : « Repos – appui », puis « Repos » additive sur le haut du
    /// corps.
    fn hb01_vivant() -> Armature {
        let mut a = hb01();
        let mut l = a.animations();
        l[0].mode = Mode::Additif;
        a.regler_animations(&l);
        a.regler_masques(&[gda::couches::Masque { id: String::new(), nom: "haut du corps".into(), os: gda::couches::Masque::os_et_descendants(&a, "o2", 1.0) }]);
        let souffle = gda::couches::Couche { masque: Some("m1".into()), ..gda::couches::Couche::de("a1") };
        a.regler_melanges(&[Melange { couches: vec![gda::couches::Couche::de("a2"), souffle], ..Melange::neuf("Repos vivant") }]);
        a
    }

    #[test]
    fn les_allures_et_l_attente_se_trouvent() {
        let l = Locomotion::de(&hb01_vivant(), 1.0);
        let (m, c) = (l.marche.clone().unwrap(), l.course.clone().unwrap());
        assert_eq!((l.lecteur.animations[m.animation].nom.as_str(), l.lecteur.animations[c.animation].nom.as_str()), ("Marche", "Course"));
        assert!((m.foulee - 1.44).abs() < 1e-9 && (c.foulee - 2.64 * 0.666667).abs() < 1e-6);
        assert_eq!(l.resume(), "Repos vivant, Marche, Course");
        // Sans mélange : l'animation de repos
        assert_eq!(Locomotion::de(&hb01(), 1.0).resume(), "Repos, Marche, Course");
    }

    #[test]
    fn la_course_pese_selon_la_vitesse() {
        let l = Locomotion::de(&hb01(), 1.0);
        assert_eq!(l.poids_course(1.0), 0.0);
        assert_eq!(l.poids_course(1.9), 0.0);
        assert!((l.poids_course(2.15) - 0.5).abs() < 1e-9);
        assert_eq!(l.poids_course(2.4), 1.0);
        assert_eq!(l.poids_course(3.2), 1.0);
    }

    #[test]
    fn la_phase_avance_d_une_foulee_par_cycle() {
        let l = Locomotion::de(&hb01(), 1.0);
        // À 1,44 m/s, la marche à son rythme : un cycle par seconde
        let mut p = Pas { marche: 1.0, ..Pas::default() };
        for _ in 0..30 {
            l.avancer(&mut p, 1.44, 1.0 / 60.0);
        }
        assert!((p.phase - 0.5).abs() < 1e-9, "{}", p.phase);
        // À 3 m/s, la course seule : 3 m pour 1,76 m de foulée
        let mut p = Pas { marche: 1.0, ..Pas::default() };
        l.avancer(&mut p, 3.0, 0.1);
        assert!((p.phase - 0.3 / (2.64 * 0.666667)).abs() < 1e-6);
    }

    /// La dérive des pieds posés, au monde : leur vitesse vers l'avant moyennée sur deux
    /// secondes, la racine avançant à `v` et la phase allant à `cadence` fois celle de
    /// `avancer`. Un pied est posé à moins de 1,5 cm de son point le plus bas. Les
    /// irrégularités de l'animation entre ses clés se compensent ; une cadence fausse fait
    /// dériver le pied.
    fn derive(v: f64, cadence: f64) -> f64 {
        let a = hb01();
        let l = Locomotion::de(&a, 1.0);
        let mut b = a.clone();
        let mut p = Pas { marche: 1.0, ..Pas::default() };
        let dt = 1.0 / 240.0;
        let chevilles = |b: &mut Armature, p: &Pas| {
            b.pose = l.allure(p.phase, l.poids_course(v)).unwrap();
            let m = b.matrices_pose();
            ["o12", "o19"].map(|i| m[i].0)
        };
        let mut l_c = vec![chevilles(&mut b, &p)];
        for _ in 0..(2.0 / dt) as usize {
            let avant = p.phase;
            l.avancer(&mut p, v, dt);
            p.phase = (avant + (p.phase - avant).rem_euclid(1.0) * cadence).rem_euclid(1.0);
            l_c.push(chevilles(&mut b, &p));
        }
        let bas = [0, 1].map(|k| l_c.iter().map(|c| c[k][1]).fold(f64::MAX, f64::min));
        // L'avant est +z : le pied en l'air y va, le pied posé en revient
        let (mut somme, mut n) = (0.0, 0);
        for w in l_c.windows(2) {
            for k in 0..2 {
                if w[0][k][1] < bas[k] + 0.015 {
                    somme += (w[1][k][2] - w[0][k][2]) / dt + v;
                    n += 1;
                }
            }
        }
        somme / n as f64
    }

    /// Relevé sur `hb-01` : la marche dérive de −9 % à toute vitesse, son allure naturelle
    /// comprise ; c'est l'animation, dont le pied posé va plus vite au milieu de l'appui
    /// qu'en moyenne. La course dérive de +3 %. Une cadence fausse de 20 % donne 14 à 31 %.
    /// On vérifie donc que la cadence suit la vitesse (la dérive ne bouge pas avec elle),
    /// et qu'elle fait mieux qu'une cadence fausse.
    #[test]
    fn la_cadence_suit_la_vitesse_et_les_pieds_ne_derivent_pas() {
        let r = |v: f64, c: f64| derive(v, c) / v;
        for (vitesses, naturelle) in [(&[1.0, 1.9][..], 1.44), (&[2.4, 3.2][..], 2.64)] {
            let reference = r(naturelle, 1.0);
            for &v in vitesses {
                let d = r(v, 1.0);
                eprintln!("{v} m/s : dérive {:+.1} %, {:+.1} % à l'allure naturelle", 100.0 * d, 100.0 * reference);
                assert!((d - reference).abs() < 0.015, "à {v} m/s, la cadence ne suit pas la vitesse : {d:+.3} contre {reference:+.3}");
            }
        }
        for v in [1.0, 1.44, 1.9, 2.15, 2.4, 2.64, 3.2] {
            let d = r(v, 1.0).abs();
            assert!(d < 0.1, "à {v} m/s, les pieds dérivent de {:.1} %", 100.0 * d);
            assert!(d < r(v, 0.8).abs() && d < r(v, 1.2).abs(), "à {v} m/s, une cadence fausse fait mieux");
        }
    }

    #[test]
    fn la_respiration_se_pose_sur_la_marche_et_pas_sur_les_jambes() {
        let a = hb01_vivant();
        let l = Locomotion::de(&a, 1.0);
        let p = Pas { phase: 0.3, t_attente: 1.5, marche: 1.0, course: 0.0 };
        let (avec, sans) = (l.pose(&p), l.allure(0.3, 0.0).unwrap());
        let d = |x: &Pose, y: &Pose, i: &str| {
            let r = |q: &Pose| q.get(i).map_or([0.0, 0.0, 0.0, 1.0], |e| e.r);
            1.0 - (0..4).map(|k| r(x)[k] * r(y)[k]).sum::<f64>().abs()
        };
        assert!(d(&avec, &sans, "o3") > 1e-9, "le thorax respire en marchant");
        for jambe in ["o10", "o11", "o12", "o17", "o18", "o19"] {
            assert!(d(&avec, &sans, jambe) < 1e-12, "{jambe} suit la marche seule");
        }
    }
}
