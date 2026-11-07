//! Le catalogue des modèles importés : des `.glb` sans peau (décors, bâtiments) venus de
//! Blender, de Rustcraft ou d'ailleurs.
//!
//! - Chaque modèle est rangé sous un identifiant : celui que porte le fichier
//!   (`extras.sage.id`, un export de Rustcraft), sinon `glb/<n>`, donné à l'import.
//! - Le fichier est copié tel quel dans `catalogue/<fichier>.glb`, l'index tient dans
//!   `catalogue/catalogue.json`.
//! - Les tuiles ne gardent que la référence des objets à leur modèle.
//!
//! glTF a les conventions de S.A.G.E. : des mètres, +y vers le haut, l'avant vers +z. À
//! l'import, le modèle est posé : son point le plus bas passe à y = 0. Si son origine est
//! hors de son emprise au sol, il est aussi centré en x et z.

use crate::image::{decoder, Image};
use crate::moteur::Alpha;
use gda::json::{self, J};
use gda::maillage::Maillage;
use std::path::PathBuf;
use std::sync::Arc;

/// Au-delà, un modèle coûte cher à dessiner et à heurter : l'éditeur le signale.
pub const TRIANGLES_LOURD: usize = 50_000;

/// Un matériau d'un modèle : sa texture décodée, son opacité, son mode d'alpha.
#[derive(Clone, Debug)]
pub struct MateriauDecor {
    pub image: Option<Arc<Image>>,
    pub opacite: f32,
    pub alpha: Alpha,
    /// Ses faces se voient des deux côtés (`doubleSided`).
    pub deux_cotes: bool,
    /// Sa couleur de base (sRVB), que portent aussi les sommets.
    pub couleur: [f32; 3],
    /// Le fichier de sa texture tel qu'il était (PNG ou JPEG), pour le ranger ailleurs.
    pub source: Option<Arc<Vec<u8>>>,
}

/// Un modèle décodé, prêt pour le moteur.
#[derive(Clone, Debug)]
pub struct Decor {
    /// L'identifiant écrit dans le fichier, s'il y en a un.
    pub id: Option<String>,
    /// Le nom de la scène ou du premier objet, s'il y en a un.
    pub nom: Option<String>,
    /// Au format `Format::Texture`.
    pub sommets: Vec<f32>,
    /// (premier sommet, nombre, matériau), un morceau par matériau.
    pub parties: Vec<(u32, u32, usize)>,
    pub materiaux: Vec<MateriauDecor>,
    pub triangles: Vec<[[f32; 3]; 3]>,
    pub boite: ([f32; 3], [f32; 3]),
    /// Les textures qui manquaient (hors du fichier, ou d'un format inconnu).
    pub manquantes: usize,
    /// Ses faces ont été retournées : elles regardaient vers l'intérieur.
    pub retourne: bool,
}

const EXTENSIONS_REFUSEES: [(&str, &str); 4] = [
    ("KHR_draco_mesh_compression", "compressé en Draco"),
    ("EXT_meshopt_compression", "compressé en meshopt"),
    ("KHR_texture_basisu", "aux textures KTX2"),
    ("EXT_texture_webp", "aux textures WebP"),
];

