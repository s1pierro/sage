//! Les réglages des procédures qui bâtissent le monde autour des routes (onglet
//! Procédures de l'éditeur), gardés dans `reglages/procedures.json`.
//!
//! Les chaussées, les accotements et les trottoirs se refont à la volée sur les tuiles
//! chargées ; le terrassement entre dans le relief des régions : il s'applique en les
//! régénérant (onglet Carte).
//!
//! Les procédures lisent les réglages en cours (`courantes`), partagés entre les fils.

use gda::json::{self, J};
use crate::ui::id;
use crate::widgets::{Body, Cx};
use std::path::PathBuf;
use std::sync::RwLock;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Procedures {
    /// Les grandes routes, rues et voies de service ont leur chaussée maillée ; sinon, elles
    /// sont seulement peintes sur le sol.
    pub chaussees: bool,
    pub accotements: bool,
    /// La largeur des accotements (m).
    pub accotement: f32,
    pub trottoirs: bool,
    /// La largeur d'un trottoir sans façade en vue (m).
    pub trottoir: f32,
    /// Jusqu'où un trottoir va chercher la façade, depuis la bordure (m).
    pub portee: f32,
    /// La hauteur de la bordure (m).
    pub bordure: f32,
    /// Le sol sous les routes : `TALUS` (plateforme et talus), `FONDU` (aplani sous la
    /// chaussée, fondu dans le relief sur les bas-côtés, comme avant la 0.35) ou `AUCUN`
    /// (le relief tel quel).
    pub terrassement: u8,
    /// La place à plat de chaque côté de la chaussée (m), pour les routes et les rues.
    pub plateforme: f32,
    /// La pente des talus (de haut par mètre de long).
    pub pente: f32,
    /// Jusqu'où, au-delà de la plateforme, un talus peut aller (m).
    pub talus: f32,
    /// Un bâtiment à moins de tant du bord d'une rue (m) a son abord terrassé à la hauteur
    /// de la rue, sous lui et jusqu'à elle ; 0 : aucun.
    pub abords: f32,
    /// Le lissage du profil en long des routes (m, environ un écart type) ; 0 : aucun.
    pub lissage: f32,
    /// Le rayon des bordures aux angles des carrefours (m) ; 0 : angles vifs.
    pub rayon: f32,
}

pub const TALUS: u8 = 0;
pub const FONDU: u8 = 1;
pub const AUCUN: u8 = 2;

/// La plus grande portée d'un trottoir : la marge où l'on cherche les bâtiments.
pub const PORTEE_MAX: f32 = 20.0;

impl Default for Procedures {
    fn default() -> Procedures {
        Procedures { chaussees: true, accotements: true, accotement: 2.0, trottoirs: true, trottoir: 2.0, portee: 12.0, bordure: 0.15, terrassement: TALUS, plateforme: 2.0, pente: 2.0 / 3.0, talus: 20.0, abords: 12.0, lissage: crate::terrassement::LISSAGE, rayon: 4.0 }
    }
}

static COURANTES: RwLock<Option<Procedures>> = RwLock::new(None);

/// Les réglages en cours.
pub fn courantes() -> Procedures {
    COURANTES.read().ok().and_then(|p| *p).unwrap_or_default()
}

/// (Pas dans les tests : ils tournent ensemble et liraient ceux d'un autre.)
fn rendre_courantes(p: Procedures) {
    if cfg!(test) {
        return;
    }
    if let Ok(mut c) = COURANTES.write() {
        *c = Some(p);
    }
}

impl Procedures {
    fn borner(&mut self) {
        let d = Procedures::default();
        let b = |v: f32, lo: f32, hi: f32, pas: f32, defaut: f32| if v.is_finite() { ((v / pas).round() * pas).clamp(lo, hi) } else { defaut };
        self.accotement = b(self.accotement, 0.5, 4.0, 0.25, d.accotement);
        self.trottoir = b(self.trottoir, 1.0, 4.0, 0.25, d.trottoir);
        self.portee = b(self.portee, 0.0, PORTEE_MAX, 1.0, d.portee);
        self.bordure = b(self.bordure, 0.05, 0.30, 0.01, d.bordure);
        self.plateforme = b(self.plateforme, 0.0, 4.0, 0.25, d.plateforme);
        self.pente = b(self.pente, 0.25, 1.5, 0.05, d.pente);
        self.talus = b(self.talus, 5.0, 30.0, 1.0, d.talus);
        self.terrassement = self.terrassement.min(AUCUN);
        self.abords = b(self.abords, 0.0, 20.0, 1.0, d.abords);
        self.lissage = b(self.lissage, 0.0, 60.0, 1.0, d.lissage);
        self.rayon = b(self.rayon, 0.0, 10.0, 0.5, d.rayon);
    }

