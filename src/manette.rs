//! Les manettes de jeu : ce qu'elles envoient, les actions du jeu qu'elles commandent, et
//! le profil de chacune.
//!
//! - La plateforme décrit chaque manette (nom, descripteur, axes présents) puis envoie ses
//!   touches et ses axes (`Evenement`). Rien ici ne connaît Android.
//! - Le jeu ne lit pas des touches mais des *actions*, chacune sous un identifiant
//!   (`ACTIONS`) : deux sticks (`marcher`, `vue`) et des boutons.
//! - Chaque manette a son profil, rangé sous son descripteur, un identifiant stable
//!   d'une connexion à l'autre. Sans profil enregistré, l'assignation est automatique,
//!   d'après la disposition normalisée d'Android.

use gda::json::{self, J};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Codes de touches d'Android (`KeyEvent.KEYCODE_*`).
pub mod touche {
    pub const HAUT: i32 = 19;
    pub const BAS: i32 = 20;
    pub const GAUCHE: i32 = 21;
    pub const DROITE: i32 = 22;
    pub const CENTRE: i32 = 23;
    pub const A: i32 = 96;
    pub const B: i32 = 97;
    pub const C: i32 = 98;
    pub const X: i32 = 99;
    pub const Y: i32 = 100;
    pub const Z: i32 = 101;
    pub const L1: i32 = 102;
    pub const R1: i32 = 103;
    pub const L2: i32 = 104;
    pub const R2: i32 = 105;
    pub const STICK_G: i32 = 106;
    pub const STICK_D: i32 = 107;
    pub const START: i32 = 108;
    pub const SELECT: i32 = 109;
    pub const MODE: i32 = 110;
    pub const RETOUR: i32 = 4;
}

/// Axes d'Android (`MotionEvent.AXIS_*`).
pub mod axe {
    pub const X: i32 = 0;
    pub const Y: i32 = 1;
    pub const Z: i32 = 11;
    pub const RX: i32 = 12;
    pub const RY: i32 = 13;
    pub const RZ: i32 = 14;
    pub const HAT_X: i32 = 15;
    pub const HAT_Y: i32 = 16;
    pub const LTRIGGER: i32 = 17;
    pub const RTRIGGER: i32 = 18;
    pub const GAS: i32 = 22;
    pub const BRAKE: i32 = 23;
}

/// Les axes que la plateforme lit sur une manette.
pub const AXES: [i32; 12] = [axe::X, axe::Y, axe::Z, axe::RX, axe::RY, axe::RZ, axe::HAT_X, axe::HAT_Y, axe::LTRIGGER, axe::RTRIGGER, axe::GAS, axe::BRAKE];

/// Les paires d'axes qui forment un stick.
const PAIRES: [(i32, i32); 4] = [(axe::X, axe::Y), (axe::Z, axe::RZ), (axe::RX, axe::RY), (axe::HAT_X, axe::HAT_Y)];

/// Ce qu'une action écoute : une touche, la moitié d'un axe (une gâchette, un côté de la
/// croix), ou un stick (deux axes).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Entree {
    Touche(i32),
    Demi(i32, i8),
    Stick(i32, i32),
}

fn nom_touche(c: i32) -> String {
    use touche::*;
    match c {
        A => "A".into(),
        B => "B".into(),
        C => "C".into(),
        X => "X".into(),
        Y => "Y".into(),
        Z => "Z".into(),
        L1 => "L1".into(),
        R1 => "R1".into(),
        L2 => "L2".into(),
        R2 => "R2".into(),
        STICK_G => "Clic du stick gauche".into(),
        STICK_D => "Clic du stick droit".into(),
        START => "Start".into(),
        SELECT => "Select".into(),
        MODE => "Mode".into(),
        RETOUR => "Retour".into(),
        HAUT => "Croix ↑".into(),
        BAS => "Croix ↓".into(),
        GAUCHE => "Croix ←".into(),
        DROITE => "Croix →".into(),
        CENTRE => "Croix, centre".into(),
        188..=203 => format!("Bouton {}", c - 187),
        _ => format!("Touche {c}"),
    }
}

