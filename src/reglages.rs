//! Les réglages, dans le panneau de gauche : la manette, puis l'affichage.

use crate::draw::Rect;
use gda::json::{self, J};
use std::path::PathBuf;
use crate::manette::{Manettes, ACTIONS};
use crate::moniteur::dec;
use crate::ui::{id, sub_id};
use crate::widgets::{self, Body, Cx};

/// Le corps de l'onglet Manette.
pub fn manette(cx: &mut Cx, b: &mut Body, m: &mut Manettes) {
    let d = cx.dp;
    if m.appareils.is_empty() {
        b.y += 6.0 * d;
        cx.paragraph(b, "Aucune manette détectée. Branchez-en une, ou appairez-la en Bluetooth, puis appuyez sur l'une de ses touches.");
        return;
    }

    cx.section(b, "MANETTE");
    if m.appareils.len() > 1 {
        // Plusieurs manettes : celle que l'on règle est celle qui commande
        let noms: Vec<String> = m.appareils.iter().map(|a| a.nom.clone()).collect();
        let refs: Vec<&str> = noms.iter().map(|s| s.as_str()).collect();
        let active = m.active().and_then(|a| m.appareils.iter().position(|x| x.id == a.id));
        if let Some(k) = cx.liens(b, id("manettes"), &refs, active) {
            m.active = Some(m.appareils[k].id);
            m.capture = None;
        }
    }
    let Some(a) = m.active() else { return };
    cx.mesure(b, "Nom", &a.nom);
    cx.mesure(b, "Profil", if a.profil.auto { "assignation automatique" } else { "personnalisé" });

    // Le témoin : les deux sticks et ce qui est enfoncé, en direct
    cx.section(b, "EN DIRECT");
    let p = &a.profil;
    let sticks = [("Marcher", p.lien("marcher").map_or([0.0; 2], |e| a.stick(e))), ("Vue", p.lien("vue").map_or([0.0; 2], |e| a.stick(e)))];
    let (r, ecart) = (34.0 * d, 24.0 * d);
    let mut x = b.x;
    for (nom, v) in sticks {
        let c = Rect::new(x, b.y + 4.0 * d, 2.0 * r, 2.0 * r);
        cx.dl.rrect(c, r, widgets::CHIP);
        cx.dl.rrect(Rect::new(c.cx() - 0.5 * d, c.y + 6.0 * d, d.max(1.0), c.h - 12.0 * d), 0.0, widgets::LINE);
        cx.dl.rrect(Rect::new(c.x + 6.0 * d, c.cy() - 0.5 * d, c.w - 12.0 * d, d.max(1.0)), 0.0, widgets::LINE);
        let k = 9.0 * d;
        let (px, py) = (c.cx() + v[0] * (r - k), c.cy() + v[1] * (r - k));
        cx.dl.rrect(Rect::new(px - k, py - k, 2.0 * k, 2.0 * k), k, widgets::ACCENT);
        cx.text_at(x, c.bottom() + 14.0 * d, nom, 13.0, widgets::DIM);
        x += 2.0 * r + ecart;
    }
    b.y += 2.0 * r + 30.0 * d;
    let enfonces = a.enfonces();
    cx.mesure(b, "Enfoncé", &if enfonces.is_empty() { "rien".to_string() } else { enfonces.join(", ") });

    cx.section(b, "ASSIGNATION");
    let profil = a.profil.clone();
    for (i, act) in ACTIONS.iter().enumerate() {
        let attend = m.capture == Some(act.id);
        let valeur = if attend { if act.stick { "poussez un stick…".to_string() } else { "appuyez…".to_string() } } else { profil.lien(act.id).map_or("—".to_string(), |e| e.nom()) };
        if cx.choix(b, sub_id(id("assignation"), i), act.nom, &valeur, attend) {
            m.capture = if attend { None } else { Some(act.id) };
        }
    }
    if m.capture.is_some() {
        cx.paragraph(b, "Appuyez sur la touche voulue, ou poussez le stick voulu. Si elle servait déjà, les deux actions s'échangent. Touchez à nouveau la ligne pour renoncer.");
    }

    cx.section(b, "RÉGLAGES");
    let (mut zone, mut sens, mut inverser) = (profil.zone_morte, profil.sensibilite, profil.inverser);
    cx.slider(b, id("zone-morte"), "Zone morte des sticks", &format!("{} %", dec(zone * 100.0, 0)), &mut zone, 0.0, 0.5);
    cx.slider(b, id("sensibilite"), "Vitesse de la vue", &format!("× {}", dec(sens, 1)), &mut sens, 0.3, 3.0);
    cx.toggle(b, id("inverser"), "Inverser la vue verticale", &mut inverser);
    let zone = (zone * 100.0).round() / 100.0;
    let sens = (sens * 10.0).round() / 10.0;
    if (zone, sens, inverser) != (profil.zone_morte, profil.sensibilite, profil.inverser) {
        m.modifier(|p| {
            p.zone_morte = zone;
            p.sensibilite = sens;
            p.inverser = inverser;
            p.auto = false;
        });
    }
    if !profil.auto && cx.liens(b, id("retablir"), &["Rétablir l'assignation automatique"], None).is_some() {
        m.retablir();
    }
    if let Some(e) = &m.erreur {
        cx.paragraph(b, e);
    }
}