    fn vers_json(&self, a_regenerer: bool) -> String {
        let n = |v: f32| J::Num((v as f64 * 1000.0).round() / 1000.0);
        let o = J::Objet(vec![
            ("format".into(), J::Num(1.0)),
            ("chaussees".into(), J::Bool(self.chaussees)),
            ("accotements".into(), J::Bool(self.accotements)),
            ("accotement".into(), n(self.accotement)),
            ("trottoirs".into(), J::Bool(self.trottoirs)),
            ("trottoir".into(), n(self.trottoir)),
            ("portee".into(), n(self.portee)),
            ("bordure".into(), n(self.bordure)),
            ("terrassement".into(), J::Num(self.terrassement as f64)),
            ("plateforme".into(), n(self.plateforme)),
            ("pente".into(), n(self.pente)),
            ("talus".into(), n(self.talus)),
            ("abords".into(), n(self.abords)),
            ("lissage".into(), n(self.lissage)),
            ("rayon".into(), n(self.rayon)),
            ("a_regenerer".into(), J::Bool(a_regenerer)),
        ]);
        let mut s = String::new();
        json::indente(&o, 0, &mut s);
        s
    }

    fn depuis_json(j: &J) -> Procedures {
        let mut p = Procedures::default();
        let oui = |k: &str, v: &mut bool| {
            if let Some(J::Bool(b)) = j.get(k) {
                *v = *b;
            }
        };
        oui("chaussees", &mut p.chaussees);
        oui("accotements", &mut p.accotements);
        oui("trottoirs", &mut p.trottoirs);
        let num = |k: &str, v: &mut f32| {
            if let Some(x) = j.get(k).and_then(J::num) {
                *v = x as f32;
            }
        };
        num("accotement", &mut p.accotement);
        num("trottoir", &mut p.trottoir);
        num("portee", &mut p.portee);
        num("bordure", &mut p.bordure);
        num("plateforme", &mut p.plateforme);
        if let Some(x) = j.get("terrassement").and_then(J::num) {
            p.terrassement = x.clamp(0.0, AUCUN as f64) as u8;
        }
        num("pente", &mut p.pente);
        num("talus", &mut p.talus);
        num("abords", &mut p.abords);
        num("lissage", &mut p.lissage);
        num("rayon", &mut p.rayon);
        p.borner();
        p
    }
}

/// Les réglages gardés dans un fichier, et ce qu'il faut refaire quand ils changent.
pub struct Reglages {
    pub p: Procedures,
    /// Le terrassement a changé depuis la dernière régénération.
    pub a_regenerer: bool,
    fichier: Option<PathBuf>,
    pub erreur: Option<String>,
}

/// Ce qu'un changement demande.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Changement {
    /// Les tuiles chargées se refont (chaussées, accotements, trottoirs).
    pub tuiles: bool,
    /// Le terrassement a changé : il entrera dans les régions régénérées.
    pub terrassement: bool,
    /// Régénérer la région visée (0), avec ses voisines (1), ou toute la carte (-1).
    pub regenerer: Option<i32>,
}

