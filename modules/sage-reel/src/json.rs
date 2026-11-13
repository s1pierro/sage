//! JSON minimal, sans dépendance : l'ordre des clés est conservé et les nombres
//! s'écrivent comme en JavaScript, pour relire et réécrire les projets de GDA.

use std::fmt::Write;

#[derive(Clone, Debug, PartialEq)]
pub enum J {
    Nul,
    Bool(bool),
    Num(f64),
    Txt(String),
    Liste(Vec<J>),
    Objet(Vec<(String, J)>),
}

impl J {
    pub fn get(&self, cle: &str) -> Option<&J> {
        match self {
            J::Objet(o) => o.iter().find(|(k, _)| k == cle).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn num(&self) -> Option<f64> {
        if let J::Num(n) = self { Some(*n) } else { None }
    }
    pub fn txt(&self) -> Option<&str> {
        if let J::Txt(s) = self { Some(s) } else { None }
    }
    pub fn liste(&self) -> Option<&[J]> {
        if let J::Liste(l) = self { Some(l) } else { None }
    }
    pub fn bool(&self) -> Option<bool> {
        if let J::Bool(b) = self { Some(*b) } else { None }
    }
    /// Tableau de `N` nombres, sinon `None`.
    pub fn nombres<const N: usize>(&self) -> Option<[f64; N]> {
        let l = self.liste()?;
        if l.len() != N {
            return None;
        }
        let mut r = [0.0; N];
        for (a, b) in r.iter_mut().zip(l) {
            *a = b.num()?;
        }
        Some(r)
    }
}

/// Nombre écrit comme `String(x)` en JavaScript.
pub fn nombre(x: f64) -> String {
    if !x.is_finite() {
        return "null".into();
    }
    if x == 0.0 {
        return "0".into();
    }
    let a = x.abs();
    if (1e-6..1e21).contains(&a) {
        return format!("{x}");
    }
    let e = format!("{x:e}");
    let (m, p) = e.split_once('e').unwrap();
    if p.starts_with('-') { format!("{m}e{p}") } else { format!("{m}e+{p}") }
}

pub fn texte(s: &str, o: &mut String) {
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            '\u{8}' => o.push_str("\\b"),
            '\u{c}' => o.push_str("\\f"),
            c if (c as u32) < 0x20 => write!(o, "\\u{:04x}", c as u32).unwrap(),
            c => o.push(c),
        }
    }
    o.push('"');
}

pub fn compact(j: &J, o: &mut String) {
    match j {
        J::Nul => o.push_str("null"),
        J::Bool(b) => o.push_str(if *b { "true" } else { "false" }),
        J::Num(n) => o.push_str(&nombre(*n)),
        J::Txt(s) => texte(s, o),
        J::Liste(l) => {
            o.push('[');
            for (i, v) in l.iter().enumerate() {
                if i > 0 {
                    o.push(',');
                }
                compact(v, o);
            }
            o.push(']');
        }
        J::Objet(m) => {
            o.push('{');
            for (i, (k, v)) in m.iter().enumerate() {
                if i > 0 {
                    o.push(',');
                }
                texte(k, o);
                o.push(':');
                compact(v, o);
            }
            o.push('}');
        }
    }
}

fn marge(n: usize, o: &mut String) {
    o.push('\n');
    for _ in 0..n {
        o.push_str("  ");
    }
}

/// Présentation de `ed_texteProjet` : indentée, mais chaque tableau sans
/// sous-tableau ni objet (un sommet, une face) tient sur une ligne.
pub fn indente(j: &J, n: usize, o: &mut String) {
    match j {
        J::Liste(l) if l.is_empty() => o.push_str("[]"),
        J::Liste(l) if l.iter().all(|v| !matches!(v, J::Liste(_) | J::Objet(_))) => {
            o.push('[');
            for (i, v) in l.iter().enumerate() {
                if i > 0 {
                    o.push_str(", ");
                }
                compact(v, o);
            }
            o.push(']');
        }
        J::Liste(l) => {
            o.push('[');
            for (i, v) in l.iter().enumerate() {
                if i > 0 {
                    o.push(',');
                }
                marge(n + 1, o);
                indente(v, n + 1, o);
            }
            marge(n, o);
            o.push(']');
        }
        J::Objet(m) if m.is_empty() => o.push_str("{}"),
        J::Objet(m) => {
            o.push('{');
            for (i, (k, v)) in m.iter().enumerate() {
                if i > 0 {
                    o.push(',');
                }
                marge(n + 1, o);
                texte(k, o);
                o.push_str(": ");
                indente(v, n + 1, o);
            }
            marge(n, o);
            o.push('}');
        }
        v => compact(v, o),
    }
}

