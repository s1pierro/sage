//! Le générateur de textures : bois, brique et pierre, qui se répètent sans couture.
//!
//! Tout est calcul pur, sans OpenGL, donc testable partout. Chaque texture couvre un
//! carré de `cote` mètres ; ses éléments (lames, briques, pierres) y tiennent un nombre
//! entier de fois, et tous les bruits ont la période de la texture : posée bout à bout,
//! elle ne montre aucune couture.

use crate::cles;

/// La nature d'une texture.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Genre {
    Bois,
    Brique,
    Pierre,
}

impl Genre {
    pub const TOUS: [Genre; 3] = [Genre::Bois, Genre::Brique, Genre::Pierre];

    /// L'identifiant, tel qu'il s'écrit dans les fichiers.
    pub fn id(self) -> &'static str {
        match self {
            Genre::Bois => "bois",
            Genre::Brique => "brique",
            Genre::Pierre => "pierre",
        }
    }

    pub fn nom(self) -> &'static str {
        match self {
            Genre::Bois => "Bois",
            Genre::Brique => "Brique",
            Genre::Pierre => "Pierre",
        }
    }

    pub fn de(id: &str) -> Option<Genre> {
        Genre::TOUS.into_iter().find(|g| g.id() == id.trim().to_lowercase())
    }

    pub fn rang(self) -> usize {
        Genre::TOUS.iter().position(|g| *g == self).unwrap_or(0)
    }

    /// Les appareils proposés (la façon de décaler les éléments d'un rang à l'autre).
    pub fn appareils(self) -> &'static [&'static str] {
        match self {
            Genre::Bois => &["Aléatoire", "½", "⅓", "Aligné"],
            Genre::Brique => &["½", "⅓", "¼", "Aligné"],
            Genre::Pierre => &["Moellons", "Assises"],
        }
    }
}

/// Une couleur en teinte (degrés), saturation et luminosité (de 0 à 1).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Tsl {
    pub t: f32,
    pub s: f32,
    pub l: f32,
}

impl Tsl {
    pub const fn new(t: f32, s: f32, l: f32) -> Tsl {
        Tsl { t, s, l }
    }

    /// Rouge, vert, bleu, de 0 à 1.
    pub fn rvb(self) -> [f32; 3] {
        let (s, l) = (self.s.clamp(0.0, 1.0), self.l.clamp(0.0, 1.0));
        let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
        let h = self.t.rem_euclid(360.0) / 60.0;
        let x = c * (1.0 - (h % 2.0 - 1.0).abs());
        let (r, g, b) = match h as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let m = l - c / 2.0;
        [r + m, g + m, b + m]
    }

    pub fn octets(self) -> [u8; 4] {
        let [r, g, b] = self.rvb();
        [octet(r), octet(g), octet(b), 255]
    }

    fn ecrire(self) -> String {
        format!("{} {} {}", arrondi(self.t, 1), arrondi(self.s, 3), arrondi(self.l, 3))
    }

    fn lire(v: &str) -> Option<Tsl> {
        let n: Vec<f32> = v.split_whitespace().filter_map(nombre).collect();
        (n.len() == 3).then(|| Tsl::new(n[0], n[1], n[2]))
    }
}

