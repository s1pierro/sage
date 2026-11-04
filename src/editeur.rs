//! L'éditeur du monde : choisir un objet d'un appui dans la vue, le déplacer, le tourner,
//! le régler, le dupliquer, le supprimer, en ajouter du catalogue. Chaque changement
//! marque les tuiles touchées, que `Tuiles` écrit ensuite dans le dépôt.

use crate::draw::Color;
use crate::moniteur::dec;
use crate::moteur::objets::hex;
use crate::moteur::{physique, Camera, Entite, Genre, Lieu, Maillages, Monde, Objet, Tuiles, TAILLE};
use crate::ui::{id, sub_id, Id};
use crate::widgets::{Body, Cx};
use std::f32::consts::{FRAC_PI_2, PI, TAU};

/// Portée de l'appui dans la vue, en mètres.
const PORTEE: f32 = 150.0;
/// Les retours gardés, au plus.
const RETOURS: usize = 50;

/// Les couleurs proposées.
pub const PALETTE: [[f32; 3]; 10] = [
    [0.82, 0.78, 0.70],
    [0.86, 0.84, 0.80],
    [0.74, 0.72, 0.68],
    [0.70, 0.72, 0.76],
    [0.62, 0.70, 0.62],
    [0.78, 0.66, 0.56],
    [0.80, 0.45, 0.35],
    [0.55, 0.42, 0.30],
    [0.45, 0.55, 0.75],
    [0.30, 0.30, 0.34],
];

/// Ce que fera le prochain appui dans la vue.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    Choisir,
    /// Poser l'objet choisi là où l'on touche.
    Deplacer,
    /// Poser un objet neuf de ce modèle là où l'on touche.
    Ajouter(Genre),
}

/// De quoi défaire un changement : l'objet tel qu'il était (rien : il n'existait pas),
/// avec la tuile d'origine du monde à ce moment-là.
#[derive(Clone, Debug)]
struct Retour {
    id: String,
    avant: Option<(Objet, Lieu)>,
    origine: [i32; 2],
    /// Le réglage qui l'a fait : les retours d'un même geste se fondent en un.
    cle: Id,
    quoi: &'static str,
}

pub struct Editeur {
    pub mode: Mode,
    retours: Vec<Retour>,
    /// Le modèle dont on demande un nouveau fichier (l'application ouvre le sélecteur).
    pub remplacement: Option<String>,
    /// Le modèle dont il faut retourner les faces.
    pub retournement: Option<String>,
}

impl Default for Editeur {
    fn default() -> Editeur {
        Editeur { mode: Mode::Choisir, retours: Vec::new(), remplacement: None, retournement: None }
    }
}

/// Le nom affiché d'un objet : celui de son modèle importé, sinon de son genre.
fn nom_de(monde: &Monde, o: &Objet) -> String {
    match o.genre {
        Genre::Modele(k) => monde.modeles.get(k).map_or("Modèle".into(), |m| m.nom.clone()),
        g => g.nom().into(),
    }
}

/// Le demi-côté au sol d'un objet, pour le poser contre une paroi.
fn rayon(monde: &Monde, o: &Objet) -> f32 {
    let (lo, hi) = o.boite(&monde.modeles);
    [lo[0].abs(), hi[0].abs(), lo[2].abs(), hi[2].abs()].into_iter().fold(0.0, f32::max)
}

/// L'objet choisi, s'il existe encore.
pub fn choisi(monde: &Monde) -> Option<(Entite, Objet, Lieu)> {
    let e = monde.choisie?;
    Some((e, *monde.objet.get(e)?, *monde.lieu.get(e)?))
}

fn angle(c: f32) -> f32 {
    c.rem_euclid(TAU)
}

impl Editeur {
    /// Combien de changements peuvent se défaire.
    pub fn retours(&self) -> usize {
        self.retours.len()
    }

