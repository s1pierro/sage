//! Le dépôt du monde : sa fiche (`monde.json`) et ses tuiles modifiées
//! (`tuiles/<i>_<j>.json`), en JSON, dans un dossier — le dossier privé de l'application
//! sur Android — ou en mémoire seulement.
//!
//! Une tuile s'écrit tout entière : son relief, ses couleurs et ses objets, chacun sous
//! son identifiant. Les normales se recalculent à la lecture. Chaque fichier passe par un
//! fichier temporaire renommé : une écriture interrompue ne laisse jamais de fichier à
//! moitié écrit.

use super::modeles::Modeles;
use super::objets::{depuis_hex, hex, Genre, Objet};
use super::tuiles::{ident, Contenu, Element, Relief};
use gda::json::{self, J};
use std::collections::HashMap;
use std::path::PathBuf;

/// La version du format des fichiers.
pub const FORMAT: f64 = 1.0;

/// La fiche du monde : son identifiant, son nom, la graine de son générateur, le prochain
/// numéro d'objet, et où était le joueur (tuile, position dans la tuile, cap).
#[derive(Clone, Debug, PartialEq)]
pub struct Fiche {
    pub id: String,
    pub nom: String,
    pub graine: u32,
    pub suivant: u64,
    pub joueur: Option<([i32; 2], [f32; 3], f32)>,
}

impl Default for Fiche {
    fn default() -> Fiche {
        Fiche { id: "monde/campagne".into(), nom: "Campagne".into(), graine: 1, suivant: 0, joueur: None }
    }
}

pub struct Depot {
    dossier: Option<PathBuf>,
    memoire: HashMap<String, String>,
}

/// Un nombre arrondi au dixième de millimètre : les fichiers restent courts.
fn n(x: f32) -> J {
    J::Num(((x as f64) * 1e4).round() / 1e4)
}

fn nombres(v: &[f32]) -> J {
    J::Liste(v.iter().map(|x| n(*x)).collect())
}

fn texte(s: &str) -> J {
    J::Txt(s.to_string())
}

