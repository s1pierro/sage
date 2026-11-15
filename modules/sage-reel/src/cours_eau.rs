//! Les cours d'eau d'un lieu, en vecteurs : le tracé de chacun (son axe, d'amont en
//! aval comme OSM le dessine : `waterway=river|canal|stream|ditch|drain`), son genre,
//! son nom, la largeur qu'OSM lui donne quand il la donne (`width`), et s'il passe sous
//! terre (`tunnel=culvert|yes`, ou `layer` négatif). C'est la base d'où se déduiront, le
//! long du tracé, la largeur, la profondeur et le type des berges.

pub const RIVIERE: u8 = 0;
pub const CANAL: u8 = 1;
pub const RUISSEAU: u8 = 2;
pub const FOSSE: u8 = 3;
pub const NOMS: [&str; 4] = ["Rivière", "Canal", "Ruisseau", "Fossé"];

#[derive(Clone, Debug, PartialEq)]
pub struct CoursEau {
    pub genre: u8,
    pub nom: String,
    /// La largeur d'OSM (m), 0 si elle n'est pas donnée.
    pub largeur: f32,
    pub souterrain: bool,
    /// x, z du jeu.
    pub points: Vec<[f32; 2]>,
}

/// Le genre d'un `waterway` d'OSM ; rien pour ce qui n'est pas un cours d'eau (barrages,
/// berges, écluses…).
pub fn genre(waterway: &str) -> Option<u8> {
    match waterway {
        "river" => Some(RIVIERE),
        "canal" => Some(CANAL),
        "stream" => Some(RUISSEAU),
        "ditch" | "drain" => Some(FOSSE),
        _ => None,
    }
}

/// « 12 », « 12.5 m », « 12,5 » : des mètres.
pub fn largeur_osm(t: &str) -> f32 {
    let t = t.trim().trim_end_matches('m').trim().replace(',', ".");
    t.parse::<f32>().ok().filter(|l| l.is_finite() && *l > 0.0 && *l < 2000.0).unwrap_or(0.0)
}

/// Réduits à ce qui est dans `dans` : coupés à son bord.
pub fn couper(v: &[CoursEau], dans: &dyn Fn(f32, f32) -> bool) -> Vec<CoursEau> {
    let mut r = Vec::new();
    for c in v {
        let mut m: Vec<[f32; 2]> = Vec::new();
        for p in &c.points {
            if dans(p[0], p[1]) {
                m.push(*p);
            } else {
                if m.len() > 1 {
                    r.push(CoursEau { points: std::mem::take(&mut m), ..c.clone() });
                }
                m.clear();
            }
        }
        if m.len() > 1 {
            r.push(CoursEau { points: m, ..c.clone() });
        }
    }
    r
}

pub(crate) fn ecrire(o: &mut Vec<u8>, v: &[CoursEau]) {
    o.extend_from_slice(&(v.len() as u32).to_le_bytes());
    for c in v {
        o.extend_from_slice(&[c.genre, c.souterrain as u8]);
        o.extend_from_slice(&c.largeur.to_le_bytes());
        o.extend_from_slice(&(c.nom.len() as u32).to_le_bytes());
        o.extend_from_slice(c.nom.as_bytes());
        o.extend_from_slice(&(c.points.len() as u32).to_le_bytes());
        for x in c.points.iter().flatten() {
            o.extend_from_slice(&x.to_le_bytes());
        }
    }
}

pub(crate) fn lire(prendre: &mut dyn FnMut(usize) -> Option<Vec<u8>>) -> Option<Vec<CoursEau>> {
    let u32_ = |p: &mut dyn FnMut(usize) -> Option<Vec<u8>>| -> Option<usize> { Some(u32::from_le_bytes(p(4)?.try_into().ok()?) as usize) };
    let f32_ = |c: &[u8]| f32::from_le_bytes(c.try_into().unwrap());
    let mut v = Vec::new();
    for _ in 0..u32_(prendre)? {
        let t = prendre(2)?;
        let largeur = f32_(&prendre(4)?);
        let n = u32_(prendre)?;
        let nom = String::from_utf8(prendre(n)?).ok()?;
        let m = u32_(prendre)?;
        let points = prendre(m * 8)?.chunks(8).map(|c| [f32_(&c[0..4]), f32_(&c[4..8])]).collect();
        v.push(CoursEau { genre: t[0], nom, largeur, souterrain: t[1] != 0, points });
    }
    Some(v)
}

/// La nature d'une surface d'eau d'OSM : le lit d'une rivière (`waterway=riverbank`,
/// `water=river|stream`), d'un canal (`water=canal`), ou un plan d'eau (le reste :
/// étangs, bassins, lacs).
pub const SURFACE_RIVIERE: u8 = 0;
pub const SURFACE_CANAL: u8 = 1;
pub const SURFACE_PLAN: u8 = 2;
pub const NOMS_SURFACES: [&str; 3] = ["Lit de rivière", "Canal", "Plan d'eau"];

/// Une surface d'eau d'OSM, en vecteurs : ses anneaux (x, z du jeu, fermés sans répéter
/// le premier point), pair-impair (un anneau dans un autre est une île). Leurs côtés
/// sont les berges.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceEau {
    pub nature: u8,
    pub anneaux: Vec<Vec<[f32; 2]>>,
}

/// La nature d'une surface d'après ses étiquettes `waterway` et `water`.
pub fn nature(waterway: Option<&str>, water: Option<&str>) -> u8 {
    match (waterway, water) {
        (Some("riverbank"), _) | (_, Some("river" | "stream" | "oxbow" | "rapids")) => SURFACE_RIVIERE,
        (_, Some("canal" | "lock")) => SURFACE_CANAL,
        _ => SURFACE_PLAN,
    }
}

