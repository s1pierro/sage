//! Les retouches de la voirie (onglet Voirie de l'éditeur), gardées dans `voirie.json`
//! dans le dossier du monde.
//!
//! Un tronçon est la part d'une route entre deux de ses points, d'ordinaire deux
//! carrefours : il se reconnaît à ces deux points (absolus, au centimètre). Retoucher un
//! tronçon coupe la route à ses bouts ; le morceau du milieu prend le genre (la priorité)
//! et la largeur de chaussée réglés, et ses trottoirs leur largeur minimale. Les routes
//! des régions et du lieu réel restent telles qu'écrites : les retouches s'appliquent à
//! la lecture (`appliquer`), aux régions chargées comme au lieu réel (pour le
//! terrassement des régions régénérées).

use crate::reel::Route;
use gda::json::{self, J};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

/// Les réglages d'un tronçon ; `None` : ceux de la route.
#[derive(Clone, Debug, PartialEq)]
pub struct Troncon {
    pub a: [f32; 2],
    pub b: [f32; 2],
    /// Le genre et la largeur de la route d'origine, pour mémoire.
    pub origine: (u8, f32),
    pub genre: Option<u8>,
    pub largeur: Option<f32>,
    /// La largeur minimale des trottoirs (m) ; 0 : pas de trottoir. Réglée, elle vaut
    /// aussi hors des quartiers.
    pub trottoir: Option<f32>,
}

impl Troncon {
    /// Rien n'y est réglé.
    pub fn vide(&self) -> bool {
        self.genre.is_none() && self.largeur.is_none() && self.trottoir.is_none()
    }
}

fn cle(p: [f32; 2]) -> (i64, i64) {
    ((p[0] as f64 * 100.0).round() as i64, (p[1] as f64 * 100.0).round() as i64)
}

/// La clé d'un tronçon : ses deux bouts, dans un ordre fixe.
pub fn cle_troncon(a: [f32; 2], b: [f32; 2]) -> ((i64, i64), (i64, i64)) {
    let (x, y) = (cle(a), cle(b));
    if x <= y { (x, y) } else { (y, x) }
}

/// Les points `a` et `b` de la route, dans l'ordre de la route.
pub fn bornes(r: &Route, a: [f32; 2], b: [f32; 2]) -> Option<(usize, usize)> {
    let (ka, kb) = (cle(a), cle(b));
    let ia = r.points.iter().position(|p| cle([p[0], p[1]]) == ka)?;
    let ib = r.points.iter().position(|p| cle([p[0], p[1]]) == kb)?;
    (ia != ib).then(|| (ia.min(ib), ia.max(ib)))
}

/// Applique les retouches aux routes : chaque tronçon retouché devient une route à lui.
pub fn appliquer_avec(routes: &mut Vec<Route>, troncons: &[Troncon]) {
    for t in troncons.iter().filter(|t| !t.vide()) {
        let (x0, x1) = (t.a[0].min(t.b[0]) - 0.01, t.a[0].max(t.b[0]) + 0.01);
        let (z0, z1) = (t.a[1].min(t.b[1]) - 0.01, t.a[1].max(t.b[1]) + 0.01);
        let mut k = 0;
        while k < routes.len() {
            let r = &routes[k];
            // Vite : les deux bouts ne sont pas dans la boîte de la route
            let dans = |p: [f32; 2]| r.points.iter().any(|q| (q[0] - p[0]).abs() < 0.01 && (q[1] - p[1]).abs() < 0.01);
            let proche = r.points.iter().any(|q| q[0] >= x0 && q[0] <= x1 && q[1] >= z0 && q[1] <= z1);
            if !proche || !dans(t.a) || !dans(t.b) {
                k += 1;
                continue;
            }
            let Some((ia, ib)) = bornes(r, t.a, t.b) else {
                k += 1;
                continue;
            };
            let r = routes.remove(k);
            let mut morceaux = Vec::new();
            if ia > 0 {
                morceaux.push(Route { genre: r.genre, largeur: r.largeur, points: r.points[..=ia].to_vec() });
            }
            morceaux.push(Route { genre: t.genre.unwrap_or(r.genre), largeur: t.largeur.unwrap_or(r.largeur), points: r.points[ia..=ib].to_vec() });
            if ib + 1 < r.points.len() {
                morceaux.push(Route { genre: r.genre, largeur: r.largeur, points: r.points[ib..].to_vec() });
            }
            let n = morceaux.len();
            for (j, m) in morceaux.into_iter().enumerate() {
                routes.insert(k + j, m);
            }
            k += n;
        }
    }
}

/// Les retouches en cours, partagées entre les fils.
static COURANTES: RwLock<Option<Arc<Vec<Troncon>>>> = RwLock::new(None);

