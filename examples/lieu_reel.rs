//! Prépare un lieu réel pour S.A.G.E. : 20 km autour d'un point de France, le relief de
//! l'IGN (RGE ALTI, Géoplateforme) et les surfaces d'eau d'OpenStreetMap (Overpass).
//!
//!   cargo run --release --example lieu_reel -- <nom> <latitude> <longitude> <sortie.reel> [pas en m]
//!
//! Le pas vaut 4 m par défaut. Les téléchargements (curl) sont gardés dans
//! `<sortie>.cache/` : relancer ne les refait pas. Le fichier produit s'ouvre dans le jeu
//! (Éditeur › Topologie › Carte › « Importer un lieu réel… »).
//!
//! Les ponts : les `highway=*` en `bridge=*`, mis bout à bout ; le tablier va d'un bout à
//! l'autre entre les hauteurs du sol à ses bouts (pondérées par la distance), sans
//! toucher au relief dessous.
//!
//! Les routes : `highway=*` hors ponts et tunnels, un genre (grande route, route, voie de
//! service, voie piétonne, chemin, sentier) et une largeur (`width`, `lanes` ou selon le
//! genre) ; un point tous les 8 m, la hauteur de la chaussée prise au relief et lissée.
//!
//! Les bâtiments : `building=*` (chemins et relations, leurs contours extérieurs), un
//! genre (maison, immeuble, industriel, édifice, annexe) et une hauteur ; le pied au plus
//! bas du sol sous le contour (un mètre dessous), le toit à la hauteur au-dessus du sol
//! moyen. La requête se fait en neuf morceaux.
//!
//! L'eau : la Seine et les plans d'eau (`natural=water`, `waterway=riverbank`), leur
//! surface au niveau médian du relief sous elles, leur fond creusé, plus profond loin des
//! berges (jusqu'à 8 m). Un plan d'eau à un autre niveau (un étang sur le plateau) n'est
//! pas gardé : le jeu n'a qu'un niveau d'eau.
//!
//! Le fichier est dérivé d'OpenStreetMap : il reste sous ODbL (citer « © les
//! contributeurs OpenStreetMap »), et ne se met pas dans le dépôt.

use gda::json::{self, J};
use sage::campagne::DEPART;
use sage::reel::{cote, lambert93, origine, Batiment, Reel, Route};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Côté d'une image demandée à l'IGN, en points.
const MORCEAU: usize = 2000;
const OVERPASS: [&str; 2] = ["https://overpass-api.de/api/interpreter", "https://overpass.kumi.systems/api/interpreter"];

