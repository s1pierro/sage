//! Une tuile en glTF binaire : l'écrire, et relire celles qu'on a écrites (un nœud, un
//! maillage, des primitives triangles aux attributs POSITION, NORMAL, TEXCOORD_0 en
//! flottants et aux indices en u32, des matériaux nommés, une image PNG au plus).

use crate::json::Json;
use crate::{Primitive, Tuile};

const FLOAT: f64 = 5126.0;
const U32: f64 = 5125.0;
const TABLEAU: f64 = 34962.0;
const INDICES: f64 = 34963.0;

fn n(v: f64) -> Json {
    Json::Nombre(v)
}

fn o(v: Vec<(&str, Json)>) -> Json {
    Json::Objet(v.into_iter().map(|(k, x)| (k.to_string(), x)).collect())
}

fn t(s: &str) -> Json {
    Json::Texte(s.to_string())
}

/// La couleur de base d'une matière (ce qui la montre sans texture).
fn couleur(m: &str) -> [f64; 3] {
    match m {
        "chaussee" => [0.22, 0.22, 0.23],
        // Le bitume bordeaux des voies de bus en site propre
        "chaussee_bus" => [0.36, 0.11, 0.13],
        "beton" => [0.80, 0.79, 0.76],
        // Les tons des bâtiments (Flow : maquette::matiere::TEINTES_BATI)
        "bati_0" => [0.92, 0.90, 0.85],
        "bati_1" => [0.91, 0.88, 0.80],
        "bati_2" => [0.89, 0.86, 0.79],
        "bati_3" => [0.85, 0.84, 0.81],
        "bati_4" => [0.88, 0.88, 0.87],
        "bati_5" => [0.93, 0.93, 0.92],
        "bati_6" => [0.87, 0.85, 0.82],
        "bati_7" => [0.91, 0.87, 0.78],
        "lit" => [0.33, 0.30, 0.24],
        "berge" | "remblai" | "quai_mur" => [0.62, 0.57, 0.48],
        "quai" => [0.55, 0.52, 0.47],
        crate::EAU => [0.26, 0.50, 0.72],
        _ => [0.8, 0.8, 0.8],
    }
}