/// Les retouches en cours (aucune dans les tests : ils tournent ensemble).
pub fn courantes() -> Arc<Vec<Troncon>> {
    COURANTES.read().ok().and_then(|c| c.clone()).unwrap_or_default()
}

fn rendre_courantes(t: &[Troncon]) {
    if cfg!(test) {
        return;
    }
    if let Ok(mut c) = COURANTES.write() {
        *c = Some(Arc::new(t.to_vec()));
    }
}

/// Applique les retouches en cours.
pub fn appliquer(routes: &mut Vec<Route>) {
    let t = courantes();
    if !t.is_empty() {
        appliquer_avec(routes, &t);
    }
}

/// La largeur minimale des trottoirs réglée pour cette route, si elle est le morceau d'un
/// tronçon retouché.
pub fn trottoir(r: &Route) -> Option<f32> {
    let t = courantes();
    if t.is_empty() || r.points.len() < 2 {
        return None;
    }
    let (a, b) = (r.points[0], r.points[r.points.len() - 1]);
    let k = cle_troncon([a[0], a[1]], [b[0], b[1]]);
    t.iter().find(|x| cle_troncon(x.a, x.b) == k).and_then(|x| x.trottoir)
}

/// Les retouches gardées dans un fichier.
pub struct Voirie {
    pub troncons: Vec<Troncon>,
    fichier: Option<PathBuf>,
    pub erreur: Option<String>,
}

fn nombre(v: f32) -> J {
    J::Num((v as f64 * 100.0).round() / 100.0)
}

impl Voirie {
    /// Les retouches relues de `fichier`, ou aucune ; elles deviennent les courantes.
    pub fn ouvrir(fichier: Option<PathBuf>) -> Voirie {
        let mut troncons = Vec::new();
        if let Some(j) = fichier.as_ref().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|s| json::lire(&s)) {
            for t in j.get("troncons").and_then(J::liste).unwrap_or(&[]) {
                let paire = |k: &str| -> Option<[f32; 2]> {
                    let l = t.get(k).and_then(J::liste)?;
                    Some([l.first().and_then(J::num)? as f32, l.get(1).and_then(J::num)? as f32])
                };
                let (Some(a), Some(b)) = (paire("a"), paire("b")) else { continue };
                let num = |k: &str| t.get(k).and_then(J::num).map(|v| v as f32);
                let origine = (num("genre_origine").unwrap_or(1.0) as u8, num("largeur_origine").unwrap_or(6.0));
                let x = Troncon { a, b, origine, genre: num("genre").map(|g| (g as u8).min(crate::reel::SENTIER)), largeur: num("largeur").map(|l| l.clamp(1.0, 30.0)), trottoir: num("trottoir").map(|w| w.clamp(0.0, 8.0)) };
                if !x.vide() {
                    troncons.push(x);
                }
            }
        }
        rendre_courantes(&troncons);
        Voirie { troncons, fichier, erreur: None }
    }

    fn vers_json(&self) -> String {
        let l = self
            .troncons
            .iter()
            .map(|t| {
                let mut o: Vec<(String, J)> = vec![
                    ("a".into(), J::Liste(vec![nombre(t.a[0]), nombre(t.a[1])])),
                    ("b".into(), J::Liste(vec![nombre(t.b[0]), nombre(t.b[1])])),
                    ("genre_origine".into(), J::Num(t.origine.0 as f64)),
                    ("largeur_origine".into(), nombre(t.origine.1)),
                ];
                if let Some(g) = t.genre {
                    o.push(("genre".into(), J::Num(g as f64)));
                }
                if let Some(v) = t.largeur {
                    o.push(("largeur".into(), nombre(v)));
                }
                if let Some(v) = t.trottoir {
                    o.push(("trottoir".into(), nombre(v)));
                }
                J::Objet(o)
            })
            .collect();
        let o = J::Objet(vec![("format".into(), J::Num(1.0)), ("troncons".into(), J::Liste(l))]);
        let mut s = String::new();
        json::indente(&o, 0, &mut s);
        s
    }

    /// Les réglages du tronçon (a, b), s'il en a.
    pub fn troncon(&self, a: [f32; 2], b: [f32; 2]) -> Option<&Troncon> {
        let k = cle_troncon(a, b);
        self.troncons.iter().find(|t| cle_troncon(t.a, t.b) == k)
    }

    /// Règle le tronçon (remplacé ; ôté s'il n'a plus rien de réglé), le garde et le rend
    /// courant. Vrai s'il a changé.
    pub fn regler(&mut self, t: Troncon) -> bool {
        let k = cle_troncon(t.a, t.b);
        let i = self.troncons.iter().position(|x| cle_troncon(x.a, x.b) == k);
        let avant = i.map(|i| self.troncons[i].clone());
        if avant.as_ref() == Some(&t) || (avant.is_none() && t.vide()) {
            return false;
        }
        match (i, t.vide()) {
            (Some(i), true) => {
                self.troncons.remove(i);
            }
            (Some(i), false) => self.troncons[i] = t,
            (None, _) => self.troncons.push(t),
        }
        rendre_courantes(&self.troncons);
        if let Some(f) = &self.fichier {
            let tmp = f.with_extension("tmp");
            let r = f.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(&tmp, self.vers_json())).and_then(|_| std::fs::rename(&tmp, f));
            self.erreur = r.err().map(|e| format!("Voirie non enregistrée : {e}"));
        }
        true
    }
}