fn curl(args: &[&str], sortie: &Path) -> bool {
    let tmp = sortie.with_extension("part");
    let ok = Command::new("curl").args(["-s", "-f", "-m", "300", "-A", "sage-lieu-reel/1.0", "-o"]).arg(&tmp).args(args).status().is_ok_and(|s| s.success());
    ok && std::fs::rename(&tmp, sortie).is_ok()
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 5 {
        eprintln!("usage : lieu_reel <nom> <latitude> <longitude> <sortie.reel> [pas en m]");
        std::process::exit(1);
    }
    let (nom, lat, lon, sortie) = (&a[1], a[2].parse::<f64>().unwrap(), a[3].parse::<f64>().unwrap(), PathBuf::from(&a[4]));
    let pas: f64 = a.get(5).map_or(4.0, |p| p.parse().unwrap());
    let cache = PathBuf::from(format!("{}.cache", sortie.display()));
    std::fs::create_dir_all(&cache).unwrap();
    let n = cote(pas);
    // Le point demandé est au départ du jeu
    let (xc, yc) = lambert93(lat, lon);
    let (x0, y0) = (xc - DEPART[0] as f64, yc + DEPART[1] as f64);
    let lx = |a: usize| x0 + origine() + a as f64 * pas;
    let ly = |b: usize| y0 - (origine() + b as f64 * pas);
    println!("{nom} : Lambert-93 ({xc:.0}, {yc:.0}), {n} × {n} points de {pas} m");

    // 1. Le relief, par morceaux : chaque pixel est centré sur un point de la grille
    let mut h = vec![f32::NAN; n * n];
    for b0 in (0..n).step_by(MORCEAU) {
        for a0 in (0..n).step_by(MORCEAU) {
            let (w, ht) = (MORCEAU.min(n - a0), MORCEAU.min(n - b0));
            let (minx, maxy) = (lx(a0) - pas / 2.0, ly(b0) + pas / 2.0);
            let (maxx, miny) = (minx + w as f64 * pas, maxy - ht as f64 * pas);
            let f = cache.join(format!("ign_{a0}_{b0}_{pas}.bil"));
            if !f.exists() {
                let url = format!(
                    "https://data.geopf.fr/wms-r/wms?SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&LAYERS=ELEVATION.ELEVATIONGRIDCOVERAGE.HIGHRES&STYLES=&FORMAT=image/x-bil;bits=32&CRS=EPSG:2154&BBOX={minx},{miny},{maxx},{maxy}&WIDTH={w}&HEIGHT={ht}"
                );
                println!("relief IGN {a0}, {b0} ({w} × {ht})…");
                assert!(curl(&[&url], &f), "le relief de l'IGN ne répond pas");
            }
            let o = std::fs::read(&f).unwrap();
            assert_eq!(o.len(), w * ht * 4, "{} : taille inattendue", f.display());
            for r in 0..ht {
                for c in 0..w {
                    let k = (r * w + c) * 4;
                    let v = f32::from_le_bytes([o[k], o[k + 1], o[k + 2], o[k + 3]]);
                    h[(b0 + r) * n + a0 + c] = if v > -1000.0 { v } else { f32::NAN };
                }
            }
        }
    }
    // Les trous (hors de France, en mer) : le plus proche connu, ligne par ligne
    let manque = h.iter().filter(|v| v.is_nan()).count();
    if manque > 0 {
        println!("{manque} points sans relief, comblés");
        let connu = h.iter().cloned().find(|v| !v.is_nan()).unwrap_or(0.0);
        let mut der = connu;
        for v in h.iter_mut() {
            if v.is_nan() {
                *v = der;
            } else {
                der = *v;
            }
        }
    }

    // 2. L'eau d'OpenStreetMap, dans un cadre en degrés qui contient la carte
    let mut cadre = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    let (dlat, dlon) = (11_000.0 / 111_000.0, 11_000.0 / (111_000.0 * lat.to_radians().cos()));
    cadre = (cadre.0.min(lat - dlat), cadre.1.min(lon - dlon), cadre.2.max(lat + dlat), cadre.3.max(lon + dlon));
    let bb = format!("{:.5},{:.5},{:.5},{:.5}", cadre.0, cadre.1, cadre.2, cadre.3);
    let j = overpass(&cache.join("osm_eau.json"), "eau", &format!("(way[\"natural\"=\"water\"]({bb});relation[\"natural\"=\"water\"]({bb});way[\"waterway\"=\"riverbank\"]({bb});relation[\"waterway\"=\"riverbank\"]({bb}););out geom;"));
    // Chaque surface : ses anneaux, en points de grille (fractionnaires)
    let vers_grille = |p: &J| -> Option<(f64, f64)> {
        let (la, lo) = (p.get("lat")?.num()?, p.get("lon")?.num()?);
        let (x, y) = lambert93(la, lo);
        Some(((x - x0 - origine()) / pas, (y0 - y - origine()) / pas))
    };
    let ligne = |g: &J| -> Vec<(f64, f64)> { g.liste().map(|l| l.iter().filter_map(vers_grille).collect()).unwrap_or_default() };
    let mut surfaces: Vec<Vec<Vec<(f64, f64)>>> = Vec::new();
    for e in j.get("elements").and_then(J::liste).unwrap_or(&[]) {
        match e.get("type").and_then(J::txt) {
            Some("way") => {
                let l = ligne(e.get("geometry").unwrap_or(&J::Nul));
                if l.len() > 2 && l.first() == l.last() {
                    surfaces.push(vec![l]);
                }
            }
            Some("relation") => {
                let morceaux: Vec<Vec<(f64, f64)>> = e.get("members").and_then(J::liste).unwrap_or(&[]).iter().filter(|m| m.get("type").and_then(J::txt) == Some("way")).map(|m| ligne(m.get("geometry").unwrap_or(&J::Nul))).filter(|l| l.len() > 1).collect();
                surfaces.push(anneaux(morceaux));
            }
            _ => {}
        }
    }
    let mut eau = vec![false; n * n];
    for s in &surfaces {
        remplir(s, n, &mut eau);
    }
    let dans_eau: Vec<f32> = (0..n * n).filter(|&i| eau[i]).map(|i| h[i]).collect();
    println!("{} surfaces d'eau, {:.2} km²", surfaces.len(), dans_eau.len() as f64 * pas * pas / 1e6);

    // 3. Le niveau de l'eau, le fond creusé
    let niveau = if dans_eau.is_empty() {
        h.iter().cloned().fold(f32::MAX, f32::min) - 10.0
    } else {
        let mut v = dans_eau.clone();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };
    // Seules les eaux à ce niveau (la Seine et ce qui la touche) se gardent : un étang
    // plus haut serait creusé jusqu'à lui ; il reste de la terre
    let mut vu = vec![false; n * n];
    let mut ecartes = 0;
    for i in 0..n * n {
        if !eau[i] || vu[i] {
            continue;
        }
        let mut groupe = vec![i];
        vu[i] = true;
        let mut k = 0;
        while k < groupe.len() {
            let j = groupe[k];
            k += 1;
            let (a, b) = (j % n, j / n);
            for (da, db) in [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)] {
                let (x, y) = (a as i64 + da, b as i64 + db);
                if x < 0 || y < 0 || x >= n as i64 || y >= n as i64 {
                    continue;
                }
                let q = y as usize * n + x as usize;
                if eau[q] && !vu[q] {
                    vu[q] = true;
                    groupe.push(q);
                }
            }
        }
        let mut v: Vec<f32> = groupe.iter().map(|&j| h[j]).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        if (v[v.len() / 2] - niveau).abs() > 1.5 {
            for j in groupe {
                eau[j] = false;
            }
            ecartes += 1;
        }
    }
    println!("{ecartes} plans d'eau hors du niveau, laissés à sec");
    let berge = distance_berge(&eau, n);
    for i in 0..n * n {
        if eau[i] {
            let fond = niveau - (0.8 + berge[i] * pas as f32 * 0.08).min(8.0);
            h[i] = h[i].min(fond);
        }
    }
    println!("eau à {niveau:.2} m ; relief de {:.1} à {:.1} m", h.iter().cloned().fold(f32::MAX, f32::min), h.iter().cloned().fold(f32::MIN, f32::max));
    // 4. Les routes, hors ponts et tunnels, la chaussée lissée le long de la route
    let jr = overpass(&cache.join("osm_routes.json"), "routes", &format!("way[\"highway\"]({bb});out tags geom;"));
    let relief = |gx: f64, gz: f64| -> f32 {
        let (a, b) = (gx.clamp(0.0, (n - 1) as f64), gz.clamp(0.0, (n - 1) as f64));
        let (a0, b0) = ((a as usize).min(n - 2), (b as usize).min(n - 2));
        let (u, v) = ((a - a0 as f64) as f32, (b - b0 as f64) as f32);
        let p = |a: usize, b: usize| h[b * n + a];
        (p(a0, b0) * (1.0 - u) + p(a0 + 1, b0) * u) * (1.0 - v) + (p(a0, b0 + 1) * (1.0 - u) + p(a0 + 1, b0 + 1) * u) * v
    };
    let mut routes = Vec::new();
    let mut ponts = Vec::new();
    let mut longueur = 0.0;
    // Les ponts d'abord : (genre, largeur, ligne) ; leurs bouts comptés, un bout partagé
    // entre deux ponts n'est pas un vrai bout (le pont continue)
    let cle = |p: (f64, f64)| ((p.0 * 100.0).round() as i64, (p.1 * 100.0).round() as i64);
    let mut lignes_ponts = Vec::new();
    let mut bouts: std::collections::HashMap<(i64, i64), u32> = Default::default();
    for e in jr.get("elements").and_then(J::liste).unwrap_or(&[]) {
        let Some((genre, largeur, true)) = genre_route(e.get("tags")) else { continue };
        let l = ligne(e.get("geometry").unwrap_or(&J::Nul));
        if l.len() < 2 {
            continue;
        }
        *bouts.entry(cle(l[0])).or_default() += 1;
        *bouts.entry(cle(l[l.len() - 1])).or_default() += 1;
        lignes_ponts.push((genre, largeur, l));
    }
    // Les ponts mis bout à bout : chaque groupe et ses vrais bouts
    let mut groupe: Vec<usize> = (0..lignes_ponts.len()).collect();
    fn racine(g: &mut [usize], mut i: usize) -> usize {
        while g[i] != i {
            g[i] = g[g[i]];
            i = g[i];
        }
        i
    }
    let mut par_bout: std::collections::HashMap<(i64, i64), usize> = Default::default();
    for (k, (_, _, l)) in lignes_ponts.iter().enumerate() {
        for b in [l[0], l[l.len() - 1]] {
            if let Some(&autre) = par_bout.get(&cle(b)) {
                let (a, c) = (racine(&mut groupe, autre), racine(&mut groupe, k));
                groupe[a] = c;
            } else {
                par_bout.insert(cle(b), k);
            }
        }
    }
    let mut culees: std::collections::HashMap<usize, Vec<(f64, f64, f32)>> = Default::default();
    for (k, (_, _, l)) in lignes_ponts.iter().enumerate() {
        let g = racine(&mut groupe, k);
        for b in [l[0], l[l.len() - 1]] {
            if bouts[&cle(b)] == 1 {
                culees.entry(g).or_default().push((b.0, b.1, relief(b.0, b.1).max(niveau + 0.5)));
            }
        }
    }
    let dans = |p: &(f64, f64)| p.0 * pas > -20.0 && p.1 * pas > -20.0 && p.0 < (n - 1) as f64 + 20.0 / pas && p.1 < (n - 1) as f64 + 20.0 / pas;
    for (k, (genre, largeur, l)) in lignes_ponts.iter().enumerate() {
        let g = racine(&mut groupe, k);
        let c = culees.get(&g).cloned().unwrap_or_default();
        let pts = densifier(l, pas, 8.0);
        let mut morceau: Vec<[f32; 3]> = Vec::new();
        for p in &pts {
            // Le tablier : la moyenne des hauteurs des bouts, pondérée par l'inverse du carré
            // de la distance (sans bouts, le plus haut du sol sous le pont)
            let y = if c.is_empty() {
                l.iter().map(|q| relief(q.0, q.1)).fold(niveau + 0.5, f32::max)
            } else {
                let (mut s, mut w) = (0.0f64, 0.0f64);
                for &(bx, bz, bh) in &c {
                    let d2 = (bx - p.0).powi(2) + (bz - p.1).powi(2);
                    if d2 < 1e-9 {
                        (s, w) = (bh as f64, 1.0);
                        break;
                    }
                    s += bh as f64 / d2;
                    w += 1.0 / d2;
                }
                (s / w) as f32
            };
            if dans(p) {
                morceau.push([(origine() + p.0 * pas) as f32, (origine() + p.1 * pas) as f32, y]);
            } else if morceau.len() > 1 {
                ponts.push(Route { genre: *genre, largeur: *largeur, points: std::mem::take(&mut morceau) });
            } else {
                morceau.clear();
            }
        }
        if morceau.len() > 1 {
            ponts.push(Route { genre: *genre, largeur: *largeur, points: morceau });
        }
    }
    println!("{} ponts OSM, {} tabliers", lignes_ponts.len(), ponts.len());
    for e in jr.get("elements").and_then(J::liste).unwrap_or(&[]) {
        let Some((genre, largeur, false)) = genre_route(e.get("tags")) else { continue };
        let l = ligne(e.get("geometry").unwrap_or(&J::Nul));
        if l.len() < 2 {
            continue;
        }
        // Un point tous les 8 m au plus, la hauteur prise au relief
        let pts = densifier(&l, pas, 8.0);
        longueur += l.windows(2).map(|s| (s[1].0 - s[0].0).hypot(s[1].1 - s[0].1) * pas).sum::<f64>();
        let hs: Vec<f32> = pts.iter().map(|p| relief(p.0, p.1)).collect();
        // Moyenne glissante sur cinq points : une chaussée sans bosses
        let lisse: Vec<f32> = (0..hs.len()).map(|i| {
            let (a, b) = (i.saturating_sub(2), (i + 3).min(hs.len()));
            hs[a..b].iter().sum::<f32>() / (b - a) as f32
        }).collect();
        // Hors de la carte, rien : la route se coupe à son bord (avec 20 m de marge)
        let mut morceau: Vec<[f32; 3]> = Vec::new();
        for (p, y) in pts.iter().zip(&lisse) {
            if dans(p) {
                morceau.push([(origine() + p.0 * pas) as f32, (origine() + p.1 * pas) as f32, *y]);
            } else if morceau.len() > 1 {
                routes.push(Route { genre, largeur, points: std::mem::take(&mut morceau) });
            } else {
                morceau.clear();
            }
        }
        if morceau.len() > 1 {
            routes.push(Route { genre, largeur, points: morceau });
        }
    }
    println!("{} routes, {:.0} km", routes.len(), longueur / 1000.0);
    // 5. Les bâtiments, en neuf morceaux (une réponse entière serait trop lourde à lire)
    let mut batiments = Vec::new();
    let mut vus = std::collections::HashSet::new();
    for (bi, bj) in (0..3).flat_map(|j| (0..3).map(move |i| (i, j))) {
        let (la0, lo0) = (cadre.0 + (cadre.2 - cadre.0) * bj as f64 / 3.0, cadre.1 + (cadre.3 - cadre.1) * bi as f64 / 3.0);
        let (la1, lo1) = (cadre.0 + (cadre.2 - cadre.0) * (bj + 1) as f64 / 3.0, cadre.1 + (cadre.3 - cadre.1) * (bi + 1) as f64 / 3.0);
        let b = format!("{la0:.5},{lo0:.5},{la1:.5},{lo1:.5}");
        let jb = overpass(&cache.join(format!("osm_bati_{bi}_{bj}.json")), "bâtiments", &format!("(way[\"building\"]({b});relation[\"building\"]({b}););out tags geom;"));
        for e in jb.get("elements").and_then(J::liste).unwrap_or(&[]) {
            let id = (e.get("type").and_then(J::txt).unwrap_or("").to_string(), e.get("id").and_then(J::num).unwrap_or(0.0) as i64);
            if !vus.insert(id) {
                continue;
            }
            let Some((genre, hauteur)) = genre_bati(e.get("tags")) else { continue };
            // Une relation : chacun de ses contours extérieurs (les cours intérieures se
            // couvrent)
            let contours: Vec<Vec<(f64, f64)>> = match e.get("type").and_then(J::txt) {
                Some("way") => vec![ligne(e.get("geometry").unwrap_or(&J::Nul))],
                _ => anneaux(e.get("members").and_then(J::liste).unwrap_or(&[]).iter().filter(|m| m.get("role").and_then(J::txt) == Some("outer")).map(|m| ligne(m.get("geometry").unwrap_or(&J::Nul))).filter(|l| l.len() > 1).collect()),
            };
            for mut c in contours {
                if c.first() == c.last() {
                    c.pop();
                }
                c.dedup();
                if c.len() < 3 || !c.iter().all(|p| p.0 > 0.0 && p.1 > 0.0 && p.0 < (n - 1) as f64 && p.1 < (n - 1) as f64) {
                    continue;
                }
                let sols: Vec<f32> = c.iter().map(|p| relief(p.0, p.1)).collect();
                let (bas, haut) = (sols.iter().cloned().fold(f32::MAX, f32::min), sols.iter().cloned().fold(f32::MIN, f32::max));
                let moyen = sols.iter().sum::<f32>() / sols.len() as f32;
                let contour = c.iter().map(|p| [(origine() + p.0 * pas) as f32, (origine() + p.1 * pas) as f32]).collect();
                batiments.push(Batiment { genre, pied: bas - 1.0, toit: (moyen + hauteur).max(haut + 2.0), contour });
            }
        }
    }
    println!("{} bâtiments", batiments.len());
    let r = Reel::new(nom, lat, lon, x0, y0, pas, niveau, &h, &eau, routes, batiments, ponts);
    std::fs::write(&sortie, r.octets()).unwrap();
    println!("{} : {} Mo", sortie.display(), std::fs::metadata(&sortie).unwrap().len() / 1_000_000);
}

