//! Les objets du décor : une entité chacun, faite d'un modèle du catalogue et de ses
//! réglages. Son maillage, dans son repère, est partagé par tous ceux qui ont les mêmes
//! réglages ; ses obstacles suivent son cap.
//!
//! Le repère d'un objet : la base au sol en (0, 0, 0), l'avant vers +z, comme celui des
//! personnages.

use super::entites::Aabb;
use super::physique::{Forme, Triangles};
use std::sync::Arc;

/// Les modèles du catalogue, par identifiant.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Genre {
    Bloc,
    Dalle,
    Poteau,
    Rampe,
}

/// Les rampes s'enfoncent de tant sous leur départ : leurs flancs ne laissent pas de jour
/// sur un sol en pente.
pub const ENFONCE: f32 = 1.0;

impl Genre {
    pub const TOUS: [Genre; 4] = [Genre::Bloc, Genre::Dalle, Genre::Poteau, Genre::Rampe];

    /// L'identifiant, tel qu'il s'écrit dans les fichiers.
    pub fn ident(self) -> &'static str {
        match self {
            Genre::Bloc => "bloc",
            Genre::Dalle => "dalle",
            Genre::Poteau => "poteau",
            Genre::Rampe => "rampe",
        }
    }

    pub fn depuis(s: &str) -> Option<Genre> {
        Genre::TOUS.into_iter().find(|g| g.ident() == s)
    }

    /// Le nom affiché.
    pub fn nom(self) -> &'static str {
        match self {
            Genre::Bloc => "Bloc",
            Genre::Dalle => "Dalle",
            Genre::Poteau => "Poteau",
            Genre::Rampe => "Rampe",
        }
    }

    /// Ce que veut dire chaque dimension.
    pub fn dimensions(self) -> [&'static str; 3] {
        match self {
            Genre::Poteau => ["Diamètre", "Hauteur", ""],
            Genre::Rampe => ["Largeur", "Montée", "Longueur"],
            _ => ["Largeur", "Hauteur", "Profondeur"],
        }
    }
}

/// Un objet : son modèle, ses dimensions (largeur x, hauteur y, profondeur z, en mètres)
/// et sa couleur.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Objet {
    pub genre: Genre,
    pub dim: [f32; 3],
    pub couleur: [f32; 3],
}

impl Objet {
    /// L'objet tel que le catalogue le propose.
    pub fn neuf(genre: Genre) -> Objet {
        match genre {
            Genre::Bloc => Objet { genre, dim: [1.6, 1.6, 1.6], couleur: [0.82, 0.78, 0.70] },
            Genre::Dalle => Objet { genre, dim: [2.4, 0.25, 2.4], couleur: [0.74, 0.72, 0.68] },
            Genre::Poteau => Objet { genre, dim: [0.3, 3.0, 0.3], couleur: [0.55, 0.42, 0.30] },
            Genre::Rampe => Objet { genre, dim: [1.2, 1.2, 5.0], couleur: [0.70, 0.64, 0.54] },
        }
    }

    /// Les dimensions bornées à ce que le modèle accepte ; un poteau est rond.
    pub fn borne(mut self) -> Objet {
        let (lo, hi) = (0.05, 40.0);
        self.dim = self.dim.map(|d| d.clamp(lo, hi));
        if self.genre == Genre::Poteau {
            self.dim[2] = self.dim[0];
        }
        self
    }

    /// L'identifiant de son maillage : le même pour les mêmes réglages.
    pub fn cle(&self) -> String {
        let d = self.dim.map(|x| (x * 1000.0).round() as i32);
        format!("objet/{}/{}x{}x{}/{}", self.genre.ident(), d[0], d[1], d[2], hex(self.couleur))
    }

    /// Ses triangles, dans son repère.
    pub fn triangles(&self) -> Vec<[[f32; 3]; 3]> {
        let [x, y, z] = self.dim;
        match self.genre {
            Genre::Bloc | Genre::Dalle => boite([-x / 2.0, 0.0, -z / 2.0], [x / 2.0, y, z / 2.0]),
            Genre::Poteau => poteau(y, x / 2.0),
            Genre::Rampe => rampe(z, x, y),
        }
    }

    /// Son maillage au format `Format::Statique`, dans son repère.
    pub fn maillage(&self) -> Vec<f32> {
        let mut v = Vec::new();
        ajouter(&mut v, &self.triangles(), self.couleur);
        v
    }

