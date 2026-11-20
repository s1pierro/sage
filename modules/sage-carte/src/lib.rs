//! S.A.G.E. : les cartes matérialisées.
//!
//! Une carte est un lieu réel (`sage-reel`) dont Flow a fait tout le travail lourd
//! (nivellement des voies, remblais, défonces, bâtiments unis au terrain) : il ne reste
//! qu'à charger des triangles, pour le rendu comme pour la physique (les mêmes). Le
//! processeur reste à la simulation.
//!
//! Une carte est un **dossier** :
//!
//! - `carte.txt` : l'index ([`Carte`]), en lignes « clé = valeur » ;
//! - `tuiles/<niveau>-<i>-<j>.glb` : une tuile ([`Tuile`]) par fichier, en glTF binaire :
//!   un nœud placé au coin de la tuile (`translation`), un maillage, une primitive par
//!   matière (le nom du matériau glTF est celui de la matière), positions relatives au
//!   coin, normales, coordonnées de texture ; la matière `sol` porte l'image de la
//!   tuile (PNG) vue d'en haut, les autres des coordonnées en mètres (répétées tous
//!   les `cote` mètres de leur texture) ;
//!   la matière [`EAU`] est la surface de l'eau, à plat au niveau : elle se montre
//!   (semi-transparente, le lit au travers) mais n'a pas de volume ; elle ne compte
//!   pas pour la physique ;
//! - `matieres/` : les réglages et images des matières (bitume, béton clair…).
//!
//! Le repère est celui du lieu : x et z au sol, y vers le haut, en mètres.

pub mod glb;
pub mod json;

use std::path::Path;

/// La version du format.
pub const FORMAT: u32 = 1;
/// Le nom de l'index dans le dossier d'une carte.
pub const INDEX: &str = "carte.txt";
/// Le sous-dossier des tuiles.
pub const TUILES: &str = "tuiles";
/// Le sous-dossier des matières.
pub const MATIERES: &str = "matieres";
/// La matière de la surface de l'eau (à montrer, pas un solide).
pub const EAU: &str = "eau";

/// Une tuile dans l'index : sa clé (niveau, i, j), sa boîte au sol (x0, z0, x1, z1), ses
/// hauteurs basse et haute, son nombre de triangles.
#[derive(Clone, Debug, PartialEq)]
pub struct EntreeTuile {
    pub cle: (u8, i32, i32),
    pub boite: [f32; 4],
    pub hmin: f32,
    pub hmax: f32,
    pub triangles: u32,
}

impl EntreeTuile {
    /// Le fichier de la tuile, relatif au dossier de la carte.
    pub fn fichier(&self) -> String {
        nom_tuile(self.cle)
    }
}

/// Le fichier d'une tuile de clé `cle`, relatif au dossier de la carte.
pub fn nom_tuile(cle: (u8, i32, i32)) -> String {
    format!("{TUILES}/{}-{}-{}.glb", cle.0, cle.1, cle.2)
}

/// Une matière : son nom (celui des matériaux des tuiles), le nom de sa texture dans
/// `matieres/` (vide : pas de texture, ou l'image de la tuile pour `sol`) et le côté
/// (m) sur lequel elle se répète.
#[derive(Clone, Debug, PartialEq)]
pub struct Matiere {
    pub nom: String,
    pub texture: String,
    pub cote: f32,
}

/// L'index d'une carte.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Carte {
    pub nom: String,
    /// Le lieu d'où elle vient (nom du fichier .reel) et son centre géographique.
    pub lieu: String,
    pub lat: f64,
    pub lon: f64,
    /// Le disque matérialisé : centre (x, z) et rayon.
    pub centre: [f32; 2],
    pub rayon: f32,
    /// Le côté des tuiles (m).
    pub cote: f32,
    /// Où poser le joueur au départ.
    pub depart: [f32; 3],
    pub matieres: Vec<Matiere>,
    pub tuiles: Vec<EntreeTuile>,
}

