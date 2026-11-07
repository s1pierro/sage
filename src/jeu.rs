//! La boucle de jeu : le joueur, sa caméra, le décor, et l'interface par-dessus. Le jeu
//! s'appuie sur le moteur (`crate::moteur`) : le monde avance par pas fixes, et `frame`
//! rend la scène que le moteur de rendu doit dessiner.
//!
//! Commandes : un doigt à gauche de l'écran est une manette (il marche dans la direction
//! glissée, d'autant plus vite que le doigt s'éloigne) ; ailleurs, un doigt tourne la vue
//! et deux doigts l'approchent ou l'éloignent.

use crate::draw::{Color, Rect};
use crate::math;
use crate::moniteur::{self, dec, Etat, Moniteur};
use crate::shell::{Layout, Shell, Side};
use crate::campagne::{Campagne, DEPART};
use crate::catalogue::{self, Catalogue, Decor, Entree};
use crate::editeur::{self, Editeur};
use crate::manette::{Evenement, Manettes};
use crate::reglages::Affichage;
use crate::moteur::depot::Depot;
use crate::moteur::{physique, systemes, Camera, Corps, Entite, Format, Horloge, Lieu, Maillages, Materiau, Mobile, Monde, Partie, Rendu, Scene, Squelette, Textures, Tuiles};
use crate::personnage::{self, Modele};
use crate::ui::{id, Id, Sense};
use std::path::PathBuf;
use crate::widgets::{self, Cx, Icon, Scrolls};
use std::sync::Arc;

const MANETTE: Id = id("manette");
const OUVRIR: Id = id("ouvrir");
const MANNEQUIN: Id = id("mannequin");
const MONITEUR: Id = id("moniteur");
const EDITEUR: Id = id("editeur");
const REGLAGES: Id = id("reglages");
/// Les onglets du panneau de gauche.
const ONGLET_MONITEUR: usize = 0;
const ONGLET_EDITEUR: usize = 1;
const ONGLET_REGLAGES: usize = 2;
/// Les sticks tournent la vue à tant de radians par seconde, au bout de leur course.
const VUE_LACET: f32 = 2.4;
const VUE_TANGAGE: f32 = 1.4;

/// Rayon de la manette, en dp.
const RAYON: f32 = 64.0;
/// Rayon du personnage contre le décor, en mètres.
pub const CORPS: f32 = 0.3;
/// Portée du regard, pour désigner ce qui est au centre de la vue.
const PORTEE: f32 = 80.0;
/// Le personnage regarde jusque-là ce que vise la caméra (m).
const REGARD_LOIN: f32 = 30.0;
/// Le tangage de départ de la caméra : à lui, le personnage regarde à l'horizontale.
const TANGAGE: f32 = 0.35;
/// Identifiant stable du joueur.
pub const JOUEUR: &str = "joueur";
/// Tuiles chargées jusqu'à cette distance du joueur (en tuiles), gardées jusqu'à la
/// suivante : 25 tuiles, 320 m de côté.
const RAYON_TUILES: i32 = 2;
const GARDE_TUILES: i32 = 3;
/// En édition, on voit jusqu'à tant de tuiles, sans brouillard : 1 km.
pub const EDITION_TUILES: i32 = 16;
/// En topologie, la caméra s'éloigne jusque-là du point visé (m).
pub const TOPO_LOIN: f32 = 2000.0;

/// L'écart des courbes de niveau, vu de `d` mètres : environ une tous les `d / 40`, au
/// pas de 1, 2, 5, 10, 25, 50 ou 100 m.
pub fn isolignes(d: f32) -> f32 {
    [100.0, 50.0, 25.0, 10.0, 5.0, 2.0].into_iter().find(|p| *p <= d / 40.0).unwrap_or(1.0)
}

/// Le fichier de l'érosion, dans le dossier du monde.
pub const EROSION: &str = "erosion.bin";

/// Le lieu réel importé, dans le dossier du monde.
pub const LIEU: &str = crate::regions::LIEU;

/// Le relief érodé de `graine`, s'il a été enregistré.
fn erosion_relue(depot: &Depot, graine: u32) -> Option<Campagne> {
    let e = crate::erosion::Erosion::depuis_octets(&depot.lire_octets(EROSION)?, graine)?;
    Some(Campagne { graine, erosion: Some(Arc::new(e)), reel: None, monde: None })
}

/// La caméra tourne autour de la tête du personnage : tant de sa hauteur (les yeux).
const PIVOT: f32 = 0.92;

/// L'angle de vue le plus bas en édition, et celui où la vue commence à se lever en
/// jeu (radians, négatif : l'œil sous la cible).
const HORIZON: f32 = -0.15;
/// Le plus loin que la vue se lève en jeu : environ 60° vers le ciel.
pub const LEVER: f32 = -1.05;
/// Le champ de vue s'élargit d'autant (radians), vue levée au plus.
const CHAMP_LEVE: f32 = 0.3;

/// Le joueur ne va pas là où l'eau est plus profonde que cela (m).
pub const PROFONDEUR: f32 = 1.2;

/// En édition, tant de tuiles se chargent par image.
const EDITION_CHARGE: usize = 4;

pub const CIEL: [f32; 3] = [0.70, 0.80, 0.92];

/// Un fichier à déposer hors de l'application.
#[derive(Clone, Debug, PartialEq)]
pub struct Sortie {
    pub nom: String,
    pub mime: &'static str,
    pub octets: Vec<u8>,
}

pub struct App {
    pub monde: Monde,
    pub maillages: Maillages,
    pub textures: Textures,
    pub tuiles: Tuiles,
    horloge: Horloge,
    pub joueur: Entite,
    /// Caméra : lacet, tangage, distance.
    pub cam: (f32, f32, f32),
    /// Le doigt de la manette : là où il s'est posé.
    manette: Option<(f32, f32)>,
    message: Option<(String, f32)>,
    /// Le sélecteur de fichiers est demandé.
    demande: bool,
    /// Les fichiers à déposer hors de l'application.
    sorties: Vec<Sortie>,
    /// Le bâtiment qu'un .glb reçu enrichira : sa région et son identifiant.
    pub remplacer_bati: Option<((i32, i32), String)>,
    /// Le .glb à garder pour le prochain lancement.
    a_garder: Option<Vec<u8>>,
    /// Le joueur a déjà bougé : l'aide s'efface.
    a_bouge: bool,
    /// Les panneaux : à gauche (en haut en portrait), le moniteur.
    pub shell: Shell,
    pub moniteur: Moniteur,
    scrolls: Scrolls,
    /// Ce qu'ouvre le panneau de gauche : le moniteur, l'éditeur ou les réglages, chacun
    /// avec ses onglets.
    pub onglet: usize,
    pub editeur: Editeur,
    /// Les manettes de jeu et leurs profils.
    pub manettes: Manettes,
    /// Les modèles importés.
    pub catalogue: Catalogue,
    /// Le prochain .glb de décor remplacera ce modèle.
    remplacer: Option<String>,
    /// L'onglet ouvert des réglages : 0 la manette, 1 l'affichage.
    pub onglet_reglages: usize,
    /// La distance d'affichage et le brouillard.
    pub affichage: Affichage,
    /// L'éditeur ouvert, le jeu est en pause et le joueur caché : la caméra tourne autour
    /// de ce point du sol, que la manette déplace.
    pub visee: Option<[f32; 3]>,
    /// Le temps qui passe, même en pause : la houle.
    instant: f32,
    /// La carte du monde de l'onglet Topologie, sa texture, et depuis quand son image
    /// n'a pas été refaite.
    carte: Option<crate::carte::Carte>,
    carte_texture: Option<crate::moteur::TextureId>,
    carte_age: f32,
    /// Le générateur de la carte, érodé ou pas encore.
    campagne: Campagne,
    /// L'érosion qui se calcule à part : sa graine, si la carte se régénère avec, et le
    /// relief qui en viendra.
    calcul: Option<(u32, bool, std::sync::mpsc::Receiver<Campagne>)>,
    /// Le relief lointain de toute la carte, au-delà des tuiles chargées.
    lointain: Option<crate::moteur::MaillageId>,
    /// Les bâtiments et les bois lointains d'un lieu réel (« bati/ »).
    lointains: Vec<crate::moteur::MaillageId>,
    /// La matérialisation du lieu réel qui se fait à part : où elle en est, et ce qu'elle
    /// rendra (les octets écrits).
    materialisation: Option<(Arc<crate::regions::Avancement>, std::sync::mpsc::Receiver<Result<u64, String>>)>,
    /// Une régénération de fragment en cours : bâtiments refaits, instances gardées.
    fragment: Option<(Arc<crate::regions::Avancement>, std::sync::mpsc::Receiver<Result<(usize, usize), String>>)>,
    /// Les bâtiments lointains suivent les retouches écrites jusqu'à celle-là.
    version_bati: u64,
}

impl App {
    /// Le monde gardé en mémoire seulement.
    pub fn new(dp: f32) -> App {
        App::avec_dossier(dp, None)
    }

    /// Les données gardées dans `dossier` : le monde sous `monde/` (ses tuiles modifiées
    /// en sont relues, et le joueur reprend où il était), les profils des manettes sous
    /// `reglages/manettes/`.
    pub fn avec_dossier(dp: f32, dossier: Option<PathBuf>) -> App {
        let mut monde = Monde::default();
        let mut maillages = Maillages::default();
        let manettes = Manettes::new(dossier.as_ref().map(|d| d.join("reglages").join("manettes")));
        let affichage = Affichage::ouvrir(dossier.as_ref().map(|d| d.join("reglages")), RAYON_TUILES, crate::moteur::scene::BROUILLARD);
        let (catalogue, relus) = Catalogue::ouvrir(dossier.as_ref().map(|d| d.join("catalogue")));
        let depot = dossier.map(|d| d.join("monde")).map_or_else(Depot::memoire, Depot::dossier);
        let fiche = depot.lire_fiche().unwrap_or_default();
        let joueur = monde.creer(JOUEUR);
        let (pos, cap) = match fiche.joueur {
            Some((t, p, c)) => {
                monde.origine = t;
                (p, c)
            }
            None => ([DEPART[0], 0.0, DEPART[1]], 0.0),
        };
        monde.lieu.mettre(joueur, Lieu::en(pos, cap));
        monde.mobile.mettre(joueur, Mobile { vitesse: 0.0, elan: [cap.sin(), cap.cos()], rayon: CORPS });
        monde.corps.mettre(joueur, Corps::new(1.75));
        // Le monde autour du joueur, tout de suite ; la suite se charge en marchant
        // Le relief érodé, s'il a déjà été calculé ; sinon le brut, le temps du calcul
        // Le monde matérialisé d'abord ; sinon le lieu réel, qui se matérialisera
        let matiere = depot.ou().and_then(|d| crate::regions::Regions::ouvrir(d));
        let reel = if matiere.is_none() { depot.lire_octets(LIEU).and_then(|o| crate::reel::Reel::depuis_octets(&o)) } else { None };
        let a_materialiser = reel.is_some() && depot.ou().is_some() && !cfg!(test);
        let relue = match (matiere, reel) {
            (Some(m), _) => Some(Campagne::materialisee(Arc::new(m))),
            (None, Some(r)) => Some(Campagne::reelle(Arc::new(r))),
            _ => erosion_relue(&depot, fiche.graine),
        };
        let a_calculer = relue.is_none() && depot.ou().is_some() && !cfg!(test);
        let campagne = relue.unwrap_or_else(|| Campagne::brute(fiche.graine));
        let source = Box::new(campagne.clone());
        let mut tuiles = Tuiles::avec(source, depot, fiche, affichage.distance, affichage.distance + GARDE_TUILES - RAYON_TUILES);
        let o = monde.origine;
        tuiles.suivre(&mut monde, &mut maillages, (o[0], o[1]), usize::MAX);
        physique::poser(&mut monde);
        if let Some(l) = monde.lieu.get_mut(joueur) {
            l.retenir();
        }
        let mut a = App {
            monde,
            maillages,
            textures: Textures::default(),
            tuiles,
            horloge: Horloge::default(),
            joueur,
            cam: (0.6, TANGAGE, 4.5),
            manette: None,
            message: None,
            demande: false,
            sorties: Vec::new(),
            remplacer_bati: None,
            a_garder: None,
            a_bouge: false,
            shell: Shell::new(dp),
            moniteur: Moniteur::default(),
            scrolls: Scrolls::new(),
            onglet: ONGLET_MONITEUR,
            editeur: Editeur::default(),
            manettes,
            catalogue,
            remplacer: None,
            onglet_reglages: 0,
            affichage,
            visee: None,
            instant: 0.0,
            carte: None,
            carte_texture: None,
            carte_age: 0.0,
            campagne,
            calcul: None,
            lointain: None,
            lointains: Vec::new(),
            materialisation: None,
            fragment: None,
            version_bati: 0,
        };
        // Les modèles du catalogue : les objets déjà posés les retrouvent
        let mut erreurs = Vec::new();
        for (e, d) in relus {
            match d {
                Ok(d) => {
                    a.installer(&e, d);
                }
                Err(x) => erreurs.push(format!("{} : {x}", e.nom)),
            }
        }
        if let Some(x) = erreurs.first() {
            a.catalogue.erreur = Some(x.clone());
        }
        a.mettre(Modele::depuis_glb(&personnage::mannequin_glb()).expect("le mannequin se relit"));
        a.refaire_lointain();
        if a_calculer {
            let g = a.tuiles.fiche.graine;
            a.lancer_erosion(g, false);
        }
        if a_materialiser {
            a.lancer_materialisation();
        }
        a
    }

