//! Les réglages, dans le panneau de gauche. Leur premier onglet : la manette.

use crate::draw::Rect;
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
