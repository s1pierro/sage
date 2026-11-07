//! Les bâtiments enrichis : un bâtiment exporté en `.glb` (pour Rustcraft ou Blender), et
//! un `.glb` qui devient sa **définition** dans sa région (voir `docs/format-stuff.md`).
//!
//! La définition a deux niveaux : `n0`, procédural (les paramètres du bâtiment, pour le
//! lointain et en secours), et `n1`, les parties du `.glb`, rangées à la manière du glTF
//! (positions, normales, UV, triangles), avec leurs matériaux et leurs textures (PNG ou
//! JPEG, telles quelles). Sa collision reprend `n1`.

use crate::catalogue::{Decor, MateriauDecor};
use crate::moteur::stuff::Stuff;
use crate::moteur::Alpha;
use crate::regions::Bati;
use gda::json::J;
use std::sync::Arc;

/// La distance jusqu'à laquelle se voient le niveau procédural et le niveau détaillé.
pub const LOIN: f64 = 3000.0;
pub const PRES: f64 = 400.0;

fn objet(o: Vec<(&str, J)>) -> J {
    J::Objet(o.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn txt(s: &str) -> J {
    J::Txt(s.into())
}

/// sRVB vers linéaire, pour `baseColorFactor`.
fn lineaire(v: f32) -> f64 {
    let v = v.clamp(0.0, 1.0) as f64;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

/// Un morceau à écrire en glTF : ses sommets (positions, normales, UV) et son matériau.
struct Groupe {
    pos: Vec<f32>,
    nrm: Vec<f32>,
    uv: Option<Vec<f32>>,
    mat: usize,
}

/// Un matériau à écrire : couleur sRVB, opacité, deux côtés, image (PNG ou JPEG).
struct MatGlb {
    nom: String,
    couleur: [f32; 3],
    opacite: f32,
    masque: bool,
    deux_cotes: bool,
    image: Option<Arc<Vec<u8>>>,
}

/// Le bâtiment en `.glb`, dans le repère de son instance : l'origine au centre de son
/// contour et au pied des murs, sans rotation. Enrichi, c'est son modèle détaillé, avec
/// ses textures ; sinon sa forme procédurale, en deux matériaux `murs` et `couverture`.
/// Le nœud porte l'identifiant du bâtiment (`extras.sage.id`).
pub fn glb(b: &Bati) -> Vec<u8> {
    if let Some(d) = &b.detail {
        let m = &d.modele;
        let groupes = m
            .parties
            .iter()
            .map(|&(premier, nombre, k)| {
                let v = &m.sommets[premier as usize * 11..(premier + nombre) as usize * 11];
                let mut g = Groupe { pos: Vec::new(), nrm: Vec::new(), uv: Some(Vec::new()), mat: k };
                for q in v.chunks(11) {
                    g.pos.extend_from_slice(&q[0..3]);
                    g.nrm.extend_from_slice(&q[3..6]);
                    g.uv.as_mut().unwrap().extend_from_slice(&q[9..11]);
                }
                g
            })
            .collect();
        let mats = m
            .materiaux
            .iter()
            .enumerate()
            .map(|(k, x)| MatGlb { nom: format!("m{k}"), couleur: x.couleur, opacite: x.opacite, masque: x.alpha == Alpha::Masque, deux_cotes: x.deux_cotes, image: x.source.clone() })
            .collect();
        return ecrire_glb(&b.id, groupes, mats);
    }
    let mut x = b.clone();
    x.tuile = (0, 0);
    x.position = [0.0; 3];
    x.cap = 0.0;
    x.refaire();
    let (mut v, mut t) = (Vec::new(), Vec::new());
    x.monter(0, 0, &mut v, &mut t);
    // Un groupe par couleur : les murs, la couverture
    let couleurs = [x.forme.murs, x.forme.couverture];
    let mut groupes: Vec<Groupe> = (0..2).map(|k| Groupe { pos: Vec::new(), nrm: Vec::new(), uv: None, mat: k }).collect();
    for s in v.chunks(9) {
        let k = if [s[6], s[7], s[8]] == couleurs[0] { 0 } else { 1 };
        groupes[k].pos.extend_from_slice(&s[0..3]);
        groupes[k].nrm.extend_from_slice(&s[3..6]);
    }
    let mat = |nom: &str, c: [f32; 3]| MatGlb { nom: nom.into(), couleur: c, opacite: 1.0, masque: false, deux_cotes: false, image: None };
    ecrire_glb(&b.id, groupes, vec![mat("murs", couleurs[0]), mat("couverture", couleurs[1])])
}

fn ecrire_glb(ident: &str, groupes: Vec<Groupe>, mats: Vec<MatGlb>) -> Vec<u8> {
    let mut bin: Vec<u8> = Vec::new();
    let (mut vues, mut acces, mut prims) = (Vec::new(), Vec::new(), Vec::new());
    let mut vue = |bin: &mut Vec<u8>, octets: &[u8]| {
        let d = bin.len();
        bin.extend_from_slice(octets);
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        vues.push(objet(vec![("buffer", J::Num(0.0)), ("byteOffset", J::Num(d as f64)), ("byteLength", J::Num(octets.len() as f64))]));
        vues.len() - 1
    };
    let octets = |v: &[f32]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
    for g in &groupes {
        let n = g.pos.len() / 3;
        if n == 0 {
            continue;
        }
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in g.pos.chunks(3) {
            for c in 0..3 {
                lo[c] = lo[c].min(p[c]);
                hi[c] = hi[c].max(p[c]);
            }
        }
        let vp = vue(&mut bin, &octets(&g.pos));
        let vn = vue(&mut bin, &octets(&g.nrm));
        let idx: Vec<u8> = (0..n as u32).flat_map(|i| i.to_le_bytes()).collect();
        let vi = vue(&mut bin, &idx);
        let l = |a: [f32; 3]| J::Liste(a.iter().map(|x| J::Num(*x as f64)).collect());
        let a0 = acces.len();
        acces.push(objet(vec![("bufferView", J::Num(vp as f64)), ("componentType", J::Num(5126.0)), ("count", J::Num(n as f64)), ("type", txt("VEC3")), ("min", l(lo)), ("max", l(hi))]));
        acces.push(objet(vec![("bufferView", J::Num(vn as f64)), ("componentType", J::Num(5126.0)), ("count", J::Num(n as f64)), ("type", txt("VEC3"))]));
        acces.push(objet(vec![("bufferView", J::Num(vi as f64)), ("componentType", J::Num(5125.0)), ("count", J::Num(n as f64)), ("type", txt("SCALAR"))]));
        let mut attributs = vec![("POSITION", J::Num(a0 as f64)), ("NORMAL", J::Num((a0 + 1) as f64))];
        if let Some(uv) = g.uv.as_ref().filter(|_| mats.get(g.mat).is_some_and(|m| m.image.is_some())) {
            let vu = vue(&mut bin, &octets(uv));
            acces.push(objet(vec![("bufferView", J::Num(vu as f64)), ("componentType", J::Num(5126.0)), ("count", J::Num(n as f64)), ("type", txt("VEC2"))]));
            attributs.push(("TEXCOORD_0", J::Num((acces.len() - 1) as f64)));
        }
        prims.push(objet(vec![("attributes", objet(attributs)), ("indices", J::Num((a0 + 2) as f64)), ("material", J::Num(g.mat as f64))]));
    }
    let (mut images, mut textures, mut materiaux) = (Vec::new(), Vec::new(), Vec::new());
    for m in &mats {
        let c = m.couleur;
        let f = J::Liste(vec![J::Num(lineaire(c[0])), J::Num(lineaire(c[1])), J::Num(lineaire(c[2])), J::Num(m.opacite as f64)]);
        let mut pbr = vec![("baseColorFactor", f), ("metallicFactor", J::Num(0.0)), ("roughnessFactor", J::Num(0.9))];
        if let Some(im) = &m.image {
            let png = im.starts_with(&[0x89, b'P', b'N', b'G']);
            let v = vue(&mut bin, im);
            images.push(objet(vec![("bufferView", J::Num(v as f64)), ("mimeType", txt(if png { "image/png" } else { "image/jpeg" }))]));
            textures.push(objet(vec![("source", J::Num((images.len() - 1) as f64))]));
            pbr.push(("baseColorTexture", objet(vec![("index", J::Num((textures.len() - 1) as f64))])));
        }
        let mut o = vec![("name", txt(&m.nom)), ("pbrMetallicRoughness", objet(pbr))];
        if m.opacite < 1.0 {
            o.push(("alphaMode", txt("BLEND")));
        } else if m.masque {
            o.push(("alphaMode", txt("MASK")));
        }
        if m.deux_cotes {
            o.push(("doubleSided", J::Bool(true)));
        }
        materiaux.push(objet(o));
    }
    let sage = objet(vec![("format", J::Num(4.0)), ("sorte", txt("decor")), ("id", txt(ident))]);
    let mut racine = vec![
        ("asset", objet(vec![("version", txt("2.0")), ("generator", txt(&crate::regions::application()))])),
        ("scene", J::Num(0.0)),
        ("scenes", J::Liste(vec![objet(vec![("name", txt(ident)), ("nodes", J::Liste(vec![J::Num(0.0)]))])])),
        ("nodes", J::Liste(vec![objet(vec![("name", txt(ident)), ("mesh", J::Num(0.0)), ("extras", objet(vec![("sage", sage)]))])])),
        ("meshes", J::Liste(vec![objet(vec![("name", txt(ident)), ("primitives", J::Liste(prims))])])),
        ("materials", J::Liste(materiaux)),
    ];
    if !images.is_empty() {
        racine.push(("images", J::Liste(images)));
        racine.push(("textures", J::Liste(textures)));
    }
    racine.push(("buffers", J::Liste(vec![objet(vec![("byteLength", J::Num(bin.len() as f64))])])));
    racine.push(("bufferViews", J::Liste(vues)));
    racine.push(("accessors", J::Liste(acces)));
    let j = objet(racine);
    let mut js = String::new();
    gda::json::compact(&j, &mut js);
    let mut js = js.into_bytes();
    while js.len() % 4 != 0 {
        js.push(b' ');
    }
    let total = 12 + 8 + js.len() + 8 + bin.len();
    let mut o = Vec::with_capacity(total);
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

/// Écrit la définition `id` d'un bâtiment dans `s`, ses données dans le tampon `k` :
/// `n0` procédural (`params`), `n1` les parties du modèle `d`. Rend la définition.
pub fn ecrire(s: &mut Stuff, k: usize, id: &str, nom: &str, params: J, d: &Decor) -> J {
    let (mut parties, mut ids) = (Vec::new(), Vec::new());
    let (mut materiaux, mut textures) = (Vec::new(), Vec::new());
    for (n, &(premier, nombre, m)) in d.parties.iter().enumerate() {
        let v = &d.sommets[premier as usize * 11..(premier + nombre) as usize * 11];
        let (mut pos, mut nrm, mut uv) = (Vec::with_capacity(nombre as usize * 3), Vec::with_capacity(nombre as usize * 3), Vec::with_capacity(nombre as usize * 2));
        for q in v.chunks(11) {
            pos.extend_from_slice(&q[0..3]);
            nrm.extend_from_slice(&q[3..6]);
            uv.extend_from_slice(&q[9..11]);
        }
        let tri: Vec<u32> = (0..nombre).collect();
        let pid = format!("p{n}");
        parties.push(objet(vec![
            ("id", txt(&pid)),
            ("materiau", txt(&format!("m{m}"))),
            ("nombre", J::Num(nombre as f64)),
            ("positions", s.ranger(k, &pos).json()),
            ("normales", s.ranger(k, &nrm).json()),
            ("uv", s.ranger(k, &uv).json()),
            ("triangles", s.ranger(k, &tri).json()),
        ]));
        ids.push(txt(&pid));
    }
    for (m, x) in d.materiaux.iter().enumerate() {
        let mut o = vec![
            ("couleur", txt(&format!("#{}", crate::moteur::objets::hex(x.couleur)))),
            ("opacite", J::Num(x.opacite as f64)),
            ("deux_cotes", J::Bool(x.deux_cotes)),
        ];
        if let Some(src) = &x.source {
            let ext = if src.starts_with(&[0x89, b'P', b'N', b'G']) { "png" } else { "jpg" };
            let r = s.ranger(k, src.as_slice()).json();
            textures.push((format!("t{m}"), objet(vec![("nom", txt(&format!("texture{m}.{ext}"))), ("image", r)])));
            o.push(("texture", txt(&format!("t{m}"))));
        }
        materiaux.push((format!("m{m}"), objet(o)));
    }
    objet(vec![
        ("id", txt(id)),
        ("nom", txt(nom)),
        ("genre", txt("batiment")),
        ("parties", J::Liste(parties)),
        ("materiaux", J::Objet(materiaux)),
        ("textures", J::Objet(textures)),
        (
            "niveaux",
            J::Liste(vec![
                objet(vec![("id", txt("n0")), ("jusqua", J::Num(LOIN)), ("procedural", objet(vec![("generateur", txt("batiment")), ("params", params)]))]),
                objet(vec![("id", txt("n1")), ("jusqua", J::Num(PRES)), ("parties", J::Liste(ids))]),
            ]),
        ),
        ("collision", objet(vec![("niveau", txt("n1"))])),
    ])
}

/// Le tampon où une définition range ses données (celui de sa première partie).
pub fn tampon(def: &J) -> Option<usize> {
    let p = def.get("parties")?.liste()?.first()?;
    p.get("positions")?.get("tampon")?.num().map(|n| n as usize)
}

/// Relit une définition : les paramètres de son niveau procédural, et le modèle de son
/// niveau détaillé, prêt pour le moteur (comme un décor du catalogue).
pub fn lire(s: &Stuff, def: &J) -> Option<(J, Arc<Decor>)> {
    let niveaux = def.get("niveaux")?.liste()?;
    let params = niveaux.iter().find_map(|n| n.get("procedural").filter(|p| p.get("generateur").and_then(J::txt) == Some("batiment")).and_then(|p| p.get("params")))?.clone();
    let detail = niveaux.iter().filter_map(|n| n.get("parties").and_then(J::liste)).next_back()?;
    let voulues: Vec<&str> = detail.iter().filter_map(J::txt).collect();
    let mats = def.get("materiaux");
    let texs = def.get("textures");
    // Les matériaux, dans l'ordre où les parties les citent
    let (mut noms, mut materiaux): (Vec<String>, Vec<MateriauDecor>) = (Vec::new(), Vec::new());
    let mut seaux: Vec<Vec<f32>> = Vec::new();
    let mut triangles = Vec::new();
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in def.get("parties")?.liste()? {
        if !voulues.contains(&p.get("id").and_then(J::txt).unwrap_or("")) {
            continue;
        }
        let pos: Vec<f32> = s.lire_ref(p.get("positions")?)?;
        let nrm: Vec<f32> = s.lire_ref(p.get("normales")?)?;
        let n = pos.len() / 3;
        let uv: Vec<f32> = p.get("uv").and_then(|r| s.lire_ref(r)).filter(|u: &Vec<f32>| u.len() == n * 2).unwrap_or_else(|| vec![0.0; n * 2]);
        let tri: Vec<u32> = s.lire_ref(p.get("triangles")?)?;
        if nrm.len() != pos.len() || tri.iter().any(|&i| i as usize >= n) {
            return None;
        }
        let mid = p.get("materiau").and_then(J::txt).unwrap_or("").to_string();
        let m = match noms.iter().position(|x| *x == mid) {
            Some(m) => m,
            None => {
                let x = mats.and_then(|o| o.get(&mid));
                let couleur = x.and_then(|x| crate::moteur::objets::depuis_hex(x.get("couleur")?.txt()?.trim_start_matches('#'))).unwrap_or([0.8; 3]);
                let opacite = x.and_then(|x| x.get("opacite")?.num()).unwrap_or(1.0) as f32;
                let source = x.and_then(|x| x.get("texture")?.txt()).and_then(|t| texs?.get(t)?.get("image")).and_then(|r| s.lire_ref::<u8>(r)).map(Arc::new);
                let image = source.as_ref().and_then(|o| crate::image::decoder(o).ok()).map(Arc::new);
                let alpha = if opacite < 1.0 {
                    Alpha::Melange
                } else if image.as_ref().is_some_and(|i| i.transparente()) {
                    Alpha::Masque
                } else {
                    Alpha::Opaque
                };
                materiaux.push(MateriauDecor { image, opacite, alpha, deux_cotes: x.and_then(|x| x.get("deux_cotes")?.bool()).unwrap_or(false), couleur, source });
                noms.push(mid);
                seaux.push(Vec::new());
                noms.len() - 1
            }
        };
        let couleur = materiaux[m].couleur;
        for t in tri.chunks_exact(3) {
            let mut q = [[0f32; 3]; 3];
            for (c, &i) in t.iter().enumerate() {
                let i = i as usize;
                let p = [pos[i * 3], pos[i * 3 + 1], pos[i * 3 + 2]];
                for a in 0..3 {
                    lo[a] = lo[a].min(p[a]);
                    hi[a] = hi[a].max(p[a]);
                }
                seaux[m].extend_from_slice(&p);
                seaux[m].extend_from_slice(&nrm[i * 3..i * 3 + 3]);
                seaux[m].extend_from_slice(&couleur);
                seaux[m].extend_from_slice(&uv[i * 2..i * 2 + 2]);
                q[c] = p;
            }
            triangles.push(q);
        }
    }
    if triangles.is_empty() {
        return None;
    }
    let mut ordre: Vec<usize> = (0..seaux.len()).filter(|&k| !seaux[k].is_empty()).collect();
    ordre.sort_by_key(|&k| materiaux[k].alpha);
    let (mut sommets, mut parties) = (Vec::new(), Vec::new());
    for k in ordre {
        let premier = (sommets.len() / 11) as u32;
        sommets.extend_from_slice(&seaux[k]);
        parties.push((premier, (seaux[k].len() / 11) as u32, k));
    }
    let nom = def.get("nom").and_then(J::txt).map(String::from);
    Some((params, Arc::new(Decor { id: def.get("id").and_then(J::txt).map(String::from), nom, sommets, parties, materiaux, triangles, boite: (lo, hi), manquantes: 0, retourne: false })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reel::{Batiment, MAISON};

    #[test]
    fn un_batiment_s_exporte_en_glb_et_revient_en_definition() {
        let b = Batiment { genre: MAISON, pied: 10.0, toit: 18.0, egout: 14.0, forme: crate::toits::DEUX_PANS, axe: 0.3, contour: vec![[100.0, 100.0], [112.0, 100.0], [112.0, 110.0], [100.0, 110.0]] };
        let bt = Bati::du_lieu("bat-7".into(), &b);
        let o = glb(&bt);
        // Le catalogue le relit : posé au pied, autour de son centre, deux matériaux
        let d = crate::catalogue::decoder_glb(&o, Some(false)).unwrap();
        assert_eq!(d.id.as_deref(), Some("bat-7"));
        assert!(d.boite.0[1].abs() < 1e-4 && (d.boite.1[1] - 8.0).abs() < 1e-3, "{:?}", d.boite);
        assert!((d.boite.0[0] + 6.0).abs() < 1e-3 && (d.boite.1[2] - 5.0).abs() < 1e-3, "{:?}", d.boite);
        assert_eq!(d.materiaux.len(), 2);
        // En définition, et relu tel quel
        let mut s = Stuff::new("essai", "region", "essai");
        let def = ecrire(&mut s, 2, "def-bat-7", "Maison", J::Objet(vec![("toit".into(), J::Txt("deux_pans".into()))]), &d);
        assert_eq!(tampon(&def), Some(2));
        let s = Stuff::lire(&s.octets()).unwrap();
        let (params, r) = lire(&s, &def).unwrap();
        assert_eq!(params.get("toit").and_then(J::txt), Some("deux_pans"));
        assert_eq!(r.triangles.len(), d.triangles.len());
        assert_eq!(r.sommets.len(), d.sommets.len());
        assert_eq!(r.parties.len(), d.parties.len());
        // Enrichi, il s'exporte avec son modèle
        let mut be = bt.clone();
        be.detail = Some(crate::regions::Detail::new("def-bat-7".into(), r, false));
        let d2 = crate::catalogue::decoder_glb(&glb(&be), Some(false)).unwrap();
        assert_eq!(d2.triangles.len(), d.triangles.len());
    }
}