fn octet(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

fn arrondi(v: f32, chiffres: i32) -> f32 {
    let k = 10f32.powi(chiffres);
    (v * k).round() / k
}

fn nombre(v: &str) -> Option<f32> {
    v.trim().replace(',', ".").parse().ok().filter(|x: &f32| x.is_finite())
}

/// Les résolutions proposées, en pixels de côté.
pub const RESOLUTIONS: [u32; 3] = [256, 512, 1024];

/// Tout ce qui décrit une texture. Les dimensions des éléments sont en centimètres,
/// les joints en millimètres, le côté de la texture en mètres.
#[derive(Clone, PartialEq, Debug)]
pub struct Reglages {
    pub genre: Genre,
    /// Le côté du carré couvert par la texture, en mètres.
    pub cote: f32,
    pub resolution: u32,
    /// Le tirage : changer la graine redistribue teintes, cernes et formes.
    pub graine: u32,
    /// Brique, lame : la longueur ; pierre : la taille moyenne.
    pub longueur: f32,
    /// Brique : la hauteur ; lame : la largeur ; pierre : la hauteur des assises.
    pub hauteur: f32,
    /// Le rang dans `Genre::appareils`.
    pub appareil: u32,
    /// Bords des éléments, forme des pierres : de 0 (net) à 1.
    pub irregularite: f32,
    /// La couleur des éléments, et celle vers laquelle ils varient (bois : les cernes).
    pub couleur: Tsl,
    pub couleur2: Tsl,
    /// De 0 (tous pareils) à 1 : l'écart de teinte entre éléments.
    pub variation: f32,
    /// De 0 à 1 : le bruit à la surface des éléments.
    pub grain: f32,
    /// L'épaisseur des joints, en millimètres, et leur couleur.
    pub joint: f32,
    pub couleur_joint: Tsl,
    /// De 0 à 1 : l'ombre des arêtes et des joints en creux.
    pub relief: f32,
    /// Bois : les cernes sur la largeur d'une lame, et les nœuds par mètre carré.
    pub cernes: f32,
    pub noeuds: f32,
}

impl Reglages {
    pub fn defaut(genre: Genre) -> Reglages {
        match genre {
            Genre::Bois => Reglages {
                genre,
                cote: 2.0,
                resolution: 512,
                graine: 1,
                longueur: 120.0,
                hauteur: 14.0,
                appareil: 0,
                irregularite: 0.1,
                couleur: Tsl::new(30.0, 0.45, 0.55),
                couleur2: Tsl::new(22.0, 0.50, 0.32),
                variation: 0.4,
                grain: 0.5,
                joint: 2.0,
                couleur_joint: Tsl::new(20.0, 0.30, 0.15),
                relief: 0.3,
                cernes: 10.0,
                noeuds: 1.5,
            },
            Genre::Brique => Reglages {
                genre,
                cote: 1.0,
                resolution: 512,
                graine: 1,
                longueur: 22.0,
                hauteur: 6.5,
                appareil: 0,
                irregularite: 0.3,
                couleur: Tsl::new(14.0, 0.55, 0.42),
                couleur2: Tsl::new(20.0, 0.45, 0.28),
                variation: 0.5,
                grain: 0.5,
                joint: 10.0,
                couleur_joint: Tsl::new(40.0, 0.10, 0.72),
                relief: 0.5,
                cernes: 10.0,
                noeuds: 0.0,
            },
            Genre::Pierre => Reglages {
                genre,
                cote: 2.0,
                resolution: 512,
                graine: 1,
                longueur: 35.0,
                hauteur: 25.0,
                appareil: 0,
                irregularite: 0.6,
                couleur: Tsl::new(42.0, 0.18, 0.66),
                couleur2: Tsl::new(35.0, 0.14, 0.48),
                variation: 0.6,
                grain: 0.7,
                joint: 15.0,
                couleur_joint: Tsl::new(40.0, 0.08, 0.76),
                relief: 0.6,
                cernes: 10.0,
                noeuds: 0.0,
            },
        }
    }

    /// Le texte du fichier de réglages, en paires `clé = valeur`.
    pub fn ecrire(&self) -> String {
        let mut s = String::from("# Matière (sage-matieres) : modifiable à la main\n");
        let mut l = |k: &str, v: String| s.push_str(&format!("{k} = {v}\n"));
        l("genre", self.genre.id().into());
        l("cote", arrondi(self.cote, 3).to_string());
        l("resolution", self.resolution.to_string());
        l("graine", self.graine.to_string());
        l("longueur", arrondi(self.longueur, 2).to_string());
        l("hauteur", arrondi(self.hauteur, 2).to_string());
        l("appareil", self.appareil.to_string());
        l("irregularite", arrondi(self.irregularite, 3).to_string());
        l("couleur", self.couleur.ecrire());
        l("couleur2", self.couleur2.ecrire());
        l("variation", arrondi(self.variation, 3).to_string());
        l("grain", arrondi(self.grain, 3).to_string());
        l("joint", arrondi(self.joint, 2).to_string());
        l("couleur_joint", self.couleur_joint.ecrire());
        l("relief", arrondi(self.relief, 3).to_string());
        if self.genre == Genre::Bois {
            l("cernes", arrondi(self.cernes, 2).to_string());
            l("noeuds", arrondi(self.noeuds, 2).to_string());
        }
        s
    }

    /// Relit un fichier de réglages ; ce qui manque ou ne se lit pas garde la valeur
    /// par défaut du genre. Sans genre lisible, rien.
    pub fn lire(texte: &str) -> Option<Reglages> {
        let paires = cles(texte);
        let genre = paires.iter().find(|(k, _)| k == "genre").and_then(|(_, v)| Genre::de(v))?;
        let mut r = Reglages::defaut(genre);
        for (k, v) in &paires {
            let n = nombre(v);
            match k.as_str() {
                "cote" => r.cote = n.unwrap_or(r.cote),
                "resolution" => r.resolution = n.map_or(r.resolution, |x| x as u32),
                "graine" => r.graine = n.map_or(r.graine, |x| x as u32),
                "longueur" => r.longueur = n.unwrap_or(r.longueur),
                "hauteur" => r.hauteur = n.unwrap_or(r.hauteur),
                "appareil" => r.appareil = n.map_or(r.appareil, |x| x as u32),
                "irregularite" => r.irregularite = n.unwrap_or(r.irregularite),
                "couleur" => r.couleur = Tsl::lire(v).unwrap_or(r.couleur),
                "couleur2" => r.couleur2 = Tsl::lire(v).unwrap_or(r.couleur2),
                "variation" => r.variation = n.unwrap_or(r.variation),
                "grain" => r.grain = n.unwrap_or(r.grain),
                "joint" => r.joint = n.unwrap_or(r.joint),
                "couleur_joint" => r.couleur_joint = Tsl::lire(v).unwrap_or(r.couleur_joint),
                "relief" => r.relief = n.unwrap_or(r.relief),
                "cernes" => r.cernes = n.unwrap_or(r.cernes),
                "noeuds" => r.noeuds = n.unwrap_or(r.noeuds),
                _ => {}
            }
        }
        r.borner();
        Some(r)
    }

    /// Ramène chaque valeur dans ce que le générateur sait faire.
    pub fn borner(&mut self) {
        self.cote = self.cote.clamp(0.25, 8.0);
        if !RESOLUTIONS.contains(&self.resolution) {
            self.resolution = 512;
        }
        self.longueur = self.longueur.clamp(2.0, 400.0);
        self.hauteur = self.hauteur.clamp(1.0, 100.0);
        self.appareil = self.appareil.min(self.genre.appareils().len() as u32 - 1);
        self.irregularite = self.irregularite.clamp(0.0, 1.0);
        self.variation = self.variation.clamp(0.0, 1.0);
        self.grain = self.grain.clamp(0.0, 1.0);
        self.joint = self.joint.clamp(0.0, 60.0);
        self.relief = self.relief.clamp(0.0, 1.0);
        self.cernes = self.cernes.clamp(1.0, 40.0);
        self.noeuds = self.noeuds.clamp(0.0, 20.0);
    }

    /// Le décalage d'un rang au suivant, en fraction d'élément : `None` pour un
    /// décalage tiré au hasard (le bois « aléatoire »).
    fn diviseur(&self) -> Option<i32> {
        match (self.genre, self.appareil) {
            (Genre::Bois, 0) => None,
            (Genre::Bois, 1) | (Genre::Brique, 0) => Some(2),
            (Genre::Bois, 2) | (Genre::Brique, 1) => Some(3),
            (Genre::Brique, 2) => Some(4),
            _ => Some(1),
        }
    }

    /// Les éléments sur la largeur et sur la hauteur de la texture : des nombres
    /// entiers, pour qu'elle se répète. Les rangs décalés vont par multiples du
    /// décalage, pour la même raison. Pour les moellons, la grille de leurs centres.
    pub fn grille(&self) -> (i32, i32) {
        let j = self.joint / 10.0;
        let c = self.cote * 100.0;
        let n = |module: f32| ((c / module.max(0.5)).round() as i32).max(1);
        if self.genre == Genre::Pierre && self.appareil == 0 {
            let k = n(self.longueur + j).max(2);
            return (k, k);
        }
        let colonnes = n(self.longueur + j);
        let mut rangs = n(self.hauteur + j);
        if let Some(d) = self.diviseur().filter(|d| *d > 1 && self.genre != Genre::Pierre) {
            rangs = (((rangs as f32) / d as f32).round() as i32).max(1) * d;
        }
        (colonnes, rangs)
    }

    /// Une ligne qui résume la texture : sa taille et ses éléments.
    pub fn resume(&self) -> String {
        let (c, r) = self.grille();
        let elements = match (self.genre, self.appareil) {
            (Genre::Bois, _) => format!("{c} × {r} lames"),
            (Genre::Brique, _) => format!("{c} × {r} briques"),
            (Genre::Pierre, 0) => format!("≈ {} pierres", c * r),
            (Genre::Pierre, _) => format!("{r} assises"),
        };
        format!("{} px · {} m · {elements}", self.resolution, nombre_court(self.cote))
    }
}

/// Un nombre sans zéros inutiles, avec une virgule.
pub fn nombre_court(v: f32) -> String {
    let s = format!("{:.2}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.replace('.', ",")
}

/// Une image RVBA, ligne par ligne depuis le haut.
#[derive(Clone, PartialEq, Debug)]
pub struct Image {
    pub l: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

// --- Hasard et bruits ---------------------------------------------------------

fn hache(a: u32, b: u32, c: u32) -> u32 {
    let mut h = a.wrapping_mul(0x8DA6_B343) ^ b.wrapping_mul(0xD816_3841) ^ c.wrapping_mul(0xCB1A_B31F) ^ 0x9E37_79B9;
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846C_A68B);
    h ^= h >> 16;
    h
}

/// Un nombre de [0, 1) tiré de trois entiers.
fn alea(a: u32, b: u32, c: u32) -> f32 {
    (hache(a, b, c) >> 8) as f32 / (1u32 << 24) as f32
}

fn lisse(x: f32) -> f32 {
    x * x * (3.0 - 2.0 * x)
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    if b <= a {
        return if x < a { 0.0 } else { 1.0 };
    }
    lisse(((x - a) / (b - a)).clamp(0.0, 1.0))
}

fn melange(a: [f32; 3], b: [f32; 3], k: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k]
}

fn fois(a: [f32; 3], k: f32) -> [f32; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

/// Bruit de valeur lissé, de [0, 1), périodique : (x, y) en mailles, période (px, py)
/// en mailles.
fn bruit(x: f32, y: f32, px: i32, py: i32, g: u32) -> f32 {
    let (xi, yi) = (x.floor(), y.floor());
    let (ux, uy) = (lisse(x - xi), lisse(y - yi));
    let (i, j) = (xi as i32, yi as i32);
    let v = |a: i32, b: i32| alea(a.rem_euclid(px.max(1)) as u32, b.rem_euclid(py.max(1)) as u32, g);
    let haut = v(i, j) + (v(i + 1, j) - v(i, j)) * ux;
    let bas = v(i, j + 1) + (v(i + 1, j + 1) - v(i, j + 1)) * ux;
    haut + (bas - haut) * uy
}

/// Somme d'octaves, de [-1, 1] environ : (s, t) en tuiles, (fx, fy) les mailles de la
/// première octave sur une tuile (des entiers : le bruit garde la période de la tuile).
fn fbm(s: f32, t: f32, fx: i32, fy: i32, octaves: u32, g: u32) -> f32 {
    let (mut somme, mut amp, mut total) = (0.0, 1.0, 0.0);
    for k in 0..octaves {
        let (ax, ay) = (fx.max(1) << k, fy.max(1) << k);
        somme += amp * (bruit(s * ax as f32, t * ay as f32, ax, ay, g.wrapping_add(k * 101)) * 2.0 - 1.0);
        total += amp;
        amp *= 0.5;
    }
    somme / total
}

// --- Génération ---------------------------------------------------------------

/// Ce qui sert à chaque pixel, calculé une fois.
struct Contexte<'a> {
    r: &'a Reglages,
    c1: [f32; 3],
    c2: [f32; 3],
    cj: [f32; 3],
    /// Le demi-joint et la largeur d'un pixel, en mètres.
    demi_joint: f32,
    pixel: f32,
    grille: (i32, i32),
    g: u32,
}

/// La part de mortier à la distance `d` (mètres) du bord du module d'un élément,
/// antialiasée sur un pixel.
fn mortier(c: &Contexte, d: f32) -> f32 {
    if c.demi_joint <= 0.0 {
        return 0.0;
    }
    1.0 - smoothstep(c.demi_joint - c.pixel * 0.5, c.demi_joint + c.pixel * 0.5, d)
}

/// La couleur du mortier, un peu granuleuse, plus sombre en creux.
fn couleur_mortier(c: &Contexte, s: f32, t: f32) -> [f32; 3] {
    let f = (c.r.cote * 40.0).round() as i32;
    let n = fbm(s, t, f, f, 3, c.g.wrapping_add(7));
    fois(c.cj, (1.0 + 0.18 * c.r.grain * n) * (1.0 - 0.3 * c.r.relief))
}

/// L'arête d'un élément rectangulaire : éclairée en haut et à gauche, dans l'ombre en
/// bas et à droite. `bords` : les distances (mètres) aux bords gauche, droit, haut, bas.
fn arete(c: &Contexte, couleur: [f32; 3], bords: [f32; 4]) -> [f32; 3] {
    let biseau = 0.003 + 0.01 * c.r.relief;
    let (mut d, mut signe) = (f32::MAX, 0.0);
    for (k, v) in bords.into_iter().enumerate() {
        if v < d {
            d = v;
            signe = if k == 0 || k == 2 { 1.0 } else { -1.0 };
        }
    }
    let k = 1.0 - smoothstep(c.demi_joint, c.demi_joint + biseau, d);
    fois(couleur, 1.0 + c.r.relief * k * (0.28 * signe - 0.1))
}

/// La teinte propre d'un élément, tirée de son identifiant.
fn teinte(c: &Contexte, a: u32, b: u32) -> [f32; 3] {
    let v = c.r.variation;
    let base = melange(c.c1, c.c2, alea(a, b, c.g) * v);
    fois(base, 1.0 + (alea(a, b, c.g.wrapping_add(1)) - 0.5) * 0.35 * v)
}

/// Le grain de surface : un bruit fin, et quelques points sombres (par pixel, ce qui
/// garde la période de la texture).
fn grain(c: &Contexte, couleur: [f32; 3], s: f32, t: f32, x: u32, y: u32, frequence: f32, fort: f32) -> [f32; 3] {
    let f = ((c.r.cote * frequence).round() as i32).max(1);
    let n = fbm(s, t, f, f, 4, c.g.wrapping_add(5));
    let mut v = fois(couleur, 1.0 + c.r.grain * fort * n);
    if alea(x, y, c.g.wrapping_add(9)) < 0.012 * c.r.grain {
        v = fois(v, 0.7);
    }
    v
}

/// Le léger tremblement des bords d'un élément, en mètres.
fn tremble(c: &Contexte, s: f32, t: f32, amplitude: f32) -> f32 {
    let f = ((c.r.cote * 25.0).round() as i32).max(1);
    c.r.irregularite * amplitude * fbm(s, t, f, f, 3, c.g.wrapping_add(3))
}

fn brique(c: &Contexte, s: f32, t: f32, x: u32, y: u32) -> [f32; 3] {
    let (colonnes, rangs) = c.grille;
    let d = c.r.diviseur().unwrap_or(1);
    let tf = t * rangs as f32;
    let rang = tf.floor() as i32;
    let decalage = rang.rem_euclid(d) as f32 / d as f32;
    let uf = s * colonnes as f32 - decalage;
    let col = uf.floor() as i32;
    let (mw, mh) = (c.r.cote / colonnes as f32, c.r.cote / rangs as f32);
    let (lx, ly) = ((uf - col as f32) * mw, (tf - rang as f32) * mh);
    let p = tremble(c, s, t, 0.006);
    let bords = [lx + p, mw - lx + p, ly + p, mh - ly + p];
    let dist = bords.iter().copied().fold(f32::MAX, f32::min);
    let (a, b) = (col.rem_euclid(colonnes) as u32, rang.rem_euclid(rangs) as u32);
    let v = grain(c, teinte(c, a, b), s, t, x, y, 60.0, 0.22);
    let v = arete(c, v, bords);
    melange(v, couleur_mortier(c, s, t), mortier(c, dist))
}

/// Un élément d'appareil en assises : sa position sur le rang, sa longueur (mètres)
/// et son rang dans l'assise.
fn assise(c: &Contexte, s: f32, rang: i32) -> (f32, f32, i32) {
    let r = c.r;
    let g = c.g.wrapping_add(20);
    let rg = rang as u32;
    let k = (((r.cote * 100.0 / (r.longueur + r.joint / 10.0)) * (0.75 + 0.5 * alea(rg, 0, g))).round() as i32).max(1);
    let u = (s - alea(rg, 1, g)).rem_euclid(1.0) * k as f32;
    let coupe = |i: i32| i as f32 + (alea(rg, 2 + i.rem_euclid(k) as u32, g) - 0.5) * r.irregularite * 0.8;
    let mut i = -1;
    while i < k && coupe(i + 1) <= u {
        i += 1;
    }
    let module = r.cote / k as f32;
    ((u - coupe(i)) * module, (coupe(i + 1) - coupe(i)) * module, i.rem_euclid(k))
}

/// La pierre : couleur propre, marbrures, grain, puis le bord assombri et le joint.
fn pierre_couleur(c: &Contexte, a: u32, b: u32, dist: f32, s: f32, t: f32, x: u32, y: u32) -> [f32; 3] {
    let f = ((c.r.cote * 5.0).round() as i32).max(1);
    let marbre = fbm(s, t, f, f, 3, c.g.wrapping_add(13).wrapping_add(a * 7 + b * 13));
    let v = fois(teinte(c, a, b), 1.0 + 0.12 * c.r.grain * marbre);
    let v = grain(c, v, s, t, x, y, 40.0, 0.3);
    let biseau = 0.01 + 0.025 * c.r.relief;
    let bord = 1.0 - smoothstep(c.demi_joint, c.demi_joint + biseau, dist);
    let bombe = smoothstep(c.demi_joint, c.demi_joint + 0.08, dist);
    let v = fois(v, 1.0 - c.r.relief * (0.32 * bord - 0.06 * bombe));
    melange(v, couleur_mortier(c, s, t), mortier(c, dist))
}

fn moellons(c: &Contexte, s: f32, t: f32, x: u32, y: u32) -> [f32; 3] {
    let n = c.grille.0;
    let nf = n as f32;
    let irr = c.r.irregularite;
    // Les bords ondulent : on déforme l'espace avant d'y chercher les pierres
    let fw = n.max(2);
    let w = irr * 0.3 / nf;
    let ws = s + w * fbm(s, t, fw, fw, 3, c.g.wrapping_add(11));
    let wt = t + w * fbm(s, t, fw, fw, 3, c.g.wrapping_add(12));
    let (gx, gy) = (ws * nf, wt * nf);
    let (ci, cj) = (gx.floor() as i32, gy.floor() as i32);
    let ecart = 0.3 + 0.6 * irr;
    let mut points = [([0.0f32; 2], 0.0f32, (0u32, 0u32)); 25];
    let mut k = 0;
    for di in -2..=2 {
        for dj in -2..=2 {
            let (i, j) = (ci + di, cj + dj);
            let h = (i.rem_euclid(n) as u32, j.rem_euclid(n) as u32);
            let p = [i as f32 + 0.5 + (alea(h.0, h.1, c.g.wrapping_add(30)) - 0.5) * ecart, j as f32 + 0.5 + (alea(h.0, h.1, c.g.wrapping_add(31)) - 0.5) * ecart];
            let d2 = (gx - p[0]).powi(2) + (gy - p[1]).powi(2);
            points[k] = (p, d2, h);
            k += 1;
        }
    }
    let proche = (0..25).min_by(|&a, &b| points[a].1.total_cmp(&points[b].1)).unwrap_or(0);
    let (p0, d0, h) = points[proche];
    // La distance à la frontière la plus proche : à chaque médiatrice
    let mut bord = f32::MAX;
    for (k, (p, d2, _)) in points.iter().enumerate() {
        if k != proche {
            let l = ((p[0] - p0[0]).powi(2) + (p[1] - p0[1]).powi(2)).sqrt().max(1e-6);
            bord = bord.min((d2 - d0) / (2.0 * l));
        }
    }
    let dist = bord * c.r.cote / nf;
    pierre_couleur(c, h.0, h.1, dist, s, t, x, y)
}

fn assises(c: &Contexte, s: f32, t: f32, x: u32, y: u32) -> [f32; 3] {
    let rangs = c.grille.1;
    let tf = t * rangs as f32;
    let rang = tf.floor() as i32;
    let mh = c.r.cote / rangs as f32;
    let ly = (tf - rang as f32) * mh;
    let (lx, longueur, i) = assise(c, s, rang.rem_euclid(rangs));
    let p = tremble(c, s, t, 0.012);
    let bords = [lx + p, longueur - lx + p, ly + p, mh - ly + p];
    let dist = bords.iter().copied().fold(f32::MAX, f32::min);
    pierre_couleur(c, i as u32, rang.rem_euclid(rangs) as u32 + 1000, dist, s, t, x, y)
}

fn bois(c: &Contexte, s: f32, t: f32, x: u32, y: u32) -> [f32; 3] {
    let r = c.r;
    let (k, rangs) = c.grille;
    let tf = t * rangs as f32;
    let rang = tf.floor() as i32;
    let rg = rang.rem_euclid(rangs) as u32;
    let decalage = match r.diviseur() {
        None => alea(rg, 0, c.g.wrapping_add(60)),
        Some(d) => rang.rem_euclid(d) as f32 / d as f32,
    };
    let uf = s * k as f32 - decalage;
    let col = uf.floor() as i32;
    let (lm, wm) = (r.cote / k as f32, r.cote / rangs as f32);
    let (lu, lv) = ((uf - col as f32) * lm, (tf - rang as f32) * wm);
    let (a, b) = (col.rem_euclid(k) as u32, rg);
    let h = |n: u32| alea(a, b, c.g.wrapping_add(40 + n));
    // Les cernes : la lame coupe des cylindres autour du cœur de l'arbre, un peu de
    // biais, qui ondulent le long du fil ; ils dessinent des arcs (les « cathédrales »)
    let cv = wm * (h(0) * 3.0 - 1.0);
    let dz = wm * (0.2 + h(1) * 1.2);
    let biais = (h(2) - 0.5) * 0.12;
    let onde = wm * 0.25 * (bruit(lu / lm * 3.0 + h(3) * 50.0, h(4) * 50.0, 1000, 1000, c.g.wrapping_add(41)) * 2.0 - 1.0);
    let mut rayon = ((lv - cv + onde).powi(2) + dz * dz).sqrt() + (lu - lm / 2.0) * biais;
    // Les nœuds : les cernes s'enroulent autour, le cœur est sombre
    let attendus = r.noeuds * lm * wm / 3.0;
    let mut coeur: f32 = 0.0;
    for q in 0..3 {
        if h(10 + q) < attendus {
            let (kx, ky) = (h(20 + q) * lm, h(30 + q) * wm);
            let rk = 0.005 + h(50 + q) * 0.01;
            let d = ((lu - kx).powi(2) + ((lv - ky) * 1.6).powi(2)).sqrt();
            rayon += rk * 2.5 * (-(d / (rk * 3.0)).powi(2)).exp();
            coeur = coeur.max(1.0 - smoothstep(rk * 0.5, rk, d));
        }
    }
    let pas = wm / r.cernes.max(0.5);
    let phase = (rayon / pas).rem_euclid(1.0);
    let fonce = phase.powi(3) * 0.85;
    let mut v = melange(c.c1, c.c2, fonce);
    // Chaque lame sa teinte
    v = melange(v, c.c2, h(5) * r.variation * 0.45);
    v = fois(v, 1.0 + (h(6) - 0.5) * 0.3 * r.variation);
    // Le fil : un bruit étiré le long des lames
    let fx = ((r.cote * 3.0).round() as i32).max(1);
    let fy = ((r.cote * 150.0).round() as i32).max(1);
    v = fois(v, 1.0 + r.grain * 0.2 * fbm(s, t, fx, fy, 3, c.g.wrapping_add(50)));
    v = melange(v, fois(c.c2, 0.55), coeur * 0.9);
    let p = tremble(c, s, t, 0.002);
    let bords = [lu + p, lm - lu + p, lv + p, wm - lv + p];
    let dist = bords.iter().copied().fold(f32::MAX, f32::min);
    let _ = (x, y);
    let v = arete(c, v, bords);
    melange(v, couleur_mortier(c, s, t), mortier(c, dist))
}

fn contexte(r: &Reglages) -> Contexte<'_> {
    Contexte {
        r,
        c1: r.couleur.rvb(),
        c2: r.couleur2.rvb(),
        cj: r.couleur_joint.rvb(),
        demi_joint: r.joint / 2000.0,
        pixel: r.cote / r.resolution.max(1) as f32,
        grille: r.grille(),
        g: r.graine.wrapping_mul(7919),
    }
}

