//! La chaussée : un enrobé sur toute la largeur d'une route ou d'une piste, ses voies,
//! leur marquage et leur usure. La texture couvre `Chaussee::largeur()` mètres en
//! travers (s, de 0 à 1, sans répétition) et `cote` mètres en long (t, qui se répète :
//! tous les bruits ont cette période, et elle tient un nombre entier de tirets).
//!
//! De dessous à dessus :
//! - le bitume et ses plaques (les deux couleurs, `variation`) ;
//! - les gravillons de l'enrobé (`granulat` mm, leur contraste `grain`, leur relief) ;
//! - l'usure (`usure`) : les traces des roues, plus sombres et lustrées (les gravillons
//!   s'y effacent), l'huile au milieu des voies, des reprises, les bords plus clairs ;
//! - les fissures (`irregularite`) ;
//! - le marquage (rives, axe, séparations : continu, discontinu, double, mixte), de la
//!   couleur des joints, et son usure (`usure_marquage`), plus forte sous les roues.

use crate::generateur::{alea, fbm, fois, melange, smoothstep, Contexte, Genre};

/// L'écartement des traces des roues autour du milieu d'une voie, et leur largeur (m).
const TRACE: f32 = 0.85;
const TRACE_LARGEUR: f32 = 0.3;

/// Une ligne de marquage à `x` mètres du bord gauche : continue ou discontinue.
#[derive(Clone, Copy)]
struct Ligne {
    x: f32,
    tirets: bool,
}

/// Les lignes à peindre : chaque trait donne une ou deux lignes.
fn lignes(c: &Contexte) -> Vec<Ligne> {
    let ch = &c.r.chaussee;
    let lw = ch.ligne / 100.0;
    let w = ch.largeur();
    let mut traits: Vec<(f32, u32, f32)> = vec![(ch.accotement, ch.rive, 1.0), (w - ch.accotement, ch.rive, -1.0)];
    let double_sens = c.r.appareil == 0;
    for i in 1..ch.voies {
        let axe = double_sens && ch.voies % 2 == 0 && i == ch.voies / 2;
        traits.push((ch.accotement + i as f32 * ch.largeur_voie, if axe { ch.axe } else { ch.separation }, 1.0));
    }
    let mut v = Vec::new();
    for (x, t, sens) in traits {
        match t {
            1 => v.push(Ligne { x, tirets: false }),
            2 => v.push(Ligne { x, tirets: true }),
            3 => {
                v.push(Ligne { x: x - lw * sens, tirets: false });
                v.push(Ligne { x: x + lw * sens, tirets: false });
            }
            // Mixte : continue à gauche, discontinue à droite
            4 => {
                v.push(Ligne { x: x - lw, tirets: false });
                v.push(Ligne { x: x + lw, tirets: true });
            }
            _ => {}
        }
    }
    v
}

/// La part de trace de roue en `x` (m) : de 0 à 1.
fn traces(c: &Contexte, x: f32) -> f32 {
    let ch = &c.r.chaussee;
    let mut m: f32 = 0.0;
    for k in 0..ch.voies {
        let milieu = ch.accotement + (k as f32 + 0.5) * ch.largeur_voie;
        let ecart = TRACE.min(ch.largeur_voie * 0.3);
        for d in [-ecart, ecart] {
            m = m.max((-((x - milieu - d) / TRACE_LARGEUR).powi(2)).exp());
        }
    }
    m
}

/// Les gravillons : un pavage de cellules périodique en long ; la valeur propre du
/// gravillon le plus proche, et le liant entre eux (0 sur un gravillon, 1 entre).
fn gravillons(c: &Contexte, s: f32, t: f32) -> (f32, f32) {
    let ch = &c.r.chaussee;
    let g = (ch.granulat / 1000.0).max(0.001);
    let (mx, my) = (((ch.largeur() / g).round() as i32).max(1), ((c.r.cote / g).round() as i32).max(1));
    let (gx, gy) = (s * mx as f32, t * my as f32);
    let (ci, cj) = (gx.floor() as i32, gy.floor() as i32);
    let mut best = (f32::MAX, 0.0f32);
    for di in -1..=1 {
        for dj in -1..=1 {
            let (i, j) = (ci + di, cj + dj);
            let h = (i.rem_euclid(mx) as u32, j.rem_euclid(my) as u32);
            let gr = c.g.wrapping_add(300);
            let p = [i as f32 + 0.15 + 0.7 * alea(h.0, h.1, gr), j as f32 + 0.15 + 0.7 * alea(h.0, h.1, gr + 1)];
            let d = (gx - p[0]).powi(2) + (gy - p[1]).powi(2);
            if d < best.0 {
                best = (d, alea(h.0, h.1, gr + 2));
            }
        }
    }
    let liant = smoothstep(0.32, 0.55, best.0.sqrt());
    (best.1, liant)
}

/// Les reprises : par voie et par tronçon, une plaque d'enrobé plus neuf. Vrai si
/// (x, y) en mètres y est, avec la distance à son bord (m).
fn reprise(c: &Contexte, x: f32, t: f32) -> Option<f32> {
    let ch = &c.r.chaussee;
    if ch.usure <= 0.0 {
        return None;
    }
    let troncons = ((c.r.cote / 6.0).round() as i32).max(1);
    let tf = t.rem_euclid(1.0) * troncons as f32;
    let k = tf.floor() as i32;
    let voie = ((x - ch.accotement) / ch.largeur_voie).floor() as i32;
    if voie < 0 || voie >= ch.voies as i32 {
        return None;
    }
    let (a, b) = (voie as u32, k.rem_euclid(troncons) as u32);
    let gr = c.g.wrapping_add(400);
    if alea(a, b, gr) >= 0.35 * ch.usure {
        return None;
    }
    let long = c.r.cote / troncons as f32;
    let (y0, y1) = (0.1 + 0.3 * alea(a, b, gr + 1), 0.5 + 0.45 * alea(a, b, gr + 2));
    let ly = (tf - k as f32) * long;
    let x0 = ch.accotement + voie as f32 * ch.largeur_voie;
    let (u0, u1) = (x0 + 0.1 + 0.4 * alea(a, b, gr + 3), x0 + ch.largeur_voie - 0.1 - 0.4 * alea(a, b, gr + 4));
    let d = (ly - y0 * long).min(y1 * long - ly).min(x - u0).min(u1 - x);
    (d >= 0.0).then_some(d)
}

