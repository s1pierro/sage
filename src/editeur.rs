//! L'éditeur du monde, en deux onglets :
//! - Ajout : choisir un objet d'un appui dans la vue, le déplacer et le tourner par ses
//!   poignées, le régler, le dupliquer, le supprimer, en ajouter du catalogue ; choisir un
//!   bâtiment d'un monde matérialisé, le régler, le déplacer, le démolir ;
//! - Topologie : retoucher le relief au pinceau (`crate::terrain`).
//!
//! Chaque changement marque les tuiles touchées, que `Tuiles` écrit ensuite dans le dépôt.

use crate::draw::Color;
use crate::moniteur::dec;
use crate::moteur::objets::hex;
use crate::moteur::{physique, Camera, Entite, Genre, Lieu, Maillages, Monde, Objet, Tuiles, TAILLE};
use crate::terrain::{self, Avant, Outil, Pinceau};
use crate::ui::{id, sub_id, Id};
use crate::widgets::{Body, Cx};
use crate::regions::{Bati, Regions};
use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::sync::Arc;

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
    /// Poser le bâtiment choisi là où l'on touche.
    DeplacerBati,
}

/// De quoi défaire un changement.
#[derive(Clone, Debug)]
enum Retour {
    /// L'objet tel qu'il était (rien : il n'existait pas), avec la tuile d'origine du
    /// monde à ce moment-là.
    Objet {
        id: String,
        avant: Option<(Objet, Lieu)>,
        origine: [i32; 2],
        /// Le réglage qui l'a fait : les retours d'un même geste se fondent en un.
        cle: Id,
        quoi: &'static str,
    },
    /// Les tuiles qu'un coup de pinceau a touchées, telles qu'elles étaient.
    Relief(Avant),
    /// Un terrassement : le relief d'avant, et l'objet posé dessus.
    Terrassement(Avant, Box<Retour>),
    /// Un bâtiment tel qu'il était, et le réglage qui l'a changé.
    Bati { avant: Bati, cle: Id, quoi: &'static str },
}

impl Retour {
    fn quoi(&self) -> &'static str {
        match self {
            Retour::Objet { quoi, .. } => quoi,
            Retour::Relief(_) => "le coup de pinceau",
            Retour::Terrassement(..) => "le terrassement",
            Retour::Bati { quoi, .. } => quoi,
        }
    }
}

/// Les onglets de l'éditeur.
pub const AJOUT: usize = 0;
pub const TOPOLOGIE: usize = 1;
pub const CARTE: usize = 2;
pub const ONGLETS: [&str; 3] = ["Ajout", "Topologie", "Carte"];

/// Les poignées de l'objet choisi.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Poignee {
    /// Le disque du centre : glisser sur le sol.
    Plan,
    /// Les flèches : le long des axes de l'objet, ou vers le haut.
    X,
    Z,
    Y,
    /// La pastille de l'anneau : le cap.
    Cap,
}

impl Poignee {
    pub const TOUTES: [Poignee; 5] = [Poignee::Plan, Poignee::X, Poignee::Z, Poignee::Y, Poignee::Cap];
}

/// Une poignée tenue : où le doigt s'est posé, et l'objet à ce moment-là.
#[derive(Clone, Copy, Debug)]
struct Saisie {
    poignee: Poignee,
    doigt: [f32; 2],
    lieu: Lieu,
    /// Là où le doigt touchait le plan du sol de l'objet.
    prise: Option<[f32; 3]>,
}

pub struct Editeur {
    pub mode: Mode,
    retours: Vec<Retour>,
    /// Le modèle dont on demande un nouveau fichier (l'application ouvre le sélecteur).
    pub remplacement: Option<String>,
    /// Le bâtiment à exporter en .glb, pour l'application.
    pub export_bati: Option<Bati>,
    /// Le bâtiment (région, identifiant) qu'un .glb à choisir enrichira.
    pub enrichir_bati: Option<((i32, i32), String)>,
    /// Le modèle dont il faut retourner les faces.
    pub retournement: Option<String>,
    /// L'onglet ouvert : `AJOUT`, `TOPOLOGIE` ou `CARTE`.
    pub onglet: usize,
    pub pinceau: Pinceau,
    /// Le coup de pinceau en cours : les tuiles touchées, telles qu'avant lui.
    coup: Option<Avant>,
    saisie: Option<Saisie>,
    /// Les gestes de poignée faits : chacun est une étape d'annulation.
    gestes: usize,
    /// L'écart des courbes de niveau montrées, en mètres (0 : aucune).
    pub isolignes: f32,
    /// « Régénérer la carte » attend sa confirmation, avec cette graine.
    confirmer: Option<u32>,
    /// La carte à régénérer avec cette graine (l'application le fait).
    pub regeneration: Option<u32>,
    /// L'érosion de cette graine se calcule : la carte se régénère quand elle est prête.
    pub erosion: Option<u32>,
    /// Le nom du lieu réel de la carte, s'il y en a un.
    pub lieu: Option<String>,
    /// L'état du monde matérialisé (régions `.stuff`), s'il y a un lieu réel.
    pub monde: Option<String>,
    /// Le sélecteur de fichiers est demandé pour un lieu réel.
    pub importer: bool,
    /// Les réglages du terrassement sous l'objet choisi.
    pub terrasse: Terrassement,
    /// Le monde matérialisé, dont on peut retoucher les bâtiments.
    pub regions: Option<Arc<Regions>>,
    /// Le bâtiment choisi : sa région, son identifiant.
    pub bati: Option<((i32, i32), String)>,
    /// Le bâtiment en surbrillance : son entité et sa forme d'alors.
    surbrillance: Option<(Entite, Bati)>,
    /// Le maillage de la surbrillance, que la scène dessine choisi.
    pub surbrillance_maillage: Option<crate::moteur::MaillageId>,
    /// Ce qui s'est passé à dire au joueur.
    pub message: Option<String>,
}