    /// Retient l'état de `e` avant un changement.
    fn retenir(&mut self, monde: &Monde, e: Entite, cle: Id, quoi: &'static str) {
        let Some(id) = monde.ident.get(e).cloned() else { return };
        if cle != 0 && self.retours.last().is_some_and(|r| r.cle == cle && r.id == id) {
            return;
        }
        let avant = monde.objet.get(e).copied().zip(monde.lieu.get(e).copied());
        self.retours.push(Retour { id, avant, origine: monde.origine, cle, quoi });
        if self.retours.len() > RETOURS {
            self.retours.remove(0);
        }
    }

    /// Défait le dernier changement.
    pub fn annuler(&mut self, monde: &mut Monde, maillages: &mut Maillages, tuiles: &mut Tuiles) {
        let Some(r) = self.retours.pop() else { return };
        let e = monde.trouver(&r.id);
        match (r.avant, e) {
            (Some((objet, mut lieu)), e) => {
                // Le monde a pu se décaler depuis
                let (dx, dz) = ((r.origine[0] - monde.origine[0]) as f32 * TAILLE, (r.origine[1] - monde.origine[1]) as f32 * TAILLE);
                lieu.pos[0] += dx;
                lieu.pos[2] += dz;
                let lieu = Lieu::en(lieu.pos, lieu.cap);
                let e = e.unwrap_or_else(|| monde.creer(&r.id));
                tuiles.modifier(monde, maillages, e, objet, lieu);
                monde.choisie = Some(e);
            }
            (None, Some(e)) => tuiles.oter(monde, maillages, e),
            (None, None) => {}
        }
        self.mode = Mode::Choisir;
    }

    /// Un appui en (x, y) dans la vue `camera`, dont le lacet est `lacet`.
    pub fn appui(&mut self, monde: &mut Monde, maillages: &mut Maillages, tuiles: &mut Tuiles, camera: &Camera, x: f32, y: f32, lacet: f32) {
        let d = camera.rayon(x, y);
        let deplace = (self.mode == Mode::Deplacer).then_some(monde.choisie).flatten();
        // Le rayon ne voit pas l'objet qu'on déplace
        let retire = deplace.and_then(|e| monde.obstacles.ote(e).map(|f| (e, f)));
        let i = physique::lancer(monde, camera.oeil, d, PORTEE);
        if let Some((e, f)) = retire {
            monde.obstacles.mettre(e, f);
        }
        // Là où poser un objet : sur ce qu'on touche ; contre une paroi, à côté d'elle
        let poser = |o: &Objet, i: &physique::Impact| {
            let mut p = i.point;
            if i.normale[1] < 0.5 {
                let r = rayon(monde, o);
                let l = i.normale[0].hypot(i.normale[2]).max(1e-6);
                p[0] += i.normale[0] / l * r;
                p[2] += i.normale[2] / l * r;
                p[1] = physique::sol(monde, p[0], p[2], p[1] + 0.5).map_or(p[1], |s| s.0);
            }
            p
        };
        match self.mode {
            Mode::Choisir => monde.choisie = i.and_then(|i| i.entite).filter(|e| monde.objet.get(*e).is_some()),
            Mode::Deplacer => {
                if let (Some((e, o, l)), Some(i)) = (choisi(monde), i) {
                    let p = poser(&o, &i);
                    self.retenir(monde, e, 0, "le déplacement");
                    tuiles.modifier(monde, maillages, e, o, Lieu::en(p, l.cap));
                }
                self.mode = Mode::Choisir;
            }
            Mode::Ajouter(g) => {
                if let Some(i) = i {
                    let o = Objet::neuf(g);
                    let p = poser(&o, &i);
                    // De face, l'avant de l'objet loin de la caméra, au quart de tour
                    let cap = angle(((lacet + PI) / FRAC_PI_2).round() * FRAC_PI_2);
                    let e = tuiles.ajouter(monde, maillages, o, Lieu::en(p, cap));
                    self.retours.push(Retour { id: monde.ident.get(e).cloned().unwrap_or_default(), avant: None, origine: monde.origine, cle: 0, quoi: "l'ajout" });
                    monde.choisie = Some(e);
                }
                self.mode = Mode::Choisir;
            }
        }
    }