/// Une requête Overpass (sa réponse gardée dans `f`), essayée sur plusieurs serveurs,
/// plusieurs fois s'ils sont occupés.
fn overpass(f: &Path, quoi: &str, requete: &str) -> J {
    if !f.exists() {
        let q = format!("data=[out:json][timeout:300];{requete}");
        let mut ok = false;
        'essais: for essai in 0..5 {
            for url in OVERPASS {
                println!("{quoi} OSM ({url})…");
                if curl(&["--data-urlencode", &q, url], f) && std::fs::read(f).is_ok_and(|o| o.first() == Some(&b'{')) {
                    ok = true;
                    break 'essais;
                }
            }
            let attente = 15 << essai;
            println!("Overpass occupé, nouvel essai dans {attente} s");
            std::thread::sleep(std::time::Duration::from_secs(attente));
        }
        assert!(ok, "Overpass ne répond pas : réessayez plus tard");
    }
    json::lire(&std::fs::read_to_string(f).unwrap()).expect("réponse d'Overpass illisible")
}

/// Une ligne (en pas de grille) avec un point tous les `m` mètres au plus.
fn densifier(l: &[(f64, f64)], pas: f64, m: f64) -> Vec<(f64, f64)> {
    let mut pts = vec![l[0]];
    for s in l.windows(2) {
        let d = (s[1].0 - s[0].0).hypot(s[1].1 - s[0].1) * pas;
        let k = (d / m).ceil().max(1.0) as usize;
        for i in 1..=k {
            let t = i as f64 / k as f64;
            pts.push((s[0].0 + (s[1].0 - s[0].0) * t, s[0].1 + (s[1].1 - s[0].1) * t));
        }
    }
    pts
}