fn nom_axe(a: i32) -> &'static str {
    match a {
        axe::X => "X",
        axe::Y => "Y",
        axe::Z => "Z",
        axe::RX => "RX",
        axe::RY => "RY",
        axe::RZ => "RZ",
        axe::HAT_X => "Croix X",
        axe::HAT_Y => "Croix Y",
        axe::LTRIGGER => "Gâchette gauche",
        axe::RTRIGGER => "Gâchette droite",
        axe::GAS => "Accélérateur",
        axe::BRAKE => "Frein",
        _ => "Axe",
    }
}

impl Entree {
    /// Le nom affiché.
    pub fn nom(&self) -> String {
        match *self {
            Entree::Touche(c) => nom_touche(c),
            Entree::Stick(axe::X, axe::Y) => "Stick gauche".into(),
            Entree::Stick(axe::Z, axe::RZ) | Entree::Stick(axe::RX, axe::RY) => "Stick droit".into(),
            Entree::Stick(axe::HAT_X, axe::HAT_Y) => "Croix".into(),
            Entree::Stick(a, b) => format!("Axes {} et {}", nom_axe(a), nom_axe(b)),
            Entree::Demi(axe::HAT_X, s) => if s < 0 { "Croix ←" } else { "Croix →" }.into(),
            Entree::Demi(axe::HAT_Y, s) => if s < 0 { "Croix ↑" } else { "Croix ↓" }.into(),
            Entree::Demi(a @ (axe::LTRIGGER | axe::RTRIGGER | axe::GAS | axe::BRAKE), _) => nom_axe(a).into(),
            Entree::Demi(a, s) => format!("Axe {} {}", nom_axe(a), if s < 0 { "−" } else { "+" }),
        }
    }

    /// L'identifiant, tel qu'il s'écrit dans le profil.
    pub fn ident(&self) -> String {
        match *self {
            Entree::Touche(c) => format!("touche/{c}"),
            Entree::Demi(a, s) => format!("axe/{a}/{}", if s < 0 { "-" } else { "+" }),
            Entree::Stick(a, b) => format!("stick/{a}/{b}"),
        }
    }

    pub fn depuis(s: &str) -> Option<Entree> {
        let p: Vec<&str> = s.split('/').collect();
        match p[..] {
            ["touche", c] => Some(Entree::Touche(c.parse().ok()?)),
            ["axe", a, "+"] => Some(Entree::Demi(a.parse().ok()?, 1)),
            ["axe", a, "-"] => Some(Entree::Demi(a.parse().ok()?, -1)),
            ["stick", a, b] => Some(Entree::Stick(a.parse().ok()?, b.parse().ok()?)),
            _ => None,
        }
    }

    fn est_stick(&self) -> bool {
        matches!(self, Entree::Stick(..))
    }
}

/// Une action du jeu : son identifiant, son nom affiché, et si elle veut un stick.
pub struct Action {
    pub id: &'static str,
    pub nom: &'static str,
    pub stick: bool,
}

pub const ACTIONS: [Action; 10] = [
    Action { id: "marcher", nom: "Marcher", stick: true },
    Action { id: "vue", nom: "Tourner la vue", stick: true },
    Action { id: "approcher", nom: "Approcher la vue", stick: false },
    Action { id: "eloigner", nom: "Éloigner la vue", stick: false },
    Action { id: "choisir", nom: "Choisir au centre (éditeur)", stick: false },
    Action { id: "annuler", nom: "Annuler (éditeur)", stick: false },
    Action { id: "moniteur", nom: "Moniteur", stick: false },
    Action { id: "editeur", nom: "Éditeur", stick: false },
    Action { id: "reglages", nom: "Réglages", stick: false },
    Action { id: "fermer", nom: "Fermer le panneau", stick: false },
];

pub fn action(id: &str) -> Option<&'static Action> {
    ACTIONS.iter().find(|a| a.id == id)
}