fn vect(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn moins(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn unitaire(a: [f64; 3]) -> [f64; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    if l < 1e-12 { [0.0, 1.0, 0.0] } else { [a[0] / l, a[1] / l, a[2] / l] }
}

/// Ce qu'un `.glb` dit de lui : est-ce un personnage (il a une peau), et son extras.
pub fn est_personnage(octets: &[u8]) -> bool {
    gda::sage::lire_glb(octets).is_some_and(|(j, _)| j.get("skins").and_then(J::liste).is_some_and(|l| !l.is_empty()))
}

/// Décode un `.glb` de décor. `retourner` : faut-il retourner ses faces ? Rien : elles le
/// sont si le modèle a l'air à l'envers (ses faces regardent vers l'intérieur).
pub fn decoder_glb(octets: &[u8], retourner: Option<bool>) -> Result<Decor, String> {
    let (j, _) = gda::sage::lire_glb(octets).ok_or("ce n'est pas un fichier .glb")?;
    for e in j.get("extensionsRequired").and_then(J::liste).unwrap_or(&[]) {
        if let Some((_, quoi)) = EXTENSIONS_REFUSEES.iter().find(|(x, _)| Some(*x) == e.txt()) {
            return Err(format!("modèle {quoi} : S.A.G.E. ne sait pas encore le lire"));
        }
    }
    let noeuds = j.get("nodes").and_then(J::liste).unwrap_or(&[]);
    let sage = noeuds.iter().find_map(|n| n.get("extras").and_then(|e| e.get("sage")));
    let id = sage.and_then(|s| s.get("id")).and_then(J::txt).map(String::from);
    let nom = j
        .get("scenes")
        .and_then(J::liste)
        .and_then(|l| l.first())
        .and_then(|s| s.get("name"))
        .and_then(J::txt)
        .filter(|s| !s.is_empty() && !matches!(*s, "Scene" | "AuxScene"))
        .or_else(|| noeuds.iter().find(|n| n.get("mesh").is_some()).and_then(|n| n.get("name")).and_then(J::txt))
        .map(String::from);

    let mut m = Maillage::depuis_gltf(octets)?;
    if m.faces.is_empty() {
        return Err("ce .glb n'a pas de maillage".into());
    }
    let retourne = retourner.unwrap_or_else(|| Maillage::a_l_envers(&[&m]));
    if retourne {
        m.retourner();
    }
    let mats = Maillage::materiaux_gltf(octets).unwrap_or_default();

    // Poser le modèle : le plus bas à 0, centré s'il le faut
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for f in &m.faces {
        for &k in &f.s {
            let p = m.sommets[k];
            for c in 0..3 {
                lo[c] = lo[c].min(p[c]);
                hi[c] = hi[c].max(p[c]);
            }
        }
    }
    let hors = lo[0] > 0.0 || hi[0] < 0.0 || lo[2] > 0.0 || hi[2] < 0.0;
    let decal = [if hors { (lo[0] + hi[0]) / 2.0 } else { 0.0 }, lo[1], if hors { (lo[2] + hi[2]) / 2.0 } else { 0.0 }];
    let pos = |k: usize| moins(m.sommets[k], decal);

    // Des normales lissées par sommet, sauf sur les arêtes vives (plus de 40°)
    let mut lisse = vec![[0.0f64; 3]; m.sommets.len()];
    let normale_face = |s: &[usize]| {
        let mut n = [0.0; 3];
        for k in 1..s.len().saturating_sub(1) {
            let v = vect(moins(pos(s[k]), pos(s[0])), moins(pos(s[k + 1]), pos(s[0])));
            n = [n[0] + v[0], n[1] + v[1], n[2] + v[2]];
        }
        n
    };
    for f in &m.faces {
        let n = normale_face(&f.s);
        for &k in &f.s {
            lisse[k] = [lisse[k][0] + n[0], lisse[k][1] + n[1], lisse[k][2] + n[2]];
        }
    }
    let seuil = 40f64.to_radians().cos();

    // Un seau par matériau, dans l'ordre des matériaux du fichier ; les inconnus en dernier
    let mut noms: Vec<String> = mats.iter().map(|x| x.nom.clone()).collect();
    let mut seaux: Vec<Vec<f32>> = vec![Vec::new(); noms.len()];
    let mut triangles = Vec::new();
    for f in &m.faces {
        if f.s.len() < 3 {
            continue;
        }
        let k = match noms.iter().position(|x| **x == *f.m) {
            Some(k) => k,
            None => {
                noms.push(f.m.to_string());
                seaux.push(Vec::new());
                noms.len() - 1
            }
        };
        let couleur = mats.get(k).map_or([0.8f32; 3], |x| [0, 1, 2].map(|c| srvb(x.couleur[c])));
        let nf = unitaire(normale_face(&f.s));
        let coin = |i: usize, v: &mut Vec<f32>| {
            let p = pos(f.s[i]);
            let nl = unitaire(lisse[f.s[i]]);
            let n = if nl[0] * nf[0] + nl[1] * nf[1] + nl[2] * nf[2] > seuil { nl } else { nf };
            v.extend([p[0] as f32, p[1] as f32, p[2] as f32, n[0] as f32, n[1] as f32, n[2] as f32]);
            v.extend(couleur);
            v.extend(f.uv.get(i).copied().unwrap_or([0.0, 0.0]));
        };
        for q in 1..f.s.len() - 1 {
            for i in [0, q, q + 1] {
                coin(i, &mut seaux[k]);
            }
            let t = [0, q, q + 1].map(|i| pos(f.s[i]).map(|c| c as f32));
            triangles.push(t);
        }
    }
    let mut materiaux: Vec<MateriauDecor> = Vec::new();
    let mut manquantes = 0;
    for (k, _) in noms.iter().enumerate() {
        let x = mats.get(k);
        let image = x.and_then(|x| x.image.as_ref()).and_then(|(_, o)| decoder(o).ok()).map(Arc::new);
        if x.is_some_and(|x| x.manquante || (x.image.is_some() && image.is_none())) {
            manquantes += 1;
        }
        let opacite = x.map_or(1.0, |x| x.couleur[3].clamp(0.0, 1.0) as f32);
        let alpha = if opacite < 1.0 {
            Alpha::Melange
        } else if image.as_ref().is_some_and(|i| i.transparente()) {
            Alpha::Masque
        } else {
            Alpha::Opaque
        };
        let couleur = x.map_or([0.8f32; 3], |x| [0, 1, 2].map(|c| srvb(x.couleur[c])));
        let source = x.and_then(|x| x.image.as_ref()).filter(|_| image.is_some()).map(|(_, o)| Arc::new(o.clone()));
        materiaux.push(MateriauDecor { image, opacite, alpha, deux_cotes: x.is_some_and(|x| x.deux_cotes), couleur, source });
    }
    // Les opaques d'abord, puis les masqués, puis les mêlés
    let mut ordre: Vec<usize> = (0..seaux.len()).filter(|&k| !seaux[k].is_empty()).collect();
    ordre.sort_by_key(|&k| materiaux[k].alpha);
    let mut sommets = Vec::new();
    let mut parties = Vec::new();
    for k in ordre {
        let premier = (sommets.len() / 11) as u32;
        sommets.extend_from_slice(&seaux[k]);
        parties.push((premier, (seaux[k].len() / 11) as u32, k));
    }
    let b = |v: [f64; 3]| v.map(|c| c as f32);
    let boite = (b(moins(lo, decal)), b(moins(hi, decal)));
    Ok(Decor { id, nom, sommets, parties, materiaux, triangles, boite, manquantes, retourne })
}

fn srvb(v: f64) -> f32 {
    let v = v.clamp(0.0, 1.0);
    (if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }) as f32
}

/// L'empreinte d'un fichier (FNV-1a sur 64 bits) : le même fichier n'est pas importé deux
/// fois.
pub fn empreinte(o: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in o {
        h = (h ^ *b as u64).wrapping_mul(0x0000_0100_0000_01B3);
    }
    format!("{h:016x}")
}

/// Une entrée de l'index.
#[derive(Clone, Debug, PartialEq)]
pub struct Entree {
    pub id: String,
    pub nom: String,
    pub fichier: String,
    pub empreinte: String,
    pub triangles: usize,
    /// Ses faces sont retournées ; rien : à décider à la lecture.
    pub retourne: Option<bool>,
}

pub struct Catalogue {
    dossier: Option<PathBuf>,
    pub entrees: Vec<Entree>,
    suivant: u64,
    pub erreur: Option<String>,
    /// Sans dossier, les fichiers restent ici.
    memoire: std::collections::HashMap<String, Vec<u8>>,
}

fn nom_de_fichier(id: &str) -> String {
    let s: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    format!("{s}.glb")
}

impl Catalogue {
    /// Le catalogue rangé dans `dossier` (rien : en mémoire seulement), et les modèles
    /// qu'il contient, relus.
    pub fn ouvrir(dossier: Option<PathBuf>) -> (Catalogue, Vec<(Entree, Result<Decor, String>)>) {
        let mut c = Catalogue { dossier, entrees: Vec::new(), suivant: 0, erreur: None, memoire: Default::default() };
        let Some(d) = c.dossier.clone() else { return (c, Vec::new()) };
        let Some(j) = std::fs::read_to_string(d.join("catalogue.json")).ok().and_then(|s| json::lire(&s)) else { return (c, Vec::new()) };
        c.suivant = j.get("suivant").and_then(J::num).unwrap_or(0.0) as u64;
        let mut relus = Vec::new();
        for m in j.get("modeles").and_then(J::liste).unwrap_or(&[]) {
            let (Some(id), Some(fichier)) = (m.get("id").and_then(J::txt), m.get("fichier").and_then(J::txt)) else { continue };
            let e = Entree {
                id: id.into(),
                nom: m.get("nom").and_then(J::txt).unwrap_or(id).into(),
                fichier: fichier.into(),
                empreinte: m.get("empreinte").and_then(J::txt).unwrap_or("").into(),
                triangles: m.get("triangles").and_then(J::num).unwrap_or(0.0) as usize,
                retourne: m.get("faces").and_then(J::txt).map(|f| f == "retournees"),
            };
            let r = std::fs::read(d.join(&e.fichier)).map_err(|x| format!("{} : {x}", e.fichier)).and_then(|o| decoder_glb(&o, e.retourne));
            c.entrees.push(e.clone());
            relus.push((e, r));
        }
        (c, relus)
    }

    /// Importe `octets`, venu du fichier `nom`. `remplace` : l'identifiant d'un modèle dont
    /// il prend la place. Renvoie l'entrée, le modèle décodé et s'il était déjà là.
    pub fn importer(&mut self, nom: &str, octets: &[u8], remplace: Option<&str>) -> Result<(Entree, Decor, bool), String> {
        let d = decoder_glb(octets, None)?;
        let empreinte = empreinte(octets);
        if remplace.is_none() {
            if let Some(e) = self.entrees.iter().find(|e| e.empreinte == empreinte) {
                return Ok((e.clone(), d, true));
            }
        }
        let id = match (remplace, &d.id) {
            (Some(r), _) => r.to_string(),
            (None, Some(i)) => i.clone(),
            (None, None) => {
                let i = format!("glb/{}", self.suivant);
                self.suivant += 1;
                i
            }
        };
        let affiche = d.nom.clone().unwrap_or_else(|| nom.trim_end_matches(".glb").trim_end_matches(".GLB").to_string());
        let deja = self.entrees.iter().any(|e| e.id == id);
        let e = Entree { id: id.clone(), nom: affiche, fichier: nom_de_fichier(&id), empreinte, triangles: d.triangles.len(), retourne: Some(d.retourne) };
        if let Some(dos) = &self.dossier {
            std::fs::create_dir_all(dos).map_err(|x| x.to_string())?;
            let tmp = dos.join("import.tmp");
            std::fs::write(&tmp, octets).and_then(|_| std::fs::rename(&tmp, dos.join(&e.fichier))).map_err(|x| format!("catalogue : {x}"))?;
        } else {
            self.memoire.insert(id.clone(), octets.to_vec());
        }
        self.entrees.retain(|x| x.id != id);
        self.entrees.push(e.clone());
        self.ecrire_index();
        Ok((e, d, deja))
    }

    /// Retourne les faces du modèle `id` : l'endroit devient l'envers. Le choix est gardé.
    pub fn retourner(&mut self, id: &str, octets: &[u8]) -> Result<(Entree, Decor), String> {
        let i = self.entrees.iter().position(|e| e.id == id).ok_or("modèle absent du catalogue")?;
        let avant = match self.entrees[i].retourne {
            Some(r) => r,
            None => decoder_glb(octets, None)?.retourne,
        };
        let d = decoder_glb(octets, Some(!avant))?;
        self.entrees[i].retourne = Some(d.retourne);
        self.ecrire_index();
        Ok((self.entrees[i].clone(), d))
    }

    /// Les octets du fichier de `id`, tel qu'il est arrivé.
    pub fn octets(&self, id: &str) -> Option<Vec<u8>> {
        let e = self.entrees.iter().find(|e| e.id == id)?;
        match &self.dossier {
            Some(d) => std::fs::read(d.join(&e.fichier)).ok(),
            None => self.memoire.get(id).cloned(),
        }
    }

    /// Retire le modèle `id` du catalogue ; ses objets restent sur la map.
    pub fn retirer(&mut self, id: &str) {
        if let Some(e) = self.entrees.iter().find(|e| e.id == id).cloned() {
            if let Some(d) = &self.dossier {
                let _ = std::fs::remove_file(d.join(&e.fichier));
            }
            self.entrees.retain(|x| x.id != id);
            self.ecrire_index();
        }
    }

    fn ecrire_index(&mut self) {
        let Some(d) = &self.dossier else { return };
        let modeles = self
            .entrees
            .iter()
            .map(|e| {
                J::Objet(vec![
                    ("id".into(), J::Txt(e.id.clone())),
                    ("nom".into(), J::Txt(e.nom.clone())),
                    ("fichier".into(), J::Txt(e.fichier.clone())),
                    ("empreinte".into(), J::Txt(e.empreinte.clone())),
                    ("triangles".into(), J::Num(e.triangles as f64)),
                    ("faces".into(), J::Txt(if e.retourne == Some(true) { "retournees" } else { "endroit" }.into())),
                ])
            })
            .collect();
        let j = J::Objet(vec![("format".into(), J::Num(1.0)), ("suivant".into(), J::Num(self.suivant as f64)), ("modeles".into(), J::Liste(modeles))]);
        let mut s = String::new();
        json::indente(&j, 0, &mut s);
        let r = std::fs::write(d.join("catalogue.tmp"), s).and_then(|_| std::fs::rename(d.join("catalogue.tmp"), d.join("catalogue.json")));
        self.erreur = r.err().map(|x| format!("catalogue.json : {x}"));
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Un cube de 2 m, son origine au centre, d'un matériau rouge nommé, et un second
    /// nœud décalé : un petit .glb comme en exporte Blender.
    pub fn cube_glb(extras_id: Option<&str>) -> Vec<u8> {
        cube_glb_faces(extras_id, false, false)
    }

    /// Le même cube, les faces tournées vers l'intérieur (`envers`), son matériau à deux
    /// côtés (`deux_cotes`).
    pub fn cube_glb_faces(extras_id: Option<&str>, envers: bool, deux_cotes: bool) -> Vec<u8> {
        let mut pos: Vec<f32> = Vec::new();
        let mut idx: Vec<u16> = Vec::new();
        let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
            ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
            ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
            ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
            ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ];
        for (n, u, w) in faces {
            let b = (pos.len() / 3) as u16;
            for (a, c) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                for k in 0..3 {
                    pos.push(n[k] + a * u[k] + c * w[k]);
                }
            }
            if envers {
                idx.extend([b, b + 2, b + 1, b, b + 3, b + 2]);
            } else {
                idx.extend([b, b + 1, b + 2, b, b + 2, b + 3]);
            }
        }
        let mut bin: Vec<u8> = pos.iter().flat_map(|v| v.to_le_bytes()).collect();
        let lp = bin.len();
        bin.extend(idx.iter().flat_map(|v| v.to_le_bytes()));
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        let deux = if deux_cotes { ",\"doubleSided\":true" } else { "" };
        let extras = extras_id.map_or(String::new(), |i| format!(",\"extras\":{{\"sage\":{{\"format\":4,\"sorte\":\"decor\",\"id\":\"{i}\"}}}}"));
        let js = format!(
            "{{\"asset\":{{\"version\":\"2.0\"}},\"scene\":0,\"scenes\":[{{\"name\":\"Maison\",\"nodes\":[0]}}],\
             \"nodes\":[{{\"name\":\"Cube\",\"mesh\":0,\"translation\":[0,3,0]{extras}}}],\
             \"meshes\":[{{\"primitives\":[{{\"attributes\":{{\"POSITION\":0}},\"indices\":1,\"material\":0}}]}}],\
             \"materials\":[{{\"name\":\"Brique\",\"pbrMetallicRoughness\":{{\"baseColorFactor\":[0.5,0.1,0.1,1]}}{deux}}}],\
             \"buffers\":[{{\"byteLength\":{}}}],\
             \"bufferViews\":[{{\"buffer\":0,\"byteOffset\":0,\"byteLength\":{lp}}},{{\"buffer\":0,\"byteOffset\":{lp},\"byteLength\":{}}}],\
             \"accessors\":[{{\"bufferView\":0,\"componentType\":5126,\"count\":{},\"type\":\"VEC3\",\"min\":[-1,-1,-1],\"max\":[1,1,1]}},\
             {{\"bufferView\":1,\"componentType\":5123,\"count\":{},\"type\":\"SCALAR\"}}]}}",
            bin.len(),
            idx.len() * 2,
            pos.len() / 3,
            idx.len()
        );
        let mut js = js.into_bytes();
        while js.len() % 4 != 0 {
            js.push(b' ');
        }
        let total = 12 + 8 + js.len() + 8 + bin.len();
        let mut o = Vec::new();
        o.extend(b"glTF");
        o.extend(2u32.to_le_bytes());
        o.extend((total as u32).to_le_bytes());
        o.extend((js.len() as u32).to_le_bytes());
        o.extend(b"JSON");
        o.extend(&js);
        o.extend((bin.len() as u32).to_le_bytes());
        o.extend(b"BIN\0");
        o.extend(&bin);
        o
    }

    #[test]
    fn un_cube_de_blender_se_pose_au_sol() {
        let o = cube_glb(None);
        assert!(!est_personnage(&o));
        let d = decoder_glb(&o, None).unwrap();
        assert_eq!(d.triangles.len(), 12);
        assert_eq!(d.nom.as_deref(), Some("Maison"));
        // Posé : de 0 à 2 en y, l'origine au centre en x et z
        assert!((d.boite.0[1]).abs() < 1e-5 && (d.boite.1[1] - 2.0).abs() < 1e-5, "{:?}", d.boite);
        assert!((d.boite.0[0] + 1.0).abs() < 1e-5);
        assert_eq!(d.parties.len(), 1);
        assert_eq!(d.sommets.len(), 12 * 3 * 11);
        // La couleur du matériau, encodée en sRVB comme le reste
        assert!((d.sommets[6] - srvb(0.5)).abs() < 1e-5);
        // Les faces d'un cube sont vives : chaque coin garde la normale de sa face
        let n = [d.sommets[3], d.sommets[4], d.sommets[5]];
        assert!(n.iter().filter(|v| v.abs() > 0.99).count() == 1, "{n:?}");
    }

    #[test]
    fn importer_range_le_fichier_et_ne_double_pas() {
        let dos = std::env::temp_dir().join(format!("sage-catalogue-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dos);
        let (mut c, relus) = Catalogue::ouvrir(Some(dos.clone()));
        assert!(relus.is_empty());
        let (e, _, deja) = c.importer("maison.glb", &cube_glb(None), None).unwrap();
        assert_eq!((e.id.as_str(), deja), ("glb/0", false));
        let (e2, _, deja) = c.importer("copie.glb", &cube_glb(None), None).unwrap();
        assert_eq!((e2.id.as_str(), deja), ("glb/0", true), "le même fichier");
        // Un export de Rustcraft garde son identifiant
        let (e3, _, _) = c.importer("r.glb", &cube_glb(Some("abc123")), None).unwrap();
        assert_eq!(e3.id, "abc123");
        // Relu au lancement suivant
        let (c2, relus) = Catalogue::ouvrir(Some(dos.clone()));
        assert_eq!(c2.entrees.len(), 2);
        assert!(relus.iter().all(|(_, r)| r.is_ok()));
        let (mut c2, _) = (c2, ());
        c2.retirer("glb/0");
        assert!(!dos.join("glb_0.glb").exists());
        assert_eq!(Catalogue::ouvrir(Some(dos.clone())).0.entrees.len(), 1);
        assert!(decoder_glb(b"pas un glb", None).is_err());
        let _ = std::fs::remove_dir_all(&dos);
    }
}