/// Le genre et la largeur (m) d'une route d'après ses étiquettes OSM, et si c'est un pont ;
/// rien pour ce qui n'est pas une voie (tunnels, escaliers, projets, surfaces…).
fn genre_route(tags: Option<&J>) -> Option<(u8, f32, bool)> {
    use sage::reel::{CHEMIN, GRANDE, PIETONNE, ROUTE, SENTIER, SERVICE};
    let tag = |k: &str| tags.and_then(|t| t.get(k)).and_then(J::txt);
    let pont = tag("bridge").is_some_and(|v| v != "no");
    if tag("tunnel").is_some_and(|v| v != "no") || tag("area") == Some("yes") || tag("indoor").is_some_and(|v| v != "no") {
        return None;
    }
    let (genre, largeur) = match tag("highway")? {
        "motorway" => (GRANDE, 15.0),
        "trunk" => (GRANDE, 12.0),
        "primary" => (GRANDE, 10.0),
        "secondary" => (GRANDE, 8.0),
        "motorway_link" | "trunk_link" | "primary_link" | "secondary_link" => (GRANDE, 7.0),
        "tertiary" | "tertiary_link" => (ROUTE, 7.0),
        "residential" | "road" => (ROUTE, 6.0),
        "unclassified" => (ROUTE, 5.5),
        "living_street" => (ROUTE, 5.0),
        "service" => (SERVICE, 4.0),
        "pedestrian" => (PIETONNE, 6.0),
        "cycleway" => (PIETONNE, 2.5),
        "footway" => (PIETONNE, 2.0),
        "track" => (CHEMIN, 3.0),
        "path" | "bridleway" => (SENTIER, 1.5),
        _ => return None,
    };
    // Une chaussée non revêtue est un chemin ; un sentier revêtu, une voie piétonne
    let terre = tag("surface").is_some_and(|s| ["unpaved", "gravel", "fine_gravel", "dirt", "earth", "ground", "grass", "compacted", "mud", "sand"].contains(&s));
    let genre = match (genre, terre) {
        (ROUTE | SERVICE | PIETONNE, true) => CHEMIN,
        (SENTIER, false) if tag("surface").is_some() => PIETONNE,
        (g, _) => g,
    };
    let largeur = tag("width").and_then(|w| w.trim_end_matches(" m").trim_end_matches('m').trim().replace(',', ".").parse::<f32>().ok()).filter(|w| (0.5..60.0).contains(w)).or_else(|| tag("lanes").and_then(|l| l.parse::<f32>().ok()).filter(|l| (1.0..12.0).contains(l)).map(|l| l * 3.25 + 1.0)).unwrap_or(largeur);
    Some((genre, largeur, pont))
}