/// Le profil d'une manette : qui commande quoi, la zone morte des sticks, la vitesse de
/// la vue et son sens vertical.
#[derive(Clone, Debug, PartialEq)]
pub struct Profil {
    pub liens: Vec<(&'static str, Entree)>,
    pub zone_morte: f32,
    pub sensibilite: f32,
    pub inverser: bool,
    /// Tel que l'assignation automatique l'a fait.
    pub auto: bool,
}

impl Profil {
    /// L'assignation automatique, d'après les axes que la manette déclare (aucun : on
    /// suppose la disposition la plus courante).
    pub fn automatique(axes: &[i32]) -> Profil {
        let a = |x: i32| axes.is_empty() || axes.contains(&x);
        let vue = if a(axe::Z) && a(axe::RZ) { Entree::Stick(axe::Z, axe::RZ) } else if a(axe::RX) && a(axe::RY) { Entree::Stick(axe::RX, axe::RY) } else { Entree::Stick(axe::Z, axe::RZ) };
        let (g, d) = if a(axe::LTRIGGER) && a(axe::RTRIGGER) {
            (Entree::Demi(axe::LTRIGGER, 1), Entree::Demi(axe::RTRIGGER, 1))
        } else if a(axe::BRAKE) && a(axe::GAS) {
            (Entree::Demi(axe::BRAKE, 1), Entree::Demi(axe::GAS, 1))
        } else {
            (Entree::Touche(touche::L2), Entree::Touche(touche::R2))
        };
        let liens = vec![
            ("marcher", Entree::Stick(axe::X, axe::Y)),
            ("vue", vue),
            ("approcher", d),
            ("eloigner", g),
            ("choisir", Entree::Touche(touche::A)),
            ("annuler", Entree::Touche(touche::X)),
            ("moniteur", Entree::Touche(touche::SELECT)),
            ("editeur", Entree::Touche(touche::Y)),
            ("reglages", Entree::Touche(touche::START)),
            ("fermer", Entree::Touche(touche::B)),
        ];
        Profil { liens, zone_morte: 0.15, sensibilite: 1.0, inverser: false, auto: true }
    }

    pub fn lien(&self, action: &str) -> Option<Entree> {
        self.liens.iter().find(|(a, _)| *a == action).map(|(_, e)| *e)
    }

    /// Donne `e` à `action`. Si une autre action l'avait, elle prend l'ancienne entrée de
    /// `action` : rien ne se retrouve en double.
    pub fn assigner(&mut self, action: &'static str, e: Entree) {
        let ancienne = self.lien(action);
        for (a, x) in self.liens.iter_mut() {
            if *a != action && *x == e {
                match ancienne {
                    Some(v) => *x = v,
                    None => *x = Entree::Touche(-1),
                }
            }
        }
        self.liens.retain(|(_, x)| *x != Entree::Touche(-1));
        match self.liens.iter_mut().find(|(a, _)| *a == action) {
            Some(l) => l.1 = e,
            None => self.liens.push((action, e)),
        }
        self.auto = false;
    }

    pub fn vers_json(&self, nom: &str, descripteur: &str) -> String {
        let liens = J::Objet(self.liens.iter().map(|(a, e)| (a.to_string(), J::Txt(e.ident()))).collect());
        let o = J::Objet(vec![
            ("format".into(), J::Num(1.0)),
            ("descripteur".into(), J::Txt(descripteur.into())),
            ("nom".into(), J::Txt(nom.into())),
            ("zone_morte".into(), J::Num((self.zone_morte as f64 * 100.0).round() / 100.0)),
            ("sensibilite".into(), J::Num((self.sensibilite as f64 * 100.0).round() / 100.0)),
            ("inverser".into(), J::Bool(self.inverser)),
            ("liens".into(), liens),
        ]);
        let mut s = String::new();
        json::indente(&o, 0, &mut s);
        s
    }

