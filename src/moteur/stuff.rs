//! Le conteneur `.stuff`, version 1 (voir `docs/format-stuff.md`) : un en-tête, un
//! manifeste JSON et des tampons binaires alignés sur 8 octets.
//!
//! Ce module ne connaît que le conteneur : lire, écrire, désigner et relire des tampons
//! typés. Le sens des sections (définitions, instances, région…) est l'affaire de ceux
//! qui s'en servent. Les clés inconnues du manifeste restent dans `manifeste`, et les
//! tampons à leur rang : réécrire un fichier relu ne perd rien.

use gda::json::{self, J};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub const MAGIQUE: &[u8; 8] = b"STUFF\0\0\0";
/// La version du conteneur et du manifeste.
pub const VERSION: u32 = 1;
const ENTETE: usize = 20;

/// Les types des éléments d'un tampon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Type {
    F32,
    U32,
    U16,
    I16,
    U8,
}

impl Type {
    pub fn nom(self) -> &'static str {
        match self {
            Type::F32 => "f32",
            Type::U32 => "u32",
            Type::U16 => "u16",
            Type::I16 => "i16",
            Type::U8 => "u8",
        }
    }

    pub fn de(s: &str) -> Option<Type> {
        Some(match s {
            "f32" => Type::F32,
            "u32" => Type::U32,
            "u16" => Type::U16,
            "i16" => Type::I16,
            "u8" => Type::U8,
            _ => return None,
        })
    }

    pub fn octets(self) -> usize {
        match self {
            Type::F32 | Type::U32 => 4,
            Type::U16 | Type::I16 => 2,
            Type::U8 => 1,
        }
    }
}

/// Une référence à des données d'un tampon : `{ tampon, debut (octets), taille
/// (éléments), type }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ref {
    pub tampon: usize,
    pub debut: usize,
    pub taille: usize,
    pub genre: Type,
}

impl Ref {
    pub fn json(&self) -> J {
        J::Objet(vec![
            ("tampon".into(), J::Num(self.tampon as f64)),
            ("debut".into(), J::Num(self.debut as f64)),
            ("taille".into(), J::Num(self.taille as f64)),
            ("type".into(), J::Txt(self.genre.nom().into())),
        ])
    }

    pub fn de(j: &J) -> Option<Ref> {
        let u = |k: &str| j.get(k)?.num().filter(|n| *n >= 0.0 && n.fract() == 0.0).map(|n| n as usize);
        let genre = Type::de(j.get("type")?.txt()?)?;
        let r = Ref { tampon: u("tampon")?, debut: u("debut")?, taille: u("taille")?, genre };
        (r.debut % genre.octets() == 0).then_some(r)
    }

    fn fin(&self) -> usize {
        self.debut + self.taille * self.genre.octets()
    }
}

/// Un fichier `.stuff` en mémoire.
#[derive(Clone, Debug, PartialEq)]
pub struct Stuff {
    /// Le manifeste, sans sa liste `tampons` : elle se refait à l'écriture.
    pub manifeste: J,
    pub tampons: Vec<Vec<u8>>,
}

/// Les éléments d'un type, en petit-boutiste.
pub trait Element: Copy {
    const TYPE: Type;
    fn ecrire(self, o: &mut Vec<u8>);
    fn lire(o: &[u8]) -> Self;
}

macro_rules! element {
    ($t:ty, $g:expr) => {
        impl Element for $t {
            const TYPE: Type = $g;
            fn ecrire(self, o: &mut Vec<u8>) {
                o.extend_from_slice(&self.to_le_bytes());
            }
            fn lire(o: &[u8]) -> Self {
                <$t>::from_le_bytes(o.try_into().unwrap())
            }
        }
    };
}
element!(f32, Type::F32);
element!(u32, Type::U32);
element!(u16, Type::U16);
element!(i16, Type::I16);
element!(u8, Type::U8);

