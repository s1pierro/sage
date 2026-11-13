//! La génération d'un lieu réel : 20 km autour d'un point de France, d'après le relief
//! de l'IGN (RGE ALTI, Géoplateforme), le MNH LiDAR HD pour les toits et OpenStreetMap
//! (Overpass) pour l'eau, les routes, les ponts, les bâtiments et l'occupation du sol.
//!
//! Le réseau est fourni par l'appelant (`Reseau` : curl pour l'outil de S.A.G.E., Java
//! pour Flow) ; l'avancement et le journal se lisent dans `Suivi`, qui peut aussi
//! arrêter le travail. Les téléchargements sont gardés dans `<sortie>.cache/` : relancer
//! ne les refait pas.
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
//! Les toits : le MNH LiDAR HD de l'IGN (la hauteur au-dessus du sol, à 50 cm), par blocs
//! de 250 m là où il y a des bâtiments (4 téléchargements à la fois). Dans chaque
//! contour, les mesures à plus de 60 cm des murs et à plus de 1,5 m du sol, le relevé
//! recalé sur le contour (jusqu'à 2 m), donnent la forme du toit (plat, deux pans,
//! croupe, appentis), son axe, l'égout et le faîtage (`toits::ajuster`). Un bâtiment
//! sans mesure (neuf, ou hors des blocs) garde la hauteur d'OSM, avec deux pans à 40°
//! sur les maisons.
//!
//! L'occupation du sol : `landuse=*`, les bois, prés et landes (`natural=*`), les parcs
//! et terrains (`leisure=*`), en cinq classes, tramées sur une grille de `2 × pas` ; là
//! où elles se recouvrent, la plus précise l'emporte. En neuf morceaux.
//!
//! L'eau : les surfaces d'eau (`natural=water`, `waterway=riverbank`), leur surface au
//! niveau médian du relief sous elles, leur fond creusé, plus profond loin des berges
//! (jusqu'à 8 m). Un plan d'eau à un autre niveau n'est pas gardé : il n'y a qu'un
//! niveau d'eau.

use crate::json::{self, J};
use crate::{cote_carte, lambert93, toits, Batiment, Grille, Lieu, Route, DEPART};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Côté d'une image demandée à l'IGN, en points.
const MORCEAU: usize = 2000;
const OVERPASS: [&str; 2] = ["https://overpass-api.de/api/interpreter", "https://overpass.kumi.systems/api/interpreter"];

/// Le réseau : télécharge `url` dans le fichier `sortie` (en entier ou pas du tout), par
/// GET, ou par POST d'un formulaire déjà encodé (`post`). Appelé depuis plusieurs
/// threads à la fois.
pub trait Reseau: Sync {
    fn telecharger(&self, url: &str, post: Option<&str>, sortie: &Path) -> Result<(), String>;
}

/// Le réseau par curl (l'outil de S.A.G.E., les essais).
pub struct Curl;

impl Reseau for Curl {
    fn telecharger(&self, url: &str, post: Option<&str>, sortie: &Path) -> Result<(), String> {
        let tmp = sortie.with_extension("part");
        let mut c = std::process::Command::new("curl");
        c.args(["-s", "-f", "-m", "300", "-A", "sage-lieu-reel/1.0", "-o"]).arg(&tmp);
        if let Some(p) = post {
            c.args(["--data-binary", p, "-H", "Content-Type: application/x-www-form-urlencoded"]);
        }
        let ok = c.arg(url).status().map_err(|e| e.to_string())?.success();
        if !ok {
            let _ = std::fs::remove_file(&tmp);
            return Err("curl a échoué".into());
        }
        std::fs::rename(&tmp, sortie).map_err(|e| e.to_string())
    }
}

/// Encode un texte pour un formulaire (application/x-www-form-urlencoded).
pub fn encoder(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => o.push(b as char),
            b' ' => o.push('+'),
            _ => o.push_str(&format!("%{b:02X}")),
        }
    }
    o
}

/// Ce qu'on demande : un nom, le centre (en degrés), le pas de la grille (4 m
/// d'ordinaire), le fichier à écrire, et si l'on mesure les toits au LiDAR.
#[derive(Clone, Debug, PartialEq)]
pub struct Demande {
    pub nom: String,
    pub lat: f64,
    pub lon: f64,
    pub pas: f64,
    pub sortie: PathBuf,
    pub toits: bool,
    /// L'emprise, en mètres : est-ouest et nord-sud, centrée sur le point ; 0 (ou plus
    /// que la carte) pour la carte entière. Le lieu ne couvre qu'elle (`Grille::emprise`).
    pub largeur: f64,
    pub longueur: f64,
}

/// L'avancement d'une génération, lu par un autre thread : l'étape, sa part du tout (de 0
/// à 1), le journal ; `arreter` l'interrompt (au prochain point d'arrêt).
#[derive(Default)]
pub struct Suivi {
    arret: AtomicBool,
    etat: Mutex<(String, f32)>,
    journal: Mutex<Vec<String>>,
    /// Imprimer aussi le journal (l'outil en ligne de commande).
    pub imprimer: bool,
}

impl Suivi {
    pub fn noter(&self, ligne: &str) {
        if self.imprimer {
            println!("{ligne}");
        }
        if let Ok(mut j) = self.journal.lock() {
            j.push(ligne.to_string());
        }
    }

    pub fn etape(&self, quoi: &str, part: f32) {
        if let Ok(mut e) = self.etat.lock() {
            *e = (quoi.to_string(), part.clamp(0.0, 1.0));
        }
    }