    /// Un profil relu ; les actions qu'il ne connaît pas prennent l'assignation
    /// automatique.
    pub fn depuis_json(s: &str, axes: &[i32]) -> Option<Profil> {
        let j = json::lire(s)?;
        let mut p = Profil::automatique(axes);
        p.zone_morte = j.get("zone_morte").and_then(J::num).map_or(p.zone_morte, |v| (v as f32).clamp(0.0, 0.6));
        p.sensibilite = j.get("sensibilite").and_then(J::num).map_or(p.sensibilite, |v| (v as f32).clamp(0.2, 4.0));
        p.inverser = j.get("inverser").and_then(J::bool).unwrap_or(false);
        let J::Objet(l) = j.get("liens")? else { return None };
        let relus: Vec<(&'static str, Entree)> = l.iter().filter_map(|(a, e)| Some((action(a)?.id, Entree::depuis(e.txt()?)?))).collect();
        for (a, e) in relus.iter() {
            if action(a).is_some_and(|x| x.stick == e.est_stick()) {
                p.assigner(a, *e);
            }
        }
        p.auto = false;
        Some(p)
    }
}

/// Ce qu'envoie la plateforme.
#[derive(Clone, Debug, PartialEq)]
pub enum Evenement {
    /// Une manette se présente, avec les axes qu'elle a.
    Appareil { id: i32, nom: String, descripteur: String, axes: Vec<i32> },
    /// Elle est partie.
    Parti(i32),
    Touche { id: i32, code: i32, bas: bool },
    Axes { id: i32, valeurs: Vec<(i32, f32)> },
}

/// Une manette connue et son état.
pub struct Appareil {
    pub id: i32,
    pub nom: String,
    pub descripteur: String,
    pub axes: Vec<i32>,
    pub profil: Profil,
    valeurs: HashMap<i32, f32>,
    repos: HashMap<i32, f32>,
    pub touches: HashSet<i32>,
}

impl Appareil {
    fn axe(&self, a: i32) -> f32 {
        self.valeurs.get(&a).copied().unwrap_or(0.0) - self.repos.get(&a).copied().unwrap_or(0.0)
    }

    /// Un bouton, enfoncé ou non.
    pub fn bouton(&self, e: Entree) -> bool {
        match e {
            Entree::Touche(c) => self.touches.contains(&c),
            Entree::Demi(a, s) => self.axe(a) * s as f32 > 0.5,
            Entree::Stick(..) => false,
        }
    }

    /// Un stick, sa zone morte ôtée : de longueur 0 à 1.
    pub fn stick(&self, e: Entree) -> [f32; 2] {
        let Entree::Stick(a, b) = e else { return [0.0; 2] };
        let v = [self.axe(a), self.axe(b)];
        let l = v[0].hypot(v[1]);
        let z = self.profil.zone_morte;
        if l <= z {
            return [0.0; 2];
        }
        let k = ((l - z) / (1.0 - z).max(1e-3)).min(1.0) / l;
        [v[0] * k, v[1] * k]
    }