    /// Ses obstacles au cap `cap`, dans le repère de son lieu sans rotation, comme la
    /// physique les veut. Une boîte tournée d'un quart de tour reste une boîte ; de
    /// travers, elle devient des triangles.
    pub fn obstacles(&self, cap: f32) -> Vec<Forme> {
        let [x, y, z] = self.dim;
        match self.genre {
            Genre::Bloc | Genre::Dalle => {
                let q = cap / std::f32::consts::FRAC_PI_2;
                if (q - q.round()).abs() < 1e-3 {
                    let impair = (q.round() as i64).rem_euclid(2) == 1;
                    let (dx, dz) = if impair { (z / 2.0, x / 2.0) } else { (x / 2.0, z / 2.0) };
                    vec![Forme::Boite(Aabb { centre: [0.0, y / 2.0, 0.0], demi: [dx, y / 2.0, dz] })]
                } else {
                    vec![Forme::Triangles(Arc::new(Triangles::new(tourner_tous(&self.triangles(), cap))))]
                }
            }
            Genre::Poteau => vec![Forme::Capsule { base: [0.0; 3], hauteur: y, rayon: x / 2.0 }],
            Genre::Rampe => vec![Forme::Triangles(Arc::new(Triangles::new(tourner_tous(&self.triangles(), cap))))],
        }
    }
}

/// Le point `p` de l'objet, tourné de `cap` comme le fait `Lieu::place`.
pub fn tourner(p: [f32; 3], cap: f32) -> [f32; 3] {
    let (s, c) = cap.sin_cos();
    [p[0] * c + p[2] * s, p[1], -p[0] * s + p[2] * c]
}

fn tourner_tous(t: &[[[f32; 3]; 3]], cap: f32) -> Vec<[[f32; 3]; 3]> {
    t.iter().map(|tri| tri.map(|p| tourner(p, cap))).collect()
}

/// Une couleur en six chiffres hexadécimaux.
pub fn hex(c: [f32; 3]) -> String {
    let o = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    format!("{:02x}{:02x}{:02x}", o[0], o[1], o[2])
}

pub fn depuis_hex(s: &str) -> Option<[f32; 3]> {
    if s.len() != 6 || !s.is_ascii() {
        return None;
    }
    let k = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok().map(|v| v as f32 / 255.0);
    Some([k(0)?, k(2)?, k(4)?])
}

/// Des triangles au format `Format::Statique`, d'une couleur, chacun de sa normale.
pub fn ajouter(v: &mut Vec<f32>, t: &[[[f32; 3]; 3]], c: [f32; 3]) {
    for [a, b, d] in t {
        let (u, w) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [d[0] - a[0], d[1] - a[1], d[2] - a[2]]);
        let n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-9);
        for p in [a, b, d] {
            v.extend_from_slice(p);
            v.extend_from_slice(&[n[0] / l, n[1] / l, n[2] / l]);
            v.extend_from_slice(&c);
        }
    }
}

/// Un quadrilatère en deux triangles, ses coins dans le sens trigonométrique vus de
/// dehors.
fn quad(a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]) -> [[[f32; 3]; 3]; 2] {
    [[a, b, c], [a, c, d]]
}

/// Une boîte de `lo` à `hi`, ses faces vers le dehors.
fn boite(lo: [f32; 3], hi: [f32; 3]) -> Vec<[[f32; 3]; 3]> {
    let p = |x: usize, y: usize, z: usize| [[lo[0], hi[0]][x], [lo[1], hi[1]][y], [lo[2], hi[2]][z]];
    let mut t = Vec::with_capacity(12);
    t.extend(quad(p(1, 0, 0), p(1, 1, 0), p(1, 1, 1), p(1, 0, 1))); // +x
    t.extend(quad(p(0, 0, 1), p(0, 1, 1), p(0, 1, 0), p(0, 0, 0))); // -x
    t.extend(quad(p(0, 1, 0), p(0, 1, 1), p(1, 1, 1), p(1, 1, 0))); // +y
    t.extend(quad(p(0, 0, 0), p(1, 0, 0), p(1, 0, 1), p(0, 0, 1))); // -y
    t.extend(quad(p(1, 0, 1), p(1, 1, 1), p(0, 1, 1), p(0, 0, 1))); // +z
    t.extend(quad(p(0, 0, 0), p(0, 1, 0), p(1, 1, 0), p(1, 0, 0))); // -z
    t
}