fn objet(o: Vec<(&str, J)>) -> J {
    J::Objet(o.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn erreur<T>(s: &str) -> Result<T, String> {
    Err(s.to_string())
}

/// Lit l'en-tête : la longueur totale annoncée et celle du manifeste.
fn entete(o: &[u8]) -> Result<(usize, usize), String> {
    if o.len() < ENTETE || &o[..8] != MAGIQUE {
        return erreur("ce n'est pas un fichier .stuff");
    }
    let u = |k: usize| u32::from_le_bytes(o[k..k + 4].try_into().unwrap());
    if u(8) > VERSION {
        return Err(format!("conteneur .stuff de version {}, plus récent que ce lecteur", u(8)));
    }
    Ok((u(12) as usize, u(16) as usize))
}

/// Le manifeste relu, vérifié, et la taille de chaque tampon.
fn manifeste(o: &[u8]) -> Result<(J, Vec<usize>), String> {
    let s = std::str::from_utf8(o).map_err(|_| "manifeste illisible (UTF-8)".to_string())?;
    let mut j = json::lire(s.trim_end()).ok_or("manifeste illisible (JSON)")?;
    if j.get("format").and_then(J::txt) != Some("stuff") {
        return erreur("le manifeste ne dit pas « stuff »");
    }
    match j.get("version").and_then(J::num) {
        Some(v) if v <= VERSION as f64 => {}
        Some(v) => return Err(format!("manifeste de version {v}, plus récent que ce lecteur")),
        None => return erreur("le manifeste n'a pas de version"),
    }
    let mut tailles = Vec::new();
    if let J::Objet(o) = &mut j {
        if let Some(k) = o.iter().position(|(c, _)| c == "tampons") {
            let (_, l) = o.remove(k);
            for t in l.liste().ok_or("tampons illisibles")? {
                tailles.push(t.get("taille").and_then(J::num).filter(|n| *n >= 0.0).ok_or("taille de tampon illisible")? as usize);
            }
        }
    }
    Ok((j, tailles))
}

/// Les débuts des tampons, depuis le premier.
fn debuts(tailles: &[usize]) -> Vec<usize> {
    let mut d = Vec::with_capacity(tailles.len());
    let mut k = 0;
    for t in tailles {
        d.push(k);
        k += t.div_ceil(8) * 8;
    }
    d
}

impl Stuff {
    /// Un fichier vide de ce genre (`entite`, `catalogue`, `region`).
    pub fn new(id: &str, genre: &str, application: &str) -> Stuff {
        let manifeste = objet(vec![
            ("format", J::Txt("stuff".into())),
            ("version", J::Num(VERSION as f64)),
            ("application", J::Txt(application.into())),
            ("id", J::Txt(id.into())),
            ("genre", J::Txt(genre.into())),
        ]);
        Stuff { manifeste, tampons: Vec::new() }
    }

    /// Range des éléments dans le tampon `k` (créé au besoin), à la suite, alignés sur 8.
    pub fn ranger<T: Element>(&mut self, k: usize, v: &[T]) -> Ref {
        while self.tampons.len() <= k {
            self.tampons.push(Vec::new());
        }
        let t = &mut self.tampons[k];
        t.resize(t.len().div_ceil(8) * 8, 0);
        let debut = t.len();
        t.reserve(v.len() * T::TYPE.octets());
        for x in v {
            x.ecrire(t);
        }
        Ref { tampon: k, debut, taille: v.len(), genre: T::TYPE }
    }

    /// Range dans le tampon 0 et rend la référence en JSON.
    pub fn tampon<T: Element>(&mut self, v: &[T]) -> J {
        self.ranger(0, v).json()
    }

    /// Une clé de la racine du manifeste, remplacée si elle y est.
    pub fn mettre(&mut self, cle: &str, v: J) {
        if let J::Objet(o) = &mut self.manifeste {
            match o.iter_mut().find(|(c, _)| c == cle) {
                Some(x) => x.1 = v,
                None => o.push((cle.into(), v)),
            }
        }
    }

    pub fn get(&self, cle: &str) -> Option<&J> {
        self.manifeste.get(cle)
    }

    /// Les éléments que désigne `r` (une référence JSON), s'ils sont de ce type et dans
    /// leur tampon.
    pub fn lire_ref<T: Element>(&self, r: &J) -> Option<Vec<T>> {
        let r = Ref::de(r)?;
        lire_dans(self.tampons.get(r.tampon)?, &r)
    }

    pub fn octets(&self) -> Vec<u8> {
        let mut m = self.manifeste.clone();
        let tailles = J::Liste(self.tampons.iter().map(|t| objet(vec![("taille", J::Num(t.len() as f64))])).collect());
        if let J::Objet(o) = &mut m {
            o.retain(|(c, _)| c != "tampons");
            // Juste après l'en-tête du manifeste (format, version, application, id, genre)
            let k = o.iter().position(|(c, _)| c == "genre").map_or(o.len(), |k| k + 1);
            o.insert(k, ("tampons".into(), tailles));
        }
        let mut s = String::new();
        json::compact(&m, &mut s);
        while (ENTETE + s.len()) % 8 != 0 {
            s.push(' ');
        }
        let corps: usize = self.tampons.iter().map(|t| t.len().div_ceil(8) * 8).sum();
        let total = ENTETE + s.len() + corps;
        let mut o = Vec::with_capacity(total);
        o.extend_from_slice(MAGIQUE);
        o.extend_from_slice(&VERSION.to_le_bytes());
        o.extend_from_slice(&(total as u32).to_le_bytes());
        o.extend_from_slice(&(s.len() as u32).to_le_bytes());
        o.extend_from_slice(s.as_bytes());
        for t in &self.tampons {
            o.extend_from_slice(t);
            o.resize(o.len().div_ceil(8) * 8, 0);
        }
        o
    }

    pub fn lire(o: &[u8]) -> Result<Stuff, String> {
        let (total, lm) = entete(o)?;
        if total != o.len() {
            return Err(format!("fichier tronqué ou rallongé : {} octets pour {total} annoncés", o.len()));
        }
        let m = o.get(ENTETE..ENTETE + lm).ok_or("manifeste tronqué")?;
        let (manifeste, tailles) = manifeste(m)?;
        let depart = (ENTETE + lm).div_ceil(8) * 8;
        let mut tampons = Vec::with_capacity(tailles.len());
        for (t, d) in tailles.iter().zip(debuts(&tailles)) {
            tampons.push(o.get(depart + d..depart + d + t).ok_or("tampon tronqué")?.to_vec());
        }
        Ok(Stuff { manifeste, tampons })
    }
}

fn lire_dans<T: Element>(t: &[u8], r: &Ref) -> Option<Vec<T>> {
    if r.genre != T::TYPE {
        return None;
    }
    let o = t.get(r.debut..r.fin())?;
    Some(o.chunks_exact(T::TYPE.octets()).map(T::lire).collect())
}

/// Un `.stuff` sur disque dont on n'a lu que le manifeste : les tampons se lisent à la
/// demande, sans charger le reste du fichier.
pub struct Fichier {
    pub manifeste: J,
    f: std::fs::File,
    /// Le début de chaque tampon dans le fichier, et sa taille.
    tampons: Vec<(u64, usize)>,
}

/// Le texte de la valeur qui suit la dernière clé `"cle":` d'un manifeste, si c'est un
/// objet ou une liste : ses accolades ou crochets équilibrés, chaînes comprises.
fn extrait<'a>(m: &'a str, cle: &str) -> Option<&'a str> {
    let motif = format!("\"{cle}\":");
    let d = m.rfind(&motif)? + motif.len();
    let o = m.as_bytes();
    let (ouvre, ferme) = match o.get(d)? {
        b'{' => (b'{', b'}'),
        b'[' => (b'[', b']'),
        _ => return None,
    };
    let (mut prof, mut chaine, mut echap) = (0i32, false, false);
    for (k, &c) in o[d..].iter().enumerate() {
        if chaine {
            match c {
                _ if echap => echap = false,
                b'\\' => echap = true,
                b'"' => chaine = false,
                _ => {}
            }
            continue;
        }
        match c {
            b'"' => chaine = true,
            _ if c == ouvre => prof += 1,
            _ if c == ferme => {
                prof -= 1;
                if prof == 0 {
                    return Some(&m[d..d + k + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

impl Fichier {
    /// Ouvre le fichier sans décoder tout le manifeste : seulement les valeurs de ces
    /// clés (leur dernière occurrence, où qu'elle soit), réunies dans un objet. Bien plus
    /// rapide pour un gros manifeste dont on ne veut qu'une petite partie. Les clés
    /// cherchées doivent être uniques dans le manifeste.
    pub fn ouvrir_partiel(chemin: &Path, cles: &[&str]) -> Result<Fichier, String> {
        let mut f = std::fs::File::open(chemin).map_err(|e| e.to_string())?;
        let mut e = [0u8; ENTETE];
        f.read_exact(&mut e).map_err(|_| "ce n'est pas un fichier .stuff".to_string())?;
        let (total, lm) = entete(&e)?;
        let longueur = f.metadata().map_err(|e| e.to_string())?.len() as usize;
        if total != longueur {
            return Err(format!("fichier tronqué ou rallongé : {longueur} octets pour {total} annoncés"));
        }
        let mut m = vec![0u8; lm];
        f.read_exact(&mut m).map_err(|_| "manifeste tronqué".to_string())?;
        let m = std::str::from_utf8(&m).map_err(|_| "manifeste illisible (UTF-8)".to_string())?;
        let mut o = Vec::new();
        for c in cles.iter().chain(std::iter::once(&"tampons")) {
            if let Some(v) = extrait(m, c).and_then(json::lire) {
                o.push((c.to_string(), v));
            }
        }
        let tailles: Vec<usize> = match o.iter().find(|(c, _)| c == "tampons").and_then(|(_, v)| v.liste()) {
            Some(l) => l.iter().map(|t| t.get("taille").and_then(J::num).map(|n| n as usize)).collect::<Option<_>>().ok_or("taille de tampon illisible")?,
            None => Vec::new(),
        };
        o.retain(|(c, _)| c != "tampons");
        let depart = (ENTETE + lm).div_ceil(8) * 8;
        let tampons = tailles.iter().zip(debuts(&tailles)).map(|(t, d)| ((depart + d) as u64, *t)).collect();
        Ok(Fichier { manifeste: J::Objet(o), f, tampons })
    }

    pub fn ouvrir(chemin: &Path) -> Result<Fichier, String> {
        let mut f = std::fs::File::open(chemin).map_err(|e| e.to_string())?;
        let mut e = [0u8; ENTETE];
        f.read_exact(&mut e).map_err(|_| "ce n'est pas un fichier .stuff".to_string())?;
        let (total, lm) = entete(&e)?;
        let longueur = f.metadata().map_err(|e| e.to_string())?.len() as usize;
        if total != longueur {
            return Err(format!("fichier tronqué ou rallongé : {longueur} octets pour {total} annoncés"));
        }
        let mut m = vec![0u8; lm];
        f.read_exact(&mut m).map_err(|_| "manifeste tronqué".to_string())?;
        let (manifeste, tailles) = manifeste(&m)?;
        let depart = (ENTETE + lm).div_ceil(8) * 8;
        let tampons = tailles.iter().zip(debuts(&tailles)).map(|(t, d)| ((depart + d) as u64, *t)).collect();
        Ok(Fichier { manifeste, f, tampons })
    }

    pub fn get(&self, cle: &str) -> Option<&J> {
        self.manifeste.get(cle)
    }

    /// Les éléments que désigne `r`, lus dans le fichier.
    pub fn lire_ref<T: Element>(&mut self, r: &J) -> Option<Vec<T>> {
        let r = Ref::de(r)?;
        let &(d, t) = self.tampons.get(r.tampon)?;
        if r.genre != T::TYPE || r.fin() > t {
            return None;
        }
        let mut o = vec![0u8; r.fin() - r.debut];
        self.f.seek(SeekFrom::Start(d + r.debut as u64)).ok()?;
        self.f.read_exact(&mut o).ok()?;
        Some(o.chunks_exact(T::TYPE.octets()).map(T::lire).collect())
    }
}

/// Le résumé d'un fichier : son en-tête, ses tampons, son manifeste indenté.
pub fn resume(o: &[u8]) -> Result<String, String> {
    let s = Stuff::lire(o)?;
    let mut t = String::new();
    let g = |k: &str| s.get(k).and_then(J::txt).unwrap_or("?").to_string();
    t.push_str(&format!("{} : {} « {} », {} octets\n", g("application"), g("genre"), g("id"), o.len()));
    for (k, b) in s.tampons.iter().enumerate() {
        t.push_str(&format!("tampon {k} : {} octets\n", b.len()));
    }
    json::indente(&s.manifeste, 0, &mut t);
    t.push('\n');
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_stuff_se_relit_a_l_identique() {
        let mut s = Stuff::new("essai", "region", "S.A.G.E. essai");
        let a = s.tampon(&[1.5f32, -2.0, 3.25]);
        let b = s.tampon(&[7u8, 8, 9]);
        let c = s.ranger(1, &[-3i16, 4]).json();
        let d = s.tampon(&[65535u16]);
        s.mettre("donnees", J::Liste(vec![a.clone(), b.clone(), c.clone(), d.clone()]));
        s.mettre("inconnue", J::Txt("gardée".into()));
        let o = s.octets();
        assert_eq!(&o[..8], MAGIQUE);
        assert_eq!(o.len() % 8, 0);
        let r = Stuff::lire(&o).unwrap();
        assert_eq!(r, s);
        assert_eq!(r.lire_ref::<f32>(&a), Some(vec![1.5, -2.0, 3.25]));
        assert_eq!(r.lire_ref::<u8>(&b), Some(vec![7, 8, 9]));
        assert_eq!(r.lire_ref::<i16>(&c), Some(vec![-3, 4]));
        assert_eq!(r.lire_ref::<u16>(&d), Some(vec![65535]));
        // Un type qui ne correspond pas, un fichier tronqué
        assert_eq!(r.lire_ref::<u32>(&a), None);
        assert!(Stuff::lire(&o[..o.len() - 8]).is_err());
        // Réécrit tel quel
        assert_eq!(r.octets(), o);
        // Le tampon commence sur un multiple de 8
        let lm = u32::from_le_bytes(o[16..20].try_into().unwrap()) as usize;
        assert_eq!((ENTETE + lm) % 8, 0);
        // Lu sur disque, tampon par tampon
        let f = std::env::temp_dir().join(format!("sage-stuff-{}.stuff", std::process::id()));
        std::fs::write(&f, &o).unwrap();
        let mut l = Fichier::ouvrir(&f).unwrap();
        assert_eq!(l.lire_ref::<i16>(&c), Some(vec![-3, 4]));
        assert_eq!(l.lire_ref::<f32>(&a), Some(vec![1.5, -2.0, 3.25]));
        // Sans décoder le reste du manifeste
        let mut p = Fichier::ouvrir_partiel(&f, &["donnees"]).unwrap();
        assert!(p.get("inconnue").is_none());
        let d = p.get("donnees").unwrap().liste().unwrap()[2].clone();
        assert_eq!(p.lire_ref::<i16>(&d), Some(vec![-3, 4]));
        let _ = std::fs::remove_file(&f);
        assert!(resume(&o).unwrap().contains("gardée"));
    }

    #[test]
    fn un_stuff_trop_recent_est_refuse() {
        let mut s = Stuff::new("x", "entite", "essai");
        s.mettre("version", J::Num(2.0));
        assert!(Stuff::lire(&s.octets()).unwrap_err().contains("plus récent"));
        assert!(Stuff::lire(b"PASSTUFF000000000000").is_err());
    }
}