/// La tuile en GLB.
pub fn ecrire(tu: &Tuile) -> Vec<u8> {
    let mut bin: Vec<u8> = Vec::new();
    let (mut vues, mut accesseurs, mut prims, mut materiaux) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut vue = |bin: &mut Vec<u8>, octets: &[u8], cible: Option<f64>| -> usize {
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        let debut = bin.len();
        bin.extend_from_slice(octets);
        let mut v = vec![("buffer", n(0.0)), ("byteOffset", n(debut as f64)), ("byteLength", n(octets.len() as f64))];
        if let Some(c) = cible {
            v.push(("target", n(c)));
        }
        vues.push(o(v));
        vues.len() - 1
    };
    let mut image = None;
    if let Some(png) = &tu.image_sol {
        image = Some(vue(&mut bin, png, None));
    }
    for p in &tu.primitives {
        let flottants = |v: &[[f32; 3]]| v.iter().flatten().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
        let (mut a, mut b) = ([f32::MAX; 3], [f32::MIN; 3]);
        for q in &p.positions {
            for k in 0..3 {
                a[k] = a[k].min(q[k]);
                b[k] = b[k].max(q[k]);
            }
        }
        if p.positions.is_empty() {
            (a, b) = ([0.0; 3], [0.0; 3]);
        }
        let nv = p.positions.len() as f64;
        let vp = vue(&mut bin, &flottants(&p.positions), Some(TABLEAU));
        accesseurs.push(o(vec![("bufferView", n(vp as f64)), ("componentType", n(FLOAT)), ("count", n(nv)), ("type", t("VEC3")), ("min", Json::Liste(a.iter().map(|x| n(*x as f64)).collect())), ("max", Json::Liste(b.iter().map(|x| n(*x as f64)).collect()))]));
        let ap = accesseurs.len() - 1;
        let vn = vue(&mut bin, &flottants(&p.normales), Some(TABLEAU));
        accesseurs.push(o(vec![("bufferView", n(vn as f64)), ("componentType", n(FLOAT)), ("count", n(nv)), ("type", t("VEC3"))]));
        let an = accesseurs.len() - 1;
        let uv: Vec<u8> = p.uv.iter().flatten().flat_map(|x| x.to_le_bytes()).collect();
        let vu = vue(&mut bin, &uv, Some(TABLEAU));
        accesseurs.push(o(vec![("bufferView", n(vu as f64)), ("componentType", n(FLOAT)), ("count", n(nv)), ("type", t("VEC2"))]));
        let au = accesseurs.len() - 1;
        let ind: Vec<u8> = p.indices.iter().flat_map(|x| x.to_le_bytes()).collect();
        let vi = vue(&mut bin, &ind, Some(INDICES));
        accesseurs.push(o(vec![("bufferView", n(vi as f64)), ("componentType", n(U32)), ("count", n(p.indices.len() as f64)), ("type", t("SCALAR"))]));
        let ai = accesseurs.len() - 1;
        let c = couleur(&p.matiere);
        let mut pbr = vec![("baseColorFactor", Json::Liste(vec![n(c[0]), n(c[1]), n(c[2]), n(1.0)])), ("metallicFactor", n(0.0)), ("roughnessFactor", n(1.0))];
        if p.matiere == "sol" && image.is_some() {
            pbr[0] = ("baseColorFactor", Json::Liste(vec![n(1.0), n(1.0), n(1.0), n(1.0)]));
            pbr.push(("baseColorTexture", o(vec![("index", n(0.0))])));
        }
        // L'eau, semi-transparente
        let mut mat = vec![("name", t(&p.matiere))];
        if p.matiere == crate::EAU {
            pbr[0] = ("baseColorFactor", Json::Liste(vec![n(c[0]), n(c[1]), n(c[2]), n(0.55)]));
            mat.push(("alphaMode", t("BLEND")));
        }
        mat.push(("pbrMetallicRoughness", o(pbr)));
        materiaux.push(o(mat));
        prims.push(o(vec![("attributes", o(vec![("POSITION", n(ap as f64)), ("NORMAL", n(an as f64)), ("TEXCOORD_0", n(au as f64))])), ("indices", n(ai as f64)), ("material", n((materiaux.len() - 1) as f64)), ("mode", n(4.0))]));
    }
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let nom = format!("tuile {} {} {}", tu.cle.0, tu.cle.1, tu.cle.2);
    let mut racine = vec![
        ("asset", o(vec![("version", t("2.0")), ("generator", t("sage-carte"))])),
        ("extras", o(vec![("sage-carte", n(crate::FORMAT as f64)), ("cle", Json::Liste(vec![n(tu.cle.0 as f64), n(tu.cle.1 as f64), n(tu.cle.2 as f64)]))])),
        ("scene", n(0.0)),
        ("scenes", Json::Liste(vec![o(vec![("nodes", Json::Liste(vec![n(0.0)]))])])),
        ("nodes", Json::Liste(vec![o(vec![("name", t(&nom)), ("mesh", n(0.0)), ("translation", Json::Liste(tu.origine.iter().map(|x| n(*x as f64)).collect()))])])),
        ("meshes", Json::Liste(vec![o(vec![("name", t(&nom)), ("primitives", Json::Liste(prims))])])),
        ("materials", Json::Liste(materiaux)),
        ("accessors", Json::Liste(accesseurs)),
    ];
    if let Some(i) = image {
        racine.push(("images", Json::Liste(vec![o(vec![("bufferView", n(i as f64)), ("mimeType", t("image/png"))])])));
        racine.push(("samplers", Json::Liste(vec![o(vec![("magFilter", n(9729.0)), ("minFilter", n(9729.0)), ("wrapS", n(33071.0)), ("wrapT", n(33071.0))])])));
        racine.push(("textures", Json::Liste(vec![o(vec![("source", n(0.0)), ("sampler", n(0.0))])])));
    }
    racine.push(("bufferViews", Json::Liste(vues)));
    racine.push(("buffers", Json::Liste(vec![o(vec![("byteLength", n(bin.len() as f64))])])));
    let mut js = o(racine).ecrire().into_bytes();
    while js.len() % 4 != 0 {
        js.push(b' ');
    }
    let total = 12 + 8 + js.len() + 8 + bin.len();
    let mut f = Vec::with_capacity(total);
    f.extend_from_slice(b"glTF");
    f.extend_from_slice(&2u32.to_le_bytes());
    f.extend_from_slice(&(total as u32).to_le_bytes());
    f.extend_from_slice(&(js.len() as u32).to_le_bytes());
    f.extend_from_slice(b"JSON");
    f.extend_from_slice(&js);
    f.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    f.extend_from_slice(b"BIN\0");
    f.extend_from_slice(&bin);
    f
}