impl Carte {
    /// L'index en texte.
    pub fn ecrire(&self) -> String {
        let mut s = String::new();
        s.push_str("# Carte matérialisée de S.A.G.E. (sage-carte), écrite par Flow\n");
        s.push_str(&format!("format = {FORMAT}\nnom = {}\nlieu = {}\ngeo = {} {}\n", self.nom, self.lieu, self.lat, self.lon));
        s.push_str(&format!("zone = {} {} {}\ncote = {}\ndepart = {} {} {}\n", self.centre[0], self.centre[1], self.rayon, self.cote, self.depart[0], self.depart[1], self.depart[2]));
        s.push_str("# matiere = nom texture côté (texture « - » : aucune)\n");
        for m in &self.matieres {
            s.push_str(&format!("matiere = {} {} {}\n", m.nom, if m.texture.is_empty() { "-" } else { &m.texture }, m.cote));
        }
        s.push_str("# tuile = niveau i j x0 z0 x1 z1 hmin hmax triangles\n");
        for t in &self.tuiles {
            s.push_str(&format!("tuile = {} {} {} {} {} {} {} {} {} {}\n", t.cle.0, t.cle.1, t.cle.2, t.boite[0], t.boite[1], t.boite[2], t.boite[3], t.hmin, t.hmax, t.triangles));
        }
        s
    }

    /// L'index lu d'un texte.
    pub fn lire(texte: &str) -> Result<Carte, String> {
        let mut c = Carte::default();
        let mut format = 0;
        for (n, ligne) in texte.lines().enumerate() {
            let ligne = ligne.trim();
            if ligne.is_empty() || ligne.starts_with('#') {
                continue;
            }
            let Some((k, v)) = ligne.split_once('=') else { return Err(format!("ligne {} : « clé = valeur » attendu", n + 1)) };
            let (k, v) = (k.trim(), v.trim());
            let mots: Vec<&str> = v.split_whitespace().collect();
            let f = |i: usize| -> Result<f32, String> { mots.get(i).and_then(|m| m.parse().ok()).ok_or_else(|| format!("ligne {} : nombre attendu", n + 1)) };
            match k {
                "format" => format = v.parse().map_err(|_| format!("ligne {} : format illisible", n + 1))?,
                "nom" => c.nom = v.to_string(),
                "lieu" => c.lieu = v.to_string(),
                "geo" => {
                    c.lat = mots.first().and_then(|m| m.parse().ok()).unwrap_or(0.0);
                    c.lon = mots.get(1).and_then(|m| m.parse().ok()).unwrap_or(0.0);
                }
                "zone" => {
                    c.centre = [f(0)?, f(1)?];
                    c.rayon = f(2)?;
                }
                "cote" => c.cote = f(0)?,
                "depart" => c.depart = [f(0)?, f(1)?, f(2)?],
                "matiere" => {
                    let texture = mots.get(1).copied().unwrap_or("-");
                    c.matieres.push(Matiere { nom: mots.first().copied().unwrap_or("").to_string(), texture: if texture == "-" { String::new() } else { texture.to_string() }, cote: f(2).unwrap_or(1.0) });
                }
                "tuile" => {
                    let e = |i: usize| -> Result<i32, String> { mots.get(i).and_then(|m| m.parse().ok()).ok_or_else(|| format!("ligne {} : entier attendu", n + 1)) };
                    c.tuiles.push(EntreeTuile { cle: (e(0)? as u8, e(1)?, e(2)?), boite: [f(3)?, f(4)?, f(5)?, f(6)?], hmin: f(7)?, hmax: f(8)?, triangles: e(9)? as u32 });
                }
                _ => {}
            }
        }
        if format == 0 || format > FORMAT {
            return Err(format!("format de carte {format} inconnu (au plus {FORMAT})"));
        }
        Ok(c)
    }

    /// L'index du dossier `dossier`.
    pub fn ouvrir(dossier: &Path) -> Result<Carte, String> {
        let t = std::fs::read_to_string(dossier.join(INDEX)).map_err(|e| format!("{} : {e}", dossier.join(INDEX).display()))?;
        Carte::lire(&t)
    }

    /// Les tuiles dont la boîte touche le disque (x, z, rayon), les plus proches d'abord.
    pub fn tuiles_autour(&self, x: f32, z: f32, rayon: f32) -> Vec<&EntreeTuile> {
        let d = |t: &EntreeTuile| {
            let dx = (t.boite[0] - x).max(0.0).max(x - t.boite[2]);
            let dz = (t.boite[1] - z).max(0.0).max(z - t.boite[3]);
            dx.hypot(dz)
        };
        let mut v: Vec<&EntreeTuile> = self.tuiles.iter().filter(|t| d(t) <= rayon).collect();
        v.sort_by(|a, b| d(a).total_cmp(&d(b)));
        v
    }
}

