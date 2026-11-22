//! La génération d'un lieu réel : 20 km autour d'un point de France, d'après le relief
//! de l'IGN (RGE ALTI, Géoplateforme), le MNH LiDAR HD pour les toits, les bâtiments et
//! l'occupation du sol de l'IGN (son WFS ; OpenStreetMap en secours) et OpenStreetMap
//! (Overpass) pour l'eau, les routes, les ponts et les transports.
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
//! Les tunnels : les `highway=*` en `tunnel=*` (hors passages sous un bâtiment, gardés au
//! sol), mis bout à bout comme les ponts ; la chaussée rejoint le sol à ses entrées et
//! descend dessous. Les voies ferrées (`railway=rail|subway|light_rail|tram|…`) se font
//! comme les routes, leurs ponts et leurs tunnels comme ceux des routes. Les arrêts : les
//! quais des bus, les arrêts de tram, les gares et stations ; les lignes : les relations
//! `route=bus|tram|subway|light_rail|train|…`, leurs quais dans l'ordre et leur tracé.
//!
//! Les routes : `highway=*` hors ponts et tunnels, un genre (grande route, route, voie de
//! service, voie piétonne, chemin, sentier, voie de bus : `busway`, `bus_guideway`) et une largeur (`width`, `lanes` ou selon le
//! genre) ; un point tous les 8 m, la hauteur de la chaussée prise au relief et lissée.
//!
//! Les bâtiments : ceux de la BD TOPO (contours du cadastre, hauteur mesurée, nature et
//! usage, `genre_bati_ign`), page par page ; en secours (hors de France, IGN
//! indisponible), `building=*` d'OSM en neuf morceaux. Un genre (maison, immeuble,
//! industriel, édifice, annexe) et une hauteur ; le pied au plus bas du sol sous le
//! contour (un mètre dessous), le toit à la hauteur au-dessus du sol moyen.
//!
//! Les toits : le MNH LiDAR HD de l'IGN (la hauteur au-dessus du sol, à 50 cm), par blocs
//! de 250 m là où il y a des bâtiments (4 téléchargements à la fois). Dans chaque
//! contour, les mesures à plus de 60 cm des murs et à plus de 1,5 m du sol, le relevé
//! recalé sur le contour (jusqu'à 2 m), donnent la forme du toit (plat, deux pans,
//! croupe, appentis), son axe, l'égout et le faîtage (`toits::ajuster`). Un bâtiment
//! sans mesure (neuf, ou hors des blocs) garde la hauteur d'OSM, avec deux pans à 40°
//! sur les maisons.
//!
//! L'occupation du sol : de l'IGN (`sol_ign` : BD CARTO en fond, ce qu'elle ne couvre
//! pas étant cultivé ; par-dessus la végétation de la BD TOPO, le RPG, les parcs et
//! cimetières), par son WFS, sans file d'attente. En secours (hors de France, IGN
//! indisponible), d'OSM : `landuse=*`, les bois, prés et landes (`natural=*`), les parcs
//! et terrains (`leisure=*`), en cinq classes, tramées sur une grille de `2 × pas` ; là
//! où elles se recouvrent, la plus précise l'emporte. En neuf morceaux.
//!
//! L'eau : les surfaces d'eau (`natural=water`, `waterway=riverbank`), leur surface au
//! niveau médian du relief sous elles. Le relief s'arrête à la surface (le LiDAR ne
//! voit pas le fond) : sous l'eau, il est posé à plat au niveau, sans lit inventé ; le
//! lit se défonce plus tard, d'après les tracés des cours d'eau (`cours_eau`). Un plan
//! d'eau à un autre niveau n'est pas gardé : il n'y a qu'un niveau d'eau.

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

    /// Le serveur de `url` répond-il vite (en une dizaine de secondes, quelle que soit
    /// la réponse) ? Rien si l'on ne sait pas le demander : on lui envoie alors la
    /// requête quand même.
    fn sonder(&self, _url: &str) -> Option<bool> {
        None
    }
}

/// Le plus long entre deux tours d'essais auprès d'Overpass (s) : sans limite de tours,
/// on attend qu'un serveur revienne (un arrêt demandé coupe l'attente).
const ATTENTE_OVERPASS_MAX: u64 = 300;

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

    fn sonder(&self, url: &str) -> Option<bool> {
        let o = std::process::Command::new("curl").args(["-s", "-m", "10", "-A", "sage-lieu-reel/1.0", "-o", "/dev/null", "-w", "%{http_code}"]).arg(url).output().ok()?;
        Some(String::from_utf8_lossy(&o.stdout).trim() != "000")
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

/// Le fichier du cache qui garde la demande (pour reprendre une génération).
pub const DEMANDE: &str = "demande.txt";

impl Demande {
    /// La demande en texte (`cle = valeur`, une par ligne), sans la sortie.
    pub fn texte(&self) -> String {
        format!("nom = {}\nlat = {}\nlon = {}\npas = {}\nlargeur = {}\nlongueur = {}\ntoits = {}\n", self.nom, self.lat, self.lon, self.pas, self.largeur, self.longueur, self.toits)
    }

    /// La demande d'une génération restée inachevée, d'après son cache (`<sortie>.cache`) :
    /// le `demande.txt` qu'elle y a laissé ; sinon (une génération d'avant), retrouvée
    /// d'après la première ligne du journal (le nom, le centre en Lambert-93, la grille)
    /// et les noms des réponses gardées (l'emprise), avec les toits. Rien si le cache ne
    /// dit pas assez.
    pub fn reprendre(sortie: &Path) -> Option<Demande> {
        let c = cache(sortie);
        if let Ok(t) = std::fs::read_to_string(c.join(DEMANDE)) {
            let v = |k: &str| t.lines().find_map(|l| l.split_once('=').filter(|(a, _)| a.trim() == k).map(|(_, b)| b.trim().to_string()));
            let n = |k: &str| v(k).and_then(|x| x.parse::<f64>().ok());
            return Some(Demande { nom: v("nom")?, lat: n("lat")?, lon: n("lon")?, pas: n("pas")?, sortie: sortie.to_path_buf(), toits: v("toits").is_none_or(|x| x != "false"), largeur: n("largeur").unwrap_or(0.0), longueur: n("longueur").unwrap_or(0.0) });
        }
        // « [0 s] nom : Lambert-93 (561728, 6928450), 10017 × 10017 points de 2 m (…) »
        let journal = std::fs::read_to_string(c.join("journal.txt")).ok()?;
        let l = journal.lines().find(|l| l.contains(" : Lambert-93 ("))?;
        let l = l.split_once("] ").map_or(l, |(_, b)| b);
        let (nom, reste) = l.split_once(" : Lambert-93 (")?;
        let (xy, reste) = reste.split_once("), ")?;
        let (x, y) = xy.split_once(", ")?;
        let (x, y): (f64, f64) = (x.trim().parse().ok()?, y.trim().parse().ok()?);
        let pas: f64 = reste.split_once(" points de ")?.1.split(' ').next()?.parse().ok()?;
        let (lat, lon) = crate::lambert93_inverse(x, y);
        // L'emprise : le suffixe des réponses d'OSM (« _6000x6000.json »), sinon la carte
        // entière
        let mut emprise = (0.0, 0.0);
        for e in std::fs::read_dir(&c).ok()?.flatten() {
            let f = e.file_name().to_string_lossy().into_owned();
            if let Some(s) = f.strip_prefix("osm_").and_then(|f| f.strip_suffix(".json")) {
                if let Some((a, b)) = s.rsplit('_').next().and_then(|d| d.split_once('x')) {
                    if let (Ok(a), Ok(b)) = (a.parse::<f64>(), b.parse::<f64>()) {
                        emprise = (a, b);
                        break;
                    }
                }
            }
        }
        Some(Demande { nom: nom.to_string(), lat, lon, pas, sortie: sortie.to_path_buf(), toits: true, largeur: emprise.0, longueur: emprise.1 })
    }
}

/// L'avancement d'une génération, lu par un autre thread : l'étape, sa part du tout (de 0
/// à 1), le journal ; `arreter` l'interrompt (au prochain point d'arrêt).
///
/// Le suivi garde aussi de quoi surveiller une génération longue (`rapport`) : le temps
/// passé dans chaque étape, une estimation de ce qui reste, les téléchargements (faits,
/// leur volume, leur durée, ceux trouvés dans le cache, les échecs). Une génération
/// l'écrit en continu dans son cache (`journal.txt`, chaque ligne datée, et `etat.txt`,
/// le rapport, refait au plus chaque seconde) : on la suit de l'extérieur, même sans
/// l'application.
#[derive(Default)]
pub struct Suivi {
    arret: AtomicBool,
    etat: Mutex<(String, f32)>,
    journal: Mutex<Vec<String>>,
    /// Imprimer aussi le journal (l'outil en ligne de commande).
    pub imprimer: bool,
    mesures: Mutex<Mesures>,
}

/// Ce que le suivi mesure.
#[derive(Default)]
struct Mesures {
    debut: Option<std::time::Instant>,
    /// Les étapes : leur nom et leur début (s depuis le début).
    etapes: Vec<(String, f64)>,
    telecharges: usize,
    octets: u64,
    /// Le temps passé à télécharger (s, en additionnant les fils).
    duree_telechargements: f64,
    en_cache: usize,
    echecs: usize,
    /// Le dossier où écrire le journal et l'état, et la dernière écriture de l'état.
    dossier: Option<PathBuf>,
    ecrit: Option<std::time::Instant>,
}

/// Le rapport d'une génération (`Suivi::rapport`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rapport {
    pub etape: String,
    pub part: f32,
    /// Le temps écoulé depuis le début (s) et l'estimation de ce qui reste (s).
    pub ecoule: f64,
    pub reste: Option<f64>,
    /// Les étapes passées et en cours : nom, durée (s).
    pub etapes: Vec<(String, f64)>,
    pub telecharges: usize,
    pub octets: u64,
    pub duree_telechargements: f64,
    pub en_cache: usize,
    pub echecs: usize,
    pub derniere: Option<String>,
    pub arrete: bool,
}

/// Une durée lisible : « 42 s », « 3 min 05 », « 1 h 12 ».
pub fn duree(s: f64) -> String {
    let s = s.max(0.0).round() as u64;
    if s < 60 {
        format!("{s} s")
    } else if s < 3600 {
        format!("{} min {:02}", s / 60, s % 60)
    } else {
        format!("{} h {:02}", s / 3600, s / 60 % 60)
    }
}