/// Le tronçon autour du segment `i` de la route `k` : jusqu'aux points qu'elle partage avec
/// une autre route de `routes` (les carrefours), ou à ses bouts. Les indices de ses bouts.
pub fn troncon_autour(routes: &[Route], k: usize, i: usize) -> (usize, usize) {
    let mut partages: HashMap<(i64, i64), u32> = HashMap::new();
    for (j, r) in routes.iter().enumerate() {
        if j == k {
            continue;
        }
        for p in &r.points {
            *partages.entry(cle([p[0], p[1]])).or_default() += 1;
        }
    }
    let r = &routes[k];
    let carrefour = |q: usize| partages.contains_key(&cle([r.points[q][0], r.points[q][1]]));
    let mut a = i;
    while a > 0 && !carrefour(a) {
        a -= 1;
    }
    let mut b = (i + 1).min(r.points.len() - 1);
    while b + 1 < r.points.len() && !carrefour(b) {
        b += 1;
    }
    (a, b)
}

/// La longueur de la route entre ses points `a` et `b` (m).
pub fn longueur(r: &Route, a: usize, b: usize) -> f32 {
    r.points[a..=b].windows(2).map(|s| (s[1][0] - s[0][0]).hypot(s[1][1] - s[0][1])).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reel::{ROUTE, SERVICE};

    fn rue() -> Vec<Route> {
        vec![
            Route { genre: ROUTE, largeur: 6.0, points: (0..=10).map(|i| [i as f32 * 10.0, 0.0, 1.0]).collect() },
            // Une rue qui croise en (30, 0) et une autre qui part de (70, 0)
            Route { genre: SERVICE, largeur: 4.0, points: vec![[30.0, -20.0, 1.0], [30.0, 0.0, 1.0], [30.0, 20.0, 1.0]] },
            Route { genre: SERVICE, largeur: 4.0, points: vec![[70.0, 0.0, 1.0], [70.0, 30.0, 1.0]] },
        ]
    }

    #[test]
    fn un_troncon_va_d_un_carrefour_a_l_autre() {
        let r = rue();
        assert_eq!(troncon_autour(&r, 0, 4), (3, 7));
        assert_eq!(troncon_autour(&r, 0, 1), (0, 3));
        assert_eq!(troncon_autour(&r, 0, 8), (7, 10));
        assert_eq!(longueur(&r[0], 3, 7), 40.0);
    }

    #[test]
    fn un_troncon_retouche_devient_une_route_a_lui() {
        let mut r = rue();
        let t = Troncon { a: [70.0, 0.0], b: [30.0, 0.0], origine: (ROUTE, 6.0), genre: Some(SERVICE), largeur: Some(9.0), trottoir: None };
        appliquer_avec(&mut r, &[t.clone()]);
        assert_eq!(r.len(), 5);
        assert_eq!((r[1].genre, r[1].largeur, r[1].points.len()), (SERVICE, 9.0, 5));
        assert_eq!((r[0].largeur, r[0].points.len(), r[2].points.len()), (6.0, 4, 4));
        // Appliquée deux fois, rien de plus
        let avant = r.clone();
        appliquer_avec(&mut r, &[t]);
        assert_eq!(r, avant);
    }

    #[test]
    fn les_retouches_se_gardent() {
        let d = std::env::temp_dir().join(format!("sage-voirie-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let f = Some(d.join("voirie.json"));
        let mut v = Voirie::ouvrir(f.clone());
        let t = Troncon { a: [30.0, 0.0], b: [70.0, 0.0], origine: (ROUTE, 6.0), genre: None, largeur: Some(8.5), trottoir: Some(3.0) };
        assert!(v.regler(t.clone()) && !v.regler(t.clone()));
        let w = Voirie::ouvrir(f.clone());
        assert_eq!(w.troncon([70.0, 0.0], [30.0, 0.0]), Some(&t));
        // Plus rien de réglé : ôté
        assert!(v.regler(Troncon { largeur: None, trottoir: None, ..t }));
        assert!(Voirie::ouvrir(f).troncons.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