/// Comment terrasser sous un objet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Terrassement {
    pub sous_sol: bool,
    /// La profondeur du sous-sol, en mètres.
    pub profondeur: f32,
    /// La plateforme déborde de l'objet de tant, de chaque côté.
    pub marge: f32,
    /// La pente du talus, en degrés.
    pub talus: f32,
}

impl Default for Terrassement {
    fn default() -> Terrassement {
        Terrassement { sous_sol: false, profondeur: 3.0, marge: 1.0, talus: 35.0 }
    }
}

/// Une graine tirée de l'heure.
fn graine_au_hasard() -> u32 {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    ((t ^ (t >> 29)).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40) as u32 % 1_000_000
}

impl Default for Editeur {
    fn default() -> Editeur {
        Editeur { mode: Mode::Choisir, retours: Vec::new(), remplacement: None, export_bati: None, enrichir_bati: None, retournement: None, onglet: AJOUT, pinceau: Pinceau::default(), coup: None, saisie: None, gestes: 0, isolignes: 0.0, confirmer: None, regeneration: None, erosion: None, lieu: None, monde: None, importer: false, terrasse: Terrassement::default(), regions: None, bati: None, surbrillance: None, surbrillance_maillage: None, message: None }
    }
}

/// Là où le rayon de la vue par (x, y) coupe le plan horizontal à la hauteur `h`.
fn sur_plan(camera: &Camera, x: f32, y: f32, h: f32) -> Option<[f32; 3]> {
    let d = camera.rayon(x, y);
    if d[1].abs() < 1e-4 {
        return None;
    }
    let t = (h - camera.oeil[1]) / d[1];
    (t > 0.0 && t < 2000.0).then(|| [camera.oeil[0] + d[0] * t, h, camera.oeil[2] + d[2] * t])
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
        if cle != 0 && matches!(self.retours.last(), Some(Retour::Objet { cle: c, id: i, .. }) if *c == cle && *i == id) {
            return;
        }
        let avant = monde.objet.get(e).copied().zip(monde.lieu.get(e).copied());
        self.pousser(Retour::Objet { id, avant, origine: monde.origine, cle, quoi });
    }

    fn pousser(&mut self, r: Retour) {
        self.retours.push(r);
        if self.retours.len() > RETOURS {
            self.retours.remove(0);
        }
    }

    /// Défait le dernier changement.
    pub fn annuler(&mut self, monde: &mut Monde, maillages: &mut Maillages, tuiles: &mut Tuiles) {
        let Some(r) = self.retours.pop() else { return };
        let (id, avant, origine) = match r {
            Retour::Objet { id, avant, origine, .. } => (id, avant, origine),
            Retour::Relief(avant) => return terrain::remettre(monde, maillages, tuiles, &avant),
            Retour::Terrassement(avant, objet) => {
                terrain::remettre(monde, maillages, tuiles, &avant);
                self.retours.push(*objet);
                return self.annuler(monde, maillages, tuiles);
            }
            Retour::Bati { avant, .. } => {
                let Some(m) = self.regions.clone() else { return };
                let k = (crate::regions::de_tuile(avant.tuile.0), crate::regions::de_tuile(avant.tuile.1));
                let ici = m.batiment(k.0, k.1, &avant.id).map(|b| b.tuile);
                m.retoucher(avant.clone());
                for t in ici.into_iter().chain(std::iter::once(avant.tuile)) {
                    tuiles.refaire_bati(monde, maillages, t);
                }
                self.bati = (!avant.masquee).then(|| (k, avant.id.clone()));
                monde.choisie = None;
                self.mode = Mode::Choisir;
                return;
            }
        };
        let r = (id, origine);
        let e = monde.trouver(&r.0);
        match (avant, e) {
            (Some((objet, mut lieu)), e) => {
                // Le monde a pu se décaler depuis
                let (dx, dz) = ((r.1[0] - monde.origine[0]) as f32 * TAILLE, (r.1[1] - monde.origine[1]) as f32 * TAILLE);
                lieu.pos[0] += dx;
                lieu.pos[2] += dz;
                let lieu = Lieu::en(lieu.pos, lieu.cap);
                let e = e.unwrap_or_else(|| monde.creer(&r.0));
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
            Mode::Choisir => {
                self.bati = None;
                // Un bâtiment d'un monde matérialisé : celui sous le point touché
                if let (Some(m), Some(i)) = (self.regions.clone(), i) {
                    let mut enrichi = None;
                    let tuile = i.entite.and_then(|e| monde.ident.get(e)).and_then(|s| {
                        let (a, b) = s.strip_prefix("bati/")?.split_once('/')?;
                        // Un bâtiment enrichi a son entité : bati/i/j/<id>
                        let (b, id) = b.split_once('/').map_or((b, None), |(b, id)| (b, Some(id)));
                        enrichi = id.map(String::from);
                        Some((a.parse::<i32>().ok()?, b.parse::<i32>().ok()?))
                    });
                    if let (Some((ti, tj)), Some(id)) = (tuile, enrichi) {
                        self.bati = Some(((crate::regions::de_tuile(ti), crate::regions::de_tuile(tj)), id));
                        monde.choisie = None;
                        return;
                    }
                    if let Some((ti, tj)) = tuile {
                        let a = monde.absolu(i.point);
                        if let Some(b) = m.bati_sous(ti, tj, a[0], a[2]) {
                            self.bati = Some(((crate::regions::de_tuile(ti), crate::regions::de_tuile(tj)), b.id));
                            monde.choisie = None;
                            return;
                        }
                    }
                }
                monde.choisie = i.and_then(|i| i.entite).filter(|e| monde.objet.get(*e).is_some());
            }
            Mode::DeplacerBati => {
                if let (Some(m), Some(((ri, rj), id)), Some(i)) = (self.regions.clone(), self.bati.clone(), i) {
                    if let Some(avant) = m.batiment(ri, rj, &id) {
                        let a = monde.absolu(i.point);
                        let (ti, tj) = ((a[0] / TAILLE as f64).floor() as i32, (a[2] / TAILLE as f64).floor() as i32);
                        if (crate::regions::de_tuile(ti), crate::regions::de_tuile(tj)) != (ri, rj) {
                            self.message = Some("Un bâtiment ne quitte pas encore sa région (1 km) : posez-le plus près".into());
                        } else {
                            let o = avant.origine();
                            let mut x = avant.clone();
                            x.tuile = (ti, tj);
                            // Le pied suit le sol
                            x.position = [(a[0] - ti as f64 * TAILLE as f64) as f32, avant.position[1] + (m.hauteur(a[0], a[2]) - m.hauteur(o[0] as f64, o[2] as f64)) as f32, (a[2] - tj as f64 * TAILLE as f64) as f32];
                            self.retoucher_bati(&m, monde, maillages, tuiles, &avant, x, 0, "le déplacement");
                        }
                    }
                }
                self.mode = Mode::Choisir;
            }
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
                    self.pousser(Retour::Objet { id: monde.ident.get(e).cloned().unwrap_or_default(), avant: None, origine: monde.origine, cle: 0, quoi: "l'ajout" });
                    monde.choisie = Some(e);
                }
                self.mode = Mode::Choisir;
            }
        }
    }

    /// Le corps du panneau de l'éditeur.
    pub fn panneau(&mut self, cx: &mut Cx, b: &mut Body, monde: &mut Monde, maillages: &mut Maillages, tuiles: &mut Tuiles) {
        let bati = self.bati.clone().zip(self.regions.clone());
        match (choisi(monde), bati) {
            (_, Some((((ri, rj), ident), m))) => self.reglages_bati(cx, b, monde, maillages, tuiles, &m, (ri, rj), &ident),
            (Some((e, o, l)), None) => self.reglages(cx, b, monde, maillages, tuiles, e, o, l),
            (None, None) => {
                b.y += 6.0 * cx.dp;
                cx.paragraph(b, if self.regions.is_some() { "Touchez un objet ou un bâtiment dans la vue pour le choisir, ou ajoutez un objet du catalogue." } else { "Touchez un objet dans la vue pour le choisir, ou ajoutez-en un du catalogue." });
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

        self.enregistrement(cx, b, monde, maillages, tuiles);
    }

    /// L'état de l'enregistrement, et de quoi annuler : au bas des deux onglets.
    fn enregistrement(&mut self, cx: &mut Cx, b: &mut Body, monde: &mut Monde, maillages: &mut Maillages, tuiles: &mut Tuiles) {
        cx.section(b, "ENREGISTREMENT");
        let lues = tuiles.chargees.values().filter(|c| c.lue).count();
        cx.mesure(b, "Tuiles modifiées chargées", &lues.to_string());
        let attente = tuiles.a_ecrire();
        cx.mesure(b, "État", &if attente > 0 { format!("{attente} en attente") } else { "à jour".to_string() });
        if let Some(err) = &tuiles.erreur {
            cx.paragraph(b, err);
        }
        if let Some(m) = &self.regions {
            let n = m.en_attente();
            if n > 0 {
                cx.mesure(b, "Bâtiments", &format!("{n} à écrire"));
            }
        }
        if let Some(r) = self.retours.last() {
            let l = format!("Annuler {}", r.quoi());
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
                self.pousser(Retour::Objet { id: monde.ident.get(n).cloned().unwrap_or_default(), avant: None, origine: monde.origine, cle: 0, quoi: "la copie" });
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
        } else {
            cx.paragraph(b, "Dans la vue, glissez le disque blanc pour le déplacer sur le sol, une flèche pour le pousser le long d'un axe ou en hauteur, la pastille jaune de l'anneau pour le tourner.");
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

        self.terrassement(cx, b, monde, maillages, tuiles, e, o, l);
        // L'objet a pu se poser sur la plateforme
        let Some(l) = monde.lieu.get(e).copied() else { return };

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

impl Editeur {
    /// L'onglet Topologie : l'outil, le rayon, la force, la couleur à peindre.
    pub fn panneau_topologie(&mut self, cx: &mut Cx, b: &mut Body, monde: &mut Monde, maillages: &mut Maillages, tuiles: &mut Tuiles) {
        cx.section(b, "PINCEAU");
        let noms: Vec<&str> = Outil::TOUS.iter().map(|o| o.nom()).collect();
        let p = &mut self.pinceau;
        let actif = p.outil.and_then(|o| Outil::TOUS.iter().position(|x| *x == o));
        if let Some(k) = cx.liens(b, id("topologie-outil"), &noms, actif) {
            p.outil = if actif == Some(k) { None } else { Some(Outil::TOUS[k]) };
        }
        cx.paragraph(b, match p.outil {
            None => "Choisissez un outil, puis glissez un doigt dans la vue. Sans outil, le doigt tourne la vue.",
            Some(Outil::Aplanir) => "Glissez dans la vue : le sol revient à la hauteur du premier point touché. Touchez l'outil de nouveau pour tourner la vue.",
            Some(_) => "Glissez dans la vue pour retoucher le sol sous le cercle. Touchez l'outil de nouveau pour tourner la vue.",
        });
        cx.slider(b, id("topologie-rayon"), "Rayon", &format!("{} m", dec(p.rayon, 0)), &mut p.rayon, terrain::RAYON_MIN, terrain::RAYON_MAX);
        p.rayon = p.rayon.round().clamp(terrain::RAYON_MIN, terrain::RAYON_MAX);
        let mut f = p.force * 100.0;
        cx.slider(b, id("topologie-force"), "Force", &format!("{} %", dec(f, 0)), &mut f, 5.0, 100.0);
        p.force = ((f / 5.0).round() * 5.0 / 100.0).clamp(0.05, 1.0);
        if p.outil == Some(Outil::Peindre) {
            let teintes: Vec<Color> = terrain::TEINTES.iter().map(|c| c.map(|v| (v * 255.0).round() as u8)).map(|c| [c[0], c[1], c[2], 255]).collect();
            let choisie = terrain::TEINTES.iter().position(|c| hex(*c) == hex(p.couleur));
            if let Some(i) = cx.nuancier(b, id("topologie-couleur"), &teintes, choisie) {
                p.couleur = terrain::TEINTES[i];
            }
        }
        if self.isolignes > 0.0 {
            cx.section(b, "COURBES DE NIVEAU");
            cx.mesure(b, "Écart", &format!("{} m · fortes tous les {} m", dec(self.isolignes, 0), dec(self.isolignes * 5.0, 0)));
            cx.paragraph(b, "L'écart suit la hauteur de la vue. Seul le relief est dessiné ici : les objets reviennent dans l'onglet Ajout.");
        }
        self.enregistrement(cx, b, monde, maillages, tuiles);
        self.regenerer(cx, b);
    }

    /// La section « Terrassement » de l'objet choisi : sans ou avec sous-sol, la marge,
    /// le talus ; « Terrasser » fait la plateforme et y pose l'objet (une seule étape
    /// d'annulation).
    #[allow(clippy::too_many_arguments)]
    fn terrassement(&mut self, cx: &mut Cx, b: &mut Body, monde: &mut Monde, maillages: &mut Maillages, tuiles: &mut Tuiles, e: Entite, o: Objet, l: Lieu) {
        cx.section(b, "TERRASSEMENT");
        let t = &mut self.terrasse;
        if let Some(i) = cx.liens(b, id("terrasse-sous-sol"), &["Sans sous-sol", "Avec sous-sol"], Some(t.sous_sol as usize)) {
            t.sous_sol = i == 1;
        }
        if t.sous_sol {
            cx.slider(b, id("terrasse-profondeur"), "Profondeur", &format!("{} m", dec(t.profondeur, 1)), &mut t.profondeur, 1.0, 8.0);
            t.profondeur = (t.profondeur * 2.0).round() / 2.0;
        }
        cx.slider(b, id("terrasse-marge"), "Marge", &format!("{} m", dec(t.marge, 1)), &mut t.marge, 0.0, 6.0);
        t.marge = (t.marge * 2.0).round() / 2.0;
        cx.slider(b, id("terrasse-talus"), "Talus", &format!("{} °", dec(t.talus, 0)), &mut t.talus, 15.0, 60.0);
        t.talus = (t.talus / 5.0).round() * 5.0;
        let texte = if t.sous_sol {
            "La plateforme se met au niveau du point le plus haut du terrain sous elle, en remblai ; la fosse du sous-sol se creuse sous l'objet, qui se pose au fond."
        } else {
            "La plateforme se met au niveau du point le plus haut du terrain sous elle, en remblai, et l'objet s'y pose."
        };
        cx.paragraph(b, texte);
        if cx.liens(b, id("terrasse-faire"), &["Terrasser"], None).is_none() {
            return;
        }
        let t = *t;
        let (lo, hi) = o.boite(&monde.modeles);
        let x = terrain::Terrasse { centre: l.pos, cap: l.cap, lo: [lo[0], lo[2]], hi: [hi[0], hi[2]], marge: t.marge, talus: t.talus, sous_sol: t.sous_sol.then_some(t.profondeur) };
        let objet = Retour::Objet { id: monde.ident.get(e).cloned().unwrap_or_default(), avant: Some((o, l)), origine: monde.origine, cle: 0, quoi: "le terrassement" };
        let (avant, niveau) = terrain::terrasser(monde, maillages, tuiles, &x);
        if avant.is_empty() {
            return;
        }
        let fond = if t.sous_sol { niveau - t.profondeur } else { niveau };
        let mut p = l.pos;
        // Sa base sur le fond ; une rampe s'enfonce d'elle-même sous son départ
        let bas = if o.genre == Genre::Rampe { 0.0 } else { lo[1] };
        p[1] = fond - bas;
        tuiles.modifier(monde, maillages, e, o, Lieu::en(p, l.cap));
        self.pousser(Retour::Terrassement(avant, Box::new(objet)));
    }

    /// La carte repart de zéro : plus rien à annuler ni à tenir.
    pub fn oublier(&mut self) {
        self.retours.clear();
        self.bati = None;
        self.coup = None;
        self.saisie = None;
        self.mode = Mode::Choisir;
        self.confirmer = None;
    }

    /// Le lien « Régénérer la carte… », et sa confirmation.
    fn regenerer(&mut self, cx: &mut Cx, b: &mut Body) {
        cx.section(b, "CARTE");
        cx.mesure(b, "Taille", "20 km de côté");
        if let Some(g) = self.erosion {
            cx.paragraph(b, &format!("Érosion du relief de la graine {g}… quelques secondes. La carte se régénère ensuite."));
            return;
        }
        if let Some(l) = &self.lieu {
            cx.mesure(b, "Lieu réel", l);
            if let Some(m) = &self.monde {
                cx.mesure(b, "Monde", m);
            }
            cx.paragraph(b, crate::reel::SOURCES);
        }
        let Some(g) = self.confirmer else {
            match cx.liens(b, id("carte-regenerer"), &["Régénérer la carte…", "Importer un lieu réel…"], None) {
                Some(0) => self.confirmer = Some(graine_au_hasard()),
                Some(_) => self.importer = true,
                None => {}
            }
            return;
        };
        cx.paragraph(b, &format!("Une île nue, au relief tiré de la graine {g}. Toutes les retouches du relief et tous les objets posés seront effacés, sans retour possible ; le catalogue des modèles reste."));
        match cx.liens(b, id("carte-confirmer"), &["Régénérer", "Autre graine", "Garder la carte"], None) {
            Some(0) => {
                self.regeneration = Some(g);
                self.confirmer = None;
            }
            Some(1) => self.confirmer = Some(graine_au_hasard()),
            Some(_) => self.confirmer = None,
            None => {}
        }
    }

    /// Un outil de topologie est pris : le doigt dans la vue peint au lieu de tourner.
    pub fn peint(&self) -> bool {
        self.onglet == TOPOLOGIE && self.pinceau.outil.is_some()
    }

    /// Le pinceau, ce temps-ci : posé en `point` (au repère du monde), ou levé. Un coup,
    /// du doigt posé au doigt levé, est une étape d'annulation.
    pub fn peindre(&mut self, monde: &mut Monde, maillages: &mut Maillages, tuiles: &mut Tuiles, point: Option<[f32; 3]>, dt: f32) {
        let Some(c) = point.filter(|_| self.peint()) else {
            if let Some(avant) = self.coup.take() {
                if !avant.is_empty() {
                    self.pousser(Retour::Relief(avant));
                }
            }
            self.pinceau.niveau = None;
            return;
        };
        if self.pinceau.niveau.is_none() {
            self.pinceau.niveau = Some(c[1]);
        }
        let avant = terrain::appliquer(monde, maillages, tuiles, &self.pinceau, c, dt);
        let coup = self.coup.get_or_insert_with(Vec::new);
        for t in avant {
            if coup.iter().all(|x| x.0 != t.0) {
                coup.push(t);
            }
        }
    }

    /// Les poignées de l'objet choisi, dessinées dans la vue `camera` (sur `zone`) ; les
    /// glisser déplace ou tourne l'objet. Vrai tant qu'une poignée est tenue.
    pub fn poignees(&mut self, cx: &mut Cx, camera: &Camera, zone: crate::draw::Rect, monde: &mut Monde, maillages: &mut Maillages, tuiles: &mut Tuiles) -> bool {
        let Some((e, o, l)) = choisi(monde).filter(|_| self.onglet == AJOUT && self.mode == Mode::Choisir) else {
            self.saisie = None;
            return false;
        };
        let dp = cx.dp;
        // Les bras gardent à peu près la même taille à l'écran
        let p = l.pos;
        let dist = ((p[0] - camera.oeil[0]).powi(2) + (p[1] - camera.oeil[1]).powi(2) + (p[2] - camera.oeil[2]).powi(2)).sqrt();
        let bras = (dist * 0.16).clamp(0.4, 40.0);
        let axe = |q: Poignee| match q {
            Poignee::X => crate::moteur::objets::tourner([1.0, 0.0, 0.0], l.cap),
            Poignee::Z => crate::moteur::objets::tourner([0.0, 0.0, 1.0], l.cap),
            _ => [0.0, 1.0, 0.0],
        };
        let anneau = bras * 0.8;
        let ou = |q: Poignee| -> [f32; 3] {
            match q {
                Poignee::Plan => p,
                Poignee::Cap => [p[0] + anneau * l.cap.sin(), p[1], p[2] + anneau * l.cap.cos()],
                _ => {
                    let a = axe(q);
                    [p[0] + a[0] * bras, p[1] + a[1] * bras, p[2] + a[2] * bras]
                }
            }
        };
        let Some(centre) = camera.ecran(p) else { return false };
        let couleur = |q: Poignee| -> Color {
            match q {
                Poignee::Plan => [255, 255, 255, 230],
                Poignee::X => [0xE0, 0x55, 0x50, 255],
                Poignee::Z => [0x50, 0x85, 0xE0, 255],
                Poignee::Y => [0x55, 0xB8, 0x60, 255],
                Poignee::Cap => [0xF0, 0xC0, 0x40, 255],
            }
        };
        // L'anneau du cap, sur le sol de l'objet
        let n = 48;
        let points: Vec<Option<[f32; 2]>> = (0..=n).map(|k| {
            let a = k as f32 / n as f32 * std::f32::consts::TAU;
            camera.ecran([p[0] + anneau * a.sin(), p[1], p[2] + anneau * a.cos()])
        }).collect();
        for w in points.windows(2) {
            if let (Some(a), Some(b)) = (w[0], w[1]) {
                cx.dl.line(a, b, 2.0 * dp, [0xF0, 0xC0, 0x40, 160]);
            }
        }
        // Les flèches, puis les pastilles ; chacune se tient au doigt
        let mut tenue = None;
        for q in Poignee::TOUTES {
            let Some(s) = camera.ecran(ou(q)) else { continue };
            if !zone.contains(s[0], s[1]) {
                continue;
            }
            if matches!(q, Poignee::X | Poignee::Z | Poignee::Y) {
                cx.dl.line(centre, s, 3.0 * dp, couleur(q));
            }
            let r = if q == Poignee::Plan { 13.0 } else { 10.0 } * dp;
            let cle = sub_id(id("poignee"), q as usize);
            let prise = 22.0 * dp;
            if let Some(d) = cx.ui.drag(cle, crate::draw::Rect::new(s[0] - prise, s[1] - prise, 2.0 * prise, 2.0 * prise)) {
                tenue = Some((q, d));
            }
            let actif = self.saisie.is_some_and(|x| x.poignee == q);
            let r = if actif { r * 1.3 } else { r };
            cx.dl.rrect(crate::draw::Rect::new(s[0] - r - dp, s[1] - r - dp, 2.0 * (r + dp), 2.0 * (r + dp)), r + dp, [0x1B, 0x1B, 0x24, 160]);
            cx.dl.rrect(crate::draw::Rect::new(s[0] - r, s[1] - r, 2.0 * r, 2.0 * r), r, couleur(q));
        }
        // Une poignée tenue : l'objet suit le doigt depuis là où il l'a prise
        let Some((q, d)) = tenue.filter(|(q, _)| self.saisie.is_none_or(|s| s.poignee == *q)) else {
            if self.saisie.take().is_some() {
                self.gestes += 1;
            }
            return false;
        };
        let s = *self.saisie.get_or_insert(Saisie { poignee: q, doigt: [d.x, d.y], lieu: l, prise: sur_plan(camera, d.x, d.y, l.pos[1]) });
        let p0 = s.lieu.pos;
        let mut lieu = s.lieu;
        match q {
            Poignee::Plan => {
                if let (Some(a), Some(b)) = (s.prise, sur_plan(camera, d.x, d.y, p0[1])) {
                    lieu.pos = [p0[0] + b[0] - a[0], p0[1], p0[2] + b[2] - a[2]];
                }
            }
            Poignee::X | Poignee::Z | Poignee::Y => {
                // Le doigt projeté sur la flèche vue à l'écran : un mètre y fait tant de pixels
                let a = axe(q);
                if let (Some(c0), Some(c1)) = (camera.ecran(p0), camera.ecran([p0[0] + a[0], p0[1] + a[1], p0[2] + a[2]])) {
                    let v = [c1[0] - c0[0], c1[1] - c0[1]];
                    let l2 = v[0] * v[0] + v[1] * v[1];
                    if l2 > 1.0 {
                        let m = ((d.x - s.doigt[0]) * v[0] + (d.y - s.doigt[1]) * v[1]) / l2;
                        let m = (m / 0.05).round() * 0.05;
                        lieu.pos = [p0[0] + a[0] * m, p0[1] + a[1] * m, p0[2] + a[2] * m];
                    }
                }
            }
            Poignee::Cap => {
                if let (Some(a), Some(b)) = (s.prise, sur_plan(camera, d.x, d.y, p0[1])) {
                    let (a0, a1) = ((a[0] - p0[0]).atan2(a[2] - p0[2]), (b[0] - p0[0]).atan2(b[2] - p0[2]));
                    let pas = 5f32.to_radians();
                    lieu.cap = angle(((s.lieu.cap + a1 - a0) / pas).round() * pas);
                }
            }
        }
        if lieu.pos != l.pos || (lieu.cap - l.cap).abs() > 1e-5 {
            let quoi = if q == Poignee::Cap { "l'orientation" } else { "le déplacement" };
            self.retenir(monde, e, sub_id(id("poignee-geste"), self.gestes), quoi);
            tuiles.modifier(monde, maillages, e, o, Lieu::en(lieu.pos, lieu.cap));
        }
        true
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

/// Les noms des formes de toit, dans l'ordre de `toits::PLAT`…
const TOITS: [&str; 4] = ["Plat", "Deux pans", "Croupe", "Appentis"];

impl Editeur {
    /// Retient le bâtiment tel qu'il était (un même réglage ne fait qu'une étape), puis
    /// le retouche : ses tuiles se refont tout de suite, sa région s'écrira.
    #[allow(clippy::too_many_arguments)]
    fn retoucher_bati(&mut self, m: &Regions, monde: &mut Monde, maillages: &mut Maillages, tuiles: &mut Tuiles, avant: &Bati, mut x: Bati, cle: Id, quoi: &'static str) {
        let meme = cle != 0 && matches!(self.retours.last(), Some(Retour::Bati { cle: c, avant: a, .. }) if *c == cle && a.id == avant.id);
        if !meme {
            self.pousser(Retour::Bati { avant: avant.clone(), cle, quoi });
        }
        x.retouche = true;
        x.refaire();
        let (t0, t1) = (avant.tuile, x.tuile);
        m.retoucher(x);
        tuiles.refaire_bati(monde, maillages, t0);
        if t1 != t0 {
            tuiles.refaire_bati(monde, maillages, t1);
        }
    }

    /// Le panneau du bâtiment choisi : d'où il vient, le déplacer, le démolir, son toit,
    /// ses hauteurs, ses couleurs.
    #[allow(clippy::too_many_arguments)]
    fn reglages_bati(&mut self, cx: &mut Cx, b: &mut Body, monde: &mut Monde, maillages: &mut Maillages, tuiles: &mut Tuiles, m: &Arc<Regions>, (ri, rj): (i32, i32), ident: &str) {
        let Some(bt) = m.batiment(ri, rj, ident) else {
            self.bati = None;
            return;
        };
        cx.section(b, "BÂTIMENT");
        cx.mesure(b, "Identifiant", &bt.id);
        let provenance = match bt.provenance.as_str() {
            "osm+lidar" => "OSM et LiDAR",
            "osm" => "OSM",
            "main" => "posé à la main",
            p => p,
        };
        cx.mesure(b, "Provenance", &format!("{provenance}{}", if bt.retouche { ", retouché" } else { "" }));
        cx.mesure(b, "Tuile", &format!("{} ; {}", bt.tuile.0, bt.tuile.1));
        if let Some(d) = &bt.detail {
            cx.mesure(b, "Modèle", &format!("{} triangles{}", d.modele.triangles.len(), if d.neuve { ", à écrire" } else { "" }));
        }
        let actif = (self.mode == Mode::DeplacerBati).then_some(0);
        match cx.liens(b, id("bati-actions"), &["Déplacer", "Démolir", "Laisser"], actif) {
            Some(0) => self.mode = if self.mode == Mode::DeplacerBati { Mode::Choisir } else { Mode::DeplacerBati },
            Some(1) => {
                let mut x = bt.clone();
                x.masquee = true;
                self.retoucher_bati(m, monde, maillages, tuiles, &bt, x, 0, "la démolition");
                self.bati = None;
                self.mode = Mode::Choisir;
                return;
            }
            Some(_) => {
                self.bati = None;
                self.mode = Mode::Choisir;
                return;
            }
            None => {}
        }
        if self.mode == Mode::DeplacerBati {
            cx.paragraph(b, "Touchez dans la vue l'endroit où poser son centre ; il garde sa hauteur au-dessus du sol.");
        }
        // L'enrichir : l'exporter pour le retravailler, le remplacer par un modèle
        let enrichi = bt.detail.is_some();
        let liens: &[&str] = if enrichi { &["Exporter en .glb", "Remplacer par un .glb…", "Revenir au procédural"] } else { &["Exporter en .glb", "Remplacer par un .glb…"] };
        match cx.liens(b, id("bati-modele"), liens, None) {
            Some(0) => self.export_bati = Some(bt.clone()),
            Some(1) => self.enrichir_bati = Some(((ri, rj), bt.id.clone())),
            Some(_) => {
                let mut x = bt.clone();
                x.detail = None;
                self.retoucher_bati(m, monde, maillages, tuiles, &bt, x, 0, "le retour au procédural");
                return;
            }
            None => {}
        }

        cx.section(b, "RÉGLAGES");
        let mut x = bt.clone();
        let mut cle = 0;
        let k = id("bati-cap");
        let mut deg = angle(bt.cap).to_degrees();
        cx.slider(b, k, "Cap", &format!("{} °", dec(deg, 0)), &mut deg, 0.0, 360.0);
        let cap = angle(((deg / 5.0).round() * 5.0).to_radians());
        if (cap - angle(bt.cap)).abs() > 1e-4 {
            x.cap = cap;
            cle = k;
        }
        if enrichi {
            if cle != 0 && x != bt {
                self.retoucher_bati(m, monde, maillages, tuiles, &bt, x, cle, "le réglage du bâtiment");
            }
            return;
        }
        let f = &bt.forme;
        if let Some(t) = cx.liens(b, id("bati-toit"), &TOITS, Some(f.toit as usize)) {
            x.forme.toit = t as u8;
            if t as u8 == crate::toits::PLAT {
                x.forme.faitage = x.forme.egout;
            } else if f.faitage - f.egout < 0.5 {
                x.forme.faitage = f.egout + 3.0;
            }
            cle = id("bati-toit");
        }
        // Les murs, depuis le pied (un peu sous le sol), et le toit au-dessus
        let k = id("bati-egout");
        let mut murs = f.egout - f.pied;
        cx.slider(b, k, "Murs", &format!("{} m", dec(murs, 1)), &mut murs, 2.0, 80.0);
        let murs = (murs * 2.0).round() / 2.0;
        if (murs - (f.egout - f.pied)).abs() > 0.01 {
            let toit = x.forme.faitage - x.forme.egout;
            x.forme.egout = f.pied + murs;
            x.forme.faitage = x.forme.egout + toit;
            cle = k;
        }
        if f.toit != crate::toits::PLAT {
            let k = id("bati-faitage");
            let mut h = f.faitage - f.egout;
            cx.slider(b, k, "Toit", &format!("{} m", dec(h, 1)), &mut h, 0.5, 25.0);
            let h = (h * 4.0).round() / 4.0;
            if (h - (f.faitage - f.egout)).abs() > 0.01 {
                x.forme.faitage = x.forme.egout + h;
                cle = k;
            }
            let k = id("bati-axe");
            let mut a = f.axe.rem_euclid(PI).to_degrees();
            cx.slider(b, k, "Sens du toit", &format!("{} °", dec(a, 0)), &mut a, 0.0, 180.0);
            let a = ((a / 5.0).round() * 5.0).to_radians();
            if (a - f.axe.rem_euclid(PI)).abs() > 1e-3 {
                x.forme.axe = a;
                cle = k;
            }
        }
        let teintes: Vec<Color> = PALETTE.iter().map(|c| [c[0], c[1], c[2]].map(|v| (v * 255.0).round() as u8)).map(|c| [c[0], c[1], c[2], 255]).collect();
        cx.mesure(b, "Murs", &format!("#{}", hex(f.murs)));
        if let Some(i) = cx.nuancier(b, id("bati-murs"), &teintes, PALETTE.iter().position(|c| hex(*c) == hex(f.murs))) {
            x.forme.murs = PALETTE[i];
            cle = id("bati-murs");
        }
        cx.mesure(b, "Couverture", &format!("#{}", hex(f.couverture)));
        if let Some(i) = cx.nuancier(b, id("bati-couverture"), &teintes, PALETTE.iter().position(|c| hex(*c) == hex(f.couverture))) {
            x.forme.couverture = PALETTE[i];
            cle = id("bati-couverture");
        }
        if cle != 0 && x != bt {
            self.retoucher_bati(m, monde, maillages, tuiles, &bt, x, cle, "le réglage du bâtiment");
        }
    }

    /// La surbrillance du bâtiment choisi : une copie de ses faces, un peu gonflée, que la
    /// scène dessine choisie. Elle suit le bâtiment, et part avec lui.
    pub fn suivre_bati(&mut self, monde: &mut Monde, maillages: &mut Maillages) {
        const ID: &str = "bati/choisi";
        let voulu = self.bati.as_ref().zip(self.regions.as_ref()).and_then(|(((ri, rj), id), m)| m.batiment(*ri, *rj, id)).filter(|b| !b.masquee);
        let Some(b) = voulu else {
            if let Some((e, _)) = self.surbrillance.take() {
                monde.detruire(e);
                maillages.ote(ID);
            }
            self.surbrillance_maillage = None;
            return;
        };
        let e = match &self.surbrillance {
            Some((e, avant)) if *avant == b => *e,
            _ => {
                let (mut v, mut t) = (Vec::new(), Vec::new());
                match &b.detail {
                    // Enrichi : les faces de son modèle, à son cap et à sa place
                    Some(d) => {
                        for tri in &d.modele.triangles {
                            let p = tri.map(|q| {
                                let r = crate::moteur::objets::tourner(q, b.cap);
                                [r[0] + b.position[0], r[1] + b.position[1], r[2] + b.position[2]]
                            });
                            let (u, w) = ([p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]], [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]]);
                            let n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
                            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-9);
                            for q in p {
                                v.extend_from_slice(&q);
                                v.extend_from_slice(&n.map(|c| c / l));
                                v.extend_from_slice(&[1.0, 1.0, 1.0]);
                            }
                        }
                    }
                    None => b.monter(b.tuile.0, b.tuile.1, &mut v, &mut t),
                }
                for q in v.chunks_mut(9) {
                    for k in 0..3 {
                        q[k] += q[3 + k] * 0.05;
                    }
                }
                let e = self.surbrillance.take().map_or_else(|| monde.creer(ID), |(e, _)| e);
                let k = maillages.mettre(ID, crate::moteur::Format::Statique, std::sync::Arc::new(v));
                monde.rendu.mettre(e, crate::moteur::Rendu { maillage: k, local: crate::math::scale(1.0) });
                self.surbrillance_maillage = Some(k);
                self.surbrillance = Some((e, b.clone()));
                e
            }
        };
        // Le monde a pu se décaler : la tuile du bâtiment dans le repère d'aujourd'hui
        monde.lieu.mettre(e, Lieu::en(Tuiles::coin(monde, b.tuile), 0.0));
    }
}

