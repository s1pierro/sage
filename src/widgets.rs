//! Composants tactiles : pastilles, pilules, onglets, interrupteurs, curseurs,
//! listes défilantes. Mode immédiat : chaque image redécrit l'interface.

use crate::draw::{Color, DrawList, Rect};
use crate::text::Text;
use crate::ui::{sub_id, Id, Sense, Ui};
use std::collections::HashMap;

pub const BG: Color = [0xF3, 0xF2, 0xFA, 255];
pub const SURFACE: Color = [255, 255, 255, 255];
pub const TEXT: Color = [0x1B, 0x1B, 0x24, 255];
pub const DIM: Color = [0x6C, 0x6C, 0x80, 255];
pub const ACCENT: Color = [0x2A, 0x22, 0xD8, 255];
pub const CHIP: Color = [0xEF, 0xEE, 0xF6, 255];
pub const LINE: Color = [0xE3, 0xE2, 0xEC, 255];
const SHADOW: Color = [0x1B, 0x1B, 0x40, 60];

#[derive(Clone, Copy, PartialEq)]
pub enum Icon {
    List,
    Sliders,
    Help,
    Close,
    /// Une courbe de pouls : le moniteur.
    Pouls,
    /// Un crayon : l'éditeur.
    Crayon,
    /// Un engrenage : les réglages.
    Engrenage,
}

/// Défilement de chaque zone : position et hauteur du contenu.
pub type Scrolls = HashMap<Id, (f32, f32)>;

pub struct Cx<'a> {
    pub ui: &'a mut Ui,
    pub text: &'a mut Text,
    pub dl: &'a mut DrawList,
    pub dp: f32,
}

/// Colonne de composants dans une zone défilante.
pub struct Body {
    pub x: f32,
    pub w: f32,
    pub y: f32,
    top: f32,
    id: Id,
    rect: Rect,
}