/// La taille d'une tuile, en mètres.
const TUILE: f32 = crate::moteur::TAILLE;

/// Les réglages de l'affichage, gardés dans `reglages/affichage.json`.
#[derive(Clone, Debug, PartialEq)]
pub struct Affichage {
    /// Les tuiles chargées autour du joueur, en tuiles : de 1 à `DISTANCE_MAX`.
    pub distance: i32,
    /// Là où le brouillard a tout recouvert, en mètres.
    pub brouillard: f32,
    fichier: Option<PathBuf>,
    pub erreur: Option<String>,
}

pub const DISTANCE_MAX: i32 = 5;
pub const BROUILLARD_MIN: f32 = 30.0;

impl Affichage {
    /// Les réglages relus de `dossier`, ou ceux d'origine.
    pub fn ouvrir(dossier: Option<PathBuf>, distance: i32, brouillard: f32) -> Affichage {
        let fichier = dossier.map(|d| d.join("affichage.json"));
        let mut a = Affichage { distance, brouillard, fichier, erreur: None };
        if let Some(j) = a.fichier.as_ref().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|s| json::lire(&s)) {
            if let Some(v) = j.get("distance").and_then(J::num) {
                a.distance = v.round() as i32;
            }
            if let Some(v) = j.get("brouillard").and_then(J::num) {
                a.brouillard = v as f32;
            }
        }
        a.borner();
        a
    }

    /// Au-delà de ce qui est chargé, on verrait le bord du monde.
    pub fn brouillard_max(&self) -> f32 {
        (self.distance as f32 + 0.25) * TUILE
    }

    fn borner(&mut self) {
        self.distance = self.distance.clamp(1, DISTANCE_MAX);
        self.brouillard = self.brouillard.clamp(BROUILLARD_MIN, self.brouillard_max());
    }

    pub fn vers_json(&self) -> String {
        let o = J::Objet(vec![
            ("format".into(), J::Num(1.0)),
            ("distance".into(), J::Num(self.distance as f64)),
            ("brouillard".into(), J::Num(self.brouillard.round() as f64)),
        ]);
        let mut s = String::new();
        json::indente(&o, 0, &mut s);
        s
    }

    fn ecrire(&mut self) {
        let Some(f) = &self.fichier else { return };
        let tmp = f.with_extension("tmp");
        let r = f.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(&tmp, self.vers_json())).and_then(|_| std::fs::rename(&tmp, f));
        self.erreur = r.err().map(|e| format!("Réglages non enregistrés : {e}"));
    }

    /// Change les réglages et les garde.
    pub fn modifier(&mut self, distance: i32, brouillard: f32) {
        let avant = self.clone();
        self.distance = distance;
        self.brouillard = brouillard;
        self.borner();
        if *self != avant {
            self.ecrire();
        }
    }
}

/// Le corps de l'onglet Affichage.
pub fn affichage(cx: &mut Cx, b: &mut Body, a: &mut Affichage) {
    cx.section(b, "DISTANCES");
    let mut d = a.distance as f32;
    let metres = a.distance as f32 * TUILE;
    let n = (2 * a.distance + 1).pow(2);
    cx.slider(b, id("distance-affichage"), "Distance d'affichage", &format!("{} m", dec(metres, 0)), &mut d, 1.0, DISTANCE_MAX as f32);
    let mut f = a.brouillard;
    let max = a.brouillard_max();
    cx.slider(b, id("distance-brouillard"), "Brouillard", &format!("{} m", dec(f.min(max), 0)), &mut f, BROUILLARD_MIN, max);
    let d = d.round() as i32;
    // Le brouillard se fait au mètre près, à 5 m près au-delà de 100 m
    let pas = if f < 100.0 { 1.0 } else { 5.0 };
    let f = (f / pas).round() * pas;
    // Plus loin, le brouillard garde sa part de la distance
    let f = if d != a.distance { f * (d as f32 + 0.25) / (a.distance as f32 + 0.25) } else { f };
    a.modifier(d, f);
    cx.mesure(b, "Tuiles autour du joueur", &format!("{n}"));
    cx.paragraph(b, "Plus loin, plus de mémoire et de calcul. Le brouillard s'arrête au bord des tuiles chargées.");
    if let Some(e) = &a.erreur {
        cx.paragraph(b, e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l_affichage_se_garde_et_reste_borne() {
        let d = std::env::temp_dir().join(format!("sage-affichage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let mut a = Affichage::ouvrir(Some(d.clone()), 2, 144.0);
        assert_eq!((a.distance, a.brouillard), (2, 144.0));
        a.modifier(4, 240.0);
        let b = Affichage::ouvrir(Some(d.clone()), 2, 144.0);
        assert_eq!((b.distance, b.brouillard), (4, 240.0));
        // Le brouillard ne dépasse pas les tuiles chargées, ni la distance ses bornes
        a.modifier(9, 1000.0);
        assert_eq!((a.distance, a.brouillard), (DISTANCE_MAX, (DISTANCE_MAX as f32 + 0.25) * TUILE));
        a.modifier(1, 10.0);
        assert_eq!((a.distance, a.brouillard), (1, BROUILLARD_MIN));
        let _ = std::fs::remove_dir_all(&d);
    }
}