struct Lecteur<'a> {
    b: &'a [u8],
    i: usize,
}

impl Lecteur<'_> {
    fn blancs(&mut self) {
        while matches!(self.b.get(self.i), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.i += 1;
        }
    }
    fn mot(&mut self, m: &str, v: J) -> Option<J> {
        if self.b[self.i..].starts_with(m.as_bytes()) {
            self.i += m.len();
            Some(v)
        } else {
            None
        }
    }
    fn hex4(&mut self) -> Option<u32> {
        let s = std::str::from_utf8(self.b.get(self.i..self.i + 4)?).ok()?;
        self.i += 4;
        u32::from_str_radix(s, 16).ok()
    }
    fn chaine(&mut self) -> Option<String> {
        self.i += 1;
        let mut o = Vec::new();
        loop {
            let c = *self.b.get(self.i)?;
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(o).ok(),
                b'\\' => {
                    let e = *self.b.get(self.i)?;
                    self.i += 1;
                    let c = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'u' => {
                            let mut u = self.hex4()?;
                            if (0xD800..0xDC00).contains(&u) && self.b[self.i..].starts_with(b"\\u") {
                                self.i += 2;
                                let v = self.hex4()?;
                                u = 0x10000 + ((u - 0xD800) << 10) + (v.wrapping_sub(0xDC00) & 0x3FF);
                            }
                            char::from_u32(u).unwrap_or('\u{FFFD}')
                        }
                        _ => return None,
                    };
                    let mut t = [0; 4];
                    o.extend_from_slice(c.encode_utf8(&mut t).as_bytes());
                }
                c => o.push(c),
            }
        }
    }
    fn valeur(&mut self) -> Option<J> {
        self.blancs();
        match *self.b.get(self.i)? {
            b'n' => self.mot("null", J::Nul),
            b't' => self.mot("true", J::Bool(true)),
            b'f' => self.mot("false", J::Bool(false)),
            b'"' => self.chaine().map(J::Txt),
            b'[' => {
                self.i += 1;
                let mut l = Vec::new();
                self.blancs();
                if self.b.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Some(J::Liste(l));
                }
                loop {
                    l.push(self.valeur()?);
                    self.blancs();
                    match *self.b.get(self.i)? {
                        b',' => self.i += 1,
                        b']' => {
                            self.i += 1;
                            return Some(J::Liste(l));
                        }
                        _ => return None,
                    }
                }
            }
            b'{' => {
                self.i += 1;
                let mut m = Vec::new();
                self.blancs();
                if self.b.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Some(J::Objet(m));
                }
                loop {
                    self.blancs();
                    if self.b.get(self.i) != Some(&b'"') {
                        return None;
                    }
                    let k = self.chaine()?;
                    self.blancs();
                    if self.b.get(self.i) != Some(&b':') {
                        return None;
                    }
                    self.i += 1;
                    m.push((k, self.valeur()?));
                    self.blancs();
                    match *self.b.get(self.i)? {
                        b',' => self.i += 1,
                        b'}' => {
                            self.i += 1;
                            return Some(J::Objet(m));
                        }
                        _ => return None,
                    }
                }
            }
            _ => {
                let d = self.i;
                while matches!(self.b.get(self.i), Some(b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')) {
                    self.i += 1;
                }
                std::str::from_utf8(&self.b[d..self.i]).ok()?.parse().ok().map(J::Num)
            }
        }
    }
}

pub fn lire(s: &str) -> Option<J> {
    let mut l = Lecteur { b: s.as_bytes(), i: 0 };
    let v = l.valeur()?;
    l.blancs();
    (l.i == s.len()).then_some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nombres_comme_javascript() {
        for (x, s) in [(0.0, "0"), (-0.0, "0"), (1.5, "1.5"), (1e-7, "1e-7"), (1e21, "1e+21"), (-2.5e-8, "-2.5e-8"), (0.00001, "0.00001"), (100.0, "100")] {
            assert_eq!(nombre(x), s);
        }
    }

    #[test]
    fn aller_retour() {
        let t = r#"{"a":[1,2.5,"é\"x"],"b":{"c":null,"d":true},"e":[]}"#;
        let mut o = String::new();
        compact(&lire(t).unwrap(), &mut o);
        assert_eq!(o, t);
        assert!(lire("[1,").is_none());
        assert!(lire("{} x").is_none());
    }
}
