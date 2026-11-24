//! Les règles de circulation d'OSM gardées avec le lieu (format 16) :
//!
//! - les interdictions de tourner (relations `type=restriction`) : `no_left_turn`,
//!   `only_straight_on`… Le lieu ne garde pas les identifiants d'OSM ; une interdiction
//!   se repère donc par des points : le carrefour (`via`), un point sur la voie d'où l'on
//!   vient et un sur celle où l'on irait, à `RECUL` mètres du carrefour ;
//! - les gabarits : la hauteur libre des voies (`maxheight:physical`, sinon
//!   `maxheight`), sous un pont ou dans un souterrain, avec le tracé de la voie.

use crate::json::J;

/// Les mouvements d'une interdiction.
pub const GAUCHE: u8 = 1;
pub const DROITE: u8 = 2;
pub const TOUT_DROIT: u8 = 3;
pub const DEMI_TOUR: u8 = 4;
/// `no_entry` : pas d'entrée dans la voie `vers` depuis le carrefour ; `no_exit` : pas
/// de sortie de la voie `depuis` vers le carrefour.
pub const ENTREE: u8 = 5;
pub const SORTIE: u8 = 6;

/// Les exceptions (`except`) : des bits.
pub const SAUF_BUS: u8 = 1;
pub const SAUF_VELO: u8 = 2;
pub const SAUF_AUTRES: u8 = 4;

/// La distance du carrefour où sont pris les points `depuis` et `vers` (m).
pub const RECUL: f32 = 10.0;

/// Une interdiction de tourner (ou, `seulement`, le seul mouvement permis).
#[derive(Clone, Debug, PartialEq)]
pub struct Interdiction {
    /// `GAUCHE`, `DROITE`, `TOUT_DROIT`, `DEMI_TOUR`, `ENTREE`, `SORTIE`.
    pub genre: u8,
    /// `only_…` : depuis `depuis`, seul ce mouvement est permis.
    pub seulement: bool,
    /// Ceux qui y échappent (`SAUF_BUS`…).
    pub sauf: u8,
    /// Un point de la voie d'où l'on vient, à `RECUL` du carrefour (x, z du jeu).
    pub depuis: [f32; 2],
    /// Le carrefour : où la voie d'où l'on vient le touche, et où en part celle où l'on
    /// irait (les mêmes pour un carrefour en un nœud ; distincts quand le `via` d'OSM
    /// est une voie, entre deux chaussées d'un boulevard par exemple).
    pub via: [[f32; 2]; 2],
    /// Un point de la voie où l'on irait, à `RECUL` du carrefour.
    pub vers: [f32; 2],
}

/// La hauteur libre d'une voie, avec son tracé.
#[derive(Clone, Debug, PartialEq)]
pub struct Gabarit {
    /// En mètres.
    pub hauteur: f32,
    /// x, z du jeu.
    pub points: Vec<[f32; 2]>,
}

/// Le genre d'une interdiction d'OSM (`no_left_turn`…), et si c'est un `only_…`.
pub fn genre(v: &str) -> Option<(u8, bool)> {
    let (seul, m) = if let Some(m) = v.strip_prefix("only_") {
        (true, m)
    } else if let Some(m) = v.strip_prefix("no_") {
        (false, m)
    } else {
        return None;
    };
    let g = match m {
        "left_turn" => GAUCHE,
        "right_turn" => DROITE,
        "straight_on" => TOUT_DROIT,
        "u_turn" => DEMI_TOUR,
        "entry" if !seul => ENTREE,
        "exit" if !seul => SORTIE,
        _ => return None,
    };
    Some((g, seul))
}

/// Les exceptions d'un `except` d'OSM (`psv;bicycle`…).
pub fn sauf(v: Option<&str>) -> u8 {
    v.map_or(0, |v| {
        v.split(';').fold(0, |s, x| {
            s | match x.trim() {
                "psv" | "bus" | "minibus" | "taxi" => SAUF_BUS,
                "bicycle" => SAUF_VELO,
                "" => 0,
                _ => SAUF_AUTRES,
            }
        })
    })
}

/// La règle qui vaut pour les voitures : `restriction`, sinon celle des véhicules à
/// moteur ou des voitures (une règle pour les seuls camions ou bus ne compte pas).
pub fn regle(tags: Option<&J>) -> Option<&str> {
    let tag = |k: &str| tags.and_then(|t| t.get(k)).and_then(J::txt);
    tag("restriction").or_else(|| tag("restriction:motor_vehicle")).or_else(|| tag("restriction:motorcar"))
}