/// Un anneau coupé au rectangle `b` (x0, z0, x1, z1) : il reste fermé, ses bouts
/// dehors suivent le bord (Sutherland-Hodgman) ; vide s'il est dehors.
pub fn couper_anneau(r: &[[f32; 2]], b: [f32; 4]) -> Vec<[f32; 2]> {
    let mut v: Vec<[f32; 2]> = r.to_vec();
    // Les quatre bords : (axe, limite, garder au-dessus ?)
    for (k, lim, dessus) in [(0usize, b[0], true), (0, b[2], false), (1, b[1], true), (1, b[3], false)] {
        if v.is_empty() {
            break;
        }
        let dedans = |p: &[f32; 2]| if dessus { p[k] >= lim } else { p[k] <= lim };
        let mut n = Vec::with_capacity(v.len() + 4);
        for i in 0..v.len() {
            let (a, c) = (v[i], v[(i + 1) % v.len()]);
            let (da, dc) = (dedans(&a), dedans(&c));
            if da {
                n.push(a);
            }
            if da != dc {
                let t = (lim - a[k]) / (c[k] - a[k]);
                let mut p = [a[0] + (c[0] - a[0]) * t, a[1] + (c[1] - a[1]) * t];
                p[k] = lim;
                n.push(p);
            }
        }
        v = n;
    }
    if v.len() < 3 {
        Vec::new()
    } else {
        v
    }
}

/// Les surfaces coupées au rectangle `b` ; celles qui en sortent tout à fait partent.
pub fn couper_surfaces(v: &[SurfaceEau], b: [f32; 4]) -> Vec<SurfaceEau> {
    v.iter()
        .filter_map(|s| {
            let anneaux: Vec<Vec<[f32; 2]>> = s.anneaux.iter().map(|r| couper_anneau(r, b)).filter(|r| !r.is_empty()).collect();
            (!anneaux.is_empty()).then_some(SurfaceEau { nature: s.nature, anneaux })
        })
        .collect()
}

pub(crate) fn ecrire_surfaces(o: &mut Vec<u8>, v: &[SurfaceEau]) {
    o.extend_from_slice(&(v.len() as u32).to_le_bytes());
    for s in v {
        o.push(s.nature);
        o.extend_from_slice(&(s.anneaux.len() as u32).to_le_bytes());
        for r in &s.anneaux {
            o.extend_from_slice(&(r.len() as u32).to_le_bytes());
            for x in r.iter().flatten() {
                o.extend_from_slice(&x.to_le_bytes());
            }
        }
    }
}

pub(crate) fn lire_surfaces(prendre: &mut dyn FnMut(usize) -> Option<Vec<u8>>) -> Option<Vec<SurfaceEau>> {
    let u32_ = |p: &mut dyn FnMut(usize) -> Option<Vec<u8>>| -> Option<usize> { Some(u32::from_le_bytes(p(4)?.try_into().ok()?) as usize) };
    let f32_ = |c: &[u8]| f32::from_le_bytes(c.try_into().unwrap());
    let mut v = Vec::new();
    for _ in 0..u32_(prendre)? {
        let nature = prendre(1)?[0];
        let mut anneaux = Vec::new();
        for _ in 0..u32_(prendre)? {
            let m = u32_(prendre)?;
            anneaux.push(prendre(m * 8)?.chunks(8).map(|c| [f32_(&c[0..4]), f32_(&c[4..8])]).collect());
        }
        v.push(SurfaceEau { nature, anneaux });
    }
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_anneau_coupe_reste_ferme_sur_le_bord() {
        let carre = vec![[-10.0, -10.0], [10.0, -10.0], [10.0, 10.0], [-10.0, 10.0]];
        let r = couper_anneau(&carre, [0.0, -100.0, 100.0, 100.0]);
        assert_eq!(r.len(), 4);
        assert!(r.iter().all(|p| p[0] >= 0.0));
        let aire: f32 = (0..r.len()).map(|i| r[i][0] * r[(i + 1) % r.len()][1] - r[(i + 1) % r.len()][0] * r[i][1]).sum::<f32>() / 2.0;
        assert!((aire.abs() - 200.0).abs() < 1e-3);
        assert!(couper_anneau(&carre, [50.0, 50.0, 60.0, 60.0]).is_empty());
        assert_eq!(nature(Some("riverbank"), None), SURFACE_RIVIERE);
        assert_eq!(nature(None, Some("pond")), SURFACE_PLAN);
    }

    #[test]
    fn genres_et_largeurs() {
        assert_eq!(genre("river"), Some(RIVIERE));
        assert_eq!(genre("drain"), Some(FOSSE));
        assert_eq!(genre("riverbank"), None);
        assert_eq!(largeur_osm("12,5 m"), 12.5);
        assert_eq!(largeur_osm("large"), 0.0);
    }

    #[test]
    fn couper_garde_les_morceaux_dedans() {
        let c = CoursEau { genre: RIVIERE, nom: "Seine".into(), largeur: 150.0, souterrain: false, points: vec![[0.0, 0.0], [10.0, 0.0], [30.0, 0.0], [40.0, 0.0], [50.0, 0.0]] };
        let r = couper(&[c], &|x, _| !(20.0..35.0).contains(&x));
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].points, vec![[0.0, 0.0], [10.0, 0.0]]);
        assert_eq!(r[1].nom, "Seine");
    }
}