/// Le genre et la hauteur (m) d'un bâtiment d'après ses étiquettes OSM : `height`, sinon
/// ses étages (`building:levels`, 3 m chacun, plus les étages du toit), sinon selon son
/// genre. Rien pour un toit seul (un auvent) ou ce qui n'est pas un bâtiment.
fn genre_bati(tags: Option<&J>) -> Option<(u8, f32)> {
    use sage::reel::{ANNEXE, EDIFICE, IMMEUBLE, INDUSTRIEL, MAISON};
    let tag = |k: &str| tags.and_then(|t| t.get(k)).and_then(J::txt);
    let b = tag("building")?;
    if ["no", "roof", "canopy", "carport", "construction", "proposed", "demolished", "ruins"].contains(&b) {
        return None;
    }
    let (genre, defaut) = match b {
        "house" | "detached" | "semidetached_house" | "terrace" | "bungalow" | "farm" | "residential" => (MAISON, 7.0),
        "apartments" | "dormitory" | "hotel" => (IMMEUBLE, 16.0),
        "industrial" | "warehouse" | "retail" | "commercial" | "supermarket" | "office" | "manufacture" | "hangar" | "barn" | "farm_auxiliary" | "greenhouse" | "service" | "transportation" | "train_station" => (INDUSTRIEL, 8.0),
        "church" | "cathedral" | "chapel" | "religious" | "public" | "civic" | "government" | "school" | "university" | "college" | "hospital" | "museum" | "townhall" | "castle" | "sports_hall" | "stadium" => (EDIFICE, 14.0),
        "garage" | "garages" | "shed" | "hut" | "kiosk" | "toilets" | "cabin" | "storage_tank" | "bunker" => (ANNEXE, 3.0),
        _ => (IMMEUBLE, 9.0),
    };
    let nombre = |k: &str| tag(k).and_then(|v| v.trim_end_matches(" m").trim_end_matches('m').trim().replace(',', ".").parse::<f32>().ok());
    let h = nombre("height")
        .filter(|h| (1.0..400.0).contains(h))
        .or_else(|| nombre("building:levels").filter(|l| (0.5..100.0).contains(l)).map(|l| l * 3.0 + nombre("roof:levels").unwrap_or(0.0).clamp(0.0, 4.0) * 2.0 + 1.0))
        .unwrap_or(defaut);
    Some((genre, h))
}