/// Une hauteur d'OSM en mètres : `3.5`, `3,5 m`, `11'6"` ; rien pour `default`,
/// `none`, `below_default`…
pub fn hauteur(v: &str) -> Option<f32> {
    let v = v.trim();
    if let Some((pieds, reste)) = v.split_once('\'') {
        let p: f32 = pieds.trim().parse().ok()?;
        let po: f32 = reste.trim().trim_end_matches('"').trim().parse().unwrap_or(0.0);
        return Some((p * 12.0 + po) * 0.0254).filter(|h| *h > 0.5 && *h < 20.0);
    }
    let v = v.replace(',', ".");
    let v = v.trim_end_matches('m').trim();
    v.parse::<f32>().ok().filter(|h| *h > 0.5 && *h < 20.0)
}

/// La hauteur libre d'une voie d'OSM : `maxheight:physical`, sinon `maxheight`.
pub fn hauteur_voie(tags: Option<&J>) -> Option<f32> {
    let tag = |k: &str| tags.and_then(|t| t.get(k)).and_then(J::txt);
    tag("maxheight:physical").and_then(hauteur).or_else(|| tag("maxheight").and_then(hauteur))
}

fn d2(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Le point de la ligne `l` à `s` mètres de son début.
fn a_distance(l: &[[f32; 2]], s: f32) -> [f32; 2] {
    let mut fait = 0.0;
    for w in l.windows(2) {
        let d = d2(w[0], w[1]);
        if fait + d >= s && d > 1e-6 {
            let t = (s - fait) / d;
            return [w[0][0] + (w[1][0] - w[0][0]) * t, w[0][1] + (w[1][1] - w[0][1]) * t];
        }
        fait += d;
    }
    *l.last().unwrap_or(&[0.0, 0.0])
}

/// Où la ligne `l` touche le carrefour (son bout le plus proche des points `via`, à
/// moins de `tolerance`), et un point à `RECUL` de là le long d'elle.
fn bout_vers(l: &[[f32; 2]], via: &[[f32; 2]], tolerance: f32) -> Option<([f32; 2], [f32; 2])> {
    if l.len() < 2 || via.is_empty() {
        return None;
    }
    let pres = |p: [f32; 2]| via.iter().map(|v| d2(p, *v)).fold(f32::MAX, f32::min);
    let (a, b) = (l[0], l[l.len() - 1]);
    if pres(a).min(pres(b)) > tolerance {
        // Le carrefour au milieu de la voie : une relation mal faite, ou une voie
        // coupée autrement ; le point le plus proche, et la plus longue des deux parts
        let (i, _) = l.iter().enumerate().map(|(i, p)| (i, pres(*p))).min_by(|x, y| x.1.total_cmp(&y.1))?;
        if pres(l[i]) > tolerance {
            return None;
        }
        let avant: f32 = l[..=i].windows(2).map(|w| d2(w[0], w[1])).sum();
        let apres: f32 = l[i..].windows(2).map(|w| d2(w[0], w[1])).sum();
        return if apres >= avant {
            Some((l[i], a_distance(&l[i..], RECUL)))
        } else {
            let r: Vec<[f32; 2]> = l[..=i].iter().rev().copied().collect();
            Some((l[i], a_distance(&r, RECUL)))
        };
    }
    if pres(a) <= pres(b) {
        Some((a, a_distance(l, RECUL)))
    } else {
        let r: Vec<[f32; 2]> = l.iter().rev().copied().collect();
        Some((b, a_distance(&r, RECUL)))
    }
}

/// Une interdiction d'après les membres d'une relation : la voie `from`, le carrefour
/// `via` (un nœud, ou les points des voies `via`), la voie `to` (x, z du jeu).
pub fn interdiction(genre: u8, seulement: bool, sauf: u8, de: &[[f32; 2]], via: &[[f32; 2]], vers: &[[f32; 2]]) -> Option<Interdiction> {
    let (va, pa) = bout_vers(de, via, 2.0)?;
    let (vb, pb) = bout_vers(vers, via, 2.0)?;
    Some(Interdiction { genre, seulement, sauf, depuis: pa, via: [va, vb], vers: pb })
}

/// Écrit les interdictions et les gabarits (format 16).
pub fn ecrire(o: &mut Vec<u8>, inter: &[Interdiction], gabarits: &[Gabarit]) {
    o.extend_from_slice(&(inter.len() as u32).to_le_bytes());
    for i in inter {
        o.extend_from_slice(&[i.genre, i.seulement as u8, i.sauf]);
        for p in [i.depuis, i.via[0], i.via[1], i.vers] {
            o.extend_from_slice(&p[0].to_le_bytes());
            o.extend_from_slice(&p[1].to_le_bytes());
        }
    }
    o.extend_from_slice(&(gabarits.len() as u32).to_le_bytes());
    for g in gabarits {
        o.extend_from_slice(&g.hauteur.to_le_bytes());
        o.extend_from_slice(&(g.points.len() as u32).to_le_bytes());
        for p in &g.points {
            o.extend_from_slice(&p[0].to_le_bytes());
            o.extend_from_slice(&p[1].to_le_bytes());
        }
    }
}

/// Relit ce qu'a écrit `ecrire` ; `prendre(n)` donne les `n` octets suivants.
pub fn lire<'a>(prendre: &mut dyn FnMut(usize) -> Option<&'a [u8]>) -> Option<(Vec<Interdiction>, Vec<Gabarit>)> {
    let u32_ = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32_ = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let n = u32_(prendre(4)?) as usize;
    let mut inter = Vec::with_capacity(n.min(100_000));
    for _ in 0..n {
        let b = prendre(3 + 32)?;
        let p = |k: usize| [f32_(&b[3 + 8 * k..]), f32_(&b[7 + 8 * k..])];
        inter.push(Interdiction { genre: b[0], seulement: b[1] != 0, sauf: b[2], depuis: p(0), via: [p(1), p(2)], vers: p(3) });
    }
    let n = u32_(prendre(4)?) as usize;
    let mut gabarits = Vec::with_capacity(n.min(100_000));
    for _ in 0..n {
        let hauteur = f32_(prendre(4)?);
        let m = u32_(prendre(4)?) as usize;
        let b = prendre(m * 8)?;
        let points = b.chunks(8).map(|c| [f32_(c), f32_(&c[4..])]).collect();
        gabarits.push(Gabarit { hauteur, points });
    }
    Some((inter, gabarits))
}