    /// Le lieu réel de la carte se matérialise dans un autre fil : ses régions `.stuff`
    /// s'écrivent dans le dossier du monde, puis la carte passe dessus.
    pub fn lancer_materialisation(&mut self) {
        self.arreter_materialisation();
        let (Some(r), Some(d)) = (self.campagne.reel.clone(), self.tuiles.depot.ou().cloned()) else { return };
        let av = Arc::new(crate::regions::Avancement::default());
        let (tx, rx) = std::sync::mpsc::channel();
        let a = av.clone();
        let fils = std::thread::available_parallelism().map_or(2, |n| n.get().saturating_sub(1).max(1));
        std::thread::spawn(move || {
            let _ = tx.send(crate::regions::materialiser(r, &d, &a, fils));
        });
        self.materialisation = Some((av, rx));
        self.dire("Matérialisation du lieu réel en régions .stuff…".into());
    }

    /// Régénère la région du point visé (`rayon` 0), ou avec ses voisines (1), dans un
    /// autre fil. Les retouches en attente s'écrivent d'abord.
    fn lancer_fragment(&mut self, rayon: i32) {
        let Some(m) = self.campagne.monde.clone() else { return };
        if self.fragment.is_some() || self.materialisation.is_some() {
            return;
        }
        if let Err(e) = m.ecrire() {
            self.dire(format!("Bâtiments : {e}"));
            return;
        }
        let o = match self.visee {
            Some(v) => {
                let (i, j) = Tuiles::tuile_de(&self.monde, v);
                [i, j]
            }
            None => self.monde.origine,
        };
        let (ri, rj) = (crate::regions::de_tuile(o[0]), crate::regions::de_tuile(o[1]));
        let e = crate::regions::etendue();
        let liste: Vec<(i32, i32)> = (-rayon..=rayon).flat_map(|dj| (-rayon..=rayon).map(move |di| (ri + di, rj + dj))).filter(|(a, b)| e.contains(a) && e.contains(b)).collect();
        let av = Arc::new(crate::regions::Avancement::default());
        let (tx, rx) = std::sync::mpsc::channel();
        let a = av.clone();
        let fils = std::thread::available_parallelism().map_or(2, |n| n.get().saturating_sub(1).max(1));
        std::thread::spawn(move || {
            let _ = tx.send(m.regenerer(&liste, &a, fils));
        });
        self.fragment = Some((av, rx));
        self.dire(format!("Régénération de {} autour de la région {ri} ; {rj}…", if rayon == 0 { "la région".to_string() } else { "9 régions".to_string() }));
    }

    /// La régénération finie : les tuiles et le lointain se refont.
    fn suivre_fragment(&mut self) {
        let Some((_, rx)) = &self.fragment else { return };
        let fin = match rx.try_recv() {
            Ok(f) => f,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("régénération perdue".into()),
        };
        self.fragment = None;
        match fin {
            Ok((refaits, gardes)) => {
                if let Some(m) = self.campagne.monde.clone() {
                    self.editeur.bati = None;
                    self.prendre_campagne(Campagne::materialisee(m));
                }
                self.dire(format!("Régénéré : {refaits} bâtiment{} refait{}, {gardes} instance{} gardée{}", pl(refaits), pl(refaits), pl(gardes), pl(gardes)));
            }
            Err(e) => self.dire(format!("Régénération : {e}")),
        }
    }

    /// Interrompt la matérialisation en cours, s'il y en a une.
    fn arreter_materialisation(&mut self) {
        if let Some((av, rx)) = self.materialisation.take() {
            av.arret.store(true, std::sync::atomic::Ordering::Relaxed);
            let _ = rx.recv();
        }
    }

    /// Les bâtiments retouchés s'écrivent dans leurs régions quand on n'y touche plus ;
    /// leurs blocs lointains suivent.
    fn suivre_retouches(&mut self) {
        let Some(m) = self.campagne.monde.clone() else { return };
        // Pas d'écriture pendant qu'une régénération refait les régions
        if self.fragment.is_none() && m.calme() {
            if let Err(e) = m.ecrire() {
                self.dire(format!("Bâtiments : {e}"));
            }
        }
        let v = m.version.load(std::sync::atomic::Ordering::Relaxed);
        if v != self.version_bati {
            self.version_bati = v;
            let b = crate::lointain::bati(&self.campagne);
            let k = self.maillages.mettre(crate::lointain::ID_BATI, Format::Terrain, Arc::new(b));
            if !self.lointains.contains(&k) {
                self.lointains.push(k);
            }
        }
    }

    /// Où en est le monde matérialisé, pour l'éditeur.
    fn etat_monde(&self) -> Option<String> {
        if let Some((av, _)) = &self.fragment {
            let (f, t) = (av.faites.load(std::sync::atomic::Ordering::Relaxed), av.total.load(std::sync::atomic::Ordering::Relaxed));
            return Some(format!("régénération, {f} / {t} région{}", pl(t)));
        }
        if let Some((av, _)) = &self.materialisation {
            let (f, t) = (av.faites.load(std::sync::atomic::Ordering::Relaxed), av.total.load(std::sync::atomic::Ordering::Relaxed));
            return Some(format!("matérialisation, {f} / {t} régions"));
        }
        let m = self.campagne.monde.as_ref()?;
        Some(format!("matérialisé, {} régions", m.silhouettes().lues))
    }

    /// La matérialisation finie : la carte passe sur les régions, qui se voient comme le
    /// lieu réel.
    fn suivre_materialisation(&mut self) {
        let Some((_, rx)) = &self.materialisation else { return };
        let fin = match rx.try_recv() {
            Ok(f) => f,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("matérialisation perdue".into()),
        };
        self.materialisation = None;
        let ouvert = self.tuiles.depot.ou().and_then(|d| crate::regions::Regions::ouvrir(d));
        match (fin, ouvert) {
            (Ok(octets), Some(m)) => {
                if let Some(d) = self.tuiles.depot.ou().cloned() {
                    self.tuiles.fiche.catalogues = crate::regions::catalogues(&d);
                }
                let n = crate::regions::toutes().len();
                self.prendre_campagne(Campagne::materialisee(Arc::new(m)));
                self.dire(format!("Monde matérialisé : {n} régions, {} Mo", octets / 1_000_000));
            }
            (Ok(_), None) => self.dire("Matérialisation : les régions ne se relisent pas".into()),
            (Err(e), _) => self.dire(format!("Matérialisation : {e}")),
        }
    }