impl Reglages {
    /// Les réglages relus de `dossier`, ou ceux d'origine ; ils deviennent les courants.
    pub fn ouvrir(dossier: Option<PathBuf>) -> Reglages {
        let fichier = dossier.map(|d| d.join("procedures.json"));
        let j = fichier.as_ref().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|s| json::lire(&s));
        let p = j.as_ref().map(Procedures::depuis_json).unwrap_or_default();
        let a_regenerer = matches!(j.as_ref().and_then(|j| j.get("a_regenerer")), Some(J::Bool(true)));
        rendre_courantes(p);
        Reglages { p, a_regenerer, fichier, erreur: None }
    }

    fn ecrire(&mut self) {
        let Some(f) = &self.fichier else { return };
        let texte = self.p.vers_json(self.a_regenerer);
        let tmp = f.with_extension("tmp");
        let r = f.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(&tmp, texte)).and_then(|_| std::fs::rename(&tmp, f));
        self.erreur = r.err().map(|e| format!("Réglages non enregistrés : {e}"));
    }

    /// Le terrassement a changé ailleurs (la voirie) : à régénérer.
    pub fn demander_regeneration(&mut self) {
        if !self.a_regenerer {
            self.a_regenerer = true;
            self.ecrire();
        }
    }

    /// Une régénération est finie : le terrassement des régions refaites est à jour.
    pub fn regenere(&mut self) {
        if self.a_regenerer {
            self.a_regenerer = false;
            self.ecrire();
        }
    }

    /// Prend `p` (borné) : le garde, le rend courant, et dit ce qu'il faut refaire.
    pub fn regler(&mut self, mut p: Procedures) -> Changement {
        p.borner();
        if p == self.p {
            return Changement::default();
        }
        let a = self.p;
        let terrassement = (a.terrassement, a.plateforme, a.pente, a.talus, a.abords, a.lissage) != (p.terrassement, p.plateforme, p.pente, p.talus, p.abords, p.lissage);
        let tuiles = Procedures { terrassement: a.terrassement, plateforme: a.plateforme, pente: a.pente, talus: a.talus, abords: a.abords, lissage: a.lissage, ..p } != a;
        self.p = p;
        self.a_regenerer |= terrassement;
        rendre_courantes(p);
        self.ecrire();
        Changement { tuiles, terrassement, regenerer: None }
    }
}

fn m(v: f32) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{} m", s.replace('.', ","))
}

/// L'avancement d'une régénération, ou, si le terrassement a changé depuis la dernière,
/// les liens pour la lancer : la région visée (0), avec ses voisines (1), toute la carte
/// (-1).
pub fn regeneration(cx: &mut Cx, b: &mut Body, r: &Reglages, regions: bool, avancement: Option<(usize, usize)>) -> Option<i32> {
    if let Some((faites, total)) = avancement {
        cx.mesure(b, "Régénération", &format!("{faites} / {total} régions"));
    } else if r.a_regenerer && regions {
        cx.paragraph(b, "Le terrassement a changé : il entre dans le relief des régions en les régénérant (les bâtiments retouchés, enrichis ou démolis restent tels quels).");
        if let Some(k) = cx.liens(b, id("regenerer-regions"), &["La région", "Avec ses voisines", "Toute la carte"], None) {
            return Some(if k == 2 { -1 } else { k as i32 });
        }
    }
    None
}

