//! Texte : polices du système, glyphes dessinés à la demande dans un atlas.
//! Aucun appel OpenGL : le moteur de rendu envoie l'atlas quand `dirty` est levé.

use crate::draw::{Color, DrawList, Rect};
use std::collections::HashMap;

pub const ATLAS: usize = 1024;
const FONTS: [&str; 3] = ["/system/fonts/RobotoStatic-Regular.ttf", "/system/fonts/Roboto-Regular.ttf", "/system/fonts/DroidSans.ttf"];

#[derive(Clone, Copy)]
struct Glyph {
    rect: Rect, // relatif au point de base
    uv0: [f32; 2],
    uv1: [f32; 2],
    advance: f32,
}

pub struct Text {
    font: Option<fontdue::Font>,
    glyphs: HashMap<(char, u32), Glyph>,
    pub atlas: Vec<u8>,
    pub dirty: bool,
    shelf: (usize, usize, usize), // x, y, hauteur de la rangée courante
}

impl Text {
    /// Sans police lisible, l'application reste utilisable, le texte n'est simplement pas dessiné.
    pub fn new() -> Text {
        let font = FONTS.iter().find_map(|p| {
            let bytes = std::fs::read(p).ok()?;
            fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default()).ok()
        });
        Text { font, glyphs: HashMap::new(), atlas: vec![0; ATLAS * ATLAS], dirty: true, shelf: (1, 1, 0) }
    }

    fn glyph(&mut self, c: char, px: f32) -> Option<Glyph> {
        let key = (c, px.to_bits());
        if let Some(g) = self.glyphs.get(&key) {
            return Some(*g);
        }
        let (m, bitmap) = self.font.as_ref()?.rasterize(c, px);
        let (w, h) = (m.width, m.height);
        let (mut x, mut y, mut row) = self.shelf;
        if x + w + 1 >= ATLAS {
            (x, y, row) = (1, y + row + 1, 0);
        }
        if y + h + 1 >= ATLAS {
            return None; // atlas plein : le glyphe ne s'affiche pas, rien ne plante
        }
        for line in 0..h {
            let at = (y + line) * ATLAS + x;
            self.atlas[at..at + w].copy_from_slice(&bitmap[line * w..(line + 1) * w]);
        }
        self.shelf = (x + w + 1, y, row.max(h));
        self.dirty = true;
        let t = ATLAS as f32;
        let g = Glyph {
            rect: Rect::new(m.xmin as f32, -(m.ymin as f32) - h as f32, w as f32, h as f32),
            uv0: [x as f32 / t, y as f32 / t],
            uv1: [(x + w) as f32 / t, (y + h) as f32 / t],
            advance: m.advance_width,
        };
        self.glyphs.insert(key, g);
        Some(g)
    }

    pub fn measure(&mut self, s: &str, px: f32) -> f32 {
        s.chars().map(|c| self.glyph(c, px).map_or(px * 0.55, |g| g.advance)).sum()
    }

    /// Décalage entre le centre d'une ligne et son point de base.
    pub fn center_offset(&self, px: f32) -> f32 {
        match self.font.as_ref().and_then(|f| f.horizontal_line_metrics(px)) {
            Some(m) => (m.ascent + m.descent) / 2.0,
            None => px * 0.35,
        }
    }

    pub fn draw(&mut self, dl: &mut DrawList, x: f32, baseline: f32, s: &str, px: f32, color: Color) {
        let (mut pen, base) = (x, baseline.round());
        for c in s.chars() {
            if let Some(g) = self.glyph(c, px) {
                if g.rect.w > 0.0 {
                    let r = Rect::new((pen + g.rect.x).round(), base + g.rect.y, g.rect.w, g.rect.h);
                    dl.glyph(r, g.uv0, g.uv1, color);
                }
                pen += g.advance;
            }
        }
    }

    /// Découpe `s` en lignes de largeur `w` au plus, aux espaces.
    pub fn wrap(&mut self, s: &str, px: f32, w: f32) -> Vec<String> {
        let mut lines = Vec::new();
        let mut line = String::new();
        for word in s.split_whitespace() {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if self.measure(&candidate, px) > w && !line.is_empty() {
                lines.push(std::mem::replace(&mut line, word.to_string()));
            } else {
                line = candidate;
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
        lines
    }
}

impl Default for Text {
    fn default() -> Text {
        Text::new()
    }
}