impl Rapport {
    /// Le rapport en quelques lignes.
    pub fn texte(&self) -> String {
        let mut t = format!("{} · {:.0} % · {}", if self.etape.is_empty() { "pas commencée" } else { &self.etape }, self.part * 100.0, duree(self.ecoule));
        if let Some(r) = self.reste {
            t += &format!(", reste ~{}", duree(r));
        }
        if self.arrete {
            t += " · arrêt demandé";
        }
        let debit = if self.duree_telechargements > 0.0 { self.octets as f64 / self.duree_telechargements / 1e6 } else { 0.0 };
        t += &format!("\ntéléchargements : {} ({:.1} Mo, {:.1} Mo/s par fil), {} dans le cache, {} échecs", self.telecharges, self.octets as f64 / 1e6, debit, self.en_cache, self.echecs);
        for (n, d) in &self.etapes {
            t += &format!("\n  {n} : {}", duree(*d));
        }
        if let Some(l) = &self.derniere {
            t += &format!("\ndernière ligne : {l}");
        }
        t
    }
}

impl Suivi {
    pub fn noter(&self, ligne: &str) {
        if self.imprimer {
            println!("{ligne}");
        }
        if let Ok(mut j) = self.journal.lock() {
            j.push(ligne.to_string());
        }
        let (dossier, t) = match self.mesures.lock() {
            Ok(m) => (m.dossier.clone(), m.debut.map_or(0.0, |d| d.elapsed().as_secs_f64())),
            Err(_) => (None, 0.0),
        };
        if let Some(d) = dossier {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(d.join("journal.txt")) {
                let _ = writeln!(f, "[{}] {ligne}", duree(t));
            }
        }
    }

    pub fn etape(&self, quoi: &str, part: f32) {
        if let Ok(mut e) = self.etat.lock() {
            *e = (quoi.to_string(), part.clamp(0.0, 1.0));
        }
        if let Ok(mut m) = self.mesures.lock() {
            let debut = *m.debut.get_or_insert_with(std::time::Instant::now);
            let t = debut.elapsed().as_secs_f64();
            // Une étape : le nom avant ses précisions entre parenthèses
            let nom = quoi.split(" (").next().unwrap_or(quoi).to_string();
            if m.etapes.last().is_none_or(|(n, _)| *n != nom) {
                m.etapes.push((nom, t));
            }
        }
        self.ecrire_etat(false);
    }

    /// Écrit le journal et l'état dans `dossier` (le cache de la génération).
    pub fn ecrire_dans(&self, dossier: &Path) {
        if let Ok(mut m) = self.mesures.lock() {
            m.debut.get_or_insert_with(std::time::Instant::now);
            m.dossier = Some(dossier.to_path_buf());
        }
        let _ = std::fs::write(dossier.join("journal.txt"), "");
    }

    /// `etat.txt` : le rapport, au plus chaque seconde (toujours si `forcer`).
    fn ecrire_etat(&self, forcer: bool) {
        let dossier = match self.mesures.lock() {
            Ok(mut m) => {
                if m.dossier.is_none() || (!forcer && m.ecrit.is_some_and(|e| e.elapsed().as_secs_f64() < 1.0)) {
                    return;
                }
                m.ecrit = Some(std::time::Instant::now());
                m.dossier.clone()
            }
            Err(_) => return,
        };
        if let Some(d) = dossier {
            let _ = std::fs::write(d.join("etat.txt"), self.rapport().texte() + "\n");
        }
    }

    /// Un téléchargement fini : réussi (son volume) ou non, et sa durée (s).
    pub fn compter_telechargement(&self, octets: Option<u64>, secondes: f64) {
        if let Ok(mut m) = self.mesures.lock() {
            match octets {
                Some(o) => {
                    m.telecharges += 1;
                    m.octets += o;
                }
                None => m.echecs += 1,
            }
            m.duree_telechargements += secondes;
        }
        self.ecrire_etat(false);
    }

    /// Un fichier trouvé dans le cache : rien à télécharger.
    pub fn compter_cache(&self) {
        if let Ok(mut m) = self.mesures.lock() {
            m.en_cache += 1;
        }
    }