/// L'onglet Procédures : ce qu'il faut refaire si les réglages ont changé. `regions` :
/// le monde est matérialisé et rien ne le refait en ce moment ; `avancement` : une
/// régénération en cours (régions faites, en tout).
pub fn panneau(cx: &mut Cx, b: &mut Body, r: &mut Reglages, regions: bool, avancement: Option<(usize, usize)>) -> Changement {
    let mut p = r.p;
    let oui_non = |cx: &mut Cx, b: &mut Body, cle: &'static str, oui: &str, non: &str, v: &mut bool| {
        if let Some(i) = cx.liens(b, id(cle), &[oui, non], Some(!*v as usize)) {
            *v = i == 0;
        }
    };
    cx.section(b, "CHAUSSÉES");
    oui_non(cx, b, "proc-chaussees", "Maillées", "Peintes", &mut p.chaussees);
    if p.chaussees {
        oui_non(cx, b, "proc-accotements", "Avec accotements", "Sans accotements", &mut p.accotements);
        if p.accotements {
            cx.slider(b, id("proc-accotement"), "Accotement", &m(p.accotement), &mut p.accotement, 0.5, 4.0);
        }
    }
    let rayon = if p.rayon > 0.0 { m(p.rayon) } else { "vifs".to_string() };
    cx.slider(b, id("proc-rayon"), "Angles des carrefours", &rayon, &mut p.rayon, 0.0, 10.0);
    cx.section(b, "TROTTOIRS");
    oui_non(cx, b, "proc-trottoirs", "Avec trottoirs", "Sans trottoirs", &mut p.trottoirs);
    if p.trottoirs {
        cx.slider(b, id("proc-trottoir"), "Largeur sans façade", &m(p.trottoir), &mut p.trottoir, 1.0, 4.0);
        cx.slider(b, id("proc-portee"), "Jusqu'à la façade", &m(p.portee), &mut p.portee, 0.0, PORTEE_MAX);
        cx.slider(b, id("proc-bordure"), "Bordure", &format!("{} cm", (p.bordure * 100.0).round()), &mut p.bordure, 0.05, 0.30);
    }
    cx.section(b, "TERRASSEMENT");
    let lissage = if p.lissage > 0.0 { format!("sur {}", m(p.lissage)) } else { "aucun".to_string() };
    cx.slider(b, id("proc-lissage"), "Profil des routes lissé", &lissage, &mut p.lissage, 0.0, 60.0);
    if let Some(k) = cx.liens(b, id("proc-terrassement"), &["Talus", "Fondu", "Aucun"], Some(p.terrassement as usize)) {
        p.terrassement = k as u8;
    }
    match p.terrassement {
        TALUS => {
            cx.slider(b, id("proc-plateforme"), "Plateforme", &m(p.plateforme), &mut p.plateforme, 0.0, 4.0);
            cx.slider(b, id("proc-pente"), "Pente des talus", &format!("{} %", (p.pente * 100.0).round()), &mut p.pente, 0.25, 1.5);
            cx.slider(b, id("proc-talus"), "Portée des talus", &m(p.talus), &mut p.talus, 5.0, 30.0);
            let abords = if p.abords > 0.0 { format!("à {}", m(p.abords)) } else { "aucun".to_string() };
            cx.slider(b, id("proc-abords"), "Bâtiments au niveau de la rue", &abords, &mut p.abords, 0.0, 20.0);
        }
        FONDU => cx.paragraph(b, "Le sol aplani sous la chaussée seule, fondu dans le relief sur les bas-côtés (comme avant la 0.35) ; pas de travées."),
        _ => cx.paragraph(b, "Le relief tel quel : les chaussées suivent leur profil lissé, au-dessus ou dans le sol ; pas de travées."),
    }
    let regenerer = regeneration(cx, b, r, regions, avancement);
    if p != Procedures::default() && cx.button(b, id("proc-origine"), "Valeurs d'origine") {
        p = Procedures::default();
    }
    let mut c = r.regler(p);
    c.regenerer = regenerer;
    if let Some(e) = &r.erreur {
        cx.paragraph(b, e);
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_procedures_se_gardent_et_disent_quoi_refaire() {
        let d = std::env::temp_dir().join(format!("sage-procedures-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let mut r = Reglages { p: Procedures::default(), a_regenerer: false, fichier: Some(d.join("procedures.json")), erreur: None };
        assert_eq!(r.regler(Procedures::default()), Changement::default());
        let c = r.regler(Procedures { trottoirs: false, accotement: 3.1, ..Procedures::default() });
        assert_eq!(c, Changement { tuiles: true, terrassement: false, regenerer: None });
        assert!(!r.a_regenerer);
        assert_eq!(r.p.accotement, 3.0);
        let c = r.regler(Procedures { pente: 9.0, ..r.p });
        assert_eq!(c, Changement { tuiles: false, terrassement: true, regenerer: None });
        assert_eq!(r.p.pente, 1.5);
        // À régénérer, et gardé tel quel jusqu'à la régénération
        assert!(r.a_regenerer && Reglages::ouvrir(Some(d.clone())).a_regenerer);
        r.regenere();
        assert!(!Reglages::ouvrir(Some(d.clone())).a_regenerer);
        let lu = Procedures::depuis_json(&json::lire(&std::fs::read_to_string(d.join("procedures.json")).unwrap()).unwrap());
        assert_eq!(lu, r.p);
        let _ = std::fs::remove_dir_all(&d);
    }
}