    /// L'érosion de `graine` se calcule dans un autre fil ; `regenerer` : la carte se
    /// régénère avec, une fois prête.
    pub fn lancer_erosion(&mut self, graine: u32, regenerer: bool) {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(Campagne::erodee(graine));
        });
        self.calcul = Some((graine, regenerer, rx));
    }

    /// Le relief lointain, refait pour le générateur de la carte (hors des tests : il
    /// couvre toute la carte et se calcule en une fraction de seconde optimisé).
    fn refaire_lointain(&mut self) {
        if cfg!(test) {
            return;
        }
        let v = crate::lointain::sommets(&self.campagne);
        self.lointain = Some(self.maillages.mettre(crate::lointain::ID, Format::Terrain, Arc::new(v)));
        self.lointains.clear();
        for (id, v) in [(crate::lointain::ID_BATI, crate::lointain::bati(&self.campagne)), (crate::vegetation::ID_LOINTAIN, crate::vegetation::lointain(&self.campagne))] {
            if !v.is_empty() {
                self.lointains.push(self.maillages.mettre(id, Format::Terrain, Arc::new(v)));
            }
        }
    }

    /// La graine dont l'érosion se calcule.
    pub fn erosion_en_cours(&self) -> Option<u32> {
        self.calcul.as_ref().map(|c| c.0)
    }

    /// L'érosion calculée arrive : elle s'enregistre, puis la carte se régénère avec, ou
    /// remplace le relief brut de la carte actuelle.
    fn suivre_erosion(&mut self) {
        let Some((g, regen, rx)) = &self.calcul else { return };
        let (g, regen) = (*g, *regen);
        let c = match rx.try_recv() {
            Ok(c) => c,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.calcul = None;
                return;
            }
        };
        self.calcul = None;
        if let Some(e) = &c.erosion {
            if let Err(x) = self.tuiles.depot.ecrire_octets(EROSION, &e.octets()) {
                self.dire(format!("{EROSION} : {x}"));
            }
        }
        if regen {
            self.regenerer(c);
        } else if g == self.tuiles.fiche.graine {
            self.prendre_campagne(c);
        }
    }

    /// Le relief de la carte change de générateur (le même, érodé) : les tuiles se
    /// rechargent, le joueur se repose sur le sol.
    pub fn prendre_campagne(&mut self, c: Campagne) {
        self.campagne = c.clone();
        self.tuiles.changer_source(&mut self.monde, &mut self.maillages, Box::new(c));
        let o = self.monde.origine;
        self.tuiles.suivre(&mut self.monde, &mut self.maillages, (o[0], o[1]), 9);
        physique::poser(&mut self.monde);
        if let Some(x) = self.monde.lieu.get_mut(self.joueur) {
            x.retenir();
        }
        self.carte = None;
        self.refaire_lointain();
        if self.campagne.erosion.is_some() {
            self.dire("Le relief est érodé".into());
        }
    }

    /// Le joueur prend ce personnage : son maillage est enregistré sous l'identifiant du
    /// modèle, l'ancien est libéré.
    fn mettre(&mut self, m: Modele) {
        let ancien = self.modele().map(|m| format!("personnage/{}", m.id));
        let cle = format!("personnage/{}", m.id);
        if let Some(a) = ancien {
            if a != cle {
                self.maillages.ote(&a);
            }
            self.textures.oter_sous(&format!("{a}/"));
        }
        // Une texture par matériau qui en a une, une partie par matériau employé
        let textures: Vec<Option<crate::moteur::TextureId>> = m.materiaux.iter().enumerate().map(|(i, x)| x.image.clone().map(|im| self.textures.mettre(&format!("{cle}/{i}"), im))).collect();
        let parties = m
            .parties
            .iter()
            .map(|p| {
                let x = &m.materiaux[p.materiau];
                Partie { premier: p.premier, nombre: p.nombre, materiau: Materiau { texture: textures[p.materiau], opacite: x.opacite, alpha: x.alpha, deux_cotes: false } }
            })
            .collect();
        let k = self.maillages.mettre_parties(&cle, Format::Peau, m.sommets.clone(), parties);
        let local = math::mul(&translation([0.0, -m.sol * m.echelle, 0.0]), &math::scale(m.echelle));
        self.monde.rendu.mettre(self.joueur, Rendu { maillage: k, local });
        // Le corps a la taille du personnage
        if let Some(c) = self.monde.corps.get_mut(self.joueur) {
            c.hauteur = m.hauteur;
        }
        self.monde.squelette.mettre(self.joueur, Squelette::nouveau(Arc::new(m)));
    }

    pub fn modele(&self) -> Option<&Modele> {
        self.monde.squelette.get(self.joueur).map(|s| &*s.modele)
    }

    pub fn lieu(&self) -> Lieu {
        *self.monde.lieu.get(self.joueur).expect("le joueur a un lieu")
    }

    pub fn mobile(&self) -> Mobile {
        *self.monde.mobile.get(self.joueur).expect("le joueur est mobile")
    }

    pub fn dire(&mut self, s: String) {
        self.message = Some((s, 4.0));
    }

    /// Un modèle du catalogue entre dans le moteur : ses textures, son maillage, sa
    /// collision ; ses objets déjà posés se refont.
    fn installer(&mut self, e: &Entree, d: Decor) -> crate::moteur::ModeleId {
        let cle = format!("modele/{}", e.id);
        self.textures.oter_sous(&format!("{cle}/"));
        let textures: Vec<Option<crate::moteur::TextureId>> = d.materiaux.iter().enumerate().map(|(i, x)| x.image.clone().map(|im| self.textures.mettre(&format!("{cle}/{i}"), im))).collect();
        let parties = d
            .parties
            .iter()
            .map(|&(premier, nombre, k)| {
                let x = &d.materiaux[k];
                Partie { premier, nombre, materiau: Materiau { texture: textures[k], opacite: x.opacite, alpha: x.alpha, deux_cotes: x.deux_cotes } }
            })
            .collect();
        let g = crate::moteur::Gabarit { sommets: Arc::new(d.sommets), parties, triangles: Arc::new(d.triangles), boite: d.boite };
        let k = self.monde.modeles.mettre(&e.id, &e.nom, g);
        crate::moteur::tuiles::rafraichir(&mut self.monde, &mut self.maillages, k);
        k
    }

    /// Un .glb devient le niveau détaillé du bâtiment `ident` de la région (ri, rj) : il
    /// se voit tout de suite, sa définition s'écrira dans la région avec les retouches.
    fn enrichir_bati(&mut self, ri: i32, rj: i32, ident: &str, nom: &str, octets: &[u8]) {
        let Some(m) = self.campagne.monde.clone() else {
            self.dire("Pas de monde matérialisé : rien à enrichir".into());
            return;
        };
        let Some(avant) = m.batiment(ri, rj, ident) else {
            self.dire(format!("{ident} : bâtiment introuvable"));
            return;
        };
        let d = match catalogue::decoder_glb(octets, None) {
            Ok(d) => d,
            Err(e) => {
                self.dire(format!("{nom} : {e}"));
                return;
            }
        };
        let (n, manque) = (d.triangles.len(), d.manquantes);
        let mut x = avant.clone();
        x.enrichir(format!("def-{ident}"), Arc::new(d), true, None);
        x.retouche = true;
        m.retoucher(x);
        self.tuiles.refaire_bati(&mut self.monde, &mut self.maillages, avant.tuile);
        let mut s = format!("{ident} enrichi : {n} triangles");
        if n > catalogue::TRIANGLES_LOURD {
            s.push_str(", lourd à dessiner");
        }
        if manque > 0 {
            s.push_str(&format!(", {manque} texture{} manquante{}", if manque > 1 { "s" } else { "" }, if manque > 1 { "s" } else { "" }));
        }
        self.dire(s);
    }

    /// Les bâtiments enrichis qui attendent leur modèle : il entre dans le moteur (ses
    /// textures, sa géométrie) une fois pour tous ceux qui le partagent.
    fn habiller_enrichis(&mut self) {
        for (e, x) in std::mem::take(&mut self.tuiles.a_habiller) {
            if !self.monde.vivante(e) {
                continue;
            }
            let m = match self.monde.modeles.chercher(&x.cle).filter(|&m| self.monde.modeles.get(m).is_some_and(|m| m.gabarit.is_some())) {
                Some(m) => m,
                None => {
                    let d = &x.modele;
                    let cle = format!("modele/{}", x.cle);
                    let textures: Vec<Option<crate::moteur::TextureId>> = d.materiaux.iter().enumerate().map(|(i, m)| m.image.clone().map(|im| self.textures.mettre(&format!("{cle}/{i}"), im))).collect();
                    let parties = d
                        .parties
                        .iter()
                        .map(|&(premier, nombre, k)| {
                            let m = &d.materiaux[k];
                            Partie { premier, nombre, materiau: Materiau { texture: textures[k], opacite: m.opacite, alpha: m.alpha, deux_cotes: m.deux_cotes } }
                        })
                        .collect();
                    let g = crate::moteur::Gabarit { sommets: Arc::new(d.sommets.clone()), parties, triangles: Arc::new(d.triangles.clone()), boite: d.boite };
                    self.monde.modeles.mettre(&x.cle, d.nom.as_deref().unwrap_or(&x.id), g)
                }
            };
            crate::moteur::tuiles::habiller(&mut self.monde, &mut self.maillages, e, m);
        }
    }

    /// Retourne les faces du modèle `id`, pour tous ses objets.
    pub fn retourner_modele(&mut self, id: &str) {
        let r = self.catalogue.octets(id).ok_or_else(|| "fichier du modèle illisible".to_string()).and_then(|o| self.catalogue.retourner(id, &o));
        match r {
            Ok((e, d)) => {
                let s = format!("« {} » : faces {}", e.nom, if d.retourne { "retournées" } else { "remises comme dans le fichier" });
                self.installer(&e, d);
                self.dire(s);
            }
            Err(x) => self.dire(format!("{id} : {x}")),
        }
    }

    /// Demande un .glb qui remplacera le modèle `id`, pour tous ses objets.
    pub fn remplacer_modele(&mut self, id: &str) {
        self.remplacer = Some(id.to_string());
        self.demande = true;
    }

    /// Un .glb reçu. Avec une peau, c'est un personnage : il remplace le joueur s'il se
    /// lit, sinon l'ancien reste. Sans peau, c'est un décor : il entre au catalogue, et
    /// l'éditeur s'ouvre pour le poser.
    pub fn charger(&mut self, nom: &str, octets: &[u8]) {
        if let Some(r) = crate::reel::Reel::depuis_octets(octets) {
            self.importer_lieu(r, octets);
            return;
        }
        if let Some(((ri, rj), ident)) = self.remplacer_bati.take() {
            if gda::sage::lire_glb(octets).is_some() && !catalogue::est_personnage(octets) {
                self.enrichir_bati(ri, rj, &ident, nom, octets);
                return;
            }
        }
        if gda::sage::lire_glb(octets).is_some() && !catalogue::est_personnage(octets) {
            let remplace = self.remplacer.take();
            match self.catalogue.importer(nom, octets, remplace.as_deref()) {
                Ok((e, d, deja)) => {
                    let (n, manque, retourne) = (d.triangles.len(), d.manquantes, d.retourne);
                    let k = self.installer(&e, d);
                    let combien = self.monde.objet.iter().filter(|(_, o)| o.genre == crate::moteur::Genre::Modele(k)).count();
                    let mut s = match (remplace.is_some(), deja) {
                        (true, _) => format!("« {} » remplacé : {combien} objet{} mis à jour", e.nom, if combien > 1 { "s" } else { "" }),
                        (false, true) => format!("« {} » est déjà au catalogue", e.nom),
                        (false, false) => format!("« {} » ajouté au catalogue : {n} triangles", e.nom),
                    };
                    if retourne && !deja {
                        s.push_str(", faces retournées (elles regardaient vers l'intérieur)");
                    }
                    if n > catalogue::TRIANGLES_LOURD {
                        s.push_str(", lourd à dessiner");
                    }
                    if manque > 0 {
                        s.push_str(&format!(", {manque} texture{} manquante{}", if manque > 1 { "s" } else { "" }, if manque > 1 { "s" } else { "" }));
                    }
                    self.dire(s);
                    if remplace.is_none() {
                        // L'éditeur ouvert, prêt à le poser
                        if !self.shell.start.open {
                            self.shell.start.toggle();
                        }
                        self.onglet = ONGLET_EDITEUR;
                        self.editeur.mode = editeur::Mode::Ajouter(crate::moteur::Genre::Modele(k));
                    }
                }
                Err(x) => self.dire(format!("{nom} : {x}")),
            }
            return;
        }
        match Modele::depuis_glb(octets) {
            Ok(m) => {
                let n = m.animations.len();
                let s = format!("« {} » : {} os, {n} animation{}", m.nom, m.armature.os.len(), if n > 1 { "s" } else { "" });
                self.mettre(m);
                self.a_garder = Some(octets.to_vec());
                self.dire(s);
            }
            Err(e) => self.dire(format!("{nom} illisible : {e}")),
        }
    }

    /// Un lieu réel reçu : il s'enregistre, et la carte repart de zéro avec lui.
    pub fn importer_lieu(&mut self, r: crate::reel::Reel, octets: &[u8]) {
        if let Err(e) = self.tuiles.depot.ecrire_octets(LIEU, octets) {
            self.dire(format!("{LIEU} : {e}"));
            return;
        }
        let nom = r.nom.clone();
        self.regenerer(Campagne::reelle(Arc::new(r)));
        self.dire(format!("Lieu réel « {nom} » : la carte repart de zéro"));
        if !cfg!(test) {
            self.lancer_materialisation();
        }
    }

    /// Relu au lancement : le dernier personnage ouvert, sans message.
    pub fn reprendre(&mut self, octets: &[u8]) {
        if let Ok(m) = Modele::depuis_glb(octets) {
            self.mettre(m);
        }
    }

    /// Écrit tout ce qui attend, et où est le joueur : l'application passe en
    /// arrière-plan.
    pub fn enregistrer(&mut self) {
        let l = self.lieu();
        self.tuiles.fiche.joueur = Some((self.monde.origine, l.pos, l.cap));
        self.tuiles.tout_ecrire(&self.monde);
        if let Some(m) = &self.campagne.monde {
            if let Err(e) = m.ecrire() {
                self.dire(format!("Bâtiments : {e}"));
            }
        }
    }

    /// Ce qu'envoie une manette de jeu.
    pub fn manette(&mut self, ev: Evenement) {
        self.manettes.recevoir(ev);
    }

    /// Ouvre le panneau sur `onglet`, ou le ferme s'il y est déjà.
    fn basculer(&mut self, onglet: usize) {
        let ouvert = self.shell.start.open && self.onglet == onglet;
        if ouvert || !self.shell.start.open {
            self.shell.start.toggle();
        }
        self.onglet = onglet;
        self.manettes.capture = None;
    }

    /// L'éditeur est ouvert : les appuis dans la vue choisissent et posent des objets.
    pub fn edite(&self) -> bool {
        self.shell.start.open && self.onglet == ONGLET_EDITEUR
    }

    /// Les fichiers à déposer hors de l'application (Téléchargements).
    pub fn prendre_sorties(&mut self) -> Vec<Sortie> {
        std::mem::take(&mut self.sorties)
    }

    pub fn prendre_demande(&mut self) -> bool {
        std::mem::take(&mut self.demande)
    }

    pub fn prendre_a_garder(&mut self) -> Option<Vec<u8>> {
        self.a_garder.take()
    }

    /// Mesures de la plateforme pour le moniteur, en secondes : intervalle réel depuis
    /// l'image précédente, temps du jeu, de l'envoi au rendu, de l'échange des tampons.
    pub fn mesurer(&mut self, intervalle: f32, jeu: f32, rendu: f32, echange: f32) {
        self.moniteur.plateforme(intervalle, jeu, rendu, echange);
    }

    fn etat(&self) -> Etat {
        let (l, m) = (self.lieu(), self.mobile());
        let (melange, course, allures, pieds) = self.monde.squelette.get(self.joueur).map_or((0.0, 0.0, String::new(), String::new()), |s| (s.pas.marche, s.pas.course, s.locomotion.resume(), if s.pieds.jambes.is_empty() { String::new() } else { s.pieds.resume() }));
        let (buste, regard) = self.monde.squelette.get(self.joueur).map_or((String::new(), String::new()), |s| {
            let (b, r) = s.posture.resume();
            (if s.posture.buste.dos.is_empty() { String::new() } else { b }, if s.posture.buste.tete.is_none() { String::new() } else { r })
        });
        Etat {
            entites: self.monde.nombre(),
            pas_total: self.horloge.pas,
            pos: l.pos,
            absolu: self.monde.absolu(l.pos),
            cap: l.cap,
            vitesse: m.vitesse,
            melange,
            course,
            allures,
            appui: self.appui(),
            vise: self.vise(),
            pieds,
            buste,
            regard,
            tuiles: self.tuiles.chargees.len(),
            tuile: self.monde.origine,
            generation_ms: self.tuiles.derniere_ms,
        }
    }

    /// Ce qui porte le joueur, et sa pente en degrés ; rien s'il est en l'air.
    fn appui(&self) -> Option<(String, f32)> {
        let c = self.monde.corps.get(self.joueur)?;
        if !c.au_sol {
            return None;
        }
        let p = self.lieu().pos;
        let (_, _, e) = physique::sol(&self.monde, p[0], p[2], p[1] + 0.01)?;
        let nom = e.and_then(|e| self.monde.ident.get(e).cloned()).unwrap_or_else(|| "relief".into());
        Some((nom, c.normale[1].clamp(-1.0, 1.0).acos().to_degrees()))
    }

    /// Le point que vise le centre de la vue : ce qu'il touche, sinon un point au loin.
    fn regarde(&self) -> Option<[f32; 3]> {
        let hauteur = self.modele().map_or(1.75, |m| m.hauteur);
        let p = self.lieu().pos;
        let o = [p[0], p[1] + hauteur * PIVOT, p[2]];
        let (sy, cy) = self.cam.0.sin_cos();
        // La caméra plonge vers le joueur : le regard, lui, est horizontal au tangage de
        // départ
        let (sp, cp) = (self.cam.1 - TANGAGE).sin_cos();
        let d = [-sy * cp, -sp, -cy * cp];
        let t = physique::lancer(&self.monde, o, d, REGARD_LOIN).map_or(REGARD_LOIN, |i| i.distance);
        Some([o[0] + d[0] * t, o[1] + d[1] * t, o[2] + d[2] * t])
    }

    /// Ce que vise le centre de la vue, au-delà du joueur, et à quelle distance.
    fn vise(&self) -> Option<(String, f32)> {
        let hauteur = self.modele().map_or(1.75, |m| m.hauteur);
        let p = self.lieu().pos;
        let cible = [p[0], p[1] + hauteur * PIVOT, p[2]];
        let (sy, cy) = self.cam.0.sin_cos();
        let (sp, cp) = self.cam.1.sin_cos();
        let i = physique::lancer(&self.monde, cible, [-sy * cp, -sp, -cy * cp], PORTEE)?;
        let nom = i.entite.and_then(|e| self.monde.ident.get(e).cloned()).unwrap_or_else(|| "relief".into());
        Some((nom, i.distance))
    }

    /// Le jeu s'anime sans cesse.
    pub fn animating(&self) -> bool {
        true
    }

    /// Avant et droite au sol, vus de la caméra.
    fn axes_camera(&self) -> ([f32; 2], [f32; 2]) {
        let (s, c) = self.cam.0.sin_cos();
        ([-s, -c], [c, -s])
    }

    /// Un pas de simulation, la manette en `j` (x vers la droite, y vers le bas de l'écran).
    pub fn avancer(&mut self, j: [f32; 2]) {
        let (av, dr) = self.axes_camera();
        let dir = [av[0] * -j[1] + dr[0] * j[0], av[1] * -j[1] + dr[1] * j[0]];
        if dir[0].hypot(dir[1]) > 0.05 {
            self.a_bouge = true;
        }
        systemes::retenir(&mut self.monde);
        systemes::marcher(&mut self.monde, self.joueur, dir);
        let avant = self.lieu().pos;
        physique::deplacer(&mut self.monde);
        // L'eau profonde arrête le joueur ; s'il y est déjà, il n'en sort que vers moins
        // profond
        let p = self.lieu().pos;
        let fond = |x: f32, z: f32| systemes::hauteur_sol(&self.monde, x, z).unwrap_or(self.campagne.mer());
        let (ici, la) = (fond(p[0], p[2]), fond(avant[0], avant[2]));
        if ici < self.campagne.mer() - PROFONDEUR && ici < la {
            if let Some(l) = self.monde.lieu.get_mut(self.joueur) {
                l.pos = avant;
            }
        }
        // Le bord de la carte est un mur
        let (lo, hi) = crate::moteur::tuiles::bords(&self.monde);
        if let Some(l) = self.monde.lieu.get_mut(self.joueur) {
            l.pos[0] = l.pos[0].clamp(lo[0] + CORPS, hi[0] - CORPS);
            l.pos[2] = l.pos[2].clamp(lo[1] + CORPS, hi[1] - CORPS);
        }
        systemes::suivre_origine(&mut self.monde, self.joueur);
    }

    /// Une carte neuve, nue, tirée de `graine` : tout ce qui a été retouché ou posé est
    /// effacé, le catalogue des modèles reste. Le joueur revient au départ.
    pub fn regenerer(&mut self, c: Campagne) {
        let graine = c.graine;
        self.campagne = c.clone();
        let reel = c.reel.is_some() || c.monde.is_some();
        // Le monde matérialisé d'avant ne vaut plus pour la nouvelle carte
        self.arreter_materialisation();
        if c.monde.is_none() {
            if let Some(d) = self.tuiles.depot.ou().cloned() {
                if let Err(e) = crate::regions::effacer(&d) {
                    self.dire(format!("{} : {e}", crate::regions::DOSSIER));
                }
            }
        }
        self.tuiles.repartir(&mut self.monde, &mut self.maillages, Box::new(c));
        if !reel {
            if let Err(e) = self.tuiles.depot.effacer_fichier(LIEU) {
                self.dire(format!("{LIEU} : {e}"));
            }
        }
        self.refaire_lointain();
        self.tuiles.fiche.graine = graine;
        self.editeur.oublier();
        self.monde.choisie = None;
        self.monde.origine = [0, 0];
        let cap = self.lieu().cap;
        let p = [DEPART[0], 0.0, DEPART[1]];
        self.monde.lieu.mettre(self.joueur, Lieu::en(p, cap));
        if let Some(m) = self.monde.mobile.get_mut(self.joueur) {
            m.vitesse = 0.0;
        }
        self.tuiles.suivre(&mut self.monde, &mut self.maillages, (0, 0), 9);
        physique::poser(&mut self.monde);
        let l = self.lieu();
        if let Some(x) = self.monde.lieu.get_mut(self.joueur) {
            x.retenir();
        }
        if self.visee.is_some() {
            self.visee = Some(l.pos);
        }
        self.tuiles.fiche.joueur = Some((self.monde.origine, l.pos, l.cap));
        self.tuiles.tout_ecrire(&self.monde);
        self.carte = None;
        self.dire(format!("Carte régénérée, nue : graine {graine}"));
    }

    /// Une image : commandes, pas de simulation, animation, caméra, interface.
    pub fn frame(&mut self, cx: &mut Cx, ecran: Rect, dt: f32) -> Scene {
        let dp = cx.dp;
        // L'onglet Carte : le panneau s'élargit d'une carte du monde
        self.shell.carte = self.edite() && self.editeur.onglet == editeur::CARTE;
        self.carte_age += dt;
        self.suivre_erosion();
        self.suivre_materialisation();
        self.suivre_fragment();
        if let Some(r) = self.editeur.fragment.take() {
            self.lancer_fragment(r);
        }
        self.editeur.fragment_possible = self.campagne.monde.is_some() && self.fragment.is_none() && self.materialisation.is_none();
        self.editeur.erosion = self.erosion_en_cours();
        self.editeur.lieu = self.campagne.nom_lieu();
        self.editeur.monde = self.etat_monde();
        self.editeur.regions = self.campagne.monde.clone();
        self.suivre_retouches();
        if std::mem::take(&mut self.editeur.importer) {
            self.demande = true;
        }
        let l = self.shell.layout(ecran, dt);
        let w = l.workspace;
        // Manette : un doigt posé dans la moitié gauche de l'espace de travail
        let zone = Rect::new(w.x, w.y + 64.0 * dp, w.w * 0.45, (w.h - 64.0 * dp).max(0.0));
        let doigt = cx.ui.drag(MANETTE, zone);
        let mut j = [0.0f32; 2];
        match doigt {
            Some(d) => {
                let o = *self.manette.get_or_insert((d.x, d.y));
                let (dx, dy) = ((d.x - o.0) / (RAYON * dp), (d.y - o.1) / (RAYON * dp));
                let l = dx.hypot(dy);
                j = if l > 1.0 { [dx / l, dy / l] } else { [dx, dy] };
            }
            None => self.manette = None,
        }
        // Vue : un doigt tourne, deux approchent
        let g = cx.ui.take_gesture();
        // L'éditeur ouvert : le jeu s'arrête, la caméra vise un point du sol, plus loin et
        // plus haut ; refermé, le joueur reprend où il était
        let edite = self.edite();
        match (edite, self.visee) {
            (true, None) => self.visee = Some(self.lieu().pos),
            (false, Some(_)) => {
                self.visee = None;
                self.cam.2 = self.cam.2.min(14.0);
                // Le sol a pu monter sous lui
                let p = self.lieu().pos;
                if let Some(h) = systemes::hauteur_sol(&self.monde, p[0], p[2]).filter(|h| *h > p[1]) {
                    if let Some(l) = self.monde.lieu.get_mut(self.joueur) {
                        l.pos[1] = h;
                        l.retenir();
                    }
                }
            }
            _ => {}
        }
        // En topologie : à la verticale, et de très haut
        let topo = edite && self.editeur.onglet == editeur::TOPOLOGIE;
        let (loin, haut) = match (edite, topo) {
            (true, true) => (TOPO_LOIN, std::f32::consts::FRAC_PI_2),
            (true, false) => (150.0, 1.5),
            _ => (14.0, 1.3),
        };
        self.cam.2 = self.cam.2.min(loin);
        self.cam.1 = self.cam.1.min(haut);
        // En jeu, la vue se lève vers le ciel ; en édition, pas sous l'horizon
        let bas = if edite { HORIZON } else { LEVER };
        self.cam.1 = self.cam.1.max(bas);
        // Un outil de topologie pris, le doigt seul dans la vue peint : il ne tourne pas
        let pinceau = if self.editeur.peint() && edite { cx.ui.doigt_seul().filter(|p| w.contains(p.0, p.1)) } else { None };
        if pinceau.is_none() {
            self.cam.0 -= g.orbit.0 * 0.006;
            self.cam.1 = (self.cam.1 + g.orbit.1 * 0.004).clamp(bas, haut);
        }
        if g.zoom.is_finite() && g.zoom > 0.0 {
            self.cam.2 = (self.cam.2 / g.zoom).clamp(1.5, loin);
        }
        // La manette de jeu : le stick de marche si le doigt n'y est pas, le stick de vue,
        // les gâchettes pour approcher
        let c = self.manettes.commandes();
        if doigt.is_none() && c.marcher != [0.0; 2] {
            j = c.marcher;
        }
        let (sens, inverser) = self.manettes.active().map_or((1.0, false), |a| (a.profil.sensibilite, a.profil.inverser));
        self.cam.0 -= c.vue[0] * VUE_LACET * sens * dt;
        let vy = if inverser { -c.vue[1] } else { c.vue[1] };
        self.cam.1 = (self.cam.1 + vy * VUE_TANGAGE * sens * dt).clamp(bas, haut);
        if c.tenu("approcher") {
            self.cam.2 = (self.cam.2 * (-1.5 * dt).exp()).clamp(1.5, loin);
        }
        if c.tenu("eloigner") {
            self.cam.2 = (self.cam.2 * (1.5 * dt).exp()).clamp(1.5, loin);
        }
        for (action, onglet) in [("moniteur", ONGLET_MONITEUR), ("editeur", ONGLET_EDITEUR), ("reglages", ONGLET_REGLAGES)] {
            if c.appui(action) {
                self.basculer(onglet);
            }
        }
        if c.appui("fermer") {
            self.shell.start.open = false;
            self.manettes.capture = None;
        }

        let (n, u) = match self.visee {
            // En pause : la manette, et deux doigts qui glissent, déplacent le point visé
            Some(mut v) => {
                let (av, dr) = {
                    let (s, c) = self.cam.0.sin_cos();
                    ([-s, -c], [c, -s])
                };
                let vite = self.cam.2.max(4.0) * dt;
                let k = self.cam.2 / w.h.max(1.0) * 1.2;
                let (dx, dz) = (dr[0] * j[0] - av[0] * j[1], dr[1] * j[0] - av[1] * j[1]);
                v[0] += dx * vite - dr[0] * g.pan.0 * k + av[0] * g.pan.1 * k;
                v[2] += dz * vite - dr[1] * g.pan.0 * k + av[1] * g.pan.1 * k;
                // On voit au-delà du bord, on n'y va pas
                let (lo, hi) = crate::moteur::tuiles::bords(&self.monde);
                v[0] = v[0].clamp(lo[0], hi[0]);
                v[2] = v[2].clamp(lo[1], hi[1]);
                self.visee = Some(v);
                (0, 1.0)
            }
            // Simulation à pas fixes ; l'image tombe entre deux pas
            None => {
                let (n, u) = self.horloge.avancer(dt);
                for _ in 0..n {
                    self.avancer(j);
                }
                (n, u)
            }
        };
        // Une tuile au plus par image : le chargement ne fait pas d'à-coups ; en pause,
        // elles suivent le point visé
        let o = match self.visee {
            Some(v) => {
                let (i, j) = Tuiles::tuile_de(&self.monde, v);
                [i, j]
            }
            None => self.monde.origine,
        };
        // Les tuiles suivent la distance d'affichage, celle de l'édition en pause ; elles se
        // chargent au fil des images
        let r = self.rayon_vu();
        self.tuiles.rayon = r;
        self.tuiles.garde = r + GARDE_TUILES - RAYON_TUILES;
        self.tuiles.suivre(&mut self.monde, &mut self.maillages, (o[0], o[1]), if self.visee.is_some() { EDITION_CHARGE } else { 1 });
        self.habiller_enrichis();
        self.tuiles.vieillir(&self.monde, dt);
        if self.visee.is_none() {
            // Le personnage regarde ce que vise la caméra
            let regard = self.regarde();
            if let Some(s) = self.monde.squelette.get_mut(self.joueur) {
                s.regard = regard;
            }
            systemes::animer(&mut self.monde, dt, u);
        }

        let camera = self.camera(w, u);
        // L'éditeur : un appui bref dans la vue, manette comprise
        let mut tenue = false;
        if edite && self.editeur.onglet == editeur::TOPOLOGIE {
            let point = pinceau.and_then(|(x, y)| physique::lancer(&self.monde, camera.oeil, camera.rayon(x, y), TOPO_LOIN * 2.0)).map(|i| i.point);
            self.editeur.peindre(&mut self.monde, &mut self.maillages, &mut self.tuiles, point, dt);
            self.cercle(cx, &camera, w, point);
            // L'altitude sous le doigt, à côté de lui
            if let (Some(p), Some((x, y))) = (point, pinceau) {
                let s = format!("{} m", dec(p[1], 1));
                let dp = cx.dp;
                let lw = cx.text.measure(&s, (15.0 * dp).round()) + 24.0 * dp;
                let r = Rect::new((x + 28.0 * dp).min(w.right() - lw - 4.0 * dp), (y - 76.0 * dp).max(w.y + 4.0 * dp), lw, 32.0 * dp);
                cx.dl.rrect(r, r.h / 2.0, [0x1B, 0x1B, 0x24, 220]);
                cx.text_at(r.x + 12.0 * dp, r.cy(), &s, 15.0, [255, 255, 255, 255]);
            }
        } else {
            self.editeur.peindre(&mut self.monde, &mut self.maillages, &mut self.tuiles, None, dt);
        }
        if edite && self.editeur.onglet == editeur::AJOUT {
            tenue = self.editeur.poignees(cx, &camera, w, &mut self.monde, &mut self.maillages, &mut self.tuiles);
        }
        if edite {
            if let Some((None | Some(MANETTE), x, y)) = cx.ui.appui_bref().filter(|_| self.editeur.onglet == editeur::AJOUT && !tenue) {
                if w.contains(x, y) {
                    self.editeur.appui(&mut self.monde, &mut self.maillages, &mut self.tuiles, &camera, x, y, self.cam.0);
                }
            }
            // À la manette : le centre de la vue
            if c.appui("choisir") {
                self.editeur.appui(&mut self.monde, &mut self.maillages, &mut self.tuiles, &camera, w.cx(), w.cy(), self.cam.0);
            }
            if c.appui("annuler") {
                self.editeur.annuler(&mut self.monde, &mut self.maillages, &mut self.tuiles);
            }
        } else if self.monde.choisie.take().is_some() || self.editeur.bati.take().is_some() {
            self.editeur.mode = editeur::Mode::Choisir;
        }
        self.editeur.suivre_bati(&mut self.monde, &mut self.maillages);
        if let Some(m) = self.editeur.message.take() {
            self.dire(m);
        }
        self.interface(cx, w, doigt.is_some().then_some(j), dt);
        self.panneau(cx, &l);
        self.poignee(cx, &l, ecran);
        let mut scene = Scene::extraire(&self.monde, &self.maillages, &self.textures, camera, u);
        if let Some(k) = self.editeur.surbrillance_maillage {
            for d in scene.dessins.iter_mut().filter(|d| d.maillage == k) {
                d.choisi = true;
            }
        }
        // En édition, pas de brouillard
        scene.brouillard = if self.visee.is_some() { 1.0e7 } else { self.affichage.brouillard_vu() };
        // Le relief lointain, sauf sous les tuiles chargées (un mètre de recouvrement)
        if let Some(k) = self.lointain {
            let o = self.monde.origine;
            let t = crate::moteur::TAILLE;
            let place = translation([-(o[0] as f32) * t, 0.0, -(o[1] as f32) * t]);
            scene.dessins.push(crate::moteur::scene::Dessin { maillage: k, place, os: Vec::new(), choisi: false });
            let mut trouees = vec![k];
            // Les bâtiments et les bois lointains (« bati/ » : la topologie ne les garde pas)
            for &b in &self.lointains {
                scene.dessins.push(crate::moteur::scene::Dessin { maillage: b, place, os: Vec::new(), choisi: false });
                trouees.push(b);
            }
            let c = match self.visee {
                Some(v) => Tuiles::tuile_de(&self.monde, v),
                None => (o[0], o[1]),
            };
            let r = self.rayon_vu();
            let (a, b) = (Tuiles::coin(&self.monde, (c.0 - r, c.1 - r)), Tuiles::coin(&self.monde, (c.0 + r + 1, c.1 + r + 1)));
            scene.trou = Some((trouees, [a[0] + 1.0, a[2] + 1.0, b[0] - 1.0, b[2] - 1.0]));
        }
        self.instant += dt;
        scene.mer = Some(self.campagne.mer());
        scene.temps = self.instant % 3600.0;
        // Les ombres portées autour du joueur ; en pause, autour du point visé, sans lui
        scene.soleil.centre = self.lieu().interpole(u).0;
        if let Some(v) = self.visee {
            scene.soleil.centre = v;
            if let Some(k) = self.monde.rendu.get(self.joueur).map(|r| r.maillage) {
                scene.dessins.retain(|d| d.maillage != k);
            }
        }
        // En topologie : le relief seul, et ses courbes de niveau, plus espacées de haut
        self.editeur.isolignes = 0.0;
        if topo {
            let m = &self.maillages;
            scene.dessins.retain(|d| m.get(d.maillage).is_some_and(|x| x.id.starts_with("tuile/")));
            self.editeur.isolignes = isolignes(self.cam.2);
            scene.isolignes = self.editeur.isolignes;
        }
        let etat = self.etat();
        self.moniteur.image(dt, n, &scene, etat);
        scene
    }

    /// Caméra à la troisième personne derrière le joueur, jamais sous le sol. En pause,
    /// elle tourne autour du point visé, à un mètre au-dessus du relief.
    fn camera(&self, w: Rect, u: f32) -> Camera {
        let hauteur = self.modele().map_or(1.75, |m| m.hauteur);
        let (p, _) = self.lieu().interpole(u);
        let (cible, d) = match self.visee {
            Some(v) => {
                let h = systemes::hauteur_sol(&self.monde, v[0], v[2]).unwrap_or(v[1]).max(self.campagne.mer());
                ([v[0], h + 1.0, v[2]], self.cam.2)
            }
            None => ([p[0], p[1] + hauteur * PIVOT, p[2]], self.cam.2 * hauteur / 1.75),
        };
        // Au-delà de l'horizon, la vue se lève : la caméra descend au ras du sol et se
        // rapproche, le champ s'élargit (de 0 à 1)
        let leve = if self.visee.is_none() { ((HORIZON - self.cam.1) / (HORIZON - LEVER)).clamp(0.0, 1.0) } else { 0.0 };
        let d = d * (1.0 - 0.35 * leve);
        let p = if self.visee.is_some() { [cible[0], cible[1] - 1.0, cible[2]] } else { p };
        let (sy, cy) = self.cam.0.sin_cos();
        let (sp, cp) = self.cam.1.sin_cos();
        let mut oeil = [cible[0] + d * sy * cp, cible[1] + d * sp, cible[2] + d * cy * cp];
        // Le haut de la vue : penché avec elle, il reste juste même à la verticale
        let haut = [-sy * sp, cp, -cy * sp];
        // Vue levée : l'œil glisse au ras du sol au lieu d'y entrer
        if leve > 0.0 {
            let sol = systemes::hauteur_sol(&self.monde, oeil[0], oeil[2]).unwrap_or(p[1]);
            oeil[1] = oeil[1].max(sol + 0.4).max(self.campagne.mer() + 0.4);
        }
        // Rien entre le joueur et l'œil : la caméra s'avance devant ce qui le cacherait
        let vers = [oeil[0] - cible[0], oeil[1] - cible[1], oeil[2] - cible[2]];
        if let Some(i) = physique::lancer(&self.monde, cible, vers, d).filter(|_| self.visee.is_none()) {
            let k = ((i.distance - 0.25) / d).max(0.05);
            oeil = [cible[0] + vers[0] * k, cible[1] + vers[1] * k, cible[2] + vers[2] * k];
        }
        // Jamais sous le sol
        let sol = systemes::hauteur_sol(&self.monde, oeil[0], oeil[2]).unwrap_or(p[1]);
        oeil[1] = oeil[1].max(sol + 0.4).max(self.campagne.mer() + 0.4);
        // Vue levée : l'œil, posé plus haut que son orbite, garde l'angle demandé ; le
        // personnage glisse vers le bas de l'image
        let vise = if self.visee.is_none() && self.cam.1 < 0.0 { [oeil[0] - sy * cp * d, oeil[1] - sp * d, oeil[2] - cy * cp * d] } else { cible };
        Camera {
            vue: math::look_at(oeil, vise, haut),
            // En édition, on voit loin : le plan proche recule, la profondeur reste précise
            proj: math::perspective(1.0 + CHAMP_LEVE * leve, w.w / w.h.max(1.0), if self.visee.is_some() { (d * 0.002).max(0.5) } else { 0.1 }, if self.visee.is_some() { ((self.rayon_vu() as f32 + 1.5) * crate::moteur::TAILLE * 1.5).max(500.0) + d } else { crate::reglages::VISIBILITE * 1.1 }),
            oeil,
            fenetre: [w.x, w.y, w.w, w.h],
        }
    }

    /// La carte du monde dans `r` : refaite quand une tuile a été retouchée (au plus deux
    /// fois par seconde), avec le point visé, la zone chargée, la direction de la vue et
    /// le joueur. La toucher ou y glisser y emmène le point visé.
    fn dessiner_carte(&mut self, cx: &mut Cx, r: Rect) {
        use crate::carte::{depuis_carte, vers_carte, Carte};
        let d = cx.dp;
        let graine = self.campagne.graine;
        if self.carte.as_ref().is_none_or(|c| c.graine != graine) {
            self.carte = Some(Carte::new(&self.campagne, &mut self.monde, &self.tuiles));
        }
        let carte = self.carte.as_mut().expect("faite");
        if carte.vue != self.tuiles.retouches && (self.carte_texture.is_none() || self.carte_age > 0.5) {
            carte.suivre(&self.monde, &self.tuiles);
            self.carte_texture = Some(self.textures.mettre("carte", Arc::new(carte.image())));
            self.carte_age = 0.0;
        }
        let Some(tex) = self.carte_texture else { return };
        cx.dl.rrect(Rect::new(r.x - d, r.y - d, r.w + 2.0 * d, r.h + 2.0 * d), 0.0, widgets::LINE);
        cx.dl.image(r, tex.0);
        let ecran = |p: [f64; 3]| {
            let q = vers_carte(p[0], p[2]);
            [r.x + q[0] * r.w, r.y + q[1] * r.h]
        };
        // La zone chargée autour du centre, et le point visé avec la direction de la vue
        let centre = self.visee.unwrap_or(self.lieu().pos);
        let c = self.monde.absolu(centre);
        let demi = (self.rayon_vu() as f64 + 0.5) * crate::moteur::TAILLE as f64;
        let (a, b) = (ecran([c[0] - demi, 0.0, c[2] - demi]), ecran([c[0] + demi, 0.0, c[2] + demi]));
        let blanc: Color = [255, 255, 255, 200];
        for (p, q) in [([a[0], a[1]], [b[0], a[1]]), ([b[0], a[1]], [b[0], b[1]]), ([b[0], b[1]], [a[0], b[1]]), ([a[0], b[1]], [a[0], a[1]])] {
            cx.dl.line(p, q, 1.5 * d, blanc);
        }
        let v = ecran(c);
        let (s, co) = self.cam.0.sin_cos();
        cx.dl.line(v, [v[0] - s * 18.0 * d, v[1] - co * 18.0 * d], 2.5 * d, [0xF0, 0xC0, 0x40, 255]);
        cx.dl.rrect(Rect::new(v[0] - 5.0 * d, v[1] - 5.0 * d, 10.0 * d, 10.0 * d), 5.0 * d, [0x1B, 0x1B, 0x24, 230]);
        cx.dl.rrect(Rect::new(v[0] - 3.5 * d, v[1] - 3.5 * d, 7.0 * d, 7.0 * d), 3.5 * d, [255, 255, 255, 255]);
        // Le joueur, s'il n'est pas au point visé
        let j = ecran(self.monde.absolu(self.lieu().pos));
        if (j[0] - v[0]).hypot(j[1] - v[1]) > 6.0 * d {
            cx.dl.rrect(Rect::new(j[0] - 4.0 * d, j[1] - 4.0 * d, 8.0 * d, 8.0 * d), 4.0 * d, [0xE0, 0x55, 0x50, 255]);
        }
        // Le doigt y emmène le point visé
        if let (Some(g), Some(v)) = (cx.ui.drag(id("carte-du-monde"), r), self.visee.as_mut()) {
            let p = depuis_carte(((g.x - r.x) / r.w).clamp(0.0, 1.0), ((g.y - r.y) / r.h).clamp(0.0, 1.0));
            let o = self.monde.origine;
            let t = crate::moteur::TAILLE as f64;
            v[0] = (p[0] - o[0] as f64 * t) as f32;
            v[2] = (p[1] - o[1] as f64 * t) as f32;
        }
    }

    /// L'onglet Carte, à côté de la carte : le point visé, où le joueur peut être
    /// téléporté, et le joueur.
    fn panneau_carte(&mut self, cx: &mut Cx, b: &mut widgets::Body) {
        let km = |p: [f64; 3]| format!("{} · {} km", dec((p[0] / 1000.0) as f32, 2), dec((p[2] / 1000.0) as f32, 2));
        cx.section(b, "POINT VISÉ");
        if let Some(v) = self.visee {
            cx.mesure(b, "Position", &km(self.monde.absolu(v)));
            let sol = systemes::hauteur_sol(&self.monde, v[0], v[2]);
            let alt = match sol {
                None => "pas encore chargé".to_string(),
                Some(h) if h < self.campagne.mer() => format!("sous l'eau, fond à {} m", dec(self.campagne.mer() - h, 1)),
                Some(h) => format!("{} m", dec(h, 1)),
            };
            cx.mesure(b, "Sol", &alt);
            if cx.button(b, id("carte-teleporter"), "Téléporter le joueur ici") {
                self.teleporter();
            }
        }
        cx.paragraph(b, "Touchez la carte ou glissez-y le doigt pour viser ailleurs.");
        if let Some(nom) = self.campagne.nom_lieu() {
            cx.section(b, &nom.to_uppercase());
            cx.paragraph(b, crate::reel::SOURCES);
        }
        cx.section(b, "JOUEUR");
        let p = self.lieu().pos;
        cx.mesure(b, "Position", &km(self.monde.absolu(p)));
        cx.mesure(b, "Altitude", &format!("{} m", dec(p[1], 1)));
        if cx.liens(b, id("carte-au-joueur"), &["Viser le joueur"], None).is_some() {
            self.visee = Some(p);
        }
    }

    /// Le joueur va au point visé, posé sur le sol ; pas en eau profonde, ni là où le sol
    /// n'est pas encore chargé.
    pub fn teleporter(&mut self) {
        let Some(v) = self.visee else { return };
        let Some(h) = systemes::hauteur_sol(&self.monde, v[0], v[2]) else {
            self.dire("Le sol n'est pas encore chargé là : attendez un instant".into());
            return;
        };
        if h < self.campagne.mer() - PROFONDEUR {
            self.dire("L'eau est trop profonde là".into());
            return;
        }
        if let Some(l) = self.monde.lieu.get_mut(self.joueur) {
            l.pos = [v[0], h, v[2]];
            l.retenir();
        }
        if let Some(m) = self.monde.mobile.get_mut(self.joueur) {
            m.vitesse = 0.0;
        }
        self.visee = Some([v[0], h, v[2]]);
        self.dire("Joueur téléporté".into());
    }

    /// Les tuiles chargées autour du centre : la distance d'affichage, ou celle de
    /// l'édition en pause.
    fn rayon_vu(&self) -> i32 {
        if self.visee.is_some() { EDITION_TUILES } else { self.affichage.distance }
    }

    /// Le cercle du pinceau, sur le relief : sous le doigt, sinon au point visé.
    fn cercle(&self, cx: &mut Cx, camera: &Camera, w: Rect, point: Option<[f32; 3]>) {
        if !self.editeur.peint() {
            return;
        }
        let Some(c) = point.or(self.visee) else { return };
        let r = self.editeur.pinceau.rayon;
        let n = 48;
        let points: Vec<Option<[f32; 2]>> = (0..=n)
            .map(|k| {
                let a = k as f32 / n as f32 * std::f32::consts::TAU;
                let (x, z) = (c[0] + r * a.sin(), c[2] + r * a.cos());
                let y = systemes::hauteur_sol(&self.monde, x, z).unwrap_or(c[1]) + 0.1;
                camera.ecran([x, y, z]).filter(|p| w.contains(p[0], p[1]))
            })
            .collect();
        let couleur = if point.is_some() { [255, 255, 255, 230] } else { [255, 255, 255, 140] };
        for s in points.windows(2) {
            if let (Some(a), Some(b)) = (s[0], s[1]) {
                cx.dl.line(a, b, 2.5 * cx.dp, couleur);
            }
        }
    }

    /// Sur l'espace de travail `w` : la pastille du moniteur, les pilules, le nom du
    /// personnage, la manette sous le doigt, l'aide et les messages.
    fn interface(&mut self, cx: &mut Cx, w: Rect, manette: Option<[f32; 2]>, dt: f32) {
        let dp = cx.dp;
        let m = 14.0 * dp;
        let (taille, h) = (52.0 * dp, 44.0 * dp);
        // Une pastille par onglet du panneau de gauche : elle l'ouvre sur son onglet, ou le
        // ferme s'il y est déjà
        let mut x = w.x + m;
        for (cle, icone, onglet) in [(MONITEUR, Icon::Pouls, ONGLET_MONITEUR), (EDITEUR, Icon::Crayon, ONGLET_EDITEUR), (REGLAGES, Icon::Engrenage, ONGLET_REGLAGES)] {
            let ouvert = self.shell.start.open && self.onglet == onglet;
            if cx.pastille(cle, Rect::new(x, w.y + m, taille, taille), icone, ouvert) {
                self.basculer(onglet);
            }
            x += taille + 10.0 * dp;
        }
        let y = w.y + m + (taille - h) / 2.0;
        let l = "Ouvrir un .glb";
        let lw = cx.pill_width(l);
        if cx.pill(OUVRIR, Rect::new(x, y, lw, h), l, false) {
            self.demande = true;
        }
        x += lw + 10.0 * dp;
        if self.modele().is_some_and(|m| m.id != "mannequin") {
            let l = "Mannequin";
            let lw = cx.pill_width(l);
            if cx.pill(MANNEQUIN, Rect::new(x, y, lw, h), l, false) {
                self.mettre(Modele::depuis_glb(&personnage::mannequin_glb()).expect("mannequin"));
                self.a_garder = Some(Vec::new());
                self.dire("Le mannequin revient".into());
            }
            x += lw + 10.0 * dp;
        }
        // Le nom s'efface quand la place manque
        if let Some(md) = self.modele() {
            let n = md.nom.clone();
            if x + cx.text.measure(&n, (15.0 * dp).round()) + 2.0 * m < w.right() {
                cx.text_right(w.right() - m, y + h / 2.0, &n, 15.0, widgets::TEXT);
            }
        }
        // La manette sous le doigt
        if let (Some(o), Some(j)) = (self.manette, manette) {
            let r = RAYON * dp;
            let anneau: Color = [255, 255, 255, 90];
            cx.dl.rrect(Rect::new(o.0 - r, o.1 - r, 2.0 * r, 2.0 * r), r, anneau);
            let k = 26.0 * dp;
            let (px, py) = (o.0 + j[0] * r, o.1 + j[1] * r);
            cx.dl.rrect(Rect::new(px - k, py - k, 2.0 * k, 2.0 * k), k, [255, 255, 255, 200]);
        }
        if !self.a_bouge {
            let t = "À gauche, glissez pour marcher ; à droite, pour tourner la vue ; à deux doigts, pour l'approcher";
            let px = (15.0 * dp).round();
            let mut cy = w.bottom() - m - 10.0 * dp;
            for ligne in cx.text.wrap(t, px, w.w - 2.0 * m).into_iter().rev() {
                cx.text_at(w.x + m, cy, &ligne, 15.0, [0x1B, 0x1B, 0x24, 200]);
                cy -= 22.0 * dp;
            }
        }
        if let Some((s, reste)) = &mut self.message {
            *reste -= dt;
            if *reste <= 0.0 {
                self.message = None;
            } else {
                let s = s.clone();
                let lw = (cx.text.measure(&s, (15.0 * dp).round()) + 32.0 * dp).min(w.w - 2.0 * m);
                let r = Rect::new(w.cx() - lw / 2.0, w.y + m + taille + 10.0 * dp, lw, 40.0 * dp);
                cx.dl.rrect(r, r.h / 2.0, [0x1B, 0x1B, 0x24, 220]);
                cx.text_at(r.x + 16.0 * dp, r.cy(), &s, 15.0, [255, 255, 255, 255]);
            }
        }
    }

    /// Le panneau de gauche (en haut en portrait) : le moniteur.
    fn panneau(&mut self, cx: &mut Cx, l: &Layout) {
        if self.shell.start.anim == 0.0 {
            return;
        }
        let d = cx.dp;
        let r = l.start;
        cx.ui.region_add(id("panneau"), r, Sense::Sink);
        cx.dl.rrect(r, 0.0, widgets::SURFACE);
        let t = d.max(1.0);
        let bord = if l.landscape { Rect::new(r.right() - t, r.y, t, r.h) } else { Rect::new(r.x, r.bottom() - t, r.w, t) };
        cx.dl.rrect(bord, 0.0, widgets::LINE);
        // En-tête : l'onglet et ✕
        let tete = 60.0 * d;
        let fermer = Rect::new(r.right() - 50.0 * d, r.y + 8.0 * d, 44.0 * d, 44.0 * d);
        if cx.ui.click(id("fermer-panneau"), fermer, None) {
            self.shell.start.open = false;
            self.manettes.capture = None;
        }
        cx.dl.rrect(fermer, 22.0 * d, widgets::CHIP);
        cx.icon(fermer.inset(11.0 * d), Icon::Close, widgets::TEXT);
        cx.dl.rrect(Rect::new(r.x, r.y + tete - d, r.w, t), 0.0, widgets::LINE);
        // Chaque pastille a ses onglets
        let onglets: &[&str] = match self.onglet {
            ONGLET_EDITEUR => &editeur::ONGLETS,
            ONGLET_REGLAGES => &["Manette", "Affichage"],
            _ => &["Moniteur"],
        };
        let choisi = match self.onglet {
            ONGLET_REGLAGES => self.onglet_reglages,
            ONGLET_EDITEUR => self.editeur.onglet,
            _ => 0,
        };
        let k = cx.tabs(id("onglets-panneau"), Rect::new(r.x + 12.0 * d, r.y + 11.0 * d, r.w - 74.0 * d, 38.0 * d), onglets, choisi);
        if self.onglet == ONGLET_EDITEUR {
            self.editeur.onglet = k;
        }
        if self.onglet == ONGLET_REGLAGES && k != self.onglet_reglages {
            self.onglet_reglages = k;
            self.manettes.capture = None;
        }
        let corps = Rect::new(r.x, r.y + tete, r.w, r.h - tete);
        if self.onglet == ONGLET_REGLAGES && self.onglet_reglages == 1 {
            let mut b = cx.body(id("corps-affichage"), corps, &self.scrolls);
            crate::reglages::affichage(cx, &mut b, &mut self.affichage);
            cx.body_end(b, &mut self.scrolls);
            return;
        }
        if self.onglet == ONGLET_REGLAGES {
            let mut b = cx.body(id("corps-manette"), corps, &self.scrolls);
            crate::reglages::manette(cx, &mut b, &mut self.manettes);
            cx.body_end(b, &mut self.scrolls);
            return;
        }
        if self.onglet == ONGLET_EDITEUR {
            // L'onglet Carte : le point visé et le joueur à gauche, la carte à droite, sur
            // toute la hauteur
            let mut corps = corps;
            if self.editeur.onglet == editeur::CARTE {
                let c = (corps.h - 16.0 * d).min(corps.w - 240.0 * d);
                if c >= 120.0 * d {
                    let outils = Rect::new(corps.x, corps.y, corps.w - c - 16.0 * d, corps.h);
                    cx.dl.rrect(Rect::new(outils.right(), corps.y, t, corps.h), 0.0, widgets::LINE);
                    self.dessiner_carte(cx, Rect::new(corps.right() - c - 8.0 * d, corps.y + (corps.h - c) / 2.0, c, c));
                    corps = outils;
                }
            }
            let mut b = cx.body(crate::ui::sub_id(id("corps-editeur"), self.editeur.onglet), corps, &self.scrolls);
            if self.editeur.onglet == editeur::CARTE {
                self.panneau_carte(cx, &mut b);
            } else if self.editeur.onglet == editeur::TOPOLOGIE {
                self.editeur.panneau_topologie(cx, &mut b, &mut self.monde, &mut self.maillages, &mut self.tuiles);
            } else {
                self.editeur.panneau(cx, &mut b, &mut self.monde, &mut self.maillages, &mut self.tuiles);
            }
            if let Some(id) = self.editeur.remplacement.take() {
                self.remplacer_modele(&id);
            }
            if let Some(id) = self.editeur.retournement.take() {
                self.retourner_modele(&id);
            }
            if let Some(bt) = self.editeur.export_bati.take() {
                self.sorties.push(Sortie { nom: format!("{}.glb", bt.id), mime: "model/gltf-binary", octets: crate::enrichi::glb(&bt) });
            }
            if let Some(k) = self.editeur.enrichir_bati.take() {
                self.remplacer_bati = Some(k);
                self.demande = true;
            }
            if let Some(g) = self.editeur.regeneration.take() {
                self.lancer_erosion(g, true);
            }
            cx.body_end(b, &mut self.scrolls);
            return;
        }
        let mut b = cx.body(id("corps-moniteur"), corps, &self.scrolls);
        let a = self.moniteur.affiche.clone();
        let ms = |v: f32| format!("{} ms", dec(v, 1));

        cx.section(&mut b, "IMAGES");
        cx.mesure(&mut b, "Cadence", &format!("{} i/s", dec(a.cadence, 0)));
        cx.mesure(&mut b, "Intervalle", &format!("{} · pire {}", ms(a.intervalle), dec(a.pire, 1)));
        let images: Vec<f32> = self.moniteur.images.iter().copied().collect();
        cx.barres(&mut b, &images, 50.0, &[1000.0 / 60.0, 1000.0 / 30.0], 1000.0 / 50.0);
        if let Some([jeu, rendu, echange]) = a.temps {
            cx.mesure(&mut b, "Jeu", &ms(jeu));
            cx.mesure(&mut b, "Envoi au rendu", &ms(rendu));
            cx.mesure(&mut b, "Échange des tampons", &ms(echange));
        }

        cx.section(&mut b, "SIMULATION");
        cx.mesure(&mut b, "Pas fixes", &format!("{} /s", dec(a.pas_seconde, 0)));
        cx.mesure(&mut b, "Pas par image", &dec(a.pas_image, 2));
        cx.mesure(&mut b, "Pas depuis le début", &a.etat.pas_total.to_string());

        cx.section(&mut b, "MONDE");
        cx.mesure(&mut b, "Entités", &a.etat.entites.to_string());
        cx.mesure(&mut b, "Objets dessinés", &a.dessins.to_string());
        cx.mesure(&mut b, "Triangles", &a.triangles.to_string());
        cx.mesure(&mut b, "Maillages", &format!("{} · {}", a.maillages, moniteur::octets(a.octets)));
        let e = &a.etat;
        cx.mesure(&mut b, "Tuiles chargées", &e.tuiles.to_string());
        cx.mesure(&mut b, "Chargement d'une tuile", &ms(e.generation_ms));
        cx.mesure(&mut b, "Tuiles écrites", &self.tuiles.ecrites.to_string());

        cx.section(&mut b, "JOUEUR");
        let e = &a.etat;
        cx.mesure(&mut b, "Tuile", &format!("{} ; {}", e.tuile[0], e.tuile[1]));
        cx.mesure(&mut b, "Dans la tuile", &format!("{} ; {}", dec(e.pos[0], 1), dec(e.pos[2], 1)));
        cx.mesure(&mut b, "Position absolue", &format!("{} ; {}", dec(e.absolu[0] as f32, 0), dec(e.absolu[2] as f32, 0)));
        cx.mesure(&mut b, "Altitude", &format!("{} m", dec(e.pos[1], 1)));
        cx.mesure(&mut b, "Cap", &format!("{} °", dec(e.cap.to_degrees().rem_euclid(360.0), 0)));
        cx.mesure(&mut b, "Vitesse", &format!("{} m/s", dec(e.vitesse, 2)));
        cx.mesure(&mut b, "Mouvement", &format!("{} %", dec(e.melange * 100.0, 0)));
        cx.mesure(&mut b, "Course", &format!("{} %", dec(e.course * 100.0, 0)));
        match &e.appui {
            Some((nom, pente)) => {
                cx.mesure(&mut b, "Appui", nom);
                cx.mesure(&mut b, "Pente", &format!("{} °", dec(*pente, 0)));
            }
            None => cx.mesure(&mut b, "Appui", "en l'air"),
        }
        match &e.vise {
            Some((nom, d)) => cx.mesure(&mut b, "Visé", &format!("{nom} · {} m", dec(*d, 1))),
            None => cx.mesure(&mut b, "Visé", "rien"),
        }
        if !e.pieds.is_empty() {
            cx.mesure(&mut b, "Pieds", &e.pieds);
        }
        if !e.buste.is_empty() {
            cx.mesure(&mut b, "Buste", &e.buste);
        }
        if !e.regard.is_empty() {
            cx.mesure(&mut b, "Regard", &e.regard);
        }
        if !e.allures.is_empty() {
            cx.paragraph(&mut b, &e.allures);
        }
        cx.body_end(b, &mut self.scrolls);
    }

    /// La poignée qui redimensionne le panneau ; un double appui rétablit sa taille.
    fn poignee(&mut self, cx: &mut Cx, l: &Layout, ecran: Rect) {
        if self.shell.start.anim < 1.0 {
            return;
        }
        let d = cx.dp;
        let cle = id("poignee-panneau");
        let bande = self.shell.handle(l, Side::Start);
        if let Some(g) = cx.ui.drag(cle, bande) {
            self.shell.resize(Side::Start, ecran, g.x, g.y);
        }
        if cx.ui.double_tapped(cle) {
            self.shell.reset_size(Side::Start, ecran);
        }
        let c = if cx.ui.pressed(cle) { widgets::ACCENT } else { [0x6C, 0x6C, 0x80, 140] };
        let (long, epais) = (52.0 * d, 5.0 * d);
        let prise = if l.landscape {
            Rect::new(bande.cx() - epais / 2.0, bande.cy() - long / 2.0, epais, long)
        } else {
            Rect::new(bande.cx() - long / 2.0, bande.cy() - epais / 2.0, long, epais)
        };
        cx.dl.rrect(prise, epais / 2.0, c);
    }
}