    /// Où en est la génération.
    pub fn rapport(&self) -> Rapport {
        let (etape, part) = self.etat();
        let mut r = Rapport { etape, part, arrete: self.arrete(), derniere: self.journal.lock().ok().and_then(|j| j.last().cloned()), ..Default::default() };
        if let Ok(m) = self.mesures.lock() {
            r.ecoule = m.debut.map_or(0.0, |d| d.elapsed().as_secs_f64());
            r.etapes = m.etapes.iter().enumerate().map(|(i, (n, t))| (n.clone(), m.etapes.get(i + 1).map_or(r.ecoule, |e| e.1) - t)).collect();
            (r.telecharges, r.octets, r.duree_telechargements, r.en_cache, r.echecs) = (m.telecharges, m.octets, m.duree_telechargements, m.en_cache, m.echecs);
        }
        // Ce qui reste, au rythme moyen, dès qu'on en a vu assez
        if r.part > 0.02 && r.part < 1.0 && r.ecoule > 5.0 {
            r.reste = Some(r.ecoule * (1.0 - r.part as f64) / r.part as f64);
        }
        r
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
        self.ecrire_etat(true);
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

/// Télécharge en comptant (`Suivi::compter_telechargement`).
fn telecharger(r: &dyn Reseau, s: &Suivi, url: &str, post: Option<&str>, f: &Path) -> Result<(), String> {
    let t = std::time::Instant::now();
    let rendu = r.telecharger(url, post, f);
    let octets = rendu.as_ref().ok().map(|_| std::fs::metadata(f).map_or(0, |m| m.len()));
    s.compter_telechargement(octets, t.elapsed().as_secs_f64());
    rendu
}

/// Le dossier des téléchargements d'une sortie.
pub fn cache(sortie: &Path) -> PathBuf {
    PathBuf::from(format!("{}.cache", sortie.display()))
}

/// Prépare le lieu demandé, l'écrit dans `d.sortie` et le rend.
pub fn generer(d: &Demande, r: &dyn Reseau, s: &Suivi) -> Result<Lieu, String> {
    let pas = d.pas;
    if !(0.5..=64.0).contains(&pas) {
        return Err(format!("pas de {pas} m : entre 0,5 et 64"));
    }
    let cache = cache(&d.sortie);
    std::fs::create_dir_all(&cache).map_err(|e| format!("{} : {e}", cache.display()))?;
    // De quoi la reprendre si elle s'interrompt
    let _ = std::fs::write(cache.join(DEMANDE), d.texte());
    s.ecrire_dans(&cache);
    let rendu = generer_dans(d, r, s, &cache);
    match &rendu {
        Ok(_) => s.noter(&format!("fini en {}", duree(s.rapport().ecoule))),
        Err(e) => s.noter(&format!("échec : {e}")),
    }
    s.ecrire_etat(true);
    rendu
}

fn generer_dans(d: &Demande, r: &dyn Reseau, s: &Suivi, cache: &Path) -> Result<Lieu, String> {
    let (nom, lat, lon, pas) = (d.nom.as_str(), d.lat, d.lon, d.pas);
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
                telecharger(r, s, &url, None, &f).map_err(|e| format!("le relief de l'IGN ne répond pas : {e}"))?;
            } else {
                s.compter_cache();
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
    let mut natures: Vec<u8> = Vec::new();
    for e in j.get("elements").and_then(J::liste).unwrap_or(&[]) {
        let tag = |k: &str| e.get("tags").and_then(|t| t.get(k)).and_then(J::txt);
        let nature = crate::cours_eau::nature(tag("waterway"), tag("water"));
        let avant = surfaces.len();
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
        natures.resize(natures.len() + surfaces.len() - avant, nature);
    }
    let mut eau = vec![false; nx * nz];
    for s in &surfaces {
        remplir(s, nx, nz, &mut eau);
    }
    let dans_eau: Vec<f32> = (0..nx * nz).filter(|&i| eau[i]).map(|i| h[i]).collect();
    s.noter(&format!("{} surfaces d'eau, {:.2} km²", surfaces.len(), dans_eau.len() as f64 * pas * pas / 1e6));

    // 3. Le niveau de l'eau ; sous elle, le relief à plat à ce niveau
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
    // Les surfaces gardées (au moins un point d'eau après le tri), en vecteurs : x, z du
    // jeu, coupées au bord de la carte (20 m de marge, comme les lignes)
    let bord_eau = [(ox - 20.0) as f32, (oz - 20.0) as f32, (ox + (nx - 1) as f64 * pas + 20.0) as f32, (oz + (nz - 1) as f64 * pas + 20.0) as f32];
    let mut surfaces_eau = Vec::new();
    for (a, &nature) in surfaces.iter().zip(&natures) {
        let mut garde = false;
        remplir_avec(a, nx, nz, |i| garde |= eau[i]);
        if !garde {
            continue;
        }
        let anneaux: Vec<Vec<[f32; 2]>> = a.iter().map(|r| {
            let mut v: Vec<[f32; 2]> = r.iter().map(|p| [(ox + p.0 * pas) as f32, (oz + p.1 * pas) as f32]).collect();
            if v.len() > 1 && v.first() == v.last() {
                v.pop();
            }
            v
        }).collect();
        surfaces_eau.extend(crate::cours_eau::couper_surfaces(&[crate::cours_eau::SurfaceEau { nature, anneaux }], bord_eau));
    }
    s.noter(&format!("{} surfaces d'eau gardées en vecteurs", surfaces_eau.len()));
    for i in 0..nx * nz {
        if eau[i] {
            h[i] = niveau;
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
    let marge = 20.0 / pas;
    let dans = |p: &(f64, f64)| p.0 > -marge && p.1 > -marge && p.0 < (nx - 1) as f64 + marge && p.1 < (nz - 1) as f64 + marge;
    // Hors de la carte, rien : une ligne se coupe à son bord (avec 20 m de marge)
    let decouper = |pts: &[(f64, f64)], ys: &[f32]| -> Vec<Vec<[f32; 3]>> {
        let mut v = Vec::new();
        let mut m: Vec<[f32; 3]> = Vec::new();
        for (p, y) in pts.iter().zip(ys) {
            if dans(p) {
                m.push([(ox + p.0 * pas) as f32, (oz + p.1 * pas) as f32, *y]);
            } else if m.len() > 1 {
                v.push(std::mem::take(&mut m));
            } else {
                m.clear();
            }
        }
        if m.len() > 1 {
            v.push(m);
        }
        v
    };
    // Au sol : un point tous les 8 m au plus, la hauteur prise au relief, lissée par une
    // moyenne glissante sur cinq points (une chaussée sans bosses)
    let au_sol = |l: &[(f64, f64)]| -> (Vec<(f64, f64)>, Vec<f32>) {
        let pts = densifier(l, pas, 8.0);
        let hs: Vec<f32> = pts.iter().map(|p| relief(p.0, p.1)).collect();
        let lisse = (0..hs.len()).map(|i| {
            let (a, b) = (i.saturating_sub(2), (i + 3).min(hs.len()));
            hs[a..b].iter().sum::<f32>() / (b - a) as f32
        }).collect();
        (pts, lisse)
    };
    use crate::transports::{self as tr, Arret, Ligne, Rail, Transports};
    let mut routes = Vec::new();
    let mut ponts = Vec::new();
    let mut tunnels = Vec::new();
    let mut longueur = 0.0;
    // Les ponts et les tunnels à part : mis bout à bout, ils vont d'un bout à l'autre
    let (mut lignes_ponts, mut lignes_tunnels) = (Vec::new(), Vec::new());
    // Les voies OSM par revêtement (le journal)
    let mut revetements = [0usize; crate::REVETEMENTS as usize];
    for e in jr.get("elements").and_then(J::liste).unwrap_or(&[]) {
        let Some((genre, largeur, ou)) = genre_route(e.get("tags")) else { continue };
        let rev = revetement_osm(e.get("tags"), genre);
        if let Some(c) = revetements.get_mut(rev as usize) {
            *c += 1;
        }
        let l = ligne(e.get("geometry").unwrap_or(&J::Nul));
        if l.len() < 2 {
            continue;
        }
        match ou {
            tr::PONT => lignes_ponts.push((genre, largeur, l, 0, rev)),
            tr::TUNNEL => lignes_tunnels.push((genre, largeur, l, niveau_osm(e.get("tags"), true), rev)),
            _ => {
                longueur += l.windows(2).map(|s| (s[1].0 - s[0].0).hypot(s[1].1 - s[0].1) * pas).sum::<f64>();
                let (pts, ys) = au_sol(&l);
                routes.extend(decouper(&pts, &ys).into_iter().map(|points| Route { revetement: rev, genre, largeur, points }));
            }
        }
    }
    for (liste, tunnel, sortie) in [(&lignes_ponts, false, &mut ponts), (&lignes_tunnels, true, &mut tunnels)] {
        let l: Vec<_> = liste.iter().map(|(_, _, l, _, _)| l.clone()).collect();
        let prof: Vec<f32> = liste.iter().map(|e| tr::profondeur(e.3)).collect();
        for ((genre, largeur, _, _, rev), (pts, ys)) in liste.iter().zip(ouvrages(&l, pas, &relief, niveau, tunnel, &prof)) {
            sortie.extend(decouper(&pts, &ys).into_iter().map(|points| Route { revetement: *rev, genre: *genre, largeur: *largeur, points }));
        }
    }
    s.noter(&format!("{} ponts OSM, {} tabliers ; {} tunnels OSM, {} morceaux", lignes_ponts.len(), ponts.len(), lignes_tunnels.len(), tunnels.len()));
    s.noter(&format!("{} routes, {:.0} km", routes.len(), longueur / 1000.0));
    s.noter(&format!("revêtements des voies OSM : {}", crate::NOMS_REVETEMENTS.iter().zip(revetements).map(|(n, c)| format!("{n} {c}")).collect::<Vec<_>>().join(", ")));

    // Les cours d'eau : leurs tracés (axes), coupés au bord de la carte
    let jc = overpass(r, s, &cache.join(format!("osm_cours_eau{suffixe}.json")), "cours d'eau", &format!("way[\"waterway\"~\"^(river|canal|stream|ditch|drain)$\"]({bb});out tags geom;"))?;
    let mut cours_eau = Vec::new();
    for e in jc.get("elements").and_then(J::liste).unwrap_or(&[]) {
        let tags = e.get("tags");
        let tag = |k: &str| tags.and_then(|t| t.get(k)).and_then(J::txt);
        let Some(genre) = tag("waterway").and_then(crate::cours_eau::genre) else { continue };
        let l = ligne(e.get("geometry").unwrap_or(&J::Nul));
        if l.len() < 2 {
            continue;
        }
        let souterrain = matches!(tag("tunnel"), Some("culvert" | "yes")) || tag("layer").and_then(|x| x.parse::<i32>().ok()).is_some_and(|n| n < 0);
        let (nom, largeur) = (tag("name").unwrap_or("").to_string(), tag("width").map_or(0.0, crate::cours_eau::largeur_osm));
        for pts in decouper(&l, &vec![0.0; l.len()]) {
            cours_eau.push(crate::cours_eau::CoursEau { genre, nom: nom.clone(), largeur, souterrain, points: pts.iter().map(|p| [p[0], p[1]]).collect() });
        }
    }
    let km = |g: u8| cours_eau.iter().filter(|c| c.genre == g).map(|c| c.points.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum::<f32>()).sum::<f32>() / 1000.0 + 0.0;
    s.noter(&format!("{} cours d'eau : rivières {:.1} km, canaux {:.1}, ruisseaux {:.1}, fossés {:.1}", cours_eau.len(), km(0), km(1), km(2), km(3)));

    // Les voies ferrées : au sol comme les routes, sur les ponts et en tunnel comme eux
    s.suivre()?;
    s.etape("voies ferrées, arrêts et lignes", 0.26);
    let jf = overpass(r, s, &cache.join(format!("osm_rails{suffixe}.json")), "voies ferrées", &format!("way[\"railway\"~\"^(rail|narrow_gauge|subway|light_rail|tram|monorail|funicular)$\"]({bb});out tags geom;"))?;
    let mut rails = Vec::new();
    let mut ouvrages_rails: [Vec<(u8, i8, Vec<(f64, f64)>)>; 2] = Default::default();
    for e in jf.get("elements").and_then(J::liste).unwrap_or(&[]) {
        let Some((mode, ou)) = genre_rail(e.get("tags")) else { continue };
        let nv = niveau_osm(e.get("tags"), ou == tr::TUNNEL);
        let l = ligne(e.get("geometry").unwrap_or(&J::Nul));
        if l.len() < 2 {
            continue;
        }
        if ou == tr::SOL {
            let (pts, ys) = au_sol(&l);
            rails.extend(decouper(&pts, &ys).into_iter().map(|points| Rail { mode, ou, niveau: nv, points }));
        } else {
            ouvrages_rails[(ou == tr::TUNNEL) as usize].push((mode, nv, l));
        }
    }
    for (k, liste) in ouvrages_rails.iter().enumerate() {
        let l: Vec<_> = liste.iter().map(|(_, _, l)| l.clone()).collect();
        let prof: Vec<f32> = liste.iter().map(|e| tr::profondeur(e.1)).collect();
        let ou = if k == 1 { tr::TUNNEL } else { tr::PONT };
        for ((mode, nv, _), (pts, ys)) in liste.iter().zip(ouvrages(&l, pas, &relief, niveau, k == 1, &prof)) {
            rails.extend(decouper(&pts, &ys).into_iter().map(|points| Rail { mode: *mode, ou, niveau: *nv, points }));
        }
    }
    let jeu = |p: (f64, f64)| [(ox + p.0 * pas) as f32, (oz + p.1 * pas) as f32];
    // Les arrêts : les quais des bus, les arrêts de tram, les gares et stations
    let ja = overpass(r, s, &cache.join(format!("osm_arrets{suffixe}.json")), "arrêts", &format!("(node[\"highway\"=\"bus_stop\"]({bb});node[\"public_transport\"=\"platform\"]({bb});node[\"railway\"~\"^(station|halt|tram_stop)$\"]({bb}););out;"))?;
    let mut arrets: Vec<Arret> = Vec::new();
    let mut par_id: std::collections::HashMap<i64, usize> = Default::default();
    // Les arrêts dont le niveau est dit (sinon, celui des voies voisines)
    let mut niveau_dit: Vec<bool> = Vec::new();
    for e in ja.get("elements").and_then(J::liste).unwrap_or(&[]) {
        let Some(p) = vers_grille(e).filter(|p| dans(p)) else { continue };
        let Some(modes) = modes_arret(e.get("tags")) else { continue };
        let tag = |k: &str| e.get("tags").and_then(|t| t.get(k)).and_then(J::txt);
        let [x, z] = jeu(p);
        par_id.insert(e.get("id").and_then(J::num).unwrap_or(-1.0) as i64, arrets.len());
        let (nv, dit) = niveau_arret(e.get("tags"));
        niveau_dit.push(dit);
        arrets.push(Arret { modes, x, z, nom: tag("name").unwrap_or("").to_string(), abri: tag("shelter") == Some("yes"), niveau: nv });
    }
    // Les lignes : leurs quais (ou leurs arrêts) dans l'ordre, et leur tracé
    let jl = overpass(r, s, &cache.join(format!("osm_lignes{suffixe}.json")), "lignes", &format!("relation[\"type\"=\"route\"][\"route\"~\"^(bus|trolleybus|tram|subway|light_rail|train|monorail|funicular)$\"]({bb});out geom({bb});"))?;
    let mut lignes = Vec::new();
    // Les voies où se recalent les tracés : les voies ferrées, et pour les bus les ponts
    // et les tunnels des routes
    let grille_rails = Segments::new(rails.iter().map(|r| (r.ou, &r.points[..])));
    let grille_routes = Segments::new(ponts.iter().map(|r| (tr::PONT, &r.points[..])).chain(tunnels.iter().map(|r| (tr::TUNNEL, &r.points[..]))));
    for e in jl.get("elements").and_then(J::liste).unwrap_or(&[]) {
        let tag = |k: &str| e.get("tags").and_then(|t| t.get(k)).and_then(J::txt);
        let mode = match tag("route") {
            Some("bus" | "trolleybus") => tr::BUS,
            // Un tram qui se dit métro (Rouen) : un métro léger
            Some("tram") if tag("name").is_some_and(|n| n.to_lowercase().starts_with("métro")) => tr::METRO,
            Some("tram") => tr::TRAM,
            Some("subway" | "light_rail") => tr::METRO,
            Some("train") => tr::TRAIN,
            Some(_) => tr::AUTRE,
            None => continue,
        };
        // Les quais et les points d'arrêt, dans l'ordre ; un quai et son point d'arrêt
        // voisin ne font qu'un arrêt
        let rayon: f32 = if mode == tr::BUS { 30.0 } else { 150.0 };
        let mut la: Vec<usize> = Vec::new();
        let mut trace: Vec<Vec<[f32; 2]>> = Vec::new();
        for m in e.get("members").and_then(J::liste).unwrap_or(&[]) {
            let role = m.get("role").and_then(J::txt).unwrap_or("");
            match m.get("type").and_then(J::txt) {
                Some(t @ ("node" | "way")) if role.starts_with("platform") || (t == "node" && role.starts_with("stop")) => {
                    let id = if t == "node" { m.get("ref").and_then(J::num).unwrap_or(-1.0) as i64 } else { i64::MIN };
                    let k = match par_id.get(&id) {
                        Some(&k) => k,
                        None => {
                            // Un nœud, ou le milieu d'un quai tracé
                            let p = if t == "node" {
                                vers_grille(m)
                            } else {
                                let l = ligne(m.get("geometry").unwrap_or(&J::Nul));
                                (!l.is_empty()).then(|| (l.iter().map(|p| p.0).sum::<f64>() / l.len() as f64, l.iter().map(|p| p.1).sum::<f64>() / l.len() as f64))
                            };
                            let Some(p) = p.filter(|p| dans(p)) else { continue };
                            let [x, z] = jeu(p);
                            // Le point d'arrêt d'un quai déjà connu (une gare est grande)
                            let proche = arrets.iter().enumerate().map(|(k, a)| (k, (a.x - x).hypot(a.z - z))).filter(|(_, d)| *d < rayon).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(k, _)| k);
                            let k = proche.unwrap_or_else(|| {
                                arrets.push(Arret { modes: 0, x, z, nom: String::new(), abri: false, niveau: 0 });
                                niveau_dit.push(false);
                                arrets.len() - 1
                            });
                            if t == "node" {
                                par_id.insert(id, k);
                            }
                            k
                        }
                    };
                    let voisin = la.last().is_some_and(|&d| d == k || (arrets[d].x - arrets[k].x).hypot(arrets[d].z - arrets[k].z) < rayon);
                    if !voisin {
                        la.push(k);
                    } else if arrets[*la.last().unwrap()].nom.is_empty() && !arrets[k].nom.is_empty() {
                        // Le quai nommé plutôt que le point d'arrêt anonyme
                        *la.last_mut().unwrap() = k;
                    }
                }
                Some("way") if !role.starts_with("platform") => {
                    let l = ligne(m.get("geometry").unwrap_or(&J::Nul));
                    let mut morceau: Vec<[f32; 2]> = Vec::new();
                    let fermer = |m: &mut Vec<[f32; 2]>, trace: &mut Vec<Vec<[f32; 2]>>| {
                        if m.len() > 1 {
                            // Bout à bout avec le précédent : un seul morceau
                            match trace.last_mut() {
                                Some(t) if t.last() == m.first() => t.extend_from_slice(&m[1..]),
                                _ => trace.push(m.clone()),
                            }
                        }
                        m.clear();
                    };
                    for p in l {
                        if dans(&p) {
                            morceau.push(jeu(p));
                        } else {
                            fermer(&mut morceau, &mut trace);
                        }
                    }
                    fermer(&mut morceau, &mut trace);
                }
                _ => {}
            }
        }
        for &k in &la {
            arrets[k].modes |= 1 << mode;
        }
        if la.is_empty() && trace.is_empty() {
            continue;
        }
        lignes.push(Ligne {
            mode,
            numero: tag("ref").unwrap_or("").to_string(),
            nom: tag("name").unwrap_or("").to_string(),
            couleur: tag("colour").and_then(tr::couleur),
            arrets: la.into_iter().map(|k| k as u32).collect(),
            trace: if mode == tr::BUS { recaler(trace, &grille_routes) } else { recaler(trace, &grille_rails) },
        });
    }
    for a in arrets.iter_mut().filter(|a| a.modes == 0) {
        a.modes = 1 << tr::BUS;
    }
    // Une station sans niveau dit prend celui de la voie ferrée la plus proche, si elle
    // est en tunnel
    for (a, _) in arrets.iter_mut().zip(&niveau_dit).filter(|(a, dit)| !**dit && a.modes & !(1 << tr::BUS) != 0) {
        let q = [a.x, a.z];
        let proche = rails.iter().flat_map(|r| r.points.windows(2).map(move |w| (r, distance_segment(q, [w[0][0], w[0][1]], [w[1][0], w[1][1]])))).filter(|(_, d)| *d < 25.0).min_by(|x, y| x.1.total_cmp(&y.1));
        if let Some((r, _)) = proche.filter(|(r, _)| r.ou == tr::TUNNEL) {
            a.niveau = r.niveau.min(-1);
        }
    }
    s.noter(&format!("{} voies ferrées, {} arrêts, {} lignes", rails.len(), arrets.len(), lignes.len()));
    let transports = Transports { tunnels, rails, arrets, lignes };
    s.etape("bâtiments", 0.3);
    // 5. Les bâtiments, en neuf morceaux (une réponse entière serait trop lourde à lire)
    let mut batiments = Vec::new();
    let mut sols_bati: Vec<(f32, f32)> = Vec::new();
    let mut vus = std::collections::HashSet::new();
    // Un contour (en points de grille) gardé s'il est dans la carte, posé sur le relief
    let ajouter = |batiments: &mut Vec<Batiment>, sols_bati: &mut Vec<(f32, f32)>, genre: u8, hauteur: f32, mut c: Vec<(f64, f64)>| {
        if c.first() == c.last() {
            c.pop();
        }
        c.dedup();
        if c.len() < 3 || !c.iter().all(|p| p.0 > 0.0 && p.1 > 0.0 && p.0 < (nx - 1) as f64 && p.1 < (nz - 1) as f64) {
            return;
        }
        let sols: Vec<f32> = c.iter().map(|p| relief(p.0, p.1)).collect();
        let (bas, haut) = (sols.iter().cloned().fold(f32::MAX, f32::min), sols.iter().cloned().fold(f32::MIN, f32::max));
        let moyen = sols.iter().sum::<f32>() / sols.len() as f32;
        let contour = c.iter().map(|p| [(ox + p.0 * pas) as f32, (oz + p.1 * pas) as f32]).collect();
        batiments.push(par_defaut(genre, bas - 1.0, (moyen + hauteur).max(haut + 2.0), haut, contour));
        sols_bati.push((moyen, haut));
    };
    // D'abord ceux de l'IGN (BD TOPO : contours du cadastre, hauteurs mesurées) ;
    // d'OSM en secours
    let en_grille_bati = |x: f64, y: f64| ((x - x0 - ox) / pas, (y0 - y - oz) / pas);
    let ign = wfs_ign(r, s, &cache, &suffixe, "BDTOPO_V3:batiment", CHAMPS_BATI_IGN, cadre, &mut |o| {
        let Some((genre, hauteur)) = o.get("properties").and_then(genre_bati_ign) else { return };
        // Chaque polygone : son contour extérieur (les cours intérieures se couvrent)
        for a in contours_geojson(o.get("geometry"), &en_grille_bati) {
            ajouter(&mut batiments, &mut sols_bati, genre, hauteur, a);
        }
    });
    match ign {
        Ok(n) if n > 0 => s.noter(&format!("bâtiments de l'IGN : {n} objets")),
        rendu => {
            match rendu {
                Err(e) => s.noter(&format!("bâtiments de l'IGN indisponibles ({e}) : OpenStreetMap en secours")),
                _ => s.noter("aucun bâtiment de l'IGN ici (hors de France ?) : OpenStreetMap en secours"),
            }
            batiments.clear();
            sols_bati.clear();
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
                    for c in contours {
                        ajouter(&mut batiments, &mut sols_bati, genre, hauteur, c);
                    }
                }
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
    // 6. L'occupation du sol : de l'IGN (BD CARTO en fond, BD TOPO, RPG), sinon
    // d'OpenStreetMap ; chaque surface avec sa classe et si elle est du fond (peinte
    // d'abord, les plus précises par-dessus)
    let (mx, mz) = g.sol();
    let en_grille = |x: f64, y: f64| ((x - x0 - ox) / pas / 2.0, (y0 - y - oz) / pas / 2.0);
    let mut surfaces_sol: Vec<(u8, Vec<Vec<(f64, f64)>>)> = Vec::new();
    let mut fond: Vec<(u8, Vec<Vec<(f64, f64)>>)> = Vec::new();
    match sol_ign(r, s, &cache, &suffixe, cadre, &en_grille) {
        Ok((f, p)) if !f.is_empty() || !p.is_empty() => {
            s.noter(&format!("occupation du sol de l'IGN : {} surfaces de fond, {} précises", f.len(), p.len()));
            (fond, surfaces_sol) = (f, p);
        }
        rendu => {
            match rendu {
                Err(e) => s.noter(&format!("occupation du sol de l'IGN indisponible ({e}) : OpenStreetMap en secours")),
                _ => s.noter("rien de l'IGN ici (hors de France ?) : OpenStreetMap en secours"),
            }
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
        }
    }
    // Les moins précises d'abord : les quartiers, puis les champs, les prés, les bois
    use crate::{BOIS, CHAMP, HABITE, PRE, ZONE};
    // Avec l'IGN, ce que la BD CARTO ne couvre pas est cultivé
    let mut sol = vec![if fond.is_empty() { 0u8 } else { CHAMP }; mx * mz];
    let mut compte = [0usize; 6];
    for liste in [&fond, &surfaces_sol] {
        for classe in [HABITE, ZONE, CHAMP, PRE, BOIS] {
            for (_, a) in liste.iter().filter(|(c, _)| *c == classe) {
                remplir_avec(a, mx, mz, |i| sol[i] = classe);
            }
        }
    }
    for c in &sol {
        compte[*c as usize] += 1;
    }
    let km2 = |k: usize| compte[k] as f64 * (pas * 2.0).powi(2) / 1e6;
    s.noter(&format!("{} surfaces d'occupation : bois {:.0} km², prés {:.0}, champs {:.0}, habité {:.0}, zones {:.0}", fond.len() + surfaces_sol.len(), km2(1), km2(2), km2(3), km2(4), km2(5)));
    s.etape("écriture", 0.98);
    let mut lieu = Lieu::sur(nom, lat, lon, x0, y0, pas, g, niveau, &h, &eau, routes, batiments, ponts).avec_sol(sol);
    lieu.transports = transports;
    lieu.cours_eau = cours_eau;
    lieu.surfaces_eau = surfaces_eau;
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
        // Les tours d'essais, sans limite : chaque serveur à son tour (un serveur muet,
        // qui ne répond même pas à sa page d'état, est sauté : sa requête attendrait
        // 5 minutes pour rien), puis une attente qui double jusqu'à 5 minutes
        let mut essai = 0u32;
        'essais: loop {
            for url in OVERPASS {
                s.suivre()?;
                if r.sonder(&url.replace("/interpreter", "/status")) == Some(false) {
                    s.noter(&format!("{quoi} OSM : {url} ne répond pas, sauté"));
                    continue;
                }
                s.noter(&format!("{quoi} OSM ({url})…"));
                let rendu = telecharger(r, s, url, Some(&q), f);
                if rendu.is_ok() && std::fs::read(f).is_ok_and(|o| o.first() == Some(&b'{')) {
                    break 'essais;
                }
                if let Err(e) = rendu {
                    s.noter(&format!("{quoi} OSM : {url} a échoué ({e})"));
                }
                let _ = std::fs::remove_file(f);
            }
            let attente = (15u64 << essai.min(5)).min(ATTENTE_OVERPASS_MAX);
            s.noter(&format!("Overpass occupé (tour {}), nouvel essai dans {} — on attend qu'un serveur revienne", essai + 1, duree(attente as f64)));
            // Par secondes : un arrêt demandé n'attend pas
            for _ in 0..attente {
                s.suivre()?;
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
            essai += 1;
        }
    } else {
        s.compter_cache();
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

/// Où passe une voie d'après ses étiquettes : sur un pont, en tunnel (un passage sous
/// un bâtiment reste au sol), ou au sol.
fn ou_passe(tags: Option<&J>) -> u8 {
    use crate::transports::{PONT, SOL, TUNNEL};
    let tag = |k: &str| tags.and_then(|t| t.get(k)).and_then(J::txt);
    if tag("bridge").is_some_and(|v| v != "no") {
        PONT
    } else if tag("tunnel").is_some_and(|v| v != "no" && v != "building_passage") {
        TUNNEL
    } else {
        SOL
    }
}

/// Le mode et le passage (sol, pont, tunnel) d'une voie ferrée ; rien pour une voie
/// abandonnée, en projet, ou de service hors des lignes (dépôts, garages).
fn genre_rail(tags: Option<&J>) -> Option<(u8, u8)> {
    use crate::transports::{AUTRE, METRO, TRAIN, TRAM};
    let tag = |k: &str| tags.and_then(|t| t.get(k)).and_then(J::txt);
    let mode = match tag("railway")? {
        "rail" | "narrow_gauge" => TRAIN,
        "subway" | "light_rail" => METRO,
        "tram" => TRAM,
        "monorail" | "funicular" => AUTRE,
        _ => return None,
    };
    if tag("service").is_some_and(|v| v == "yard") {
        return None;
    }
    Some((mode, ou_passe(tags)))
}

/// Les modes (un bit chacun) d'un arrêt d'après ses étiquettes ; rien si ce n'en est pas un.
fn modes_arret(tags: Option<&J>) -> Option<u8> {
    use crate::transports::{AUTRE, BUS, METRO, TRAIN, TRAM};
    let tag = |k: &str| tags.and_then(|t| t.get(k)).and_then(J::txt);
    let oui = |k: &str| tag(k) == Some("yes");
    let mut m = 0u8;
    if tag("highway") == Some("bus_stop") || oui("bus") || oui("trolleybus") {
        m |= 1 << BUS;
    }
    if tag("railway") == Some("tram_stop") || oui("tram") {
        m |= 1 << TRAM;
    }
    if oui("subway") || oui("light_rail") {
        m |= 1 << METRO;
    }
    if oui("train") {
        m |= 1 << TRAIN;
    }
    if let Some("station" | "halt") = tag("railway") {
        m |= 1 << match tag("station") {
            Some("subway" | "light_rail") => METRO,
            Some("tram") => TRAM,
            Some("funicular" | "monorail") => AUTRE,
            _ if m != 0 => return Some(m),
            _ => TRAIN,
        };
    }
    if m == 0 && tag("public_transport") == Some("platform") {
        m = 1 << BUS;
    }
    (m != 0).then_some(m)
}

/// Les hauteurs le long d'ouvrages (ponts ou tunnels, en pas de grille) : mis bout à
/// bout, chaque groupe va de ses vrais bouts (un bout partagé entre deux morceaux n'en
/// est pas un), un point tous les 8 m. La hauteur est la moyenne de celles des bouts
/// (le sol, au-dessus de l'eau pour un pont), pondérée par l'inverse du carré de la
/// distance ; sans bouts, le plus haut du sol sous le pont. Un tunnel descend en plus
/// sous le relief, de 12 cm par mètre loin de ses entrées, jusqu'à sa profondeur
/// (`profondeurs`, une par ligne : `transports::profondeur` de son niveau).
fn ouvrages(lignes: &[Vec<(f64, f64)>], pas: f64, relief: &dyn Fn(f64, f64) -> f32, niveau: f32, tunnel: bool, profondeurs: &[f32]) -> Vec<(Vec<(f64, f64)>, Vec<f32>)> {
    let cle = |p: (f64, f64)| ((p.0 * 100.0).round() as i64, (p.1 * 100.0).round() as i64);
    let mut bouts: std::collections::HashMap<(i64, i64), u32> = Default::default();
    for l in lignes {
        *bouts.entry(cle(l[0])).or_default() += 1;
        *bouts.entry(cle(l[l.len() - 1])).or_default() += 1;
    }
    let mut groupe: Vec<usize> = (0..lignes.len()).collect();
    fn racine(g: &mut [usize], mut i: usize) -> usize {
        while g[i] != i {
            g[i] = g[g[i]];
            i = g[i];
        }
        i
    }
    let mut par_bout: std::collections::HashMap<(i64, i64), usize> = Default::default();
    for (k, l) in lignes.iter().enumerate() {
        for b in [l[0], l[l.len() - 1]] {
            if let Some(&autre) = par_bout.get(&cle(b)) {
                let (a, c) = (racine(&mut groupe, autre), racine(&mut groupe, k));
                groupe[a] = c;
            } else {
                par_bout.insert(cle(b), k);
            }
        }
    }
    let haut_bout = |x: f64, z: f64| if tunnel { relief(x, z) } else { relief(x, z).max(niveau + 0.5) };
    let mut culees: std::collections::HashMap<usize, Vec<(f64, f64, f32)>> = Default::default();
    for (k, l) in lignes.iter().enumerate() {
        let g = racine(&mut groupe, k);
        for b in [l[0], l[l.len() - 1]] {
            if bouts[&cle(b)] == 1 {
                culees.entry(g).or_default().push((b.0, b.1, haut_bout(b.0, b.1)));
            }
        }
    }
    let mut v = Vec::new();
    for (k, l) in lignes.iter().enumerate() {
        let g = racine(&mut groupe, k);
        let c = culees.get(&g).cloned().unwrap_or_default();
        let pts = densifier(l, pas, 8.0);
        let ys = pts.iter().map(|p| {
            let y = if c.is_empty() {
                if tunnel { relief(p.0, p.1) } else { l.iter().map(|q| relief(q.0, q.1)).fold(niveau + 0.5, f32::max) }
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
            if tunnel {
                let d = c.iter().map(|&(bx, bz, _)| (bx - p.0).hypot(bz - p.1) * pas).fold(f64::MAX, f64::min);
                y.min(relief(p.0, p.1) - (d as f32 * 0.12).min(profondeurs.get(k).copied().unwrap_or(6.0)))
            } else {
                y
            }
        }).collect();
        v.push((pts, ys));
    }
    v
}

/// Le niveau d'une voie (`layer` d'OSM) ; un tunnel est au moins au niveau −1.
fn niveau_osm(tags: Option<&J>, tunnel: bool) -> i8 {
    let n = tags.and_then(|t| t.get("layer")).and_then(J::txt).and_then(|v| v.trim().parse::<i8>().ok()).unwrap_or(0);
    if tunnel { n.min(-1) } else { n }
}

/// Le niveau d'un arrêt (`level`, le plus bas s'il y en a plusieurs ; −1 pour
/// `location=underground`), et s'il est dit.
fn niveau_arret(tags: Option<&J>) -> (i8, bool) {
    let tag = |k: &str| tags.and_then(|t| t.get(k)).and_then(J::txt);
    let dit = tag("level").and_then(|v| v.split(';').filter_map(|x| x.trim().replace(',', ".").parse::<f32>().ok()).map(|x| x.floor() as i8).min());
    match (dit, tag("location")) {
        (Some(n), _) => (n, true),
        (None, Some("underground")) => (-1, true),
        _ => (0, false),
    }
}

/// La distance du point `p` au segment `a`–`b`.
fn distance_segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dz * dz;
    let t = if l2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dz) / l2).clamp(0.0, 1.0) } else { 0.0 };
    (p[0] - a[0] - dx * t).hypot(p[1] - a[1] - dz * t)
}

/// Des segments de voies (x, z du jeu) rangés par cases de 16 m, avec où ils passent.
struct Segments(std::collections::HashMap<(i32, i32), Vec<([f32; 2], [f32; 2], u8)>>);

impl Segments {
    const CASE: f32 = 16.0;

    fn new<'a>(voies: impl Iterator<Item = (u8, &'a [[f32; 3]])>) -> Segments {
        let mut m: std::collections::HashMap<(i32, i32), Vec<([f32; 2], [f32; 2], u8)>> = Default::default();
        for (ou, pts) in voies {
            for w in pts.windows(2) {
                let (a, b) = ([w[0][0], w[0][1]], [w[1][0], w[1][1]]);
                let c = |v: f32| (v / Segments::CASE).floor() as i32;
                for i in c(a[0].min(b[0]) - 4.0)..=c(a[0].max(b[0]) + 4.0) {
                    for j in c(a[1].min(b[1]) - 4.0)..=c(a[1].max(b[1]) + 4.0) {
                        m.entry((i, j)).or_default().push((a, b, ou));
                    }
                }
            }
        }
        Segments(m)
    }