    /// L'étape et l'avancement.
    pub fn etat(&self) -> (String, f32) {
        self.etat.lock().map(|e| e.clone()).unwrap_or_default()
    }

    /// Les lignes du journal à partir de la `depuis`-ième.
    pub fn journal(&self, depuis: usize) -> Vec<String> {
        self.journal.lock().map(|j| j.get(depuis..).unwrap_or(&[]).to_vec()).unwrap_or_default()
    }

    pub fn arreter(&self) {
        self.arret.store(true, Ordering::Relaxed);
    }

    pub fn arrete(&self) -> bool {
        self.arret.load(Ordering::Relaxed)
    }

    /// Une erreur si l'on a demandé l'arrêt.
    fn suivre(&self) -> Result<(), String> {
        if self.arrete() {
            Err("arrêté".into())
        } else {
            Ok(())
        }
    }
}

/// Le dossier des téléchargements d'une sortie.
pub fn cache(sortie: &Path) -> PathBuf {
    PathBuf::from(format!("{}.cache", sortie.display()))
}

/// Prépare le lieu demandé, l'écrit dans `d.sortie` et le rend.
pub fn generer(d: &Demande, r: &dyn Reseau, s: &Suivi) -> Result<Lieu, String> {
    let (nom, lat, lon, pas) = (d.nom.as_str(), d.lat, d.lon, d.pas);
    if !(0.5..=64.0).contains(&pas) {
        return Err(format!("pas de {pas} m : entre 0,5 et 64"));
    }
    let cache = cache(&d.sortie);
    std::fs::create_dir_all(&cache).map_err(|e| format!("{} : {e}", cache.display()))?;
    let g = Grille::emprise(pas, d.largeur, d.longueur);
    let entiere = g.est_entiere(pas);
    // La grille du lieu : nx × nz points, le premier en (ox, oz) au repère du jeu
    let (nx, nz) = (g.nx, g.nz);
    let [ox, oz, _, _] = g.etendue(pas);
    // Les réponses d'OSM dépendent de l'emprise : gardées sous un autre nom
    let suffixe = if entiere { String::new() } else { format!("_{:.0}x{:.0}", d.largeur.min(cote_carte()), d.longueur.min(cote_carte())) };
    // Le point demandé est au départ du jeu
    let (xc, yc) = lambert93(lat, lon);
    let (x0, y0) = (xc - DEPART[0] as f64, yc + DEPART[1] as f64);
    let lx = |a: usize| x0 + ox + a as f64 * pas;
    let ly = |b: usize| y0 - (oz + b as f64 * pas);
    s.noter(&format!("{nom} : Lambert-93 ({xc:.0}, {yc:.0}), {nx} × {nz} points de {pas} m ({:.1} × {:.1} km)", (nx - 1) as f64 * pas / 1000.0, (nz - 1) as f64 * pas / 1000.0));

    s.etape("relief de l'IGN", 0.0);
    // 1. Le relief, par morceaux : chaque pixel est centré sur un point de la grille
    let mut h = vec![f32::NAN; nx * nz];
    for b0 in (0..nz).step_by(MORCEAU) {
        for a0 in (0..nx).step_by(MORCEAU) {
            s.suivre()?;
            s.etape("relief de l'IGN", 0.15 * (b0 * nx + a0) as f32 / (nx * nz) as f32);
            let (w, ht) = (MORCEAU.min(nx - a0), MORCEAU.min(nz - b0));
            let (minx, maxy) = (lx(a0) - pas / 2.0, ly(b0) + pas / 2.0);
            let (maxx, miny) = (minx + w as f64 * pas, maxy - ht as f64 * pas);
            // Nommé par sa place dans la carte (et sa taille, pour une emprise)
            let f = cache.join(if entiere { format!("ign_{a0}_{b0}_{pas}.bil") } else { format!("ign_{}_{}_{w}x{ht}_{pas}.bil", g.a0 + a0, g.b0 + b0) });
            if !f.exists() {
                let url = format!(
                    "https://data.geopf.fr/wms-r/wms?SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&LAYERS=ELEVATION.ELEVATIONGRIDCOVERAGE.HIGHRES&STYLES=&FORMAT=image/x-bil;bits=32&CRS=EPSG:2154&BBOX={minx},{miny},{maxx},{maxy}&WIDTH={w}&HEIGHT={ht}"
                );
                s.noter(&format!("relief IGN {a0}, {b0} ({w} × {ht})…"));
                r.telecharger(&url, None, &f).map_err(|e| format!("le relief de l'IGN ne répond pas : {e}"))?;
            }
            let o = std::fs::read(&f).map_err(|e| e.to_string())?;
            if o.len() != w * ht * 4 {
                let _ = std::fs::remove_file(&f);
                return Err(format!("relief de l'IGN : {} octets au lieu de {}", o.len(), w * ht * 4));
            }
            for r in 0..ht {
                for c in 0..w {
                    let k = (r * w + c) * 4;
                    let v = f32::from_le_bytes([o[k], o[k + 1], o[k + 2], o[k + 3]]);
                    h[(b0 + r) * nx + a0 + c] = if v > -1000.0 { v } else { f32::NAN };
                }
            }
        }
    }
    // Les trous (hors de France, en mer) : le plus proche connu, ligne par ligne
    let manque = h.iter().filter(|v| v.is_nan()).count();
    if manque > 0 {
        s.noter(&format!("{manque} points sans relief, comblés"));
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

    s.etape("eau", 0.15);
    // 2. L'eau d'OpenStreetMap, dans un cadre en degrés qui contient la carte
    let mut cadre = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    // Un kilomètre de marge autour de l'emprise
    let demi = |n: usize| ((n - 1) as f64 * pas / 2.0 + 1000.0).min(11_000.0);
    let (dlat, dlon) = (demi(nz) / 111_000.0, demi(nx) / (111_000.0 * lat.to_radians().cos()));
    cadre = (cadre.0.min(lat - dlat), cadre.1.min(lon - dlon), cadre.2.max(lat + dlat), cadre.3.max(lon + dlon));
    let bb = format!("{:.5},{:.5},{:.5},{:.5}", cadre.0, cadre.1, cadre.2, cadre.3);
    let j = overpass(r, s, &cache.join(format!("osm_eau{suffixe}.json")), "eau", &format!("(way[\"natural\"=\"water\"]({bb});relation[\"natural\"=\"water\"]({bb});way[\"waterway\"=\"riverbank\"]({bb});relation[\"waterway\"=\"riverbank\"]({bb}););out geom;"))?;
    // Chaque surface : ses anneaux, en points de grille (fractionnaires)
    let vers_grille = |p: &J| -> Option<(f64, f64)> {
        let (la, lo) = (p.get("lat")?.num()?, p.get("lon")?.num()?);
        let (x, y) = lambert93(la, lo);
        Some(((x - x0 - ox) / pas, (y0 - y - oz) / pas))
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
    let mut eau = vec![false; nx * nz];
    for s in &surfaces {
        remplir(s, nx, nz, &mut eau);
    }
    let dans_eau: Vec<f32> = (0..nx * nz).filter(|&i| eau[i]).map(|i| h[i]).collect();
    s.noter(&format!("{} surfaces d'eau, {:.2} km²", surfaces.len(), dans_eau.len() as f64 * pas * pas / 1e6));

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
    let mut vu = vec![false; nx * nz];
    let mut ecartes = 0;
    for i in 0..nx * nz {
        if !eau[i] || vu[i] {
            continue;
        }
        let mut groupe = vec![i];
        vu[i] = true;
        let mut k = 0;
        while k < groupe.len() {
            let j = groupe[k];
            k += 1;
            let (a, b) = (j % nx, j / nx);
            for (da, db) in [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)] {
                let (x, y) = (a as i64 + da, b as i64 + db);
                if x < 0 || y < 0 || x >= nx as i64 || y >= nz as i64 {
                    continue;
                }
                let q = y as usize * nx + x as usize;
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
    s.noter(&format!("{ecartes} plans d'eau hors du niveau, laissés à sec"));
    let berge = distance_berge(&eau, nx, nz);
    for i in 0..nx * nz {
        if eau[i] {
            let fond = niveau - (0.8 + berge[i] * pas as f32 * 0.08).min(8.0);
            h[i] = h[i].min(fond);
        }
    }
    s.noter(&format!("eau à {niveau:.2} m ; relief de {:.1} à {:.1} m", h.iter().cloned().fold(f32::MAX, f32::min), h.iter().cloned().fold(f32::MIN, f32::max)));
    s.suivre()?;
    s.etape("routes et ponts", 0.22);
    // 4. Les routes, hors ponts et tunnels, la chaussée lissée le long de la route
    let jr = overpass(r, s, &cache.join(format!("osm_routes{suffixe}.json")), "routes", &format!("way[\"highway\"]({bb});out tags geom;"))?;
    let relief = |gx: f64, gz: f64| -> f32 {
        let (a, b) = (gx.clamp(0.0, (nx - 1) as f64), gz.clamp(0.0, (nz - 1) as f64));
        let (a0, b0) = ((a as usize).min(nx - 2), (b as usize).min(nz - 2));
        let (u, v) = ((a - a0 as f64) as f32, (b - b0 as f64) as f32);
        let p = |a: usize, b: usize| h[b * nx + a];
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
    let marge = 20.0 / pas;
    let dans = |p: &(f64, f64)| p.0 > -marge && p.1 > -marge && p.0 < (nx - 1) as f64 + marge && p.1 < (nz - 1) as f64 + marge;
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
                morceau.push([(ox + p.0 * pas) as f32, (oz + p.1 * pas) as f32, y]);
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
    s.noter(&format!("{} ponts OSM, {} tabliers", lignes_ponts.len(), ponts.len()));
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
                morceau.push([(ox + p.0 * pas) as f32, (oz + p.1 * pas) as f32, *y]);
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
    s.noter(&format!("{} routes, {:.0} km", routes.len(), longueur / 1000.0));
    s.etape("bâtiments", 0.3);
    // 5. Les bâtiments, en neuf morceaux (une réponse entière serait trop lourde à lire)
    let mut batiments = Vec::new();
    let mut sols_bati: Vec<(f32, f32)> = Vec::new();
    let mut vus = std::collections::HashSet::new();
    for (bi, bj) in (0..3).flat_map(|j| (0..3).map(move |i| (i, j))) {
        s.suivre()?;
        let (la0, lo0) = (cadre.0 + (cadre.2 - cadre.0) * bj as f64 / 3.0, cadre.1 + (cadre.3 - cadre.1) * bi as f64 / 3.0);
        let (la1, lo1) = (cadre.0 + (cadre.2 - cadre.0) * (bj + 1) as f64 / 3.0, cadre.1 + (cadre.3 - cadre.1) * (bi + 1) as f64 / 3.0);
        let b = format!("{la0:.5},{lo0:.5},{la1:.5},{lo1:.5}");
        let jb = overpass(r, s, &cache.join(format!("osm_bati_{bi}_{bj}{suffixe}.json")), "bâtiments", &format!("(way[\"building\"]({b});relation[\"building\"]({b}););out tags geom;"))?;
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
                if c.len() < 3 || !c.iter().all(|p| p.0 > 0.0 && p.1 > 0.0 && p.0 < (nx - 1) as f64 && p.1 < (nz - 1) as f64) {
                    continue;
                }
                let sols: Vec<f32> = c.iter().map(|p| relief(p.0, p.1)).collect();
                let (bas, haut) = (sols.iter().cloned().fold(f32::MAX, f32::min), sols.iter().cloned().fold(f32::MIN, f32::max));
                let moyen = sols.iter().sum::<f32>() / sols.len() as f32;
                let contour = c.iter().map(|p| [(ox + p.0 * pas) as f32, (oz + p.1 * pas) as f32]).collect();
                batiments.push(par_defaut(genre, bas - 1.0, (moyen + hauteur).max(haut + 2.0), haut, contour));
                sols_bati.push((moyen, haut));
            }
        }
    }
    s.noter(&format!("{} bâtiments", batiments.len()));
    // 5 bis. Les toits, d'après le MNH LiDAR HD
    let dossier = cache.join("mnh");
    std::fs::create_dir_all(&dossier).map_err(|e| e.to_string())?;
    let mut blocs = std::collections::BTreeSet::new();
    for b in &batiments {
        let (mut x0b, mut z0b, mut x1b, mut z1b) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in &b.contour {
            (x0b, z0b, x1b, z1b) = (x0b.min(p[0]), z0b.min(p[1]), x1b.max(p[0]), z1b.max(p[1]));
        }
        for j in (z0b as f64 / BLOC).floor() as i32..=(z1b as f64 / BLOC).floor() as i32 {
            for i in (x0b as f64 / BLOC).floor() as i32..=(x1b as f64 / BLOC).floor() as i32 {
                blocs.insert((i, j));
            }
        }
    }
    if d.toits {
        s.etape("toits : téléchargement du LiDAR", 0.45);
        telecharger_mnh(r, s, &dossier, &blocs.iter().cloned().collect::<Vec<_>>(), x0, y0)?;
    }
    let mut mnh = Mnh { dossier: dossier.clone(), blocs: Default::default(), ordre: Default::default() };
    // Sans le LiDAR, les toits gardent leur forme par défaut
    let mut ordre: Vec<usize> = if d.toits { (0..batiments.len()).collect() } else { Vec::new() };
    ordre.sort_by_key(|&k| {
        let c = batiments[k].centre();
        ((c[1] as f64 / BLOC).floor() as i32, (c[0] as f64 / BLOC).floor() as i32)
    });
    let mut formes = [0usize; 5];
    for (fait, &k) in ordre.iter().enumerate() {
        if fait % 2000 == 0 {
            s.suivre()?;
            s.etape("toits : ajustement", 0.85 + 0.08 * fait as f32 / ordre.len() as f32);
            if fait % 20000 == 0 {
                s.noter(&format!("toits : {fait}/{}", ordre.len()));
            }
        }
        let b = &batiments[k];
        let (moyen, haut) = sols_bati[k];
        let c = &b.contour;
        let (mut x0b, mut z0b, mut x1b, mut z1b) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in c {
            (x0b, z0b, x1b, z1b) = (x0b.min(p[0]), z0b.min(p[1]), x1b.max(p[0]), z1b.max(p[1]));
        }
        // Le recalage : le contour d'OSM est souvent décalé d'un ou deux mètres du relevé.
        // Le décalage (jusqu'à 2 m, par pas de 50 cm) qui met le plus de points bâtis
        // (au moins 1,5 m) dans le contour, à 1 m près ; à égalité, le plus petit
        let mut dedans = Vec::new();
        let mut z = z0b + 0.5;
        while z < z1b {
            let mut x = x0b + 0.5;
            while x < x1b {
                if dans_contour(c, x, z, 0.6) {
                    dedans.push((x, z));
                }
                x += 1.0;
            }
            z += 1.0;
        }
        let mut decalage = (0.0f32, 0.0f32);
        if !dedans.is_empty() {
            let mut mieux = (-1.0f32, 0.0f32);
            for dz in -4..=4 {
                for dx in -4..=4 {
                    let (ddx, ddz) = (dx as f32 * 0.5, dz as f32 * 0.5);
                    let bati = dedans.iter().filter(|(x, z)| mnh.lire(x + ddx, z + ddz).is_some_and(|h| h >= 1.5)).count() as f32 / dedans.len() as f32;
                    let d = ddx.hypot(ddz);
                    let note = bati - 0.01 * d;
                    if note > mieux.0 + 1e-6 || ((note - mieux.0).abs() <= 1e-6 && d < mieux.1) {
                        mieux = (note, d);
                        decalage = (ddx, ddz);
                    }
                }
            }
        }
        // Les mesures, aux centres des points du MNH, dans le contour à 60 cm des murs
        let mut m = Vec::new();
        let (a0, a1) = ((x0b as f64 / RES).floor() as i64, (x1b as f64 / RES).ceil() as i64);
        let (b0, b1) = ((z0b as f64 / RES).floor() as i64, (z1b as f64 / RES).ceil() as i64);
        for bz in b0..b1 {
            for ax in a0..a1 {
                let (x, z) = (((ax as f64 + 0.5) * RES) as f32, ((bz as f64 + 0.5) * RES) as f32);
                if !dans_contour(c, x, z, 0.6) {
                    continue;
                }
                // Plus bas que 1,5 m : le jardin ou la cour (le contour d'OSM est souvent
                // décalé d'un ou deux mètres), pas le toit
                if let Some(h) = mnh.lire(x + decalage.0, z + decalage.1).filter(|h| *h >= 1.5) {
                    let sol = relief(((x as f64) - ox) / pas, ((z as f64) - oz) / pas);
                    m.push([x, z, sol + h - moyen]);
                }
            }
        }
        if let Some((forme, egout, toit, axe)) = toits::ajuster(c, &m) {
            let egout = (moyen + egout).max(haut + 1.0);
            let b = &mut batiments[k];
            (b.forme, b.egout, b.toit, b.axe) = (forme, egout, (moyen + toit).max(egout), axe);
            formes[forme as usize] += 1;
        } else {
            formes[4] += 1;
        }
    }
    if d.toits {
        s.noter(&format!("toits mesurés : {} plats, {} à deux pans, {} en croupe, {} en appentis ; {} sans mesure", formes[0], formes[1], formes[2], formes[3], formes[4]));
    }
    s.etape("occupation du sol", 0.93);
    // 6. L'occupation du sol, en neuf morceaux, tramée classe après classe
    let (mx, mz) = g.sol();
    let mut surfaces_sol: Vec<(u8, Vec<Vec<(f64, f64)>>)> = Vec::new();
    let mut vus = std::collections::HashSet::new();
    let filtre = "[\"natural\"~\"^(wood|scrub|grassland|heath)$\"]";
    let loisirs = "[\"leisure\"~\"^(park|garden|pitch|golf_course|recreation_ground|playground)$\"]";
    for (bi, bj) in (0..3).flat_map(|j| (0..3).map(move |i| (i, j))) {
        s.suivre()?;
        let (la0, lo0) = (cadre.0 + (cadre.2 - cadre.0) * bj as f64 / 3.0, cadre.1 + (cadre.3 - cadre.1) * bi as f64 / 3.0);
        let (la1, lo1) = (cadre.0 + (cadre.2 - cadre.0) * (bj + 1) as f64 / 3.0, cadre.1 + (cadre.3 - cadre.1) * (bi + 1) as f64 / 3.0);
        let b = format!("{la0:.5},{lo0:.5},{la1:.5},{lo1:.5}");
        let q = format!("(way[\"landuse\"]({b});relation[\"landuse\"]({b});way{filtre}({b});relation{filtre}({b});way{loisirs}({b});relation{loisirs}({b}););out geom;");
        let js = overpass(r, s, &cache.join(format!("osm_sol_{bi}_{bj}{suffixe}.json")), "occupation du sol", &q)?;
        for e in js.get("elements").and_then(J::liste).unwrap_or(&[]) {
            let id = (e.get("type").and_then(J::txt).unwrap_or("").to_string(), e.get("id").and_then(J::num).unwrap_or(0.0) as i64);
            if !vus.insert(id) {
                continue;
            }
            let Some(classe) = classe_sol(e.get("tags")) else { continue };
            // En points de la grille de 8 m
            let demi = |l: Vec<(f64, f64)>| l.into_iter().map(|p| (p.0 / 2.0, p.1 / 2.0)).collect::<Vec<_>>();
            let anneaux_ = match e.get("type").and_then(J::txt) {
                Some("way") => {
                    let l = ligne(e.get("geometry").unwrap_or(&J::Nul));
                    if l.len() < 4 || l.first() != l.last() {
                        continue;
                    }
                    vec![demi(l)]
                }
                _ => anneaux(e.get("members").and_then(J::liste).unwrap_or(&[]).iter().filter(|m| m.get("type").and_then(J::txt) == Some("way")).map(|m| ligne(m.get("geometry").unwrap_or(&J::Nul))).filter(|l| l.len() > 1).collect()).into_iter().map(demi).collect(),
            };
            surfaces_sol.push((classe, anneaux_));
        }
    }
    // Les moins précises d'abord : les quartiers, puis les champs, les prés, les bois
    use crate::{BOIS, CHAMP, HABITE, PRE, ZONE};
    let mut sol = vec![0u8; mx * mz];
    let mut compte = [0usize; 6];
    for classe in [HABITE, ZONE, CHAMP, PRE, BOIS] {
        for (_, a) in surfaces_sol.iter().filter(|(c, _)| *c == classe) {
            remplir_avec(a, mx, mz, |i| sol[i] = classe);
        }
    }
    for c in &sol {
        compte[*c as usize] += 1;
    }
    let km2 = |k: usize| compte[k] as f64 * (pas * 2.0).powi(2) / 1e6;
    s.noter(&format!("{} surfaces d'occupation : bois {:.0} km², prés {:.0}, champs {:.0}, habité {:.0}, zones {:.0}", surfaces_sol.len(), km2(1), km2(2), km2(3), km2(4), km2(5)));
    s.etape("écriture", 0.98);
    let lieu = Lieu::sur(nom, lat, lon, x0, y0, pas, g, niveau, &h, &eau, routes, batiments, ponts).avec_sol(sol);
    lieu.ecrire(&d.sortie).map_err(|e| format!("{} : {e}", d.sortie.display()))?;
    s.noter(&format!("{} : {} Mo", d.sortie.display(), std::fs::metadata(&d.sortie).map_or(0, |m| m.len()) / 1_000_000));
    s.etape("fini", 1.0);
    Ok(lieu)
}


/// Une requête Overpass (sa réponse gardée dans `f`), essayée sur plusieurs serveurs,
/// plusieurs fois s'ils sont occupés.
fn overpass(r: &dyn Reseau, s: &Suivi, f: &Path, quoi: &str, requete: &str) -> Result<J, String> {
    if !f.exists() {
        let q = format!("data={}", encoder(&format!("[out:json][timeout:300];{requete}")));
        let mut ok = false;
        'essais: for essai in 0..5 {
            for url in OVERPASS {
                s.suivre()?;
                s.noter(&format!("{quoi} OSM ({url})…"));
                let rendu = r.telecharger(url, Some(&q), f);
                if rendu.is_ok() && std::fs::read(f).is_ok_and(|o| o.first() == Some(&b'{')) {
                    ok = true;
                    break 'essais;
                }
                let _ = std::fs::remove_file(f);
            }
            let attente = 15 << essai;
            s.noter(&format!("Overpass occupé, nouvel essai dans {attente} s"));
            // Par secondes : un arrêt demandé n'attend pas
            for _ in 0..attente {
                s.suivre()?;
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
        if !ok {
            return Err("Overpass ne répond pas : réessayez plus tard".into());
        }
    }
    let texte = std::fs::read_to_string(f).map_err(|e| e.to_string())?;
    json::lire(&texte).ok_or_else(|| {
        let _ = std::fs::remove_file(f);
        format!("réponse d'Overpass illisible ({quoi})")
    })
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
    use crate::{CHEMIN, GRANDE, PIETONNE, ROUTE, SENTIER, SERVICE};
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

/// Un bâtiment sans mesure : la hauteur d'OSM ; deux pans à 40° (au plus 40 % de la
/// hauteur) sur les maisons, annexes, édifices et petits immeubles, plat sinon.
fn par_defaut(genre: u8, pied: f32, toit: f32, haut: f32, contour: Vec<[f32; 2]>) -> Batiment {
    use crate::{aire, INDUSTRIEL, IMMEUBLE};
    let hauteur = toit - haut;
    if genre == INDUSTRIEL || aire(&contour).abs() > 1500.0 || (genre == IMMEUBLE && hauteur > 15.0) {
        return Batiment::plat(genre, pied, toit, contour);
    }
    let axe = toits::axe_long(&contour);
    let k = toits::Cadre::new(&contour, axe);
    let pente = (0.4 * hauteur).min(40f32.to_radians().tan() * k.b);
    Batiment { genre, pied, toit, egout: (toit - pente).max(haut + 1.5).min(toit), forme: toits::DEUX_PANS, axe, contour }
}

/// Le point (x, z) est-il dans le contour, à plus de `marge` mètres de ses côtés ?
fn dans_contour(c: &[[f32; 2]], x: f32, z: f32, marge: f32) -> bool {
    let mut dedans = false;
    for e in 0..c.len() {
        let (a, b) = (c[e], c[(e + 1) % c.len()]);
        let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
        let l2 = dx * dx + dz * dz;
        let t = if l2 > 0.0 { (((x - a[0]) * dx + (z - a[1]) * dz) / l2).clamp(0.0, 1.0) } else { 0.0 };
        if (x - a[0] - dx * t).hypot(z - a[1] - dz * t) < marge {
            return false;
        }
        if (a[1] > z) != (b[1] > z) && x < a[0] + (z - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
            dedans = !dedans;
        }
    }
    dedans
}

/// Côté d'un bloc du MNH (m), sa résolution (m) et son nombre de points par côté.
const BLOC: f64 = 250.0;
const RES: f64 = 0.5;
const POINTS: usize = 500;

fn fichier_mnh(dossier: &Path, (i, j): (i32, i32)) -> PathBuf {
    dossier.join(format!("{i}_{j}.bil"))
}

fn mnh_present(f: &Path) -> bool {
    std::fs::metadata(f).is_ok_and(|m| m.len() == (POINTS * POINTS * 4) as u64)
}

/// Télécharge les blocs du MNH qui manquent, quatre à la fois, en affichant l'avancement
/// (et en l'écrivant dans `avancement.txt`). Un bloc qui échoue quatre fois manque : ses
/// bâtiments gardent leur toit par défaut.
fn telecharger_mnh(r: &dyn Reseau, s: &Suivi, dossier: &Path, blocs: &[(i32, i32)], x0: f64, y0: f64) -> Result<(), String> {
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed};
    let taille = (POINTS * POINTS * 4) as u64;
    let reste: Vec<(i32, i32)> = blocs.iter().cloned().filter(|k| !mnh_present(&fichier_mnh(dossier, *k))).collect();
    let total = reste.len();
    s.noter(&format!("MNH : {} blocs de 250 m, {} déjà là, {:.2} Go à télécharger", blocs.len(), blocs.len() - total, (total as u64 * taille) as f64 / 1e9));
    if total == 0 {
        return Ok(());
    }
    let file = Mutex::new(reste);
    let (faits, octets, rates) = (AtomicUsize::new(0), AtomicU64::new(0), AtomicUsize::new(0));
    let debut = std::time::Instant::now();
    let avancement = dossier.join("avancement.txt");
    std::thread::scope(|sc| {
        for _ in 0..4 {
            sc.spawn(|| loop {
                if s.arrete() {
                    break;
                }
                let Some((i, j)) = file.lock().ok().and_then(|mut f| f.pop()) else { break };
                let (minx, maxy) = (x0 + i as f64 * BLOC, y0 - j as f64 * BLOC);
                let url = format!(
                    "https://data.geopf.fr/wms-r/wms?SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&LAYERS=IGNF_LIDAR-HD_MNH_ELEVATION.ELEVATIONGRIDCOVERAGE.LAMB93&STYLES=&FORMAT=image/x-bil;bits=32&CRS=EPSG:2154&BBOX={minx},{},{},{maxy}&WIDTH={POINTS}&HEIGHT={POINTS}",
                    maxy - BLOC,
                    minx + BLOC
                );
                let f = fichier_mnh(dossier, (i, j));
                let mut ok = false;
                for essai in 0..4 {
                    if s.arrete() {
                        break;
                    }
                    if r.telecharger(&url, None, &f).is_ok() && mnh_present(&f) {
                        ok = true;
                        break;
                    }
                    let _ = std::fs::remove_file(&f);
                    std::thread::sleep(std::time::Duration::from_secs(2 << essai));
                }
                if ok {
                    octets.fetch_add(taille, Relaxed);
                } else {
                    rates.fetch_add(1, Relaxed);
                }
                faits.fetch_add(1, Relaxed);
            });
        }
        // L'avancement, chaque seconde ; le journal, toutes les dix
        let mut tour = 0;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            tour += 1;
            let (f, o, ra) = (faits.load(Relaxed), octets.load(Relaxed), rates.load(Relaxed));
            let t = debut.elapsed().as_secs_f64();
            let encore = if f > 0 { t / f as f64 * (total - f) as f64 / 60.0 } else { f64::NAN };
            let ligne = format!(
                "MNH : {f}/{total} blocs ({:.0} %), {:.2} Go sur {:.2}, {:.1} Mo/s, encore ~{encore:.0} min{}",
                100.0 * f as f64 / total as f64,
                o as f64 / 1e9,
                (total as u64 * taille) as f64 / 1e9,
                o as f64 / 1e6 / t,
                if ra > 0 { format!(", {ra} en échec") } else { String::new() }
            );
            s.etape(&format!("toits : téléchargement du LiDAR ({f}/{total})"), 0.45 + 0.4 * f as f32 / total as f32);
            if tour % 10 == 0 || f == total {
                s.noter(&ligne);
                let _ = std::fs::write(&avancement, format!("{ligne}\n"));
            }
            // Arrêté : les téléchargements en cours finissent, le cadre les attend
            if f == total || s.arrete() {
                break;
            }
        }
    });
    s.suivre()
}

/// Les blocs du MNH lus, les 64 derniers gardés en mémoire.
struct Mnh {
    dossier: PathBuf,
    blocs: std::collections::HashMap<(i32, i32), Option<Vec<f32>>>,
    ordre: std::collections::VecDeque<(i32, i32)>,
}

impl Mnh {
    /// La hauteur au-dessus du sol en (x, z) du jeu ; rien hors des blocs ou sans mesure.
    fn lire(&mut self, x: f32, z: f32) -> Option<f32> {
        let k = ((x as f64 / BLOC).floor() as i32, (z as f64 / BLOC).floor() as i32);
        if !self.blocs.contains_key(&k) {
            if self.ordre.len() >= 64 {
                let vieux = self.ordre.pop_front().unwrap();
                self.blocs.remove(&vieux);
            }
            let f = fichier_mnh(&self.dossier, k);
            let v = std::fs::read(&f).ok().filter(|o| o.len() == POINTS * POINTS * 4).map(|o| o.chunks(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect());
            self.blocs.insert(k, v);
            self.ordre.push_back(k);
        }
        let v = self.blocs[&k].as_ref()?;
        let c = ((x as f64 - k.0 as f64 * BLOC) / RES).floor().clamp(0.0, (POINTS - 1) as f64) as usize;
        let r = ((z as f64 - k.1 as f64 * BLOC) / RES).floor().clamp(0.0, (POINTS - 1) as f64) as usize;
        let h = v[r * POINTS + c];
        (h > -100.0 && h < 500.0).then_some(h)
    }
}

/// La classe d'occupation du sol d'une surface d'après ses étiquettes OSM.
fn classe_sol(tags: Option<&J>) -> Option<u8> {
    use crate::{BOIS, CHAMP, HABITE, PRE, ZONE};
    let tag = |k: &str| tags.and_then(|t| t.get(k)).and_then(J::txt);
    if let Some(n) = tag("natural") {
        return match n {
            "wood" | "scrub" => Some(BOIS),
            "grassland" | "heath" => Some(PRE),
            _ => None,
        };
    }
    if tag("leisure").is_some() && tag("landuse").is_none() {
        return Some(PRE);
    }
    match tag("landuse")? {
        "forest" => Some(BOIS),
        "meadow" | "grass" | "recreation_ground" | "village_green" | "cemetery" | "allotments" | "orchard" | "vineyard" | "plant_nursery" | "greenfield" => Some(PRE),
        "farmland" | "farmyard" => Some(CHAMP),
        "residential" => Some(HABITE),
        "industrial" | "commercial" | "retail" | "railway" | "construction" | "brownfield" | "garages" | "military" | "quarry" | "landfill" | "depot" | "port" => Some(ZONE),
        _ => None,
    }
}

/// Le genre et la hauteur (m) d'un bâtiment d'après ses étiquettes OSM : `height`, sinon
/// ses étages (`building:levels`, 3 m chacun, plus les étages du toit), sinon selon son
/// genre. Rien pour un toit seul (un auvent) ou ce qui n'est pas un bâtiment.
fn genre_bati(tags: Option<&J>) -> Option<(u8, f32)> {
    use crate::{ANNEXE, EDIFICE, IMMEUBLE, INDUSTRIEL, MAISON};
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
fn remplir(anneaux: &[Vec<(f64, f64)>], nx: usize, nz: usize, eau: &mut [bool]) {
    remplir_avec(anneaux, nx, nz, |i| eau[i] = true);
}

/// Pareil, en appelant `f` pour chaque point dedans (son indice dans la grille de
/// `nx` × `nz`).
fn remplir_avec(anneaux: &[Vec<(f64, f64)>], nx: usize, nz: usize, mut f: impl FnMut(usize)) {
    let (mut b0, mut b1) = (f64::MAX, f64::MIN);
    for r in anneaux {
        for p in r {
            b0 = b0.min(p.1);
            b1 = b1.max(p.1);
        }
    }
    let (b0, b1) = (b0.ceil().max(0.0) as usize, (b1.floor().min(nz as f64 - 1.0)).max(-1.0) as i64);
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
            let (a0, a1) = (p[0].ceil().max(0.0) as i64, p[1].floor().min(nx as f64 - 1.0) as i64);
            for a in a0..=a1 {
                f(b as usize * nx + a as usize);
            }
        }
    }
}

/// Pour chaque point d'eau, sa distance à la berge, en points (chanfrein 3-4).
fn distance_berge(eau: &[bool], nx: usize, nz: usize) -> Vec<f32> {
    let mut d: Vec<f32> = eau.iter().map(|e| if *e { f32::MAX } else { 0.0 }).collect();
    let (dr, dg) = (1.0, std::f32::consts::SQRT_2);
    for b in 0..nz {
        for a in 0..nx {
            let i = b * nx + a;
            if d[i] == 0.0 {
                continue;
            }
            let mut m = d[i];
            if a > 0 { m = m.min(d[i - 1] + dr); }
            if b > 0 {
                m = m.min(d[i - nx] + dr);
                if a > 0 { m = m.min(d[i - nx - 1] + dg); }
                if a + 1 < nx { m = m.min(d[i - nx + 1] + dg); }
            }
            d[i] = m;
        }
    }
    for b in (0..nz).rev() {
        for a in (0..nx).rev() {
            let i = b * nx + a;
            if d[i] == 0.0 {
                continue;
            }
            let mut m = d[i];
            if a + 1 < nx { m = m.min(d[i + 1] + dr); }
            if b + 1 < nz {
                m = m.min(d[i + nx] + dr);
                if a + 1 < nx { m = m.min(d[i + nx + 1] + dg); }
                if a > 0 { m = m.min(d[i + nx - 1] + dg); }
            }
            d[i] = m;
        }
    }
    d.iter().map(|v| v.min(1e4)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un réseau sans réseau : le relief à 50 m partout, OSM vide.
    struct Faux(Mutex<Vec<String>>);

    impl Reseau for Faux {
        fn telecharger(&self, url: &str, _post: Option<&str>, sortie: &Path) -> Result<(), String> {
            self.0.lock().unwrap().push(url.to_string());
            let o = if url.contains("wms") {
                let lire = |k: &str| url.split(&format!("&{k}=")).nth(1).and_then(|v| v.split('&').next()).and_then(|v| v.parse::<usize>().ok()).unwrap();
                50f32.to_le_bytes().repeat(lire("WIDTH") * lire("HEIGHT"))
            } else {
                b"{\"elements\": []}".to_vec()
            };
            std::fs::write(sortie, o).map_err(|e| e.to_string())
        }
    }

    #[test]
    fn une_emprise_ne_produit_que_son_rectangle() {
        let dossier = std::env::temp_dir().join(format!("sage-reel-gen-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dossier);
        let sortie = dossier.join("essai.reel");
        let d = Demande { nom: "Essai".into(), lat: 49.36, lon: 1.36, pas: 2.0, sortie: sortie.clone(), toits: false, largeur: 1000.0, longueur: 600.0 };
        let r = Faux(Mutex::new(Vec::new()));
        let l = generer(&d, &r, &Suivi::default()).unwrap();
        // Le lieu ne couvre que l'emprise : environ 501 × 301 points, pas la carte entière
        let g = l.grille;
        assert!(g.nx.abs_diff(501) <= 3 && g.nz.abs_diff(301) <= 3, "{g:?}");
        assert_eq!(g, Grille::emprise(2.0, 1000.0, 600.0));
        // Et le fichier aussi : ~2 octets par point, loin des 200 Mo de la carte entière
        let taille = std::fs::metadata(&sortie).unwrap().len();
        assert!(taille < 400_000, "{taille} octets");
        assert_eq!(Lieu::lire(&sortie).unwrap().grille, g);
        assert!((l.relief(0.0, 0.0) - 50.0).abs() < 0.01);
        // Le relief demandé à l'IGN ne déborde pas de l'emprise
        let urls = r.0.lock().unwrap();
        let points: usize = urls.iter().filter(|u| u.contains("wms")).map(|u| {
            let lire = |k: &str| u.split(&format!("&{k}=")).nth(1).and_then(|v| v.split('&').next()).and_then(|v| v.parse::<usize>().ok()).unwrap();
            lire("WIDTH") * lire("HEIGHT")
        }).sum();
        assert_eq!(points, g.points());
        let _ = std::fs::remove_dir_all(&dossier);
    }
}