pub fn translation(t: [f32; 3]) -> math::Mat4 {
    [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., t[0], t[1], t[2], 1.]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sans obstacles, sur un sol plat : pour mesurer le déplacement seul.
    fn a_plat() -> App {
        let mut a = App::new(1.0);
        a.monde.obstacles = Default::default();
        for (_, r) in a.monde.relief.iter_mut() {
            r.h.iter_mut().for_each(|h| *h = 0.0);
        }
        physique::poser(&mut a.monde);
        a.monde.lieu.get_mut(a.joueur).unwrap().retenir();
        a
    }

    #[test]
    fn la_manette_avance_dans_le_sens_de_la_vue() {
        let mut a = a_plat();
        a.cam.0 = 0.0; // caméra en +Z, qui regarde vers -Z
        let depart = a.monde.absolu(a.lieu().pos);
        for _ in 0..60 {
            a.avancer([0.0, -1.0]);
        }
        let l = a.lieu();
        let p = a.monde.absolu(l.pos);
        assert!(p[2] < depart[2] - 1.0 && (p[0] - depart[0]).abs() < 1e-3, "vers -Z : {p:?}");
        assert!((l.cap.abs() - std::f32::consts::PI).abs() < 0.05, "tourné vers -Z : {}", l.cap);
        // À droite de l'écran : +X
        for _ in 0..60 {
            a.avancer([1.0, 0.0]);
        }
        assert!(a.monde.absolu(a.lieu().pos)[0] > p[0] + 1.0);
    }

    #[test]
    fn le_decor_arrete_le_joueur() {
        let mut a = a_plat();
        let mur = a.monde.creer("mur");
        let p = a.lieu().pos;
        a.monde.lieu.mettre(mur, Lieu::en(p, 0.0));
        a.monde.obstacles.mettre(mur, vec![crate::moteur::Forme::Boite(crate::moteur::Aabb { centre: [0.0, 1.0, -3.0], demi: [1.0; 3] })]);
        a.cam.0 = 0.0;
        for _ in 0..240 {
            a.avancer([0.0, -1.0]);
        }
        assert!(a.lieu().pos[2] > p[2] - 2.0 - CORPS - 1e-3, "devant le bloc : {:?}", a.lieu().pos);
    }

    #[test]
    fn le_joueur_marche_sur_le_relief() {
        let mut a = App::new(1.0);
        a.monde.obstacles = Default::default();
        physique::poser(&mut a.monde);
        for _ in 0..300 {
            a.avancer([0.3, -1.0]);
            let p = a.lieu().pos;
            let h = systemes::hauteur_sol(&a.monde, p[0], p[2]).unwrap();
            assert_eq!(p[1], h);
        }
    }

    #[test]
    fn marcher_loin_charge_et_decharge_les_tuiles() {
        let mut a = a_plat();
        a.cam.0 = std::f32::consts::FRAC_PI_2; // la vue regarde vers -X
        let n = (2 * RAYON_TUILES + 1).pow(2) as usize;
        assert_eq!(a.tuiles.chargees.len(), n);
        let depart = a.monde.absolu(a.lieu().pos);
        // Trois cents mètres vers -X, une image par pas
        for _ in 0..(300.0 / systemes::VITESSE_MAX * 60.0) as usize {
            a.avancer([0.0, -1.0]);
            let o = a.monde.origine;
            a.tuiles.suivre(&mut a.monde, &mut a.maillages, (o[0], o[1]), 1);
        }
        let p = a.monde.absolu(a.lieu().pos);
        assert!(depart[0] - p[0] > 280.0, "{depart:?} → {p:?}");
        assert!(a.monde.origine[0] <= -4);
        assert!((0.0..crate::moteur::TAILLE).contains(&a.lieu().pos[0]), "le joueur reste près de l'origine");
        for _ in 0..30 {
            let o = a.monde.origine;
            a.tuiles.suivre(&mut a.monde, &mut a.maillages, (o[0], o[1]), 1);
        }
        let o = a.monde.origine;
        assert!(a.tuiles.chargees.len() <= (2 * GARDE_TUILES + 1).pow(2) as usize);
        assert!(a.tuiles.chargees.keys().all(|&(i, j)| (i - o[0]).abs().max((j - o[1]).abs()) <= GARDE_TUILES));
        assert!(a.tuiles.chargees.contains_key(&(o[0], o[1])));
        // Les maillages suivent : le simplifié de chaque tuile, le complet des proches
        let terrains = a.maillages.tous().iter().filter(|(_, m)| m.format == Format::Terrain).count();
        let n = a.tuiles.chargees.len();
        assert!((n + 9..=n + 25).contains(&terrains), "{terrains} pour {n} tuiles");
    }

    #[test]
    fn un_fichier_illisible_garde_le_personnage() {
        let mut a = App::new(1.0);
        a.charger("x.glb", b"pas un glb");
        assert_eq!(a.modele().unwrap().id, "mannequin");
        assert!(a.prendre_a_garder().is_none());
        a.charger("m.glb", &personnage::mannequin_glb());
        assert!(a.prendre_a_garder().is_some());
    }

    #[test]
    fn changer_de_personnage_libere_l_ancien_maillage() {
        let mut a = App::new(1.0);
        let personnages = |a: &App| a.maillages.tous().into_iter().map(|(_, m)| m.id).filter(|i| i.starts_with("personnage/")).collect::<Vec<_>>();
        assert_eq!(personnages(&a), ["personnage/mannequin"]);
        let mut m = Modele::depuis_glb(&personnage::mannequin_glb()).unwrap();
        m.id = "autre".into();
        a.mettre(m);
        assert_eq!(personnages(&a), ["personnage/autre"]);
    }

    #[test]
    fn la_scene_porte_le_decor_et_le_joueur_anime() {
        use crate::text::Text;
        use crate::ui::Ui;
        let mut a = App::new(1.0);
        let (mut ui, mut text, mut dl) = (Ui::new(1.0), Text::new(), crate::draw::DrawList::default());
        let mut cx = Cx { ui: &mut ui, text: &mut text, dl: &mut dl, dp: 1.0 };
        let s = a.frame(&mut cx, Rect::new(0.0, 0.0, 800.0, 600.0), 1.0 / 30.0);
        assert!(s.dessins.len() > 25, "les tuiles et le joueur : {}", s.dessins.len());
        assert!(s.dessins.iter().any(|d| d.os.len() == 19), "le mannequin et ses os");
        assert_eq!(s.soleil.centre, a.lieu().pos, "les ombres autour du joueur");
        assert!(s.textures.iter().any(|(_, t)| t.id == "personnage/mannequin/1" || t.id.starts_with("personnage/mannequin/")), "la veste du mannequin");
        assert_eq!(s.camera.fenetre, [0.0, 0.0, 800.0, 600.0]);
    }

    #[test]
    fn la_pastille_ouvre_le_moniteur_et_l_espace_se_reduit() {
        use crate::text::Text;
        use crate::ui::{Event, Ui};
        let mut a = App::new(1.0);
        let (mut ui, mut text, mut dl) = (Ui::new(1.0), Text::new(), crate::draw::DrawList::default());
        let ecran = Rect::new(0.0, 0.0, 1600.0, 900.0);
        let mut s = None;
        for i in 0..40 {
            if i == 2 {
                ui.event(Event::Down { id: 0, x: 40.0, y: 40.0 }, 0.1);
                ui.event(Event::Up { id: 0 }, 0.15);
            }
            dl.clear();
            let mut cx = Cx { ui: &mut ui, text: &mut text, dl: &mut dl, dp: 1.0 };
            s = Some(a.frame(&mut cx, ecran, 1.0 / 30.0));
            ui.end_frame();
        }
        assert!(a.shell.start.open);
        let f = s.unwrap().camera.fenetre;
        assert!(f[0] > 100.0 && f[2] < 1500.0, "la 3D laisse la place au panneau : {f:?}");
        assert!(a.moniteur.affiche.cadence > 25.0);
        assert_eq!(a.moniteur.affiche.etat.tuiles, 25);
        assert!(a.moniteur.affiche.etat.entites > 26);
    }

    #[test]
    fn le_monde_modifie_se_retrouve_au_lancement_suivant() {
        use crate::moteur::{Genre, Objet};
        let d = std::env::temp_dir().join(format!("sage-monde-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let mut a = App::avec_dossier(1.0, Some(d.clone()));
        // Un poteau devant le joueur, un objet généré ôté, le joueur un peu plus loin
        let p = a.lieu().pos;
        let poteau = a.tuiles.ajouter(&mut a.monde, &mut a.maillages, Objet::neuf(Genre::Poteau), Lieu::en([p[0] + 2.0, p[1], p[2]], 0.0));
        let id = a.monde.ident.get(poteau).cloned().unwrap();
        let bloc = a.tuiles.ajouter(&mut a.monde, &mut a.maillages, Objet::neuf(Genre::Bloc), Lieu::en([p[0] - 4.0, p[1], p[2]], 0.0));
        let ote = a.monde.ident.get(bloc).cloned().unwrap();
        a.tuiles.oter(&mut a.monde, &mut a.maillages, bloc);
        a.cam.0 = 0.0;
        for _ in 0..90 {
            a.avancer([0.0, -1.0]);
        }
        let ici = a.monde.absolu(a.lieu().pos);
        a.enregistrer();
        assert!(d.join("monde/monde.json").exists());
        let b = App::avec_dossier(1.0, Some(d.clone()));
        assert!(b.monde.trouver(&id).is_some(), "le poteau est relu");
        assert!(b.monde.trouver(&ote).is_none(), "l'objet ôté ne revient pas");
        let la = b.monde.absolu(b.lieu().pos);
        assert!((la[0] - ici[0]).abs() < 0.01 && (la[2] - ici[2]).abs() < 0.01, "{la:?} {ici:?}");
        // Le prochain objet ne reprend pas un identifiant déjà donné
        let mut b = b;
        let n = b.tuiles.nouvel_id();
        assert_ne!(n, id);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn la_pastille_de_l_editeur_ouvre_son_onglet_et_l_appui_choisit() {
        use crate::text::Text;
        use crate::ui::{Event, Ui};
        let mut a = App::new(1.0);
        // La carte est nue : des blocs tout autour du joueur, l'un sera en vue
        let p = a.lieu().pos;
        for k in 0..8 {
            let t = k as f32 * std::f32::consts::TAU / 8.0;
            let (x, z) = (p[0] + 6.0 * t.sin(), p[2] + 6.0 * t.cos());
            let y = systemes::hauteur_sol(&a.monde, x, z).unwrap() - 0.2;
            a.tuiles.ajouter(&mut a.monde, &mut a.maillages, crate::moteur::Objet::neuf(crate::moteur::Genre::Bloc), Lieu::en([x, y, z], 0.0));
        }
        let (mut ui, mut text, mut dl) = (Ui::new(1.0), Text::new(), crate::draw::DrawList::default());
        let ecran = Rect::new(0.0, 0.0, 1600.0, 900.0);
        let mut image = |a: &mut App, ui: &mut Ui| {
            dl.clear();
            let mut cx = Cx { ui, text: &mut text, dl: &mut dl, dp: 1.0 };
            let s = a.frame(&mut cx, ecran, 1.0 / 30.0);
            ui.end_frame();
            s
        };
        image(&mut a, &mut ui);
        // La seconde pastille : 14 + 52 + 10 dp, puis la moitié
        ui.event(Event::Down { id: 0, x: 102.0, y: 40.0 }, 0.1);
        ui.event(Event::Up { id: 0 }, 0.15);
        let mut s = image(&mut a, &mut ui);
        for _ in 0..30 {
            s = image(&mut a, &mut ui);
        }
        assert!(a.edite());
        // Un appui sur un objet de la vue le choisit : on vise le premier dessiné
        let cam = s.camera;
        let (e, l) = a.monde.objet.iter().filter_map(|(e, _)| Some((e, *a.monde.lieu.get(e)?))).find(|(e, l)| {
            let o = a.monde.objet.get(*e).unwrap();
            let c = [l.pos[0], l.pos[1] + o.dim[1] / 2.0, l.pos[2]];
            let k = math::appliquer(&math::mul(&cam.proj, &cam.vue), c);
            let (x, y) = (k[0] / k[3], k[1] / k[3]);
            k[3] > 0.0 && x.abs() < 0.8 && y.abs() < 0.8 && physique::lancer(&a.monde, cam.oeil, [c[0] - cam.oeil[0], c[1] - cam.oeil[1], c[2] - cam.oeil[2]], 150.0).is_some_and(|i| i.entite == Some(*e))
        }).expect("un objet en vue");
        let o = a.monde.objet.get(e).unwrap();
        let k = math::appliquer(&math::mul(&cam.proj, &cam.vue), [l.pos[0], l.pos[1] + o.dim[1] / 2.0, l.pos[2]]);
        let f = cam.fenetre;
        let (x, y) = (f[0] + (k[0] / k[3] + 1.0) / 2.0 * f[2], f[1] + (1.0 - k[1] / k[3]) / 2.0 * f[3]);
        ui.event(Event::Down { id: 0, x, y }, 1.0);
        ui.event(Event::Up { id: 0 }, 1.05);
        let s = image(&mut a, &mut ui);
        assert_eq!(a.monde.choisie, Some(e));
        image(&mut a, &mut ui);
        assert!(image(&mut a, &mut ui).dessins.iter().any(|d| d.choisi), "en surbrillance");
        // Fermer l'éditeur laisse l'objet : sa pastille a suivi l'espace de travail
        let x0 = s.camera.fenetre[0] + 102.0;
        ui.event(Event::Down { id: 0, x: x0, y: 40.0 }, 2.0);
        ui.event(Event::Up { id: 0 }, 2.05);
        image(&mut a, &mut ui);
        image(&mut a, &mut ui);
        assert!(!a.edite() && a.monde.choisie.is_none());
    }

    #[test]
    fn la_manette_de_jeu_fait_marcher_et_ouvre_les_reglages() {
        use crate::manette::{axe, touche};
        use crate::text::Text;
        use crate::ui::Ui;
        let mut a = a_plat();
        let (mut ui, mut text, mut dl) = (Ui::new(1.0), Text::new(), crate::draw::DrawList::default());
        let ecran = Rect::new(0.0, 0.0, 1600.0, 900.0);
        a.manette(Evenement::Appareil { id: 3, nom: "Essai".into(), descripteur: "essai".into(), axes: vec![] });
        a.manette(Evenement::Axes { id: 3, valeurs: vec![(axe::X, 0.0), (axe::Y, -1.0), (axe::Z, 0.6)] });
        let (depart, lacet) = (a.monde.absolu(a.lieu().pos), a.cam.0);
        for _ in 0..30 {
            dl.clear();
            let mut cx = Cx { ui: &mut ui, text: &mut text, dl: &mut dl, dp: 1.0 };
            a.frame(&mut cx, ecran, 1.0 / 30.0);
            ui.end_frame();
        }
        let p = a.monde.absolu(a.lieu().pos);
        assert!((p[0] - depart[0]).hypot(p[2] - depart[2]) > 1.0, "le stick gauche fait marcher");
        assert!(a.cam.0 < lacet - 0.5, "le stick droit tourne la vue vers la droite : {}", a.cam.0);
        a.manette(Evenement::Touche { id: 3, code: touche::START, bas: true });
        let mut cx = Cx { ui: &mut ui, text: &mut text, dl: &mut dl, dp: 1.0 };
        a.frame(&mut cx, ecran, 1.0 / 30.0);
        assert!(a.shell.start.open && a.onglet == ONGLET_REGLAGES);
        a.manette(Evenement::Touche { id: 3, code: touche::B, bas: true });
        a.frame(&mut cx, ecran, 1.0 / 30.0);
        assert!(!a.shell.start.open, "B ferme le panneau");
    }

    #[test]
    fn un_glb_de_decor_entre_au_catalogue_et_se_retrouve() {
        use crate::moteur::{Genre, Objet};
        let d = std::env::temp_dir().join(format!("sage-decor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let mut a = App::avec_dossier(1.0, Some(d.clone()));
        a.charger("maison.glb", &crate::catalogue::tests::cube_glb(None));
        assert_eq!(a.modele().unwrap().id, "mannequin", "le joueur ne change pas");
        let k = a.monde.modeles.chercher("glb/0").expect("au catalogue");
        assert!(a.edite() && a.editeur.mode == editeur::Mode::Ajouter(Genre::Modele(k)), "l'éditeur s'ouvre pour le poser");
        // Posé à l'échelle 2 devant le joueur : un cube de 4 m, qui porte ce qui monte dessus
        let p = a.lieu().pos;
        let o = a.tuiles.ajouter(&mut a.monde, &mut a.maillages, Objet { dim: [2.0; 3], ..Objet::neuf(Genre::Modele(k)) }, Lieu::en([p[0] + 6.0, p[1], p[2]], 0.3));
        let dessus = physique::sol(&a.monde, p[0] + 6.0, p[2], 100.0).unwrap();
        assert!((dessus.0 - p[1] - 4.0).abs() < 1e-3 && dessus.2 == Some(o), "{dessus:?}");
        let m = a.maillages.get(a.monde.rendu.get(o).unwrap().maillage).unwrap();
        assert_eq!(m.format, Format::Texture);
        a.enregistrer();
        // Relancé : le modèle revient du catalogue, l'objet le retrouve
        let b = App::avec_dossier(1.0, Some(d.clone()));
        let o = b.monde.trouver("o/0").unwrap();
        assert_eq!(b.maillages.get(b.monde.rendu.get(o).unwrap().maillage).unwrap().format, Format::Texture);
        // Sans son fichier : une boîte grise, et la tuile garde la référence
        std::fs::remove_file(d.join("catalogue/glb_0.glb")).unwrap();
        let mut c = App::avec_dossier(1.0, Some(d.clone()));
        let o = c.monde.trouver("o/0").unwrap();
        assert_eq!(c.maillages.get(c.monde.rendu.get(o).unwrap().maillage).unwrap().id, "modele-absent");
        let k = Tuiles::tuile_de(&c.monde, c.monde.lieu.get(o).unwrap().pos);
        c.tuiles.marquer(k);
        c.enregistrer();
        let s = std::fs::read_to_string(d.join(format!("monde/tuiles/{}_{}.json", k.0, k.1))).unwrap();
        assert!(s.contains("\"modele\":\"glb/0\"") && s.contains("\"echelle\":2"), "{s}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn un_decor_a_l_envers_est_retourne_et_le_choix_se_garde() {
        use crate::catalogue::tests::cube_glb_faces;
        let d = std::env::temp_dir().join(format!("sage-envers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let mut a = App::avec_dossier(1.0, Some(d.clone()));
        a.charger("envers.glb", &cube_glb_faces(None, true, true));
        assert!(a.catalogue.entrees[0].retourne == Some(true), "vu à l'envers, il est retourné");
        assert!(a.message.as_ref().is_some_and(|m| m.0.contains("retournées")), "{:?}", a.message);
        let k = a.monde.modeles.chercher("glb/0").unwrap();
        let g = a.monde.modeles.get(k).unwrap().gabarit.clone().unwrap();
        assert!(g.parties.iter().all(|p| p.materiau.deux_cotes), "doubleSided est lu");
        // Une fois retourné, chaque triangle regarde vers l'extérieur du cube
        let dehors = |g: &crate::moteur::Gabarit| {
            g.triangles.iter().all(|t| {
                let (u, w) = ([0, 1, 2].map(|c| t[1][c] - t[0][c]), [0, 1, 2].map(|c| t[2][c] - t[0][c]));
                let n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
                let c = [0, 1, 2].map(|i| (t[0][i] + t[1][i] + t[2][i]) / 3.0 - [0.0, 1.0, 0.0][i]);
                n[0] * c[0] + n[1] * c[1] + n[2] * c[2] > 0.0
            })
        };
        assert!(dehors(&g));
        // Le lien de l'éditeur remet les faces comme dans le fichier, et le choix reste
        a.retourner_modele("glb/0");
        assert!(!dehors(a.monde.modeles.get(k).unwrap().gabarit.as_ref().unwrap()));
        let b = App::avec_dossier(1.0, Some(d.clone()));
        assert_eq!(b.catalogue.entrees[0].retourne, Some(false));
        let k = b.monde.modeles.chercher("glb/0").unwrap();
        assert!(!dehors(b.monde.modeles.get(k).unwrap().gabarit.as_ref().unwrap()));
        // Un cube à l'endroit, lui, ne bouge pas
        let mut c = App::new(1.0);
        c.charger("endroit.glb", &crate::catalogue::tests::cube_glb(None));
        assert_eq!(c.catalogue.entrees[0].retourne, Some(false));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn l_affichage_charge_plus_loin_et_se_retrouve() {
        let d = std::env::temp_dir().join(format!("sage-affichage-jeu-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let mut a = App::avec_dossier(1.0, Some(d.clone()));
        a.affichage.modifier(4, 200.0);
        let (mut ui, mut text, mut dl) = (crate::ui::Ui::new(1.0), crate::text::Text::new(), crate::draw::DrawList::default());
        let mut brouillard = 0.0;
        for _ in 0..120 {
            dl.clear();
            let mut cx = Cx { ui: &mut ui, text: &mut text, dl: &mut dl, dp: 1.0 };
            brouillard = a.frame(&mut cx, Rect::new(0.0, 0.0, 1280.0, 800.0), 1.0 / 30.0).brouillard;
            ui.end_frame();
        }
        assert_eq!(brouillard, 200.0);
        assert_eq!(a.tuiles.chargees.len(), 81, "neuf tuiles de côté");
        let b = App::avec_dossier(1.0, Some(d.clone()));
        assert_eq!((b.affichage.distance, b.affichage.brouillard), (4, 200.0));
        assert_eq!(b.tuiles.chargees.len(), 81);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn les_courbes_de_niveau_s_espacent_avec_la_hauteur() {
        assert_eq!(isolignes(10.0), 1.0);
        assert_eq!(isolignes(200.0), 5.0);
        assert_eq!(isolignes(1000.0), 25.0);
        assert_eq!(isolignes(TOPO_LOIN), 50.0);
    }
    #[test]
    fn un_lieu_reel_s_importe_et_se_retrouve() {
        let d = std::env::temp_dir().join(format!("sage-lieu-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let mut a = App::avec_dossier(1.0, Some(d.clone()));
        // Un lieu plat à 10 m, de l'eau à 3 m dans sa moitié ouest
        let pas = 64.0;
        let n = crate::reel::cote(pas);
        let h: Vec<f32> = (0..n * n).map(|i| if i % n < n / 2 { -2.0 } else { 10.0 }).collect();
        let eau: Vec<bool> = (0..n * n).map(|i| i % n < n / 2).collect();
        let r = crate::reel::Reel::new("Essai", 49.0, 1.0, 0.0, 0.0, pas, 3.0, &h, &eau, Vec::new(), Vec::new(), Vec::new());
        a.charger("essai.reel", &r.octets());
        assert_eq!(a.campagne.mer(), 3.0);
        let p = a.lieu().pos;
        assert!((systemes::hauteur_sol(&a.monde, p[0], p[2]).unwrap() - 10.0).abs() < 0.1);
        // Relancé : toujours ce lieu ; une île régénérée l'oublie
        let mut b = App::avec_dossier(1.0, Some(d.clone()));
        assert_eq!(b.campagne.reel.as_ref().map(|r| r.nom.as_str()), Some("Essai"));
        b.regenerer(Campagne::brute(5));
        assert!(App::avec_dossier(1.0, Some(d.clone())).campagne.reel.is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn la_vue_se_leve_vers_le_ciel_en_jeu() {
        let mut a = App::avec_dossier(1.0, None);
        let w = Rect::new(0.0, 0.0, 800.0, 600.0);
        a.cam.1 = LEVER;
        let c = a.camera(w, 1.0);
        let r = c.rayon(w.cx(), w.cy());
        assert!(r[1] > 0.7, "la vue regarde en l'air : {r:?}");
        let sol = systemes::hauteur_sol(&a.monde, c.oeil[0], c.oeil[2]).unwrap();
        assert!(c.oeil[1] >= sol + 0.39, "l'œil reste au-dessus du sol");
        // Le joueur est dans le bas de l'image
        let p = a.lieu().pos;
        let e = c.ecran([p[0], p[1] + 1.0, p[2]]).expect("le joueur se voit");
        assert!(e[1] > w.cy(), "{e:?}");
    }

    #[test]
    fn la_carte_teleporte_le_joueur_au_point_vise() {
        let mut a = App::avec_dossier(1.0, None);
        let p = a.lieu().pos;
        // Un point chargé, à terre : le joueur y va, posé sur le sol
        let v = [p[0] + 40.0, 0.0, p[2] - 25.0];
        a.visee = Some(v);
        a.teleporter();
        let q = a.lieu().pos;
        let sol = systemes::hauteur_sol(&a.monde, v[0], v[2]).unwrap();
        assert_eq!((q[0], q[2]), (v[0], v[2]));
        assert!((q[1] - sol).abs() < 1e-4);
        // Hors des tuiles chargées : il ne bouge pas
        a.visee = Some([p[0] + 5000.0, 0.0, p[2]]);
        a.teleporter();
        assert_eq!(a.lieu().pos, q);
    }

    #[test]
    fn la_carte_est_finie_et_se_regenere_nue() {
        use crate::moteur::tuiles::{dans_la_carte, DEMI_CARTE};
        use crate::moteur::{Genre, Objet};
        let d = std::env::temp_dir().join(format!("sage-regenerer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let mut a = App::avec_dossier(1.0, Some(d.clone()));
        assert!(a.monde.objet.iter().next().is_none(), "nue dès le départ");
        // Un objet posé, une tuile retouchée, écrits
        let p = a.lieu().pos;
        a.tuiles.ajouter(&mut a.monde, &mut a.maillages, Objet::neuf(Genre::Poteau), Lieu::en([p[0] + 2.0, p[1], p[2]], 0.0));
        a.enregistrer();
        assert!(d.join("monde/tuiles").read_dir().unwrap().next().is_some());
        // Au bord de la carte : le joueur s'arrête au mur, rien n'est chargé au-delà
        a.regenerer(Campagne::brute(42));
        assert!(a.monde.objet.iter().next().is_none() && !d.join("monde/tuiles").exists());
        assert_eq!(App::avec_dossier(1.0, Some(d.clone())).tuiles.fiche.graine, 42);
        a.monde.origine = [DEMI_CARTE, 0];
        a.monde.lieu.get_mut(a.joueur).unwrap().pos = [60.0, 50.0, 30.0];
        for _ in 0..30 {
            let o = a.monde.origine;
            a.tuiles.suivre(&mut a.monde, &mut a.maillages, (o[0], o[1]), usize::MAX);
        }
        assert!(a.tuiles.chargees.keys().all(|&(i, j)| dans_la_carte(i, j)));
        a.cam.0 = -std::f32::consts::FRAC_PI_2; // la vue regarde vers +X
        for _ in 0..120 {
            a.avancer([0.0, -1.0]);
        }
        let x = a.monde.absolu(a.lieu().pos)[0];
        assert!(x <= ((DEMI_CARTE + 1) as f32 * crate::moteur::TAILLE - CORPS) as f64 + 1e-3, "{x}");
        let _ = std::fs::remove_dir_all(&d);
    }
}

/// La marque du pluriel.
fn pl(n: usize) -> &'static str {
    if n > 1 { "s" } else { "" }
}