    /// Les noms de ce qui est enfoncé, pour le témoin.
    pub fn enfonces(&self) -> Vec<String> {
        let mut t: Vec<i32> = self.touches.iter().copied().collect();
        t.sort();
        let mut v: Vec<String> = t.into_iter().map(nom_touche).collect();
        for a in [axe::LTRIGGER, axe::RTRIGGER, axe::GAS, axe::BRAKE] {
            if self.axe(a) > 0.5 {
                v.push(nom_axe(a).into());
            }
        }
        for (a, s) in [(axe::HAT_X, -1), (axe::HAT_X, 1), (axe::HAT_Y, -1), (axe::HAT_Y, 1)] {
            if self.axe(a) * s as f32 > 0.5 {
                v.push(Entree::Demi(a, s).nom());
            }
        }
        v
    }
}

/// Les commandes d'une image : les deux sticks, et les boutons tout juste enfoncés ou
/// tenus.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Commandes {
    pub marcher: [f32; 2],
    pub vue: [f32; 2],
    pub appuis: Vec<&'static str>,
    pub tenus: Vec<&'static str>,
}

impl Commandes {
    pub fn appui(&self, a: &str) -> bool {
        self.appuis.contains(&a)
    }
    pub fn tenu(&self, a: &str) -> bool {
        self.tenus.contains(&a)
    }
}

pub struct Manettes {
    pub appareils: Vec<Appareil>,
    /// La dernière manette qui a servi : c'est elle qui commande.
    pub active: Option<i32>,
    /// L'action qui attend sa touche ou son stick.
    pub capture: Option<&'static str>,
    /// Les profils sont rangés là, un fichier par descripteur.
    dossier: Option<PathBuf>,
    avant: HashSet<&'static str>,
    /// La capture vient de finir : la touche qui l'a faite ne déclenche rien.
    muet: bool,
    pub erreur: Option<String>,
}

/// Un descripteur réduit à ce qui va dans un nom de fichier.
fn fichier(descripteur: &str) -> String {
    let s: String = descripteur.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    format!("{}.json", if s.is_empty() { "manette".into() } else { s })
}

impl Manettes {
    pub fn new(dossier: Option<PathBuf>) -> Manettes {
        Manettes { appareils: Vec::new(), active: None, capture: None, dossier, avant: HashSet::new(), muet: false, erreur: None }
    }

    pub fn appareil(&self, id: i32) -> Option<&Appareil> {
        self.appareils.iter().find(|a| a.id == id)
    }

    pub fn active(&self) -> Option<&Appareil> {
        self.active.and_then(|i| self.appareil(i)).or(self.appareils.first())
    }

    fn active_mut(&mut self) -> Option<&mut Appareil> {
        let id = self.active().map(|a| a.id)?;
        self.appareils.iter_mut().find(|a| a.id == id)
    }

    pub fn recevoir(&mut self, ev: Evenement) {
        match ev {
            Evenement::Appareil { id, nom, descripteur, axes } => {
                self.appareils.retain(|a| a.id != id);
                let profil = self
                    .dossier
                    .as_ref()
                    .and_then(|d| std::fs::read_to_string(d.join(fichier(&descripteur))).ok())
                    .and_then(|s| Profil::depuis_json(&s, &axes))
                    .unwrap_or_else(|| Profil::automatique(&axes));
                self.appareils.push(Appareil { id, nom, descripteur, axes, profil, valeurs: HashMap::new(), repos: HashMap::new(), touches: HashSet::new() });
                if self.active.is_none() {
                    self.active = Some(id);
                }
            }
            Evenement::Parti(id) => {
                self.appareils.retain(|a| a.id != id);
                if self.active == Some(id) {
                    self.active = self.appareils.first().map(|a| a.id);
                    self.capture = None;
                }
            }
            Evenement::Touche { id, code, bas } => {
                let Some(a) = self.appareils.iter_mut().find(|a| a.id == id) else { return };
                if bas {
                    a.touches.insert(code);
                    self.active = Some(id);
                    if let Some(c) = self.capture.filter(|c| !action(c).is_some_and(|x| x.stick)) {
                        self.finir(c, Entree::Touche(code));
                    }
                } else {
                    a.touches.remove(&code);
                }
            }
            Evenement::Axes { id, valeurs } => {
                let Some(a) = self.appareils.iter_mut().find(|a| a.id == id) else { return };
                let mut pousse = None;
                for (x, v) in valeurs {
                    // Une gâchette au repos à −1 : son repos est là
                    let r = *a.repos.entry(x).or_insert(if v.abs() > 0.9 && !matches!(x, axe::X | axe::Y | axe::HAT_X | axe::HAT_Y) { v } else { 0.0 });
                    a.valeurs.insert(x, v);
                    if (v - r).abs() > 0.6 && pousse.is_none_or(|(_, d): (i32, f32)| (v - r).abs() > d.abs()) {
                        pousse = Some((x, v - r));
                    }
                }
                if pousse.is_some() {
                    self.active = Some(id);
                }
                if let (Some(c), Some((x, d))) = (self.capture, pousse) {
                    let e = if action(c).is_some_and(|a| a.stick) {
                        let Some(&(p, q)) = PAIRES.iter().find(|(p, q)| *p == x || *q == x) else { return };
                        Entree::Stick(p, q)
                    } else {
                        Entree::Demi(x, if d < 0.0 { -1 } else { 1 })
                    };
                    self.finir(c, e);
                }
            }
        }
    }