fn objet(o: Vec<(&str, J)>) -> J {
    J::Objet(o.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn f3(j: Option<&J>) -> Option<[f32; 3]> {
    j?.nombres::<3>().map(|v| v.map(|x| x as f32))
}

impl Depot {
    pub fn memoire() -> Depot {
        Depot { dossier: None, memoire: HashMap::new() }
    }

    pub fn dossier(d: PathBuf) -> Depot {
        Depot { dossier: Some(d), memoire: HashMap::new() }
    }

    /// Le dossier, s'il y en a un.
    pub fn ou(&self) -> Option<&PathBuf> {
        self.dossier.as_ref()
    }

    fn lire(&self, chemin: &str) -> Option<String> {
        match &self.dossier {
            Some(d) => std::fs::read_to_string(d.join(chemin)).ok(),
            None => self.memoire.get(chemin).cloned(),
        }
    }

    fn ecrire(&mut self, chemin: &str, s: String) -> std::io::Result<()> {
        let Some(d) = &self.dossier else {
            self.memoire.insert(chemin.to_string(), s);
            return Ok(());
        };
        let f = d.join(chemin);
        if let Some(p) = f.parent() {
            std::fs::create_dir_all(p)?;
        }
        let tmp = f.with_extension("tmp");
        std::fs::write(&tmp, s)?;
        std::fs::rename(&tmp, &f)
    }

    fn chemin(i: i32, j: i32) -> String {
        format!("tuiles/{i}_{j}.json")
    }

    /// La tuile (i, j) si elle a été écrite ; une erreur si son fichier est illisible.
    pub fn lire_tuile(&self, i: i32, j: i32, modeles: &mut Modeles) -> Result<Option<Contenu>, String> {
        let Some(s) = self.lire(&Depot::chemin(i, j)) else { return Ok(None) };
        depuis_json(&s, modeles).map(Some).ok_or_else(|| format!("{} illisible", Depot::chemin(i, j)))
    }

    pub fn ecrire_tuile(&mut self, i: i32, j: i32, c: &Contenu, modeles: &Modeles) -> std::io::Result<()> {
        let s = vers_json(i, j, c, modeles);
        self.ecrire(&Depot::chemin(i, j), s)
    }

    pub fn lire_fiche(&self) -> Option<Fiche> {
        let j = json::lire(&self.lire("monde.json")?)?;
        let joueur = j.get("joueur").and_then(|p| {
            let t = p.get("tuile")?.nombres::<2>()?;
            Some(([t[0] as i32, t[1] as i32], f3(p.get("pos"))?, p.get("cap")?.num()? as f32))
        });
        Some(Fiche {
            id: j.get("id")?.txt()?.to_string(),
            nom: j.get("nom").and_then(J::txt).unwrap_or("Monde").to_string(),
            graine: j.get("graine")?.num()? as u32,
            suivant: j.get("suivant").and_then(J::num).unwrap_or(0.0) as u64,
            joueur,
        })
    }

    pub fn ecrire_fiche(&mut self, f: &Fiche) -> std::io::Result<()> {
        let mut o = vec![("format", J::Num(FORMAT)), ("id", texte(&f.id)), ("nom", texte(&f.nom)), ("graine", J::Num(f.graine as f64)), ("suivant", J::Num(f.suivant as f64))];
        if let Some((t, p, c)) = f.joueur {
            o.push(("joueur", objet(vec![("tuile", J::Liste(vec![J::Num(t[0] as f64), J::Num(t[1] as f64)])), ("pos", nombres(&p)), ("cap", n(c))])));
        }
        let mut s = String::new();
        json::indente(&objet(o), 0, &mut s);
        self.ecrire("monde.json", s)
    }
}

/// La tuile (i, j) en JSON.
pub fn vers_json(i: i32, j: i32, c: &Contenu, modeles: &Modeles) -> String {
    let objets = c
        .objets
        .iter()
        .map(|e| match e.objet.genre {
            // Un modèle importé : sa référence et son échelle, jamais sa géométrie
            Genre::Modele(k) => objet(vec![
                ("id", texte(&e.id)),
                ("genre", texte("modele")),
                ("modele", texte(modeles.id(k))),
                ("pos", nombres(&e.pos)),
                ("cap", n(e.cap)),
                ("echelle", n(e.objet.dim[0])),
            ]),
            _ => objet(vec![
                ("id", texte(&e.id)),
                ("genre", texte(e.objet.genre.ident())),
                ("pos", nombres(&e.pos)),
                ("cap", n(e.cap)),
                ("dim", nombres(&e.objet.dim)),
                ("couleur", texte(&hex(e.objet.couleur))),
            ]),
        })
        .collect();
    let t = objet(vec![
        ("format", J::Num(FORMAT)),
        ("id", texte(&ident(i, j))),
        ("n", J::Num(c.relief.n as f64)),
        ("hauteurs", nombres(&c.relief.h)),
        ("couleurs", J::Liste(c.couleurs.iter().map(|x| texte(&hex(*x))).collect())),
        ("objets", J::Liste(objets)),
    ]);
    let mut s = String::new();
    json::compact(&t, &mut s);
    s
}

/// Une tuile relue ; `None` si le fichier ne se tient pas.
pub fn depuis_json(s: &str, modeles: &mut Modeles) -> Option<Contenu> {
    let j = json::lire(s)?;
    if j.get("format")?.num()? > FORMAT {
        return None;
    }
    let n = j.get("n")?.num()? as usize;
    let k = (n + 1) * (n + 1);
    let h: Vec<f32> = j.get("hauteurs")?.liste()?.iter().map(|x| x.num().map(|v| v as f32)).collect::<Option<_>>()?;
    let couleurs: Vec<[f32; 3]> = j.get("couleurs")?.liste()?.iter().map(|x| depuis_hex(x.txt()?)).collect::<Option<_>>()?;
    if n == 0 || h.len() != k || couleurs.len() != k {
        return None;
    }
    let relief = Relief { n, h };
    let mut objets = Vec::new();
    for o in j.get("objets")?.liste()? {
        let g = o.get("genre")?.txt()?;
        let objet = if g == "modele" {
            let k = modeles.inscrire(o.get("modele")?.txt()?);
            let s = o.get("echelle").and_then(J::num).unwrap_or(1.0) as f32;
            Objet { genre: Genre::Modele(k), dim: [s; 3], couleur: Objet::neuf(Genre::Modele(k)).couleur }
        } else {
            Objet { genre: Genre::depuis(g)?, dim: f3(o.get("dim"))?, couleur: depuis_hex(o.get("couleur")?.txt()?)? }
        };
        objets.push(Element {
            id: o.get("id")?.txt()?.to_string(),
            objet,
            pos: f3(o.get("pos"))?,
            cap: o.get("cap")?.num()? as f32,
        });
    }
    Some(Contenu { normales: relief.normales(), relief, couleurs, objets })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tuile() -> Contenu {
        let relief = Relief { n: 2, h: vec![0.0, 0.5, 1.0, 0.25, 0.75, 1.25, 0.5, 1.0, 1.5] };
        let objets = vec![Element { id: "o/3".into(), objet: Objet::neuf(Genre::Rampe), pos: [10.0, 0.5, 20.25], cap: 1.5 }];
        Contenu { normales: relief.normales(), relief, couleurs: vec![[0.2, 0.4, 0.6]; 9], objets }
    }

    #[test]
    fn une_tuile_se_relit_telle_qu_elle_s_est_ecrite() {
        let c = tuile();
        let mut m = Modeles::default();
        let s = vers_json(3, -1, &c, &m);
        assert!(s.contains("\"id\":\"tuile/3/-1\"") && s.contains("\"genre\":\"rampe\""), "{s}");
        let r = depuis_json(&s, &mut m).unwrap();
        assert_eq!(r.relief, c.relief);
        assert_eq!(r.objets[0].id, "o/3");
        assert_eq!(r.objets[0].pos, c.objets[0].pos);
        assert!((r.couleurs[0][1] - 0.4).abs() < 0.003);
        assert_eq!(r.normales.len(), 9);
        assert!(depuis_json("{\"format\":1}", &mut m).is_none());
        assert!(depuis_json(&s.replace("\"rampe\"", "\"inconnu\""), &mut m).is_none());
    }

    #[test]
    fn le_dossier_garde_les_tuiles_et_la_fiche() {
        let d = std::env::temp_dir().join(format!("sage-depot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let mut p = Depot::dossier(d.clone());
        let mut m = Modeles::default();
        assert_eq!(p.lire_tuile(0, 0, &mut m), Ok(None));
        p.ecrire_tuile(0, 0, &tuile(), &m).unwrap();
        let f = Fiche { suivant: 12, joueur: Some(([2, -3], [1.0, 2.0, 3.0], 0.5)), ..Fiche::default() };
        p.ecrire_fiche(&f).unwrap();
        let q = Depot::dossier(d.clone());
        assert_eq!(q.lire_tuile(0, 0, &mut m).unwrap().unwrap().objets.len(), 1);
        assert_eq!(q.lire_fiche(), Some(f));
        assert!(!d.join("tuiles/0_0.tmp").exists(), "le temporaire est renommé");
        std::fs::write(d.join("tuiles/1_0.json"), "{").unwrap();
        assert!(q.lire_tuile(1, 0, &mut m).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn un_modele_importe_ne_garde_que_sa_reference() {
        let mut m = Modeles::default();
        let k = m.inscrire("glb/3");
        let mut c = tuile();
        c.objets.push(Element { id: "o/9".into(), objet: Objet { genre: Genre::Modele(k), dim: [2.5; 3], couleur: [0.5; 3] }, pos: [1.0, 2.0, 3.0], cap: 0.5 });
        let s = vers_json(0, 0, &c, &m);
        assert!(s.contains("\"modele\":\"glb/3\"") && s.contains("\"echelle\":2.5"), "{s}");
        // Relu dans un monde neuf : le modèle s'inscrit sous son identifiant, encore absent
        let mut n = Modeles::default();
        let r = depuis_json(&s, &mut n).unwrap();
        let o = r.objets.iter().find(|e| e.id == "o/9").unwrap();
        let Genre::Modele(j) = o.objet.genre else { panic!() };
        assert_eq!(n.id(j), "glb/3");
        assert!(n.get(j).unwrap().gabarit.is_none());
        assert_eq!(o.objet.dim, [2.5; 3]);
    }
}