    /// Où passe la voie la plus proche du segment `a`–`b` (à 3 m de son milieu, à peu
    /// près parallèle) ; au sol s'il n'y en a pas.
    fn ou(&self, a: [f32; 2], b: [f32; 2]) -> u8 {
        let m = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
        let c = ((m[0] / Segments::CASE).floor() as i32, (m[1] / Segments::CASE).floor() as i32);
        let l = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-6);
        let mut mieux = (3.0f32, crate::transports::SOL);
        for &(p, q, ou) in self.0.get(&c).map_or(&[][..], |v| v.as_slice()) {
            let lq = (q[0] - p[0]).hypot(q[1] - p[1]).max(1e-6);
            let cos = ((b[0] - a[0]) * (q[0] - p[0]) + (b[1] - a[1]) * (q[1] - p[1])).abs() / (l * lq);
            let d = distance_segment(m, p, q);
            if cos > 0.7 && d < mieux.0 {
                mieux = (d, ou);
            }
        }
        mieux.1
    }
}

/// Le tracé d'une ligne coupé en morceaux au sol, sur un pont ou en tunnel, d'après les
/// voies où il passe.
fn recaler(trace: Vec<Vec<[f32; 2]>>, voies: &Segments) -> Vec<crate::transports::Morceau> {
    use crate::transports::Morceau;
    let mut v: Vec<Morceau> = Vec::new();
    for t in trace {
        let mut courant: Option<Morceau> = None;
        for w in t.windows(2) {
            let ou = voies.ou(w[0], w[1]);
            match &mut courant {
                Some(m) if m.ou == ou => m.points.push(w[1]),
                _ => {
                    v.extend(courant.take());
                    courant = Some(Morceau { ou, points: vec![w[0], w[1]] });
                }
            }
        }
        v.extend(courant);
    }
    v
}