/// Ce qui reste dans le rectangle dit par `dans` : les interdictions dont le carrefour
/// y est, les gabarits coupés à son bord.
pub fn couper(inter: &[Interdiction], gabarits: &[Gabarit], dans: &dyn Fn(f32, f32) -> bool) -> (Vec<Interdiction>, Vec<Gabarit>) {
    let i = inter.iter().filter(|i| dans(i.via[0][0], i.via[0][1])).cloned().collect();
    let mut g = Vec::new();
    for x in gabarits {
        let mut m: Vec<[f32; 2]> = Vec::new();
        for p in &x.points {
            if dans(p[0], p[1]) {
                m.push(*p);
            } else if m.len() > 1 {
                g.push(Gabarit { hauteur: x.hauteur, points: std::mem::take(&mut m) });
            } else {
                m.clear();
            }
        }
        if m.len() > 1 {
            g.push(Gabarit { hauteur: x.hauteur, points: m });
        }
    }
    (i, g)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_hauteurs_d_osm_se_lisent() {
        assert_eq!(hauteur("3.5"), Some(3.5));
        assert_eq!(hauteur("3,2 m"), Some(3.2));
        assert!((hauteur("11'6\"").unwrap() - 3.5052).abs() < 1e-3);
        assert_eq!(hauteur("default"), None);
        assert_eq!(hauteur("none"), None);
    }

    #[test]
    fn les_genres_se_lisent() {
        assert_eq!(genre("no_left_turn"), Some((GAUCHE, false)));
        assert_eq!(genre("only_straight_on"), Some((TOUT_DROIT, true)));
        assert_eq!(genre("no_entry"), Some((ENTREE, false)));
        assert_eq!(genre("only_entry"), None);
        assert_eq!(sauf(Some("psv;bicycle")), SAUF_BUS | SAUF_VELO);
    }

    #[test]
    fn une_interdiction_prend_ses_points_a_dix_metres() {
        // Arrivée par l'ouest, départ vers le nord, carrefour en (0, 0)
        let de = [[-50.0, 0.0], [0.0, 0.0]];
        let vers = [[0.0, 0.0], [0.0, -40.0]];
        let i = interdiction(GAUCHE, false, 0, &de, &[[0.0, 0.0]], &vers).unwrap();
        assert_eq!(i.via, [[0.0, 0.0], [0.0, 0.0]]);
        assert!(d2(i.depuis, [-10.0, 0.0]) < 1e-4 && d2(i.vers, [0.0, -10.0]) < 1e-4);
        let mut o = Vec::new();
        ecrire(&mut o, &[i.clone()], &[Gabarit { hauteur: 3.5, points: vec![[1.0, 2.0], [3.0, 4.0]] }]);
        let mut k = 0;
        let (a, g) = lire(&mut |n| {
            k += n;
            o.get(k - n..k)
        })
        .unwrap();
        assert_eq!(a, vec![i]);
        assert_eq!(g[0].points, vec![[1.0, 2.0], [3.0, 4.0]]);
    }
}