/// Une tuile écrite par `ecrire`.
pub fn lire(f: &[u8]) -> Result<Tuile, String> {
    let u32_a = |i: usize| -> Result<usize, String> { f.get(i..i + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize).ok_or_else(|| "GLB tronqué".to_string()) };
    if f.get(0..4) != Some(b"glTF") {
        return Err("pas un GLB".into());
    }
    let lj = u32_a(12)?;
    let js = std::str::from_utf8(f.get(20..20 + lj).ok_or("GLB tronqué")?).map_err(|_| "JSON illisible")?;
    let j = Json::lire(js)?;
    let debut_bin = 20 + lj + 8;
    let lb = u32_a(20 + lj)?;
    let bin = f.get(debut_bin..debut_bin + lb).ok_or("GLB tronqué")?;
    let vue = |k: usize| -> Result<&[u8], String> {
        let v = j.get("bufferViews").and_then(|v| v.at(k)).ok_or("vue absente")?;
        let (d, l) = (v.entier("byteOffset").unwrap_or(0), v.entier("byteLength").ok_or("vue sans longueur")?);
        bin.get(d..d + l).ok_or_else(|| "vue hors du tampon".to_string())
    };
    let flottants = |k: usize| -> Result<Vec<f32>, String> {
        let a = j.get("accessors").and_then(|v| v.at(k)).ok_or("accesseur absent")?;
        let b = vue(a.entier("bufferView").ok_or("accesseur sans vue")?)?;
        Ok(b.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect())
    };
    let cle = j.get("extras").and_then(|e| e.get("cle")).map(|c| (c.at(0).and_then(Json::nombre).unwrap_or(0.0) as u8, c.at(1).and_then(Json::nombre).unwrap_or(0.0) as i32, c.at(2).and_then(Json::nombre).unwrap_or(0.0) as i32)).unwrap_or((0, 0, 0));
    let noeud = j.get("nodes").and_then(|v| v.at(0)).ok_or("pas de nœud")?;
    let tr = noeud.get("translation");
    let origine = [0, 1, 2].map(|k| tr.and_then(|t| t.at(k)).and_then(Json::nombre).unwrap_or(0.0) as f32);
    let mut primitives = Vec::new();
    let maillage = j.get("meshes").and_then(|v| v.at(noeud.entier("mesh").unwrap_or(0))).ok_or("pas de maillage")?;
    for p in maillage.get("primitives").map(Json::liste).unwrap_or(&[]) {
        let at = p.get("attributes").ok_or("primitive sans attributs")?;
        let trois = |v: Vec<f32>| v.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect::<Vec<_>>();
        let positions = trois(flottants(at.entier("POSITION").ok_or("sans POSITION")?)?);
        let normales = match at.entier("NORMAL") {
            Some(k) => trois(flottants(k)?),
            None => Vec::new(),
        };
        let uv = match at.entier("TEXCOORD_0") {
            Some(k) => flottants(k)?.chunks_exact(2).map(|c| [c[0], c[1]]).collect(),
            None => Vec::new(),
        };
        let ki = p.entier("indices").ok_or("primitive sans indices")?;
        let ai = j.get("accessors").and_then(|v| v.at(ki)).ok_or("accesseur absent")?;
        let indices = vue(ai.entier("bufferView").ok_or("accesseur sans vue")?)?.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
        let matiere = p.entier("material").and_then(|m| j.get("materials").and_then(|v| v.at(m))).and_then(|m| m.get("name")).and_then(Json::texte).unwrap_or("").to_string();
        primitives.push(Primitive { matiere, positions, normales, uv, indices });
    }
    let image_sol = match j.get("images").and_then(|v| v.at(0)).and_then(|i| i.entier("bufferView")) {
        Some(k) => Some(vue(k)?.to_vec()),
        None => None,
    };
    Ok(Tuile { cle, origine, primitives, image_sol })
}