/// Le genre et la largeur (m) d'une route d'après ses étiquettes OSM, et où elle passe
/// (`transports::SOL`, `PONT`, `TUNNEL`) ; rien pour ce qui n'est pas une voie
/// (escaliers, projets, surfaces, voies intérieures…).
fn genre_route(tags: Option<&J>) -> Option<(u8, f32, u8)> {
    use crate::{BUS, CHEMIN, GRANDE, PIETONNE, ROUTE, RUE_PIETONNE, SENTIER, SERVICE};
    let tag = |k: &str| tags.and_then(|t| t.get(k)).and_then(J::txt);
    let pont = ou_passe(tags);
    if tag("area") == Some("yes") || tag("indoor").is_some_and(|v| v != "no") {
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
        // Les sites propres des bus ; une voie de service qui leur est réservée aussi
        "busway" | "bus_guideway" => (BUS, 3.5),
        "service" if ["bus", "psv"].iter().any(|k| tag(k) == Some("designated")) => (BUS, 3.5),
        "service" => (SERVICE, 4.0),
        "pedestrian" => (RUE_PIETONNE, 6.0),
        "cycleway" => (PIETONNE, 2.5),
        "footway" => (PIETONNE, 2.0),
        "track" => (CHEMIN, 3.0),
        "path" | "bridleway" => (SENTIER, 1.5),
        _ => return None,
    };
    // Une chaussée non revêtue est un chemin ; un sentier revêtu, une voie piétonne
    let terre = tag("surface").is_some_and(|s| ["unpaved", "gravel", "fine_gravel", "dirt", "earth", "ground", "grass", "compacted", "mud", "sand"].contains(&s));
    let genre = match (genre, terre) {
        (ROUTE | SERVICE | PIETONNE | RUE_PIETONNE, true) => CHEMIN,
        (SENTIER, false) if tag("surface").is_some() => PIETONNE,
        (g, _) => g,
    };
    let largeur = tag("width").and_then(|w| w.trim_end_matches(" m").trim_end_matches('m').trim().replace(',', ".").parse::<f32>().ok()).filter(|w| (0.5..60.0).contains(w)).or_else(|| tag("lanes").and_then(|l| l.parse::<f32>().ok()).filter(|l| (1.0..12.0).contains(l)).map(|l| l * 3.25 + 1.0)).unwrap_or(largeur);
    Some((genre, largeur, pont))
}