impl Cx<'_> {
    fn label(&mut self, x: f32, cy: f32, s: &str, size: f32, color: Color) {
        let px = (size * self.dp).round();
        let base = cy + self.text.center_offset(px);
        self.text.draw(self.dl, x, base, s, px, color);
    }

    pub fn text_at(&mut self, x: f32, cy: f32, s: &str, size: f32, color: Color) {
        self.label(x, cy, s, size, color);
    }

    /// Texte calé à droite sur `right`.
    pub fn text_right(&mut self, right: f32, cy: f32, s: &str, size: f32, color: Color) {
        let w = self.text.measure(s, (size * self.dp).round());
        self.label(right - w, cy, s, size, color);
    }

    fn centered(&mut self, r: Rect, s: &str, size: f32, color: Color) {
        let w = self.text.measure(s, (size * self.dp).round());
        self.label(r.cx() - w / 2.0, r.cy(), s, size, color);
    }

    pub fn clip(&mut self, c: Option<Rect>) {
        self.dl.set_clip(c);
        self.ui.set_clip(c);
    }

    pub fn icon(&mut self, r: Rect, icon: Icon, color: Color) {
        let (cx, cy, u) = (r.cx(), r.cy(), r.w.min(r.h) / 24.0);
        let t = 2.2 * u;
        let p = |x: f32, y: f32| [cx + x * u, cy + y * u];
        match icon {
            Icon::List => {
                for y in [-6.0, 0.0, 6.0] {
                    self.dl.rrect(Rect::new(cx - 8.5 * u - 1.3 * u, cy + (y - 1.3) * u, 2.6 * u, 2.6 * u), 1.3 * u, color);
                    self.dl.line(p(-4.0, y), p(9.0, y), t, color);
                }
            }
            Icon::Sliders => {
                for (y, k) in [(-6.0, 3.0), (0.0, -3.5), (6.0, 2.0)] {
                    self.dl.line(p(-9.0, y), p(9.0, y), 1.6 * u, color);
                    self.dl.rrect(Rect::new(cx + (k - 2.5) * u, cy + (y - 2.5) * u, 5.0 * u, 5.0 * u), 2.5 * u, color);
                }
            }
            Icon::Help => self.centered(r, "?", 0.85 * r.h / self.dp, color),
            Icon::Close => {
                self.dl.line(p(-6.0, -6.0), p(6.0, 6.0), t, color);
                self.dl.line(p(6.0, -6.0), p(-6.0, 6.0), t, color);
            }
            Icon::Crayon => {
                self.dl.line(p(-5.0, 5.0), p(6.5, -6.5), 4.6 * u, color);
                self.dl.line(p(-9.0, 9.0), p(-6.6, 6.6), 1.8 * u, color);
                self.dl.line(p(-8.2, 4.4), p(-4.4, 8.2), 1.6 * u, color);
            }
            Icon::Engrenage => {
                let n = 8;
                for k in 0..n {
                    let a = k as f32 / n as f32 * std::f32::consts::TAU;
                    let (s, c) = a.sin_cos();
                    self.dl.line(p(c * 6.0, s * 6.0), p(c * 9.5, s * 9.5), 3.4 * u, color);
                }
                let m = 16;
                for k in 0..m {
                    let (a, b) = (k as f32 / m as f32 * std::f32::consts::TAU, (k + 1) as f32 / m as f32 * std::f32::consts::TAU);
                    self.dl.line(p(a.cos() * 5.6, a.sin() * 5.6), p(b.cos() * 5.6, b.sin() * 5.6), 2.6 * u, color);
                }
            }
            Icon::Pouls => {
                let pts = [(-10.0, 1.0), (-5.0, 1.0), (-2.5, -6.0), (1.5, 7.0), (4.5, -2.0), (6.5, 1.0), (10.0, 1.0)];
                for w in pts.windows(2) {
                    self.dl.line(p(w[0].0, w[0].1), p(w[1].0, w[1].1), 1.9 * u, color);
                }
            }
        }
    }

    /// Bouton rond flottant.
    pub fn pastille(&mut self, id: Id, r: Rect, icon: Icon, active: bool) -> bool {
        let clicked = self.ui.click(id, r, None);
        let pressed = self.ui.pressed(id);
        let s = 8.0 * self.dp;
        self.dl.shadow(Rect::new(r.x, r.y + 2.0 * self.dp, r.w, r.h), r.w / 2.0, s, SHADOW);
        let bg = if active { ACCENT } else if pressed { CHIP } else { SURFACE };
        self.dl.rrect(r, r.w / 2.0, bg);
        self.icon(r.inset(r.w * 0.16), icon, if active { SURFACE } else { ACCENT });
        clicked
    }

    /// Pilule flottante, sur l'espace de travail.
    pub fn pill(&mut self, id: Id, r: Rect, label: &str, active: bool) -> bool {
        let clicked = self.ui.click(id, r, None);
        let pressed = self.ui.pressed(id);
        self.dl.shadow(Rect::new(r.x, r.y + 2.0 * self.dp, r.w, r.h), r.h / 2.0, 6.0 * self.dp, SHADOW);
        self.dl.rrect(r, r.h / 2.0, if active { ACCENT } else if pressed { CHIP } else { SURFACE });
        self.centered(r, label, 15.0, if active { SURFACE } else { TEXT });
        clicked
    }

    pub fn pill_width(&mut self, label: &str) -> f32 {
        self.text.measure(label, (15.0 * self.dp).round()) + 36.0 * self.dp
    }

    /// Rangée d'onglets dans l'en-tête d'un panneau. Renvoie l'onglet choisi.
    pub fn tabs(&mut self, id: Id, r: Rect, labels: &[&str], selected: usize) -> usize {
        let (mut x, mut result) = (r.x, selected);
        // Trop d'onglets pour la place : les marges se serrent, puis le texte rapetisse
        let n = labels.len().max(1) as f32;
        let ecart = 6.0 * self.dp;
        let textes: f32 = labels.iter().map(|l| self.text.measure(l, (15.0 * self.dp).round())).sum();
        let libre = r.w - ecart * (n - 1.0);
        let marge = ((libre - textes) / n).clamp(12.0 * self.dp, 32.0 * self.dp);
        let taille = if textes + marge * n > libre { (15.0 * (libre - marge * n) / textes).max(11.0) } else { 15.0 };
        for (i, label) in labels.iter().enumerate() {
            let w = self.text.measure(label, (taille * self.dp).round()) + marge;
            let t = Rect::new(x, r.y, w, r.h);
            if self.ui.click(sub_id(id, i), t, None) {
                result = i;
            }
            let on = i == selected;
            self.dl.rrect(t, r.h / 2.0, if on { ACCENT } else { CHIP });
            self.centered(t, label, taille, if on { SURFACE } else { TEXT });
            x += w + ecart;
        }
        result
    }

    pub fn body(&mut self, id: Id, rect: Rect, scrolls: &Scrolls) -> Body {
        self.ui.region_add(id, rect, Sense::Scroll);
        let delta = self.ui.take_scroll(id);
        let (off, content) = scrolls.get(&id).copied().unwrap_or((0.0, 0.0));
        let off = (off - delta).clamp(0.0, (content - rect.h).max(0.0));
        self.clip(Some(rect));
        let pad = 14.0 * self.dp;
        Body { x: rect.x + pad, w: rect.w - 2.0 * pad, y: rect.y + 8.0 * self.dp - off, top: rect.y - off, id, rect }
    }

    pub fn body_end(&mut self, b: Body, scrolls: &mut Scrolls) {
        self.clip(None);
        let content = b.y - b.top + 12.0 * self.dp;
        let off = b.rect.y - b.top;
        scrolls.insert(b.id, (off, content));
        if content > b.rect.h {
            let h = (b.rect.h * b.rect.h / content).max(32.0 * self.dp);
            let y = b.rect.y + off / (content - b.rect.h) * (b.rect.h - h);
            self.dl.rrect(Rect::new(b.rect.right() - 6.0 * self.dp, y, 3.0 * self.dp, h), 1.5 * self.dp, [0x6C, 0x6C, 0x80, 120]);
        }
    }

    pub fn section(&mut self, b: &mut Body, title: &str) {
        b.y += 10.0 * self.dp;
        self.label(b.x, b.y + 12.0 * self.dp, title, 12.5, DIM);
        b.y += 28.0 * self.dp;
    }

    pub fn paragraph(&mut self, b: &mut Body, s: &str) {
        let px = (15.0 * self.dp).round();
        for line in self.text.wrap(s, px, b.w) {
            self.label(b.x, b.y + 11.0 * self.dp, &line, 15.0, TEXT);
            b.y += 22.0 * self.dp;
        }
        b.y += 8.0 * self.dp;
    }

    /// Ligne de liste bord à bord. Renvoie vrai à l'appui.
    pub fn row(&mut self, b: &mut Body, id: Id, label: &str, swatch: Option<Color>, selected: bool) -> bool {
        let r = Rect::new(b.rect.x, b.y, b.rect.w, 48.0 * self.dp);
        let clicked = self.ui.click(id, r, Some(b.id));
        if self.ui.pressed(id) || selected {
            self.dl.rrect(r.inset(2.0 * self.dp), 10.0 * self.dp, CHIP);
        }
        let mut x = b.x;
        if let Some(c) = swatch {
            let s = 22.0 * self.dp;
            self.dl.rrect(Rect::new(x, r.cy() - s / 2.0, s, s), 6.0 * self.dp, c);
            x += s + 12.0 * self.dp;
        }
        self.label(x, r.cy(), label, 16.0, if selected { ACCENT } else { TEXT });
        b.y += r.h;
        clicked
    }

    /// Une mesure : libellé à gauche, valeur à droite.
    pub fn mesure(&mut self, b: &mut Body, label: &str, valeur: &str) {
        let h = 28.0 * self.dp;
        let cy = b.y + h / 2.0;
        self.label(b.x, cy, label, 14.5, DIM);
        self.text_right(b.x + b.w, cy, valeur, 14.5, TEXT);
        b.y += h;
    }

    /// Des barres, la plus récente à droite, sur l'échelle 0…`haut`, avec des traits de
    /// repère aux valeurs `reperes`. Une barre au-delà de `alerte` passe en couleur vive.
    pub fn barres(&mut self, b: &mut Body, valeurs: &[f32], haut: f32, reperes: &[f32], alerte: f32) {
        let d = self.dp;
        let r = Rect::new(b.x, b.y + 4.0 * d, b.w, 72.0 * d);
        self.dl.rrect(r, 8.0 * d, CHIP);
        let n = valeurs.len().max(1);
        let w = r.w / n as f32;
        for (i, v) in valeurs.iter().enumerate() {
            let h = (v / haut).clamp(0.0, 1.0) * (r.h - 4.0 * d);
            let c = if *v > alerte { [0xD8, 0x4A, 0x22, 255] } else { ACCENT };
            self.dl.rrect(Rect::new(r.x + i as f32 * w, r.bottom() - 2.0 * d - h, (w - 0.5 * d).max(d), h), 0.0, c);
        }
        for v in reperes {
            let y = r.bottom() - 2.0 * d - (v / haut).clamp(0.0, 1.0) * (r.h - 4.0 * d);
            self.dl.rrect(Rect::new(r.x, y, r.w, d.max(1.0)), 0.0, [0x6C, 0x6C, 0x80, 140]);
        }
        b.y += r.h + 10.0 * d;
    }

    pub fn toggle(&mut self, b: &mut Body, id: Id, label: &str, value: &mut bool) {
        let r = Rect::new(b.x, b.y, b.w, 48.0 * self.dp);
        if self.ui.click(id, r, Some(b.id)) {
            *value = !*value;
        }
        self.label(b.x, r.cy(), label, 16.0, TEXT);
        let (w, h) = (46.0 * self.dp, 26.0 * self.dp);
        let track = Rect::new(r.right() - w, r.cy() - h / 2.0, w, h);
        self.dl.rrect(track, h / 2.0, if *value { ACCENT } else { LINE });
        let k = h - 6.0 * self.dp;
        let kx = if *value { track.right() - k - 3.0 * self.dp } else { track.x + 3.0 * self.dp };
        self.dl.rrect(Rect::new(kx, track.y + 3.0 * self.dp, k, k), k / 2.0, SURFACE);
        b.y += r.h;
    }

    pub fn slider(&mut self, b: &mut Body, id: Id, label: &str, shown: &str, value: &mut f32, lo: f32, hi: f32) {
        let r = Rect::new(b.x, b.y, b.w, 64.0 * self.dp);
        self.label(b.x, r.y + 16.0 * self.dp, label, 16.0, TEXT);
        self.text_right(r.right(), r.y + 16.0 * self.dp, shown, 14.0, DIM);
        let track_y = r.y + 44.0 * self.dp;
        let knob = 11.0 * self.dp;
        let (x0, x1) = (r.x + knob, r.right() - knob);
        if let Some(d) = self.ui.drag(id, r) {
            *value = lo + ((d.x - x0) / (x1 - x0)).clamp(0.0, 1.0) * (hi - lo);
        }
        let t = 4.0 * self.dp;
        let at = x0 + (*value - lo) / (hi - lo) * (x1 - x0);
        self.dl.rrect(Rect::new(x0, track_y - t / 2.0, x1 - x0, t), t / 2.0, LINE);
        self.dl.rrect(Rect::new(x0, track_y - t / 2.0, at - x0, t), t / 2.0, ACCENT);
        let grow = if self.ui.pressed(id) { 2.0 * self.dp } else { 0.0 };
        self.dl.rrect(Rect::new(at - knob - grow, track_y - knob - grow, 2.0 * (knob + grow), 2.0 * (knob + grow)), knob + grow, ACCENT);
        b.y += r.h;
    }

    /// Des liens côte à côte, qui passent à la ligne quand la place manque. Renvoie celui
    /// qui est touché. Un lien `actif` est souligné.
    pub fn liens(&mut self, b: &mut Body, id: Id, labels: &[&str], actif: Option<usize>) -> Option<usize> {
        let d = self.dp;
        let (h, ecart) = (36.0 * d, 18.0 * d);
        let px = (15.0 * d).round();
        let (mut x, mut y, mut touche) = (b.x, b.y, None);
        for (i, label) in labels.iter().enumerate() {
            let w = self.text.measure(label, px);
            if x > b.x && x + w > b.x + b.w {
                x = b.x;
                y += h;
            }
            let r = Rect::new(x - 6.0 * d, y, w + 12.0 * d, h);
            let k = sub_id(id, i);
            if self.ui.click(k, r, Some(b.id)) {
                touche = Some(i);
            }
            if self.ui.pressed(k) {
                self.dl.rrect(r, 8.0 * d, CHIP);
            }
            self.label(x, y + h / 2.0, label, 15.0, ACCENT);
            if actif == Some(i) {
                self.dl.rrect(Rect::new(x, y + h / 2.0 + 10.0 * d, w, 2.0 * d), d, ACCENT);
            }
            x += w + ecart;
        }
        b.y = y + h + 4.0 * d;
        touche
    }

    /// Un réglage qui se choisit d'un appui : libellé à gauche, valeur à droite en
    /// couleur de lien ; `actif` le met en avant. Vrai à l'appui.
    pub fn choix(&mut self, b: &mut Body, id: Id, label: &str, valeur: &str, actif: bool) -> bool {
        let h = 40.0 * self.dp;
        let r = Rect::new(b.rect.x, b.y, b.rect.w, h);
        let clicked = self.ui.click(id, r, Some(b.id));
        if self.ui.pressed(id) || actif {
            self.dl.rrect(r.inset(2.0 * self.dp), 8.0 * self.dp, CHIP);
        }
        self.label(b.x, r.cy(), label, 14.5, if actif { TEXT } else { DIM });
        self.text_right(b.x + b.w, r.cy(), valeur, 14.5, ACCENT);
        b.y += h;
        clicked
    }

    /// Des pastilles de couleur ; renvoie celle qui est touchée.
    pub fn nuancier(&mut self, b: &mut Body, id: Id, couleurs: &[Color], choisie: Option<usize>) -> Option<usize> {
        let d = self.dp;
        let (c, ecart) = (30.0 * d, 10.0 * d);
        let (mut x, mut y, mut touche) = (b.x, b.y + 4.0 * d, None);
        for (i, col) in couleurs.iter().enumerate() {
            if x > b.x && x + c > b.x + b.w {
                x = b.x;
                y += c + ecart;
            }
            let r = Rect::new(x, y, c, c);
            if self.ui.click(sub_id(id, i), r.inset(-4.0 * d), Some(b.id)) {
                touche = Some(i);
            }
            if choisie == Some(i) {
                self.dl.rrect(r.inset(-3.0 * d), c / 2.0 + 3.0 * d, ACCENT);
            }
            self.dl.rrect(r, c / 2.0, *col);
            x += c + ecart;
        }
        b.y = y + c + 12.0 * d;
        touche
    }

    pub fn button(&mut self, b: &mut Body, id: Id, label: &str) -> bool {
        let r = Rect::new(b.x, b.y + 4.0 * self.dp, b.w, 44.0 * self.dp);
        let clicked = self.ui.click(id, r, Some(b.id));
        self.dl.rrect(r, r.h / 2.0, if self.ui.pressed(id) { LINE } else { CHIP });
        self.centered(r, label, 15.0, ACCENT);
        b.y += 52.0 * self.dp;
        clicked
    }
}