/// Un poteau à huit pans, de `h` de haut, coiffé d'un cône bas.
fn poteau(h: f32, r: f32) -> Vec<[[f32; 3]; 3]> {
    let n = 8;
    let coin = |k: usize, y: f32| {
        let a = k as f32 / n as f32 * std::f32::consts::TAU;
        [r * a.cos(), y, r * a.sin()]
    };
    let sommet = [0.0, h, 0.0];
    let mut t = Vec::new();
    for k in 0..n {
        let (a, b) = (k, (k + 1) % n);
        t.extend(quad(coin(a, 0.0), coin(a, h - r), coin(b, h - r), coin(b, 0.0)));
        t.push([coin(a, h - r), sommet, coin(b, h - r)]);
    }
    t
}

/// Une rampe de `long` sur `large`, partant de l'origine vers +z, qui monte de `haut` ;
/// ses flancs descendent jusqu'à `ENFONCE` sous le départ.
fn rampe(long: f32, large: f32, haut: f32) -> Vec<[[f32; 3]; 3]> {
    let p = |s: f32, c: f32, y: f32| [-c * large / 2.0, y, s];
    let bas = -ENFONCE;
    let mut t = Vec::new();
    // Le dessus, puis les flancs, puis le bout de départ
    t.extend(quad(p(0.0, -1.0, 0.0), p(0.0, 1.0, 0.0), p(long, 1.0, haut), p(long, -1.0, haut)));
    t.extend(quad(p(0.0, 1.0, bas), p(long, 1.0, bas), p(long, 1.0, haut), p(0.0, 1.0, 0.0)));
    t.extend(quad(p(long, -1.0, bas), p(0.0, -1.0, bas), p(0.0, -1.0, 0.0), p(long, -1.0, haut)));
    t.extend(quad(p(0.0, -1.0, bas), p(0.0, 1.0, bas), p(0.0, 1.0, 0.0), p(0.0, -1.0, 0.0)));
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_boite_regarde_dehors_et_se_tourne() {
        let o = Objet::neuf(Genre::Bloc);
        let t = o.triangles();
        assert_eq!(t.len(), 12);
        for [a, b, c] in &t {
            let (u, w) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
            let n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
            let m = [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0 - o.dim[1] / 2.0, (a[2] + b[2] + c[2]) / 3.0];
            assert!(n[0] * m[0] + n[1] * m[1] + n[2] * m[2] > 0.0, "vers le dehors");
        }
        let o = Objet { dim: [4.0, 1.0, 1.0], ..o };
        match &o.obstacles(std::f32::consts::FRAC_PI_2)[..] {
            [Forme::Boite(b)] => assert!((b.demi[0] - 0.5).abs() < 1e-5 && (b.demi[2] - 2.0).abs() < 1e-5, "{b:?}"),
            f => panic!("{f:?}"),
        }
        assert!(matches!(o.obstacles(0.3)[..], [Forme::Triangles(_)]));
    }

    #[test]
    fn la_cle_suit_les_reglages_et_la_couleur_se_relit() {
        let a = Objet::neuf(Genre::Dalle);
        assert_eq!(a.cle(), Objet::neuf(Genre::Dalle).cle());
        assert_ne!(a.cle(), Objet { couleur: [0.1, 0.2, 0.3], ..a }.cle());
        let c = depuis_hex(&hex([0.2, 0.5, 1.0])).unwrap();
        assert!((c[0] - 0.2).abs() < 0.003 && c[2] == 1.0);
        assert_eq!(depuis_hex("zz0000"), None);
        assert_eq!(Genre::depuis("rampe"), Some(Genre::Rampe));
    }

    #[test]
    fn la_rampe_tournee_suit_son_cap() {
        let o = Objet::neuf(Genre::Rampe);
        // Vers +x : le haut de la rampe est à x = longueur
        let t = tourner_tous(&o.triangles(), std::f32::consts::FRAC_PI_2);
        let haut = t.iter().flatten().filter(|p| (p[1] - o.dim[1]).abs() < 1e-5).map(|p| p[0]).fold(f32::MIN, f32::max);
        assert!((haut - o.dim[2]).abs() < 1e-4, "{haut}");
    }
}