/// La couleur au point (s, t) de la texture (en tuiles : de 0 à 1 sur la texture,
/// au-delà elle se répète) ; (x, y) : le pixel, pour le grain.
fn couleur(c: &Contexte, s: f32, t: f32, x: u32, y: u32) -> [f32; 3] {
    match (c.r.genre, c.r.appareil) {
        (Genre::Bois, _) => bois(c, s, t, x, y),
        (Genre::Brique, _) => brique(c, s, t, x, y),
        (Genre::Pierre, 0) => moellons(c, s, t, x, y),
        (Genre::Pierre, _) => assises(c, s, t, x, y),
    }
}

/// Génère la texture, sur tous les cœurs du processeur.
pub fn generer(r: &Reglages) -> Image {
    let n = r.resolution.max(1);
    let c = contexte(r);
    let mut rgba = vec![0u8; (n * n * 4) as usize];
    let fils = std::thread::available_parallelism().map_or(4, |v| v.get()).clamp(1, 8);
    let lignes = (n as usize).div_ceil(fils);
    std::thread::scope(|portee| {
        for (k, bande) in rgba.chunks_mut(lignes * n as usize * 4).enumerate() {
            let c = &c;
            portee.spawn(move || {
                for (i, px) in bande.chunks_exact_mut(4).enumerate() {
                    let i = i as u32;
                    let (x, y) = (i % n, (k * lignes) as u32 + i / n);
                    let (s, t) = ((x as f32 + 0.5) / n as f32, (y as f32 + 0.5) / n as f32);
                    let [rr, gg, bb] = couleur(c, s, t, x, y);
                    px.copy_from_slice(&[octet(rr), octet(gg), octet(bb), 255]);
                }
            });
        }
    });
    Image { l: n, h: n, rgba }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_reglages_se_relisent() {
        for g in Genre::TOUS {
            let mut r = Reglages::defaut(g);
            r.graine = 42;
            r.couleur = Tsl::new(200.0, 0.25, 0.5);
            assert_eq!(Reglages::lire(&r.ecrire()), Some(r));
        }
        assert_eq!(Reglages::lire("cote = 2"), None);
    }

    #[test]
    fn la_texture_se_repete_sans_couture() {
        for g in Genre::TOUS {
            for appareil in 0..g.appareils().len() as u32 {
                let mut r = Reglages::defaut(g);
                r.appareil = appareil;
                let c = contexte(&r);
                for (s, t) in [(0.013, 0.31), (0.5, 0.0021), (0.77, 0.93)] {
                    let a = couleur(&c, s, t, 0, 0);
                    for (ds, dt) in [(1.0, 0.0), (0.0, 1.0), (-1.0, 2.0)] {
                        let b = couleur(&c, s + ds, t + dt, 0, 0);
                        for i in 0..3 {
                            assert!((a[i] - b[i]).abs() < 2e-3, "{g:?} {appareil} ({s}, {t}) + ({ds}, {dt}) : {a:?} ≠ {b:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn les_rangs_decales_vont_par_multiples() {
        let mut r = Reglages::defaut(Genre::Brique);
        r.appareil = 1; // ⅓
        assert_eq!(r.grille().1 % 3, 0);
        r.appareil = 0; // ½
        assert_eq!(r.grille(), (4, 14));
    }

    #[test]
    fn le_joint_prend_sa_couleur() {
        let mut r = Reglages::defaut(Genre::Brique);
        r.couleur_joint = Tsl::new(0.0, 0.0, 1.0);
        r.relief = 0.0;
        r.grain = 0.0;
        let c = contexte(&r);
        // Le coin d'un module est dans le joint
        let v = couleur(&c, 0.0001, 0.0001, 1, 1);
        assert!(v.iter().all(|x| *x > 0.95), "{v:?}");
    }

    #[test]
    fn une_image_a_sa_taille() {
        let mut r = Reglages::defaut(Genre::Pierre);
        r.resolution = 256;
        let i = generer(&r);
        assert_eq!((i.l, i.h, i.rgba.len()), (256, 256, 256 * 256 * 4));
    }
}