    fn finir(&mut self, action: &'static str, e: Entree) {
        self.capture = None;
        self.muet = true;
        self.modifier(|p| p.assigner(action, e));
    }

    /// Change le profil de la manette active, et l'écrit.
    pub fn modifier(&mut self, f: impl FnOnce(&mut Profil)) {
        let Some(a) = self.active_mut() else { return };
        f(&mut a.profil);
        let (s, nom) = (a.profil.vers_json(&a.nom, &a.descripteur), fichier(&a.descripteur));
        if let Some(d) = &self.dossier {
            let r = std::fs::create_dir_all(d).and_then(|_| std::fs::write(d.join("profil.tmp"), s)).and_then(|_| std::fs::rename(d.join("profil.tmp"), d.join(nom)));
            self.erreur = r.err().map(|e| format!("profil : {e}"));
        }
    }

    /// Revient à l'assignation automatique : le profil enregistré s'efface.
    pub fn retablir(&mut self) {
        self.capture = None;
        let Some(a) = self.active_mut() else { return };
        let axes = a.axes.clone();
        a.profil = Profil::automatique(&axes);
        let nom = fichier(&a.descripteur);
        if let Some(d) = &self.dossier {
            let _ = std::fs::remove_file(d.join(nom));
        }
    }

    /// Les commandes de cette image, d'après la manette active.
    pub fn commandes(&mut self) -> Commandes {
        let Some(a) = self.active() else {
            self.avant.clear();
            return Commandes::default();
        };
        let p = &a.profil;
        let stick = |id: &str| p.lien(id).map_or([0.0; 2], |e| a.stick(e));
        let tenus: Vec<&'static str> = ACTIONS.iter().filter(|x| !x.stick).filter(|x| p.lien(x.id).is_some_and(|e| a.bouton(e))).map(|x| x.id).collect();
        let mut c = Commandes { marcher: stick("marcher"), vue: stick("vue"), appuis: Vec::new(), tenus: Vec::new() };
        if self.capture.is_some() {
            c.marcher = [0.0; 2];
            c.vue = [0.0; 2];
        } else if !self.muet {
            c.appuis = tenus.iter().copied().filter(|x| !self.avant.contains(x)).collect();
            c.tenus = tenus.clone();
        }
        self.muet = false;
        self.avant = tenus.into_iter().collect();
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manette(m: &mut Manettes, axes: Vec<i32>) {
        m.recevoir(Evenement::Appareil { id: 7, nom: "Manette".into(), descripteur: "ab:cd/12".into(), axes });
    }

    #[test]
    fn l_assignation_automatique_suit_les_axes_presents() {
        let p = Profil::automatique(&[axe::X, axe::Y, axe::RX, axe::RY, axe::BRAKE, axe::GAS]);
        assert_eq!(p.lien("vue"), Some(Entree::Stick(axe::RX, axe::RY)));
        assert_eq!(p.lien("approcher"), Some(Entree::Demi(axe::GAS, 1)));
        let p = Profil::automatique(&[]);
        assert_eq!(p.lien("vue"), Some(Entree::Stick(axe::Z, axe::RZ)));
        assert_eq!(p.lien("marcher").unwrap().nom(), "Stick gauche");
        assert!(ACTIONS.iter().all(|a| p.lien(a.id).is_some()), "chaque action a son entrée");
    }

    #[test]
    fn les_sticks_et_les_boutons_font_des_commandes() {
        let mut m = Manettes::new(None);
        manette(&mut m, vec![]);
        m.recevoir(Evenement::Axes { id: 7, valeurs: vec![(axe::X, 0.1), (axe::Y, -1.0), (axe::Z, 0.05)] });
        m.recevoir(Evenement::Touche { id: 7, code: touche::A, bas: true });
        let c = m.commandes();
        assert!((c.marcher[1] + 0.995).abs() < 0.01 && c.marcher[0].abs() < 0.2, "ramené à 1 : {:?}", c.marcher);
        assert_eq!(c.vue, [0.0; 2], "dans la zone morte");
        assert!(c.appui("choisir"));
        // Tenu : plus un appui à l'image suivante
        let c = m.commandes();
        assert!(!c.appui("choisir") && c.tenu("choisir"));
        // La gâchette, en demi-axe
        m.recevoir(Evenement::Axes { id: 7, valeurs: vec![(axe::RTRIGGER, 0.8)] });
        assert!(m.commandes().tenu("approcher"));
    }

    #[test]
    fn capturer_une_touche_echange_et_s_enregistre() {
        let d = std::env::temp_dir().join(format!("sage-manette-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let mut m = Manettes::new(Some(d.clone()));
        manette(&mut m, vec![]);
        // « Choisir » prend B, que « Fermer » avait : Fermer prend A
        m.capture = Some("choisir");
        m.recevoir(Evenement::Touche { id: 7, code: touche::B, bas: true });
        assert_eq!(m.capture, None);
        let p = &m.active().unwrap().profil;
        assert_eq!(p.lien("choisir"), Some(Entree::Touche(touche::B)));
        assert_eq!(p.lien("fermer"), Some(Entree::Touche(touche::A)));
        assert!(!p.auto);
        assert!(m.commandes().appuis.is_empty(), "la touche de la capture ne déclenche rien");
        // Un stick pour la vue : le stick droit poussé vers le haut
        m.capture = Some("vue");
        m.recevoir(Evenement::Touche { id: 7, code: touche::X, bas: true });
        assert_eq!(m.capture, Some("vue"), "une touche ne fait pas un stick");
        m.recevoir(Evenement::Axes { id: 7, valeurs: vec![(axe::RX, 0.0), (axe::RY, -0.9)] });
        assert_eq!(m.active().unwrap().profil.lien("vue"), Some(Entree::Stick(axe::RX, axe::RY)));
        // Relu à la reconnexion, sous son descripteur
        m.recevoir(Evenement::Parti(7));
        assert!(m.active().is_none());
        manette(&mut m, vec![]);
        let p = &m.active().unwrap().profil;
        assert_eq!(p.lien("choisir"), Some(Entree::Touche(touche::B)));
        assert_eq!(p.lien("vue"), Some(Entree::Stick(axe::RX, axe::RY)));
        assert!(d.join("ab_cd_12.json").exists());
        m.retablir();
        assert!(m.active().unwrap().profil.auto);
        assert_eq!(m.active().unwrap().profil.lien("choisir"), Some(Entree::Touche(touche::A)));
        assert!(!d.join("ab_cd_12.json").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn une_gachette_au_repos_a_moins_un_ne_s_assigne_pas_seule() {
        let mut m = Manettes::new(None);
        manette(&mut m, vec![]);
        m.capture = Some("approcher");
        m.recevoir(Evenement::Axes { id: 7, valeurs: vec![(axe::Z, -1.0), (axe::RZ, -1.0)] });
        assert_eq!(m.capture, Some("approcher"), "le repos n'est pas un appui");
        m.recevoir(Evenement::Axes { id: 7, valeurs: vec![(axe::Z, -1.0), (axe::RZ, 0.9)] });
        assert_eq!(m.active().unwrap().profil.lien("approcher"), Some(Entree::Demi(axe::RZ, 1)));
    }

    #[test]
    fn les_entrees_se_relisent() {
        for e in [Entree::Touche(96), Entree::Demi(17, 1), Entree::Demi(15, -1), Entree::Stick(11, 14)] {
            assert_eq!(Entree::depuis(&e.ident()), Some(e));
        }
        assert_eq!(Entree::depuis("bidule/3"), None);
    }
}