/// La couleur de la chaussée au point (s, t) ; (x, y) le pixel.
pub(crate) fn couleur(c: &Contexte, s: f32, t: f32, x: u32, y: u32) -> [f32; 3] {
    debug_assert_eq!(c.r.genre, Genre::Chaussee);
    let r = c.r;
    let ch = &r.chaussee;
    let w = ch.largeur();
    let (xm, ym) = (s * w, t * r.cote);
    let f = |m: f32, l: f32| ((l / m).round() as i32).max(1);

    // Le bitume et ses plaques
    let n = fbm(s, t, f(4.0, w), f(4.0, r.cote), 4, c.g.wrapping_add(200)) * 0.5 + 0.5;
    let mut v = melange(c.c1, c.c2, smoothstep(0.3, 0.7, n) * r.variation);

    // L'usure : traces des roues, huile au milieu des voies, bords
    let u = ch.usure;
    let trace = traces(c, xm);
    let mut huile: f32 = 0.0;
    for k in 0..ch.voies {
        let milieu = ch.accotement + (k as f32 + 0.5) * ch.largeur_voie;
        huile = huile.max((-((xm - milieu) / 0.22).powi(2)).exp());
    }
    let flaques = fbm(s, t, f(3.0, w), f(1.5, r.cote), 3, c.g.wrapping_add(210)) * 0.5 + 0.5;
    let bord = 1.0 - smoothstep(0.0, ch.accotement.max(0.3), xm.min(w - xm));
    let neuve = reprise(c, xm, t);

    // Les gravillons, effacés dans les traces (lustrées) et sur une reprise
    let (gv, liant) = gravillons(c, s, t);
    let poli = 1.0 - 0.65 * u * trace;
    let fraiche = if neuve.is_some() { 0.5 } else { 1.0 };
    let contraste = r.grain * poli * fraiche;
    let clair = if gv > 0.93 { 0.35 } else { 0.0 };
    v = fois(v, 1.0 + contraste * (0.45 * (gv - 0.5) + clair) * (1.0 - liant));
    v = fois(v, 1.0 - r.relief * 0.25 * liant * (1.0 + bord * u));
    // Un bruit fin, au pixel
    v = fois(v, 1.0 + r.grain * 0.08 * (alea(x, y, c.g.wrapping_add(220)) - 0.5));

    v = fois(v, 1.0 - 0.16 * u * trace);
    v = fois(v, 1.0 - 0.3 * u * huile * smoothstep(0.35, 0.8, flaques));
    v = fois(v, 1.0 + 0.12 * u * bord);
    if let Some(d) = neuve {
        // Plus sombre et plus uni ; un joint de reprise sombre sur le bord
        v = fois(v, 1.0 - 0.1 * u);
        v = fois(v, 1.0 - 0.2 * u * (1.0 - smoothstep(0.0, 0.025, d)));
    }

    // Les fissures : les crêtes d'un bruit, là où un second bruit le permet
    if r.irregularite > 0.0 {
        let fis = fbm(s, t, f(1.5, w), f(1.5, r.cote), 4, c.g.wrapping_add(230));
        let ou = fbm(s, t, f(6.0, w), f(6.0, r.cote), 2, c.g.wrapping_add(231)) * 0.5 + 0.5;
        let masque = smoothstep(1.0 - r.irregularite, 1.15 - r.irregularite, ou);
        let fente = 1.0 - smoothstep(0.004, 0.012 + c.pixel, fis.abs());
        v = fois(v, 1.0 - 0.55 * fente * masque);
    }

    // Le marquage
    let lw = ch.ligne / 100.0;
    let px = c.pixel;
    let cycle = ch.cycle();
    let mut couvre: f32 = 0.0;
    for l in lignes(c) {
        let dx = (xm - l.x).abs();
        let mut k = 1.0 - smoothstep(lw / 2.0 - px / 2.0, lw / 2.0 + px / 2.0, dx);
        if k <= 0.0 {
            continue;
        }
        if l.tirets {
            let p = ym.rem_euclid(cycle);
            let dans = p.min(ch.tiret - p);
            k *= smoothstep(-px / 2.0, px / 2.0, dans);
        }
        couvre = couvre.max(k);
    }
    if couvre > 0.0 {
        let um = ch.usure_marquage;
        if um > 0.0 {
            // Effacée par plaques, plus sous les roues ; écaillée au gravillon
            let e = um * (0.55 + 0.9 * trace);
            let tenue = fbm(s, t, f(0.6, w), f(0.6, r.cote), 4, c.g.wrapping_add(240)) * 0.5 + 0.5;
            couvre *= smoothstep(e - 0.08, e + 0.08, tenue);
            if gv < um * 0.35 {
                couvre *= 0.25;
            }
        }
        let peinture = fois(c.cj, 1.0 + 0.06 * r.grain * (gv - 0.5) - 0.12 * liant * r.relief);
        v = melange(v, peinture, couvre * 0.95);
    }
    v
}