    /// Le corps du panneau de l'éditeur.
    pub fn panneau(&mut self, cx: &mut Cx, b: &mut Body, monde: &mut Monde, maillages: &mut Maillages, tuiles: &mut Tuiles) {
        match choisi(monde) {
            Some((e, o, l)) => self.reglages(cx, b, monde, maillages, tuiles, e, o, l),
            None => {
                b.y += 6.0 * cx.dp;
                cx.paragraph(b, "Touchez un objet dans la vue pour le choisir, ou ajoutez-en un du catalogue.");
            }
        }

        cx.section(b, "AJOUTER");
        let noms: Vec<&str> = Genre::TOUS.iter().map(|g| g.nom()).collect();
        let actif = match self.mode {
            Mode::Ajouter(g) => Genre::TOUS.iter().position(|x| *x == g),
            _ => None,
        };
        if let Some(k) = cx.liens(b, id("editeur-ajouter"), &noms, actif) {
            self.mode = if actif == Some(k) { Mode::Choisir } else { Mode::Ajouter(Genre::TOUS[k]) };
        }
        // Les modèles importés, sous leur nom
        let importes: Vec<(Genre, String)> = monde.modeles.presents().into_iter().map(|k| (Genre::Modele(k), monde.modeles.get(k).map_or(String::new(), |m| m.nom.clone()))).collect();
        if !importes.is_empty() {
            let refs: Vec<&str> = importes.iter().map(|(_, n)| n.as_str()).collect();
            let actif = match self.mode {
                Mode::Ajouter(g) => importes.iter().position(|(x, _)| *x == g),
                _ => None,
            };
            if let Some(k) = cx.liens(b, id("editeur-importes"), &refs, actif) {
                self.mode = if actif == Some(k) { Mode::Choisir } else { Mode::Ajouter(importes[k].0) };
            }
        }
        if let Mode::Ajouter(g) = self.mode {
            let quoi = match g {
                Genre::Modele(_) => format!("« {} »", nom_de(monde, &Objet::neuf(g))),
                _ => article(g).to_string(),
            };
            cx.paragraph(b, &format!("Touchez dans la vue l'endroit où poser {quoi}."));
        } else {
            cx.paragraph(b, "Pour ajouter un décor ou un bâtiment, touchez « Ouvrir un .glb » : un modèle sans squelette entre au catalogue.");
        }

        cx.section(b, "ENREGISTREMENT");
        let lues = tuiles.chargees.values().filter(|c| c.lue).count();
        cx.mesure(b, "Tuiles modifiées chargées", &lues.to_string());
        let attente = tuiles.a_ecrire();
        cx.mesure(b, "État", &if attente > 0 { format!("{attente} en attente") } else { "à jour".to_string() });
        if let Some(err) = &tuiles.erreur {
            cx.paragraph(b, err);
        }
        if let Some(r) = self.retours.last() {
            let l = format!("Annuler {}", r.quoi);
            if cx.liens(b, id("editeur-annuler"), &[&l], None).is_some() {
                self.annuler(monde, maillages, tuiles);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn reglages(&mut self, cx: &mut Cx, b: &mut Body, monde: &mut Monde, maillages: &mut Maillages, tuiles: &mut Tuiles, e: Entite, o: Objet, l: Lieu) {
        cx.section(b, &nom_de(monde, &o).to_uppercase());
        let ident = monde.ident.get(e).cloned().unwrap_or_default();
        cx.mesure(b, "Identifiant", &ident);
        let k = Tuiles::tuile_de(monde, l.pos);
        let c = Tuiles::coin(monde, k);
        cx.mesure(b, "Tuile", &format!("{} ; {}", k.0, k.1));
        cx.mesure(b, "Dans la tuile", &format!("{} ; {}", dec(l.pos[0] - c[0], 1), dec(l.pos[2] - c[2], 1)));
        cx.mesure(b, "Altitude", &format!("{} m", dec(l.pos[1], 2)));
        let actions = ["Déplacer", "Dupliquer", "Supprimer", "Laisser"];
        let actif = (self.mode == Mode::Deplacer).then_some(0);
        match cx.liens(b, id("editeur-actions"), &actions, actif) {
            Some(0) => self.mode = if self.mode == Mode::Deplacer { Mode::Choisir } else { Mode::Deplacer },
            Some(1) => {
                // À côté, vers la droite de l'objet
                let r = crate::moteur::objets::tourner([2.0 * rayon(monde, &o) + 0.5, 0.0, 0.0], l.cap);
                let p = [l.pos[0] + r[0], l.pos[1], l.pos[2] + r[2]];
                let p = [p[0], physique::sol(monde, p[0], p[2], p[1] + 1.0).map_or(p[1], |s| s.0.max(p[1] - 1.0)), p[2]];
                let n = tuiles.ajouter(monde, maillages, o, Lieu::en(p, l.cap));
                self.retours.push(Retour { id: monde.ident.get(n).cloned().unwrap_or_default(), avant: None, origine: monde.origine, cle: 0, quoi: "la copie" });
                monde.choisie = Some(n);
                return;
            }
            Some(2) => {
                self.retenir(monde, e, 0, "la suppression");
                tuiles.oter(monde, maillages, e);
                self.mode = Mode::Choisir;
                return;
            }
            Some(3) => {
                monde.choisie = None;
                self.mode = Mode::Choisir;
                return;
            }
            _ => {}
        }
        if self.mode == Mode::Deplacer {
            cx.paragraph(b, "Touchez dans la vue l'endroit où le poser.");
        }
        if let Genre::Modele(k) = o.genre {
            let m = monde.modeles.get(k);
            match m.and_then(|m| m.gabarit.as_ref()) {
                Some(g) => {
                    let n = g.triangles.len();
                    cx.mesure(b, "Triangles", &n.to_string());
                    if n > crate::catalogue::TRIANGLES_LOURD {
                        cx.paragraph(b, "Modèle lourd : il ralentit le dessin et les collisions.");
                    }
                }
                None => cx.paragraph(b, &format!("Le modèle « {} » n'est pas au catalogue : l'objet reste une boîte grise. Ouvrez son .glb pour qu'il revienne.", monde.modeles.id(k))),
            }
            let present = monde.modeles.get(k).is_some_and(|m| m.gabarit.is_some());
            let liens: &[&str] = if present { &["Remplacer le fichier du modèle", "Retourner ses faces"] } else { &["Remplacer le fichier du modèle"] };
            match cx.liens(b, id("editeur-remplacer"), liens, None) {
                Some(0) => self.remplacement = Some(monde.modeles.id(k).to_string()),
                Some(_) => self.retournement = Some(monde.modeles.id(k).to_string()),
                None => {}
            }
        }

        cx.section(b, "RÉGLAGES");
        let (mut x, mut lieu) = (o, l);
        let cle_cap = id("editeur-cap");
        let mut deg = angle(l.cap).to_degrees();
        cx.slider(b, cle_cap, "Cap", &format!("{} °", dec(deg, 0)), &mut deg, 0.0, 360.0);
        let cap = angle(((deg / 5.0).round() * 5.0).to_radians());
        let mut cle = 0;
        if (cap - angle(l.cap)).abs() > 1e-4 {
            lieu.cap = cap;
            cle = cle_cap;
        }
        let noms = o.genre.dimensions();
        for (i, nom) in noms.iter().enumerate() {
            if nom.is_empty() {
                continue;
            }
            let hi = match (o.genre, i) {
                (Genre::Poteau, 0) => 1.5,
                (Genre::Rampe, 1) => 6.0,
                (Genre::Dalle, 1) => 1.0,
                _ => 12.0,
            };
            let k = sub_id(id("editeur-dim"), i);
            let mut v = o.dim[i];
            cx.slider(b, k, nom, &format!("{} m", dec(v, 2)), &mut v, 0.05, hi);
            let v = ((v / 0.05).round() * 0.05).max(0.05);
            if (v - o.dim[i]).abs() > 1e-4 {
                x.dim[i] = v;
                cle = k;
            }
        }
        if let Genre::Modele(_) = o.genre {
            // L'échelle, sur une règle logarithmique : de ÷ 100 (un fichier en centimètres)
            // à × 100, arrondie à trois chiffres significatifs
            let k = id("editeur-echelle");
            let mut t = o.dim[0].log10();
            let e = o.dim[0];
            let texte = if e < 0.1 { dec(e, 3) } else { dec(e, 2) };
            cx.slider(b, k, "Échelle", &format!("× {texte}"), &mut t, -2.0, 2.0);
            let v = 10f32.powf(t);
            let pas = 10f32.powf(v.log10().floor() - 2.0);
            let v = (v / pas).round() * pas;
            if (v - o.dim[0]).abs() > 1e-4 {
                x.dim = [v; 3];
                cle = k;
            }
        }
        x = x.borne();
        // L'enfoncement : de combien la base est sous le relief
        let sol = crate::moteur::systemes::hauteur_sol(monde, l.pos[0], l.pos[2]).unwrap_or(l.pos[1]);
        let k = id("editeur-enfoncement");
        let mut enf = sol - l.pos[1];
        cx.slider(b, k, "Enfoncement", &format!("{} m", dec(enf, 2)), &mut enf, -1.0, 3.0);
        let enf = (enf / 0.01).round() * 0.01;
        if (enf - (sol - l.pos[1])).abs() > 0.005 {
            lieu.pos[1] = sol - enf;
            cle = k;
        }
        if let Genre::Modele(_) = o.genre {
            if cle != 0 && (x != o || lieu.pos != l.pos || lieu.cap != l.cap) {
                self.retenir(monde, e, cle, "le réglage");
                tuiles.modifier(monde, maillages, e, x, Lieu::en(lieu.pos, lieu.cap));
            }
            return;
        }
        let teintes: Vec<Color> = PALETTE.iter().map(|c| [c[0], c[1], c[2]].map(|v| (v * 255.0).round() as u8)).map(|c| [c[0], c[1], c[2], 255]).collect();
        let choisie = PALETTE.iter().position(|c| hex(*c) == hex(o.couleur));
        if let Some(i) = cx.nuancier(b, id("editeur-couleur"), &teintes, choisie) {
            x.couleur = PALETTE[i];
            cle = id("editeur-couleur");
        }
        if cle != 0 && (x != o || lieu.pos != l.pos || lieu.cap != l.cap) {
            self.retenir(monde, e, cle, "le réglage");
            tuiles.modifier(monde, maillages, e, x, Lieu::en(lieu.pos, lieu.cap));
        }
    }
}

fn article(g: Genre) -> &'static str {
    match g {
        Genre::Bloc => "le bloc",
        Genre::Dalle => "la dalle",
        Genre::Poteau => "le poteau",
        Genre::Rampe => "la rampe",
        Genre::Modele(_) => "le modèle",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math;

    /// Un monde plat de neuf tuiles, sans objets, et une caméra qui plonge sur (32, 0, 32).
    fn monde() -> (Monde, Maillages, Tuiles, Camera) {
        struct Plat;
        impl crate::moteur::Source for Plat {
            fn tuile(&self, _: i32, _: i32) -> crate::moteur::Contenu {
                let r = crate::moteur::Relief { n: 4, h: vec![0.0; 25] };
                crate::moteur::Contenu { normales: r.normales(), relief: r, couleurs: vec![[0.5; 3]; 25], objets: vec![] }
            }
        }
        let mut t = Tuiles::new(Box::new(Plat), 1, 1);
        let (mut m, mut r) = (Monde::default(), Maillages::default());
        t.suivre(&mut m, &mut r, (0, 0), usize::MAX);
        let oeil = [32.0, 10.0, 42.0];
        let cam = Camera { vue: math::look_at(oeil, [32.0, 0.0, 32.0], [0.0, 1.0, 0.0]), proj: math::perspective(1.0, 1.0, 0.1, 500.0), oeil, fenetre: [0.0, 0.0, 400.0, 400.0] };
        (m, r, t, cam)
    }

    #[test]
    fn ajouter_choisir_deplacer_et_annuler() {
        let (mut m, mut r, mut t, cam) = monde();
        let mut ed = Editeur { mode: Mode::Ajouter(Genre::Bloc), ..Default::default() };
        // Le centre de la vue : le bloc s'y pose, et reste choisi
        ed.appui(&mut m, &mut r, &mut t, &cam, 200.0, 200.0, 0.0);
        let (e, _, l) = choisi(&m).expect("posé et choisi");
        assert!((l.pos[0] - 32.0).abs() < 0.05 && (l.pos[2] - 32.0).abs() < 0.05 && l.pos[1].abs() < 0.05, "{:?}", l.pos);
        assert_eq!(m.ident.get(e).unwrap(), "o/0");
        assert_eq!(ed.mode, Mode::Choisir);
        // Toucher à côté le laisse ; le toucher le reprend
        ed.appui(&mut m, &mut r, &mut t, &cam, 20.0, 380.0, 0.0);
        assert_eq!(m.choisie, None);
        ed.appui(&mut m, &mut r, &mut t, &cam, 200.0, 200.0, 0.0);
        assert_eq!(m.choisie, Some(e));
        // Déplacé plus bas dans la vue : plus près de la caméra
        ed.mode = Mode::Deplacer;
        ed.appui(&mut m, &mut r, &mut t, &cam, 200.0, 330.0, 0.0);
        let z = m.lieu.get(e).unwrap().pos[2];
        assert!(z > 33.0, "{z}");
        assert!(m.obstacles.get(e).is_some(), "ses obstacles reviennent");
        assert_eq!(t.a_ecrire(), 2, "sa tuile et la fiche");
        // Annuler le déplacement, puis l'ajout
        ed.annuler(&mut m, &mut r, &mut t);
        assert!((m.lieu.get(e).unwrap().pos[2] - 32.0).abs() < 0.05);
        ed.annuler(&mut m, &mut r, &mut t);
        assert!(m.trouver("o/0").is_none());
        assert_eq!(ed.retours(), 0);
    }

    #[test]
    fn un_objet_tourne_de_biais_garde_ses_obstacles_a_jour() {
        let (mut m, mut r, mut t, cam) = monde();
        let mut ed = Editeur { mode: Mode::Ajouter(Genre::Rampe), ..Default::default() };
        ed.appui(&mut m, &mut r, &mut t, &cam, 200.0, 200.0, 0.0);
        let (e, o, l) = choisi(&m).unwrap();
        // La caméra regarde vers -z : la rampe part vers -z
        assert!((l.cap - PI).abs() < 1e-4, "{}", l.cap);
        // Le haut de la rampe : loin de la caméra, à sa montée
        let h = physique::sol(&m, l.pos[0], l.pos[2] - o.dim[2] + 0.2, 10.0).unwrap();
        assert!((h.0 - l.pos[1] - o.dim[1] * (o.dim[2] - 0.2) / o.dim[2]).abs() < 0.05, "{h:?}");
        assert_eq!(h.2, Some(e));
    }
}