/// Les morceaux d'une relation mis bout à bout en anneaux fermés.
fn anneaux(mut morceaux: Vec<Vec<(f64, f64)>>) -> Vec<Vec<(f64, f64)>> {
    let mut faits = Vec::new();
    while let Some(mut a) = morceaux.pop() {
        loop {
            if a.first() == a.last() && a.len() > 2 {
                break;
            }
            let fin = *a.last().unwrap();
            let Some(k) = morceaux.iter().position(|m| m.first() == Some(&fin) || m.last() == Some(&fin)) else { break };
            let mut m = morceaux.swap_remove(k);
            if m.first() != Some(&fin) {
                m.reverse();
            }
            a.extend(m.into_iter().skip(1));
        }
        // Un anneau resté ouvert (coupé par le cadre) se ferme tout droit
        if a.first() != a.last() {
            a.push(a[0]);
        }
        faits.push(a);
    }
    faits
}

/// Remplit, pair-impair, les points de grille dans les anneaux d'une surface (ses trous
/// restent vides).
fn remplir(anneaux: &[Vec<(f64, f64)>], n: usize, eau: &mut [bool]) {
    let (mut b0, mut b1) = (f64::MAX, f64::MIN);
    for r in anneaux {
        for p in r {
            b0 = b0.min(p.1);
            b1 = b1.max(p.1);
        }
    }
    let (b0, b1) = (b0.ceil().max(0.0) as usize, (b1.floor().min(n as f64 - 1.0)).max(-1.0) as i64);
    for b in b0 as i64..=b1 {
        let y = b as f64;
        let mut x: Vec<f64> = Vec::new();
        for r in anneaux {
            for s in r.windows(2) {
                let ((xa, ya), (xb, yb)) = (s[0], s[1]);
                if (ya <= y) != (yb <= y) {
                    x.push(xa + (y - ya) / (yb - ya) * (xb - xa));
                }
            }
        }
        x.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for p in x.chunks(2) {
            if p.len() < 2 {
                continue;
            }
            let (a0, a1) = (p[0].ceil().max(0.0) as i64, p[1].floor().min(n as f64 - 1.0) as i64);
            for a in a0..=a1 {
                eau[b as usize * n + a as usize] = true;
            }
        }
    }
}

