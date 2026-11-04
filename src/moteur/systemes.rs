//! Les systèmes : des fonctions qui font avancer le monde, chacune sur les composants
//! dont elle a besoin.

use super::entites::{Entite, Monde};
use super::horloge::PAS_FIXE;
use super::tuiles::TAILLE;

/// Vitesse au plus, commande au bout.
pub const VITESSE_MAX: f32 = 3.2;

/// Début d'un pas : chaque lieu retient où il en était, pour l'interpolation.
pub fn retenir(monde: &mut Monde) {
    for (_, l) in monde.lieu.iter_mut() {
        l.retenir();
    }
}

/// La commande de marche de `e`, vers `dir` au sol (x, z), de longueur 0 à 1 selon
/// l'allure : sa vitesse et sa direction, que la physique suit (`physique::deplacer`). Le
/// pas suit la direction tout de suite ; seul le corps met un instant à tourner. À
/// l'arrêt, l'élan garde la dernière direction.
pub fn marcher(monde: &mut Monde, e: Entite, dir: [f32; 2]) {
    let (Some(m), Some(l)) = (monde.mobile.get_mut(e), monde.lieu.get_mut(e)) else { return };
    let dt = PAS_FIXE;
    let force = dir[0].hypot(dir[1]).min(1.0);
    // Accélère et freine en un quart de seconde environ
    m.vitesse += (force * VITESSE_MAX - m.vitesse) * (1.0 - (-dt * 8.0).exp());
    if force > 0.05 {
        let n = dir[0].hypot(dir[1]);
        m.elan = [dir[0] / n, dir[1] / n];
        let mut ecart = m.elan[0].atan2(m.elan[1]) - l.cap;
        ecart -= std::f32::consts::TAU * (ecart / std::f32::consts::TAU).round();
        l.cap += ecart * (1.0 - (-dt * 10.0).exp());
        l.cap -= std::f32::consts::TAU * (l.cap / std::f32::consts::TAU).round();
    }
}

/// La hauteur du sol en (x, z), sur le relief de la tuile qui contient ce point, si elle
/// est chargée.
pub fn hauteur_sol(monde: &Monde, x: f32, z: f32) -> Option<f32> {
    monde.relief.iter().find_map(|(e, r)| {
        let o = monde.lieu.get(e)?.pos;
        let (u, v) = (x - o[0], z - o[2]);
        ((0.0..=TAILLE).contains(&u) && (0.0..=TAILLE).contains(&v)).then(|| o[1] + r.hauteur(u, v))
    })
}


/// L'origine suit `e` : dès qu'il sort de la tuile d'origine, le monde se décale pour
/// qu'il y revienne.
pub fn suivre_origine(monde: &mut Monde, e: Entite) {
    let Some(p) = monde.lieu.get(e).map(|l| l.pos) else { return };
    let (di, dj) = ((p[0] / TAILLE).floor() as i32, (p[2] / TAILLE).floor() as i32);
    monde.rebaser(di, dj);
}

/// Les squelettes prennent leur pose : l'attente, la locomotion à la vitesse du corps, et
/// les couches additives par-dessus. Voir `locomotion`.
pub fn animer(monde: &mut Monde, dt: f32, u: f32) {
    let qui: Vec<Entite> = monde.squelette.iter().map(|(e, _)| e).collect();
    for e in qui {
        let vitesse = monde.mobile.get(e).map_or(0.0, |m| m.vitesse);
        let au_sol = monde.corps.get(e).is_none_or(|c| c.au_sol);
        let lieu = monde.lieu.get(e).map(|l| l.interpole(u));
        let Some(s) = monde.squelette.get_mut(e) else { continue };
        let l = s.locomotion.clone();
        l.avancer(&mut s.pas, vitesse as f64, dt as f64);
        let pose = l.pose(&s.pas);
        let (modele, mut pieds) = (s.modele.clone(), std::mem::take(&mut s.pieds));
        let pose = match lieu {
            Some((p, cap)) => {
                // Le dos et le regard, puis les pieds, en dernier, au monde tel qu'il est
                // dessiné
                let cible = s.regard.map(|w| super::pieds::vers_armature(&modele, p, cap, w));
                let pose = s.posture.poser(&modele, pose, vitesse, cap, au_sol, cible, dt);
                pieds.caler(monde, &modele, p, cap, au_sol, pose, dt)
            }
            None => pose,
        };
        let s = monde.squelette.get_mut(e).unwrap();
        s.pieds = pieds;
        s.os = modele.matrices(&pose);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::moteur::{Lieu, Mobile};

    fn marcheur(m: &mut Monde) -> Entite {
        let e = m.creer("j");
        m.lieu.mettre(e, Lieu::en([0.0; 3], 0.0));
        m.mobile.mettre(e, Mobile { vitesse: 0.0, elan: [0.0, 1.0], rayon: 0.3 });
        m.corps.mettre(e, crate::moteur::Corps::new(1.8));
        e
    }

    #[test]
    fn la_vitesse_suit_la_commande_et_retombe() {
        let mut m = Monde::default();
        let e = marcheur(&mut m);
        for _ in 0..120 {
            marcher(&mut m, e, [0.0, -1.0]);
        }
        let x = *m.mobile.get(e).unwrap();
        assert!((x.vitesse - VITESSE_MAX).abs() < 0.05 && x.elan == [0.0, -1.0]);
        assert!((m.lieu.get(e).unwrap().cap.abs() - std::f32::consts::PI).abs() < 0.05);
        for _ in 0..120 {
            marcher(&mut m, e, [0.0, 0.0]);
        }
        assert!(m.mobile.get(e).unwrap().vitesse < 0.01);
    }

    #[test]
    fn le_marcheur_suit_le_sol_et_ramene_l_origine() {
        use crate::moteur::{physique, Relief};
        let mut m = Monde::default();
        let e = marcheur(&mut m);
        m.lieu.get_mut(e).unwrap().pos = [60.0, 0.0, 10.0];
        // Trois tuiles, chacune une marche de 20 cm plus haut
        for i in 0..3 {
            let t = m.creer(&format!("t{i}"));
            m.lieu.mettre(t, Lieu::en([i as f32 * TAILLE, i as f32 * 0.2, 0.0], 0.0));
            m.relief.mettre(t, Relief { n: 1, h: vec![0.0, 0.0, 0.0, 0.0] });
        }
        physique::poser(&mut m);
        assert_eq!(m.lieu.get(e).unwrap().pos[1], 0.0);
        for _ in 0..120 {
            marcher(&mut m, e, [1.0, 0.0]);
            physique::deplacer(&mut m);
            suivre_origine(&mut m, e);
        }
        let l = *m.lieu.get(e).unwrap();
        assert_eq!(m.origine, [1, 0], "passé dans la tuile suivante");
        assert!((0.0..TAILLE).contains(&l.pos[0]));
        assert!((l.pos[1] - 0.2).abs() < 1e-5, "sur le relief de la deuxième tuile : {}", l.pos[1]);
        assert!(m.absolu(l.pos)[0] > 64.0);
    }
}
