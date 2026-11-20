//! Un JSON minimal (repris de `flow::glb`) : assez pour l'en-tête des GLB.

/// Une valeur JSON.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Nul,
    Bool(bool),
    Nombre(f64),
    Texte(String),
    Liste(Vec<Json>),
    Objet(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, k: &str) -> Option<&Json> {
        match self {
            Json::Objet(v) => v.iter().find(|e| e.0 == k).map(|e| &e.1),
            _ => None,
        }
    }

    pub fn at(&self, i: usize) -> Option<&Json> {
        match self {
            Json::Liste(v) => v.get(i),
            _ => None,
        }
    }

    pub fn liste(&self) -> &[Json] {
        match self {
            Json::Liste(v) => v,
            _ => &[],
        }
    }

    pub fn nombre(&self) -> Option<f64> {
        match self {
            Json::Nombre(n) => Some(*n),
            _ => None,
        }
    }

    pub fn entier(&self, k: &str) -> Option<usize> {
        self.get(k)?.nombre().filter(|n| *n >= 0.0).map(|n| n as usize)
    }

    pub fn texte(&self) -> Option<&str> {
        match self {
            Json::Texte(s) => Some(s),
            _ => None,
        }
    }

    pub fn lire(s: &str) -> Result<Json, String> {
        let b = s.as_bytes();
        let mut i = 0;
        let v = valeur(b, &mut i)?;
        blancs(b, &mut i);
        if i != b.len() {
            return Err(format!("JSON : reste inattendu à l'octet {i}"));
        }
        Ok(v)
    }

    pub fn ecrire(&self) -> String {
        match self {
            Json::Nul => "null".into(),
            Json::Bool(b) => b.to_string(),
            Json::Nombre(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    format!("{}", *n as i64)
                } else {
                    format!("{n}")
                }
            }
            Json::Texte(s) => {
                let mut o = String::from("\"");
                for c in s.chars() {
                    match c {
                        '"' => o.push_str("\\\""),
                        '\\' => o.push_str("\\\\"),
                        '\n' => o.push_str("\\n"),
                        c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
                        c => o.push(c),
                    }
                }
                o.push('"');
                o
            }
            Json::Liste(v) => format!("[{}]", v.iter().map(Json::ecrire).collect::<Vec<_>>().join(",")),
            Json::Objet(v) => format!("{{{}}}", v.iter().map(|(k, x)| format!("{}:{}", Json::Texte(k.clone()).ecrire(), x.ecrire())).collect::<Vec<_>>().join(",")),
        }
    }
}

fn blancs(b: &[u8], i: &mut usize) {
    while *i < b.len() && b[*i].is_ascii_whitespace() {
        *i += 1;
    }
}

fn valeur(b: &[u8], i: &mut usize) -> Result<Json, String> {
    blancs(b, i);
    let err = |i: usize| format!("JSON mal formé à l'octet {i}");
    match b.get(*i).copied() {
        Some(b'{') => {
            *i += 1;
            let mut v = Vec::new();
            loop {
                blancs(b, i);
                if b.get(*i) == Some(&b'}') {
                    *i += 1;
                    return Ok(Json::Objet(v));
                }
                let Json::Texte(k) = valeur(b, i)? else { return Err(err(*i)) };
                blancs(b, i);
                if b.get(*i) != Some(&b':') {
                    return Err(err(*i));
                }
                *i += 1;
                v.push((k, valeur(b, i)?));
                blancs(b, i);
                match b.get(*i) {
                    Some(b',') => *i += 1,
                    Some(b'}') => {}
                    _ => return Err(err(*i)),
                }
            }
        }
        Some(b'[') => {
            *i += 1;
            let mut v = Vec::new();
            loop {
                blancs(b, i);
                if b.get(*i) == Some(&b']') {
                    *i += 1;
                    return Ok(Json::Liste(v));
                }
                v.push(valeur(b, i)?);
                blancs(b, i);
                match b.get(*i) {
                    Some(b',') => *i += 1,
                    Some(b']') => {}
                    _ => return Err(err(*i)),
                }
            }
        }
        Some(b'"') => {
            *i += 1;
            let mut o: Vec<u8> = Vec::new();
            loop {
                match b.get(*i).copied() {
                    None => return Err(err(*i)),
                    Some(b'"') => {
                        *i += 1;
                        return String::from_utf8(o).map(Json::Texte).map_err(|_| err(*i));
                    }
                    Some(b'\\') => {
                        let c = b.get(*i + 1).copied().ok_or_else(|| err(*i))?;
                        *i += 2;
                        let ch = match c {
                            b'n' => '\n',
                            b't' => '\t',
                            b'r' => '\r',
                            b'b' => '\u{8}',
                            b'f' => '\u{c}',
                            b'u' => {
                                let h = std::str::from_utf8(b.get(*i..*i + 4).ok_or_else(|| err(*i))?).map_err(|_| err(*i))?;
                                *i += 4;
                                char::from_u32(u32::from_str_radix(h, 16).map_err(|_| err(*i))?).unwrap_or('\u{fffd}')
                            }
                            c => c as char,
                        };
                        let mut t = [0u8; 4];
                        o.extend_from_slice(ch.encode_utf8(&mut t).as_bytes());
                    }
                    Some(c) => {
                        o.push(c);
                        *i += 1;
                    }
                }
            }
        }
        Some(b't') if b[*i..].starts_with(b"true") => {
            *i += 4;
            Ok(Json::Bool(true))
        }
        Some(b'f') if b[*i..].starts_with(b"false") => {
            *i += 5;
            Ok(Json::Bool(false))
        }
        Some(b'n') if b[*i..].starts_with(b"null") => {
            *i += 4;
            Ok(Json::Nul)
        }
        Some(c) if c == b'-' || c.is_ascii_digit() => {
            let debut = *i;
            *i += 1;
            while *i < b.len() && (b[*i].is_ascii_digit() || matches!(b[*i], b'.' | b'e' | b'E' | b'+' | b'-')) {
                *i += 1;
            }
            std::str::from_utf8(&b[debut..*i]).ok().and_then(|s| s.parse().ok()).map(Json::Nombre).ok_or_else(|| err(debut))
        }
        _ => Err(err(*i)),
    }
}