/// Les triangles d'une matière dans une tuile.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Primitive {
    pub matiere: String,
    /// Positions relatives au coin de la tuile (`Tuile::origine`).
    pub positions: Vec<[f32; 3]>,
    pub normales: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

/// Une tuile : son coin (x0, 0, z0), ses primitives, et l'image (PNG) de la matière
/// `sol`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Tuile {
    pub cle: (u8, i32, i32),
    pub origine: [f32; 3],
    pub primitives: Vec<Primitive>,
    pub image_sol: Option<Vec<u8>>,
}

impl Tuile {
    /// Tous les triangles en coordonnées du monde (pour la physique).
    pub fn triangles_monde(&self) -> Vec<[[f32; 3]; 3]> {
        let o = self.origine;
        let mut v = Vec::new();
        for p in &self.primitives {
            for t in p.indices.chunks_exact(3) {
                v.push([0, 1, 2].map(|k| {
                    let q = p.positions[t[k] as usize];
                    [q[0] + o[0], q[1] + o[1], q[2] + o[2]]
                }));
            }
        }
        v
    }

    /// Le nombre de triangles.
    pub fn nombre_triangles(&self) -> usize {
        self.primitives.iter().map(|p| p.indices.len() / 3).sum()
    }

    /// La tuile lue de son fichier.
    pub fn ouvrir(chemin: &Path) -> Result<Tuile, String> {
        let o = std::fs::read(chemin).map_err(|e| format!("{} : {e}", chemin.display()))?;
        glb::lire(&o)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tuile() -> Tuile {
        let p = Primitive { matiere: "chaussee".into(), positions: vec![[0.0, 1.0, 0.0], [2.0, 1.0, 0.0], [0.0, 1.0, 2.0]], normales: vec![[0.0, 1.0, 0.0]; 3], uv: vec![[0.0, 0.0], [2.0, 0.0], [0.0, 2.0]], indices: vec![0, 2, 1] };
        let s = Primitive { matiere: "sol".into(), positions: vec![[5.0, 2.0, 5.0], [6.0, 2.0, 5.0], [5.0, 2.0, 6.0]], normales: vec![[0.0, 1.0, 0.0]; 3], uv: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]], indices: vec![0, 2, 1] };
        Tuile { cle: (7, 3, -4), origine: [100.0, 0.0, -50.0], primitives: vec![p, s], image_sol: Some(vec![0x89, b'P', b'N', b'G', 1, 2, 3]) }
    }

    #[test]
    fn une_tuile_se_relit_pareille() {
        let t = tuile();
        let o = glb::ecrire(&t);
        assert_eq!(&o[0..4], b"glTF");
        let r = glb::lire(&o).unwrap();
        assert_eq!(r, t);
        assert_eq!(r.triangles_monde()[0][1], [100.0, 1.0, -48.0]);
    }

    #[test]
    fn l_index_se_relit_pareil() {
        let c = Carte {
            nom: "essai".into(),
            lieu: "rouen10".into(),
            lat: 49.4435,
            lon: 1.0984,
            centre: [-387.5, 438.75],
            rayon: 500.0,
            cote: 78.125,
            depart: [-387.5, 12.0, 438.75],
            matieres: vec![Matiere { nom: "sol".into(), texture: String::new(), cote: 78.125 }, Matiere { nom: "chaussee".into(), texture: "bitume".into(), cote: 4.0 }],
            tuiles: vec![EntreeTuile { cle: (7, 3, -4), boite: [100.0, -50.0, 178.125, 28.125], hmin: -2.5, hmax: 31.0, triangles: 1234 }],
        };
        assert_eq!(Carte::lire(&c.ecrire()).unwrap(), c);
        assert_eq!(c.tuiles_autour(150.0, 0.0, 1.0).len(), 1);
        assert!(c.tuiles_autour(1000.0, 0.0, 10.0).is_empty());
    }
}