/// Le revêtement d'une voie d'après son `surface=*` ; sans étiquette, selon son genre
/// (un chemin est en gravier, un sentier en terre, le reste en bitume).
fn revetement_osm(tags: Option<&J>, genre: u8) -> u8 {
    use crate::{BETON, BITUME, CHEMIN, GRAVIER, HERBE, PAVES, SENTIER, TERRE};
    let surface = tags.and_then(|t| t.get("surface")).and_then(J::txt).unwrap_or("");
    // « paving_stones:30 », « concrete:plates » : la famille avant les deux-points
    let famille = surface.split(':').next().unwrap_or("");
    match (surface, famille) {
        ("grass_paver", _) => HERBE,
        (_, "asphalt" | "paved" | "chipseal" | "tartan" | "rubber") => BITUME,
        (_, "concrete" | "metal" | "wood") => BETON,
        (_, "paving_stones" | "sett" | "cobblestone" | "unhewn_cobblestone" | "bricks" | "stone" | "brick") => PAVES,
        (_, "gravel" | "fine_gravel" | "pebblestone" | "compacted" | "rock" | "shells") => GRAVIER,
        (_, "dirt" | "earth" | "ground" | "mud" | "sand" | "clay" | "unpaved" | "soil") => TERRE,
        (_, "grass") => HERBE,
        _ => match genre {
            CHEMIN => GRAVIER,
            SENTIER => TERRE,
            _ => BITUME,
        },
    }
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
    for _ in total..blocs.len() {
        s.compter_cache();
    }
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
                    if telecharger(r, s, &url, None, &f).is_ok() && mnh_present(&f) {
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

/// Les surfaces classées d'une couche (fond ou précises).
pub type Surfaces = Vec<(u8, Vec<Vec<(f64, f64)>>)>;

/// Le WFS de la Géoplateforme de l'IGN.
const WFS_IGN: &str = "https://data.geopf.fr/wfs/ows";
/// Les objets par page du WFS.
const PAGE_WFS: usize = 5000;

/// Une couche du WFS de l'IGN dans le cadre (degrés : lat0, lon0, lat1, lon1), en
/// Lambert 93, page par page (gardées dans le cache), réduite aux champs `champs` (tous
/// si vide) : chaque objet donné à `f`, page après page (une couche entière serait
/// lourde à garder) ; rend leur nombre.
fn wfs_ign(r: &dyn Reseau, s: &Suivi, cache: &Path, suffixe: &str, couche: &str, champs: &str, cadre: (f64, f64, f64, f64), f_objet: &mut dyn FnMut(&J)) -> Result<usize, String> {
    let court = couche.rsplit(':').next().unwrap_or(couche);
    let base = format!("{WFS_IGN}?SERVICE=WFS&VERSION=2.0.0&REQUEST=GetFeature&TYPENAMES={couche}&BBOX={:.5},{:.5},{:.5},{:.5},urn:ogc:def:crs:EPSG::4326", cadre.0, cadre.1, cadre.2, cadre.3);
    let proprietes = if champs.is_empty() { String::new() } else { format!("&PROPERTYNAME={champs}") };
    let fichier = |page: usize| cache.join(format!("ign_{court}_{page}{suffixe}.json"));
    // Une page dans le cache (si elle n'y est pas déjà) : trois essais, à dix secondes
    // d'écart
    let charger = |page: usize| -> Result<(), String> {
        let f = fichier(page);
        if f.exists() {
            s.compter_cache();
            return Ok(());
        }
        let url = format!("{base}&OUTPUTFORMAT=application/json&SRSNAME=EPSG:2154&COUNT={PAGE_WFS}&STARTINDEX={}{proprietes}", page * PAGE_WFS);
        let mut rendu = Err(String::new());
        for essai in 0..3 {
            if essai > 0 {
                for _ in 0..10 {
                    s.suivre()?;
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
            }
            rendu = telecharger(r, s, &url, None, &f).and_then(|_| if std::fs::read(&f).is_ok_and(|o| o.first() == Some(&b'{')) { Ok(()) } else { Err("réponse qui n'est pas du JSON".into()) });
            if rendu.is_ok() {
                break;
            }
            let _ = std::fs::remove_file(&f);
        }
        rendu.map_err(|e| format!("{court} : {e}"))
    };
    // Le nombre d'objets (une requête légère) : les pages se téléchargent alors
    // `FILS_WFS` à la fois (le serveur sert chaque requête lentement)
    if let Some(n) = nombre_wfs(r, s, &cache.join(format!("ign_{court}_nombre{suffixe}.xml")), &format!("{base}&RESULTTYPE=hits")) {
        let pages = n.div_ceil(PAGE_WFS).max(1);
        s.noter(&format!("IGN : {court}, {n} objets en {pages} pages, {FILS_WFS} à la fois…"));
        let suivante = std::sync::atomic::AtomicUsize::new(0);
        let faites = std::sync::atomic::AtomicUsize::new(0);
        let erreur: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
        std::thread::scope(|sc| {
            for _ in 0..FILS_WFS.min(pages) {
                sc.spawn(|| loop {
                    let p = suivante.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    if p >= pages || erreur.lock().map_or(true, |e| e.is_some()) {
                        break;
                    }
                    let deja = fichier(p).exists();
                    if let Err(e) = s.suivre().and_then(|_| charger(p)) {
                        if let Ok(mut m) = erreur.lock() {
                            m.get_or_insert(e);
                        }
                        break;
                    }
                    let k = faites.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                    if !deja {
                        s.noter(&format!("IGN : {court}, {k}/{pages} pages"));
                    }
                });
            }
        });
        if let Some(e) = erreur.into_inner().ok().flatten() {
            return Err(e);
        }
    }
    // Puis lues dans l'ordre (sans le nombre : téléchargées en les lisant, une à une)
    let mut objets = 0;
    for page in 0.. {
        s.suivre()?;
        if !fichier(page).exists() {
            s.noter(&format!("IGN : {court}, page {}…", page + 1));
        }
        charger(page)?;
        let f = fichier(page);
        let texte = std::fs::read_to_string(&f).map_err(|e| e.to_string())?;
        let Some(j) = json::lire(&texte) else {
            let _ = std::fs::remove_file(&f);
            return Err(format!("{court} : réponse illisible"));
        };
        let liste = j.get("features").and_then(J::liste).ok_or_else(|| format!("{court} : réponse sans objets"))?;
        let n = liste.len();
        for o in liste {
            f_objet(o);
        }
        objets += n;
        if n < PAGE_WFS {
            break;
        }
    }
    Ok(objets)
}

/// Les pages du WFS téléchargées à la fois.
const FILS_WFS: usize = 4;

/// Le nombre d'objets d'une requête WFS (`RESULTTYPE=hits`, gardé dans `f`) ; rien si
/// le serveur ne le dit pas.
fn nombre_wfs(r: &dyn Reseau, s: &Suivi, f: &Path, url: &str) -> Option<usize> {
    if !f.exists() && telecharger(r, s, url, None, f).is_err() {
        let _ = std::fs::remove_file(f);
        return None;
    }
    let t = std::fs::read_to_string(f).ok()?;
    let n = t.split("numberMatched=\"").nth(1)?.split('"').next()?.parse().ok();
    if n.is_none() {
        let _ = std::fs::remove_file(f);
    }
    n
}

/// Les anneaux d'une géométrie GeoJSON (Polygon ou MultiPolygon), par `en_grille`.
fn anneaux_geojson(g: Option<&J>, en_grille: &dyn Fn(f64, f64) -> (f64, f64)) -> Vec<Vec<(f64, f64)>> {
    let Some(g) = g else { return Vec::new() };
    let anneau = |a: &J| -> Vec<(f64, f64)> { a.liste().unwrap_or(&[]).iter().filter_map(|p| p.nombres::<2>()).map(|[x, y]| en_grille(x, y)).collect() };
    let polygones: Vec<&J> = match g.get("type").and_then(J::txt) {
        Some("Polygon") => g.get("coordinates").into_iter().collect(),
        Some("MultiPolygon") => g.get("coordinates").and_then(J::liste).unwrap_or(&[]).iter().collect(),
        _ => Vec::new(),
    };
    polygones.into_iter().flat_map(|p| p.liste().unwrap_or(&[]).iter().map(anneau)).filter(|a| a.len() > 3).collect()
}

/// L'occupation du sol de l'IGN : en fond, la BD CARTO (bâti, zones d'activités,
/// forêts, prairies : tout le pays, au 1:50 000) ; par-dessus, la végétation de la
/// BD TOPO (les bois au mètre), les parcelles agricoles déclarées (RPG : champs ou
/// prés selon la culture), les parcs, squares et jardins, les cimetières.
pub fn sol_ign(r: &dyn Reseau, s: &Suivi, cache: &Path, suffixe: &str, cadre: (f64, f64, f64, f64), en_grille: &dyn Fn(f64, f64) -> (f64, f64)) -> Result<(Surfaces, Surfaces), String> {
    use crate::PRE;
    let couche = |nom: &str, classe: &dyn Fn(&J) -> Option<u8>| -> Result<Surfaces, String> {
        let mut v = Vec::new();
        wfs_ign(r, s, cache, suffixe, nom, "", cadre, &mut |o| {
            let Some(c) = o.get("properties").and_then(classe) else { return };
            let a = anneaux_geojson(o.get("geometry"), en_grille);
            if !a.is_empty() {
                v.push((c, a));
            }
        })?;
        Ok(v)
    };
    let txt = |p: &J, k: &str| p.get(k).and_then(J::txt).map(str::to_string).unwrap_or_default();
    let fond = couche("BDCARTO_V5:occupation_du_sol", &|p| classe_bdcarto(&txt(p, "nature")))?;
    let mut precises = couche("BDTOPO_V3:zone_de_vegetation", &|p| classe_vegetation(&txt(p, "nature")))?;
    precises.extend(couche("IGNF_RPG_PARCELLES-AGRICOLES-CATEGORISEES_2024:parcelles_agricole_categorisees_2024", &|p| Some(classe_rpg(&txt(p, "code_cultu"))))?);
    precises.extend(couche("BDTOPO_V3:zone_d_activite_ou_d_interet", &|p| classe_interet(&txt(p, "nature"), &txt(p, "nature_detaillee")))?);
    precises.extend(couche("BDTOPO_V3:cimetiere", &|_| Some(PRE))?);
    Ok((fond, precises))
}

/// Les champs des bâtiments demandés à l'IGN.
const CHAMPS_BATI_IGN: &str = "geometrie,nature,usage_1,construction_legere,etat_de_l_objet,hauteur,nombre_d_etages";

/// Les contours extérieurs d'une géométrie GeoJSON (le premier anneau de chaque
/// polygone), par `en_grille`.
fn contours_geojson(g: Option<&J>, en_grille: &dyn Fn(f64, f64) -> (f64, f64)) -> Vec<Vec<(f64, f64)>> {
    let Some(g) = g else { return Vec::new() };
    let polygones: Vec<&J> = match g.get("type").and_then(J::txt) {
        Some("Polygon") => g.get("coordinates").into_iter().collect(),
        Some("MultiPolygon") => g.get("coordinates").and_then(J::liste).unwrap_or(&[]).iter().collect(),
        _ => Vec::new(),
    };
    polygones
        .into_iter()
        .filter_map(|p| p.liste()?.first())
        .map(|a| a.liste().unwrap_or(&[]).iter().filter_map(|p| p.liste().filter(|l| l.len() >= 2).and_then(|l| Some((l[0].num()?, l[1].num()?)))).map(|(x, y)| en_grille(x, y)).collect())
        .collect()
}

/// Le genre et la hauteur (m) d'un bâtiment de la BD TOPO : sa hauteur mesurée, sinon
/// ses étages, sinon selon son genre ; le genre d'après sa nature et son usage (une
/// construction légère est une annexe ; un résidentiel ou indifférencié, une maison
/// jusqu'à deux niveaux et 9 m, un immeuble au-delà). Rien pour ce qui n'est pas (ou
/// plus) en service.
fn genre_bati_ign(p: &J) -> Option<(u8, f32)> {
    use crate::{ANNEXE, EDIFICE, IMMEUBLE, INDUSTRIEL, MAISON};
    let txt = |k: &str| p.get(k).and_then(J::txt).unwrap_or("");
    if !matches!(txt("etat_de_l_objet"), "" | "En service") {
        return None;
    }
    let mesuree = p.get("hauteur").and_then(J::num).filter(|h| *h > 0.5).map(|h| h as f32);
    let etages = p.get("nombre_d_etages").and_then(J::num).filter(|e| *e > 0.0).map(|e| e as f32);
    let h = mesuree.or(etages.map(|e| 3.0 * e + 1.0));
    let (nature, usage) = (txt("nature"), txt("usage_1"));
    let genre = if p.get("construction_legere").and_then(J::bool) == Some(true) || usage == "Annexe" {
        ANNEXE
    } else if matches!(nature, "Eglise" | "Chapelle" | "Tour, donjon" | "Monument" | "Château" | "Fort, blockhaus, casemate" | "Arc de triomphe") || matches!(usage, "Religieux" | "Sportif") {
        EDIFICE
    } else if matches!(nature, "Industriel, agricole ou commercial" | "Serre" | "Silo") || matches!(usage, "Industriel" | "Agricole") {
        INDUSTRIEL
    } else if h.is_some_and(|h| h <= 9.0) && etages.is_none_or(|e| e <= 2.0) {
        MAISON
    } else if h.is_some() {
        IMMEUBLE
    } else {
        MAISON
    };
    let defaut = match genre {
        ANNEXE => 3.0,
        EDIFICE => 14.0,
        INDUSTRIEL => 8.0,
        IMMEUBLE => 12.0,
        _ => 7.0,
    };
    Some((genre, h.unwrap_or(defaut)))
}

/// La classe d'une surface de la BD CARTO (`nature`). Ses terres cultivées n'y sont
/// pas : c'est ce qui reste entre ses surfaces (des champs).
fn classe_bdcarto(nature: &str) -> Option<u8> {
    use crate::{BOIS, HABITE, PRE, ZONE};
    match nature {
        "Forêt" | "Bois" | "Peupleraie" => Some(BOIS),
        "Prairie" | "Broussailles" | "Marais, tourbière" | "Vigne, verger" => Some(PRE),
        "Bâti" => Some(HABITE),
        "Zone d'activités" | "Carrière, décharge" => Some(ZONE),
        _ => None,
    }
}

/// La classe d'une zone de végétation de la BD TOPO (`nature`) : les vergers, vignes
/// et landes en prés, le reste en bois.
fn classe_vegetation(nature: &str) -> Option<u8> {
    use crate::{BOIS, PRE};
    match nature {
        "Verger" | "Vigne" | "Lande herbacée" => Some(PRE),
        "" => None,
        _ => Some(BOIS),
    }
}

/// La classe d'une parcelle du RPG d'après sa culture (`code_cultu`) : les prairies
/// (permanentes, temporaires, pâturages) et jachères en prés, le reste en champs.
fn classe_rpg(code: &str) -> u8 {
    use crate::{CHAMP, PRE};
    let pre = ["PPH", "PRL", "PTR", "PPR", "PRA", "SPH", "SPL", "PAT", "BOP", "CAE", "CEE", "JAC", "JNO", "J5M", "J6P", "J6S", "MLG"];
    if pre.contains(&code) {
        PRE
    } else {
        CHAMP
    }
}

/// La classe d'une zone d'activité ou d'intérêt de la BD TOPO : les parcs, squares et
/// jardins en prés, les zones industrielles et d'activités en zones ; le reste (écoles,
/// places, mairies…) ne dit rien du sol.
fn classe_interet(nature: &str, detaillee: &str) -> Option<u8> {
    use crate::{PRE, ZONE};
    match (nature, detaillee) {
        (_, "Parc" | "Square" | "Jardin public" | "Jardins familiaux" | "Jardin botanique" | "Arboretum") => Some(PRE),
        ("Parc" | "Golf" | "Camping" | "Parc de loisirs", _) => Some(PRE),
        ("Zone industrielle" | "Usine" | "Divers industriel" | "Centrale électrique" | "Déchèterie" | "Carrière" | "Marché", _) | (_, "Zone d'activités") => Some(ZONE),
        _ => None,
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

    /// Un réseau dont le second serveur Overpass est muet.
    struct Muet(Mutex<Vec<String>>);

    impl Reseau for Muet {
        fn telecharger(&self, url: &str, _post: Option<&str>, sortie: &Path) -> Result<(), String> {
            self.0.lock().unwrap().push(url.to_string());
            std::fs::write(sortie, b"{\"elements\": []}").map_err(|e| e.to_string())
        }
        fn sonder(&self, url: &str) -> Option<bool> {
            Some(!url.contains("kumi"))
        }
    }

    #[test]
    fn une_demande_se_reprend_de_son_cache() {
        let sortie = std::env::temp_dir().join(format!("sage-reel-reprise-{}.reel", std::process::id()));
        let c = cache(&sortie);
        let _ = std::fs::remove_dir_all(&c);
        std::fs::create_dir_all(&c).unwrap();
        // D'après le journal (une génération d'avant)
        let (x, y) = crate::lambert93(49.4431, 1.0993);
        std::fs::write(c.join("journal.txt"), format!("[0 s] Rouen centre : Lambert-93 ({x:.0}, {y:.0}), 3001 × 2501 points de 2 m (6.0 × 5.0 km)\n[3 s] eau…\n")).unwrap();
        std::fs::write(c.join("osm_eau_6000x5000.json"), "{}").unwrap();
        let d = Demande::reprendre(&sortie).unwrap();
        assert_eq!((d.nom.as_str(), d.pas, d.largeur, d.longueur, d.toits), ("Rouen centre", 2.0, 6000.0, 5000.0, true));
        assert!((d.lat - 49.4431).abs() < 1e-4 && (d.lon - 1.0993).abs() < 1e-4, "{} {}", d.lat, d.lon);
        // D'après la demande gardée
        let e = Demande { nom: "Rouen = centre".into(), lat: 49.1, lon: 1.2, pas: 4.0, sortie: sortie.clone(), toits: false, largeur: 3000.0, longueur: 0.0 };
        std::fs::write(c.join(DEMANDE), e.texte()).unwrap();
        let d = Demande::reprendre(&sortie).unwrap();
        let _ = std::fs::remove_dir_all(&c);
        assert_eq!((d.nom.as_str(), d.lat, d.lon, d.pas, d.toits, d.largeur, d.longueur), ("Rouen = centre", 49.1, 1.2, 4.0, false, 3000.0, 0.0));
    }

    #[test]
    fn un_serveur_overpass_muet_est_saute() {
        let f = std::env::temp_dir().join(format!("sage-reel-muet-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&f);
        let r = Muet(Mutex::new(Vec::new()));
        // Le premier serveur échoue (fichier pas en JSON) : le second, muet, est sauté
        // sans rien lui demander ; ici le premier répond du premier coup
        overpass(&r, &Suivi::default(), &f, "essai", "node(1);out;").unwrap();
        let _ = std::fs::remove_file(&f);
        assert_eq!(r.0.lock().unwrap().as_slice(), [OVERPASS[0]]);
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

    #[test]
    fn tunnels_rails_et_arrets_se_reconnaissent() {
        use crate::transports::*;
        let j = |t: &str| json::lire(t);
        assert_eq!(genre_route(j(r#"{"highway":"primary","tunnel":"yes"}"#).as_ref()).map(|g| g.2), Some(TUNNEL));
        assert_eq!(genre_route(j(r#"{"highway":"service","tunnel":"building_passage"}"#).as_ref()).map(|g| g.2), Some(SOL));
        assert_eq!(genre_rail(j(r#"{"railway":"light_rail","tunnel":"yes"}"#).as_ref()), Some((METRO, TUNNEL)));
        assert_eq!(genre_rail(j(r#"{"railway":"rail","bridge":"viaduct"}"#).as_ref()), Some((TRAIN, PONT)));
        assert_eq!(genre_rail(j(r#"{"railway":"abandoned"}"#).as_ref()), None);
        assert_eq!(modes_arret(j(r#"{"highway":"bus_stop","public_transport":"platform"}"#).as_ref()), Some(1 << BUS));
        assert_eq!(modes_arret(j(r#"{"railway":"station","station":"subway"}"#).as_ref()), Some(1 << METRO));
        assert_eq!(modes_arret(j(r#"{"railway":"station"}"#).as_ref()), Some(1 << TRAIN));
        assert_eq!(modes_arret(j(r#"{"railway":"tram_stop","bus":"yes"}"#).as_ref()), Some(1 << BUS | 1 << TRAM));
        assert_eq!(modes_arret(j(r#"{"amenity":"bench"}"#).as_ref()), None);
    }

    #[test]
    fn un_tunnel_descend_sous_le_relief() {
        // Deux morceaux bout à bout sous une colline à 50 m : les entrées au sol, le
        // milieu 6 m dessous
        let l = vec![vec![(0.0, 0.0), (50.0, 0.0)], vec![(50.0, 0.0), (100.0, 0.0)]];
        let v = ouvrages(&l, 2.0, &|_, _| 50.0, 0.0, true, &[6.0, 6.0]);
        assert!((v[0].1[0] - 50.0).abs() < 1e-3);
        assert!((v[1].1.last().unwrap() - 50.0).abs() < 1e-3);
        assert!((v[0].1.last().unwrap() - 44.0).abs() < 1e-3, "{:?}", v[0].1.last());
        // Au niveau −2, 200 m dedans : 14 m sous le sol
        let v = ouvrages(&l, 4.0, &|_, _| 50.0, 0.0, true, &[crate::transports::profondeur(-2); 2]);
        assert!((v[0].1.last().unwrap() - 36.0).abs() < 1e-3, "{:?}", v[0].1.last());
    }

    #[test]
    fn un_trace_se_coupe_ou_il_passe_sous_terre() {
        use crate::transports::*;
        let voies = Segments::new([(SOL, &[[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]][..]), (TUNNEL, &[[100.0, 0.0, 0.0], [300.0, 0.0, 0.0]][..]), (SOL, &[[200.0, -50.0, 0.0], [200.0, 50.0, 0.0]][..])].into_iter());
        // Le long des voies, à 1 m à côté ; la voie en travers ne compte pas
        let t = recaler(vec![vec![[0.0, 1.0], [50.0, 1.0], [100.0, 1.0], [150.0, 1.0], [190.0, 1.0], [210.0, 1.0], [400.0, 1.0]]], &voies);
        let ous: Vec<(u8, usize)> = t.iter().map(|m| (m.ou, m.points.len())).collect();
        assert_eq!(ous, vec![(SOL, 3), (TUNNEL, 4), (SOL, 2)]);
        assert_eq!(niveau_arret(json::lire(r#"{"level":"-1;-2"}"#).as_ref()), (-2, true));
        assert_eq!(niveau_arret(json::lire(r#"{"location":"underground"}"#).as_ref()), (-1, true));
        assert_eq!(niveau_arret(json::lire(r#"{"name":"Europe"}"#).as_ref()), (0, false));
        assert_eq!(niveau_osm(json::lire(r#"{"layer":"-2"}"#).as_ref(), true), -2);
        assert_eq!(niveau_osm(None, true), -1);
    }

    #[test]
    fn le_revetement_vient_de_surface() {
        use crate::{BETON, BITUME, GRAVIER, HERBE, PAVES, TERRE};
        let r = |t: &str, g: u8| revetement_osm(json::lire(t).as_ref(), g);
        assert_eq!(r(r#"{"surface":"sett"}"#, crate::ROUTE), PAVES);
        assert_eq!(r(r#"{"surface":"paving_stones:30"}"#, crate::PIETONNE), PAVES);
        assert_eq!(r(r#"{"surface":"concrete:plates"}"#, crate::ROUTE), BETON);
        assert_eq!(r(r#"{"surface":"asphalt"}"#, crate::CHEMIN), BITUME);
        assert_eq!(r(r#"{"surface":"compacted"}"#, crate::CHEMIN), GRAVIER);
        assert_eq!(r(r#"{"surface":"dirt"}"#, crate::SENTIER), TERRE);
        assert_eq!(r(r#"{"surface":"grass_paver"}"#, crate::SERVICE), HERBE);
        assert_eq!(r("{}", crate::ROUTE), BITUME);
        assert_eq!(r("{}", crate::CHEMIN), GRAVIER);
        assert_eq!(r("{}", crate::SENTIER), TERRE);
    }

    #[test]
    fn les_voies_de_bus_sont_un_genre_a_part() {
        let g = |t: &str| genre_route(json::lire(t).as_ref()).map(|(g, l, _)| (g, l));
        assert_eq!(g(r#"{"highway":"busway"}"#), Some((crate::BUS, 3.5)));
        assert_eq!(g(r#"{"highway":"bus_guideway","lanes":"2"}"#), Some((crate::BUS, 7.5)));
        assert_eq!(g(r#"{"highway":"service","psv":"designated"}"#), Some((crate::BUS, 3.5)));
        assert_eq!(g(r#"{"highway":"service"}"#), Some((crate::SERVICE, 4.0)));
        // Les rues piétonnes à part des trottoirs ; une place (surface) n'est pas lue
        assert_eq!(g(r#"{"highway":"pedestrian"}"#), Some((crate::RUE_PIETONNE, 6.0)));
        assert_eq!(g(r#"{"highway":"footway","footway":"sidewalk"}"#), Some((crate::PIETONNE, 2.0)));
        assert_eq!(g(r#"{"highway":"pedestrian","area":"yes"}"#), None);
        assert!(crate::carrossable(crate::RUE_PIETONNE) && !crate::carrossable(crate::PIETONNE));
        assert!(crate::carrossable(crate::BUS) && !crate::carrossable(crate::SENTIER));
    }
}