/// Pour chaque point d'eau, sa distance à la berge, en points (chanfrein 3-4).
fn distance_berge(eau: &[bool], n: usize) -> Vec<f32> {
    let mut d: Vec<f32> = eau.iter().map(|e| if *e { f32::MAX } else { 0.0 }).collect();
    let (dr, dg) = (1.0, std::f32::consts::SQRT_2);
    for b in 0..n {
        for a in 0..n {
            let i = b * n + a;
            if d[i] == 0.0 {
                continue;
            }
            let mut m = d[i];
            if a > 0 { m = m.min(d[i - 1] + dr); }
            if b > 0 {
                m = m.min(d[i - n] + dr);
                if a > 0 { m = m.min(d[i - n - 1] + dg); }
                if a + 1 < n { m = m.min(d[i - n + 1] + dg); }
            }
            d[i] = m;
        }
    }
    for b in (0..n).rev() {
        for a in (0..n).rev() {
            let i = b * n + a;
            if d[i] == 0.0 {
                continue;
            }
            let mut m = d[i];
            if a + 1 < n { m = m.min(d[i + 1] + dr); }
            if b + 1 < n {
                m = m.min(d[i + n] + dr);
                if a + 1 < n { m = m.min(d[i + n + 1] + dg); }
                if a > 0 { m = m.min(d[i + n - 1] + dg); }
            }
            d[i] = m;
        }
    }
    d.iter().map(|v| v.min(1e4)).collect()
}
