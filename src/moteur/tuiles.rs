//! Le monde en tuiles : des carrés de `TAILLE` mètres, repérés par `(i, j)`, chargés autour
//! du joueur et déchargés quand il s'éloigne.
//!
//! Les positions sont relatives à une *tuile d'origine* qui suit le joueur
//! (`Monde::origine`) : quand il passe dans une autre tuile, tout le monde se décale d'un
//! nombre entier de tuiles. Les flottants restent petits, donc précis, à n'importe quelle
//! distance du point de départ.

use super::depot::{Depot, Fiche};
use super::entites::{Entite, Lieu, Monde, Rendu};
use super::objets::Objet;
use super::ressources::{Format, MaillageId, Maillages};
use crate::math;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

/// Côté d'une tuile, en mètres.
pub const TAILLE: f32 = 64.0;
/// Les tuiles à plus de `PROCHE` tuiles du joueur dessinent leur terrain simplifié.
pub const PROCHE: i32 = 1;
/// Le terrain simplifié prend un point de la grille sur `SIMPLIFIE`.
pub const SIMPLIFIE: usize = 4;
/// Les jupes du terrain simplifié descendent de tant : elles cachent les fentes avec le
/// terrain complet voisin.
const JUPE: f32 = 1.5;

/// Le relief d'une tuile : hauteurs sur une grille de (n + 1)² points, au pas de
/// `TAILLE / n`, dans le repère de la tuile (de 0 à `TAILLE` sur x et z).
#[derive(Clone, Debug, PartialEq)]
pub struct Relief {
    pub n: usize,
    pub h: Vec<f32>,
}

impl Relief {
    pub fn pas(&self) -> f32 {
        TAILLE / self.n as f32
    }

    pub fn sommet(&self, a: usize, b: usize) -> f32 {
        self.h[b * (self.n + 1) + a]
    }

    /// La hauteur en (x, z), sur les mêmes triangles que le maillage : chaque carré est
    /// coupé le long de sa diagonale de (a, b) à (a + 1, b + 1).
    pub fn hauteur(&self, x: f32, z: f32) -> f32 {
        let n = self.n as f32;
        let (u, v) = ((x / self.pas()).clamp(0.0, n), (z / self.pas()).clamp(0.0, n));
        let (a, b) = ((u.floor() as usize).min(self.n - 1), (v.floor() as usize).min(self.n - 1));
        let (fu, fv) = (u - a as f32, v - b as f32);
        let (h00, h10, h01, h11) = (self.sommet(a, b), self.sommet(a + 1, b), self.sommet(a, b + 1), self.sommet(a + 1, b + 1));
        if fu >= fv {
            h00 + (h10 - h00) * fu + (h11 - h10) * fv
        } else {
            h00 + (h01 - h00) * fv + (h11 - h01) * fu
        }
    }

    /// La normale du triangle sous (x, z), vers le haut.
    pub fn normale(&self, x: f32, z: f32) -> [f32; 3] {
        let n = self.n as f32;
        let p = self.pas();
        let (u, v) = ((x / p).clamp(0.0, n), (z / p).clamp(0.0, n));
        let (a, b) = ((u.floor() as usize).min(self.n - 1), (v.floor() as usize).min(self.n - 1));
        let (fu, fv) = (u - a as f32, v - b as f32);
        let (h00, h10, h01, h11) = (self.sommet(a, b), self.sommet(a + 1, b), self.sommet(a, b + 1), self.sommet(a + 1, b + 1));
        // Les pentes du triangle, par mètre
        let (dx, dz) = if fu >= fv { ((h10 - h00) / p, (h11 - h10) / p) } else { ((h11 - h01) / p, (h01 - h00) / p) };
        let l = (dx * dx + dz * dz + 1.0).sqrt();
        [-dx / l, 1.0 / l, -dz / l]
    }

    /// Les triangles du relief, au format `Format::Terrain`, avec la normale et la couleur
    /// de chaque point de la grille.
    pub fn maillage(&self, normales: &[[f32; 3]], couleurs: &[[f32; 3]]) -> Vec<f32> {
        let n = self.n;
        let p = self.pas();
        let mut v = Vec::with_capacity(n * n * 6 * 9);
        let mut point = |a: usize, b: usize| {
            let k = b * (n + 1) + a;
            v.extend_from_slice(&[a as f32 * p, self.h[k], b as f32 * p]);
            v.extend_from_slice(&normales[k]);
            v.extend_from_slice(&couleurs[k]);
        };
        for b in 0..n {
            for a in 0..n {
                // Vus d'en haut, dans le sens trigonométrique
                for (da, db) in [(0, 0), (1, 1), (1, 0), (0, 0), (0, 1), (1, 1)] {
                    point(a + da, b + db);
                }
            }
        }
        v
    }
}

impl Relief {
    /// Le terrain simplifié : un point de la grille sur `k`, et une jupe sur le pourtour
    /// qui descend de `JUPE`, vue des deux côtés.
    pub fn maillage_loin(&self, normales: &[[f32; 3]], couleurs: &[[f32; 3]], k: usize) -> Vec<f32> {
        let n = self.n;
        if k <= 1 || n % k != 0 {
            return self.maillage(normales, couleurs);
        }
        let p = self.pas();
        let m = n / k;
        let mut v = Vec::with_capacity((m * m * 6 + m * 4 * 12) * 9);
        let mut point = |a: usize, b: usize, bas: f32| {
            let i = b * (n + 1) + a;
            v.extend_from_slice(&[a as f32 * p, self.h[i] - bas, b as f32 * p]);
            v.extend_from_slice(&normales[i]);
            v.extend_from_slice(&couleurs[i]);
        };
        for b in 0..m {
            for a in 0..m {
                for (da, db) in [(0, 0), (1, 1), (1, 0), (0, 0), (0, 1), (1, 1)] {
                    point((a + da) * k, (b + db) * k, 0.0);
                }
            }
        }
        // Les jupes : le long des quatre bords, chaque segment en deux faces
        let bords: [Box<dyn Fn(usize) -> (usize, usize)>; 4] = [Box::new(|t| (t, 0)), Box::new(|t| (n, t)), Box::new(move |t| (n - t, n)), Box::new(move |t| (0, n - t))];
        for bord in &bords {
            for s in 0..m {
                let (a0, b0) = bord(s * k);
                let (a1, b1) = bord((s + 1) * k);
                for (c, d) in [((a0, b0), (a1, b1)), ((a1, b1), (a0, b0))] {
                    point(c.0, c.1, 0.0);
                    point(d.0, d.1, 0.0);
                    point(d.0, d.1, JUPE);
                    point(c.0, c.1, 0.0);
                    point(d.0, d.1, JUPE);
                    point(c.0, c.1, JUPE);
                }
            }
        }
        v
    }
}

impl Relief {
    /// Les normales de chaque point de la grille, par différences centrées (d'un seul
    /// côté sur les bords).
    pub fn normales(&self) -> Vec<[f32; 3]> {
        let n = self.n;
        let p = self.pas();
        let mut v = Vec::with_capacity((n + 1) * (n + 1));
        for b in 0..=n {
            for a in 0..=n {
                let (a0, a1, b0, b1) = (a.saturating_sub(1), (a + 1).min(n), b.saturating_sub(1), (b + 1).min(n));
                let dx = (self.sommet(a1, b) - self.sommet(a0, b)) / ((a1 - a0) as f32 * p);
                let dz = (self.sommet(a, b1) - self.sommet(a, b0)) / ((b1 - b0) as f32 * p);
                let l = (dx * dx + dz * dz + 1.0).sqrt();
                v.push([-dx / l, 1.0 / l, -dz / l]);
            }
        }
        v
    }
}

/// Un objet posé dans une tuile : son identifiant stable, ses réglages, sa base dans le
/// repère de la tuile et son cap.
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    pub id: String,
    pub objet: Objet,
    pub pos: [f32; 3],
    pub cap: f32,
}

/// Ce qu'une source donne pour une tuile : le relief, ses normales et ses couleurs, et
/// les objets posés dessus, dans le repère de la tuile.
#[derive(Clone, Debug, PartialEq)]
pub struct Contenu {
    pub relief: Relief,
    pub normales: Vec<[f32; 3]>,
    pub couleurs: Vec<[f32; 3]>,
    pub objets: Vec<Element>,
}

/// D'où viennent les tuiles jamais modifiées : un générateur, chacune par son
/// identifiant.
pub trait Source: Send {
    fn tuile(&self, i: i32, j: i32) -> Contenu;
}

/// L'identifiant stable d'une tuile.
pub fn ident(i: i32, j: i32) -> String {
    format!("tuile/{i}/{j}")
}

/// Une tuile modifiée s'écrit quand on n'y a plus touché depuis tant de secondes.
pub const DELAI: f32 = 2.0;

/// Une tuile chargée : son terrain, ses couleurs (pour la réécrire), ses deux maillages.
pub struct Chargee {
    pub terrain: Entite,
    couleurs: Arc<Vec<[f32; 3]>>,
    proche: MaillageId,
    loin: Option<MaillageId>,
    /// Lue d'un fichier plutôt que générée.
    pub lue: bool,
}

/// Les tuiles chargées, et comment les choisir. Une tuile modifiée est écrite tout
/// entière dans le dépôt, et relue de là ensuite : elle ne doit plus rien au générateur.
pub struct Tuiles {
    source: Box<dyn Source>,
    pub depot: Depot,
    pub chargees: HashMap<(i32, i32), Chargee>,
    /// Les tuiles modifiées pas encore écrites, et depuis combien de secondes.
    sales: HashMap<(i32, i32), f32>,
    /// La fiche du monde, et si elle a changé depuis la dernière écriture.
    pub fiche: Fiche,
    fiche_sale: bool,
    /// Chargées jusqu'à `rayon` tuiles du joueur, gardées jusqu'à `garde`.
    pub rayon: i32,
    pub garde: i32,
    /// Durée du dernier chargement, en ms.
    pub derniere_ms: f32,
    /// Tuiles écrites depuis le lancement.
    pub ecrites: usize,
    /// La dernière erreur du dépôt.
    pub erreur: Option<String>,
}

impl Tuiles {
    /// Des tuiles gardées en mémoire seulement.
    pub fn new(source: Box<dyn Source>, rayon: i32, garde: i32) -> Tuiles {
        Tuiles::avec(source, Depot::memoire(), Fiche::default(), rayon, garde)
    }

    pub fn avec(source: Box<dyn Source>, depot: Depot, fiche: Fiche, rayon: i32, garde: i32) -> Tuiles {
        Tuiles {
            source,
            depot,
            chargees: HashMap::new(),
            sales: HashMap::new(),
            fiche,
            fiche_sale: false,
            rayon,
            garde: garde.max(rayon),
            derniere_ms: 0.0,
            ecrites: 0,
            erreur: None,
        }
    }

    /// La tuile qui contient le point `p` du monde.
    pub fn tuile_de(monde: &Monde, p: [f32; 3]) -> (i32, i32) {
        (monde.origine[0] + (p[0] / TAILLE).floor() as i32, monde.origine[1] + (p[2] / TAILLE).floor() as i32)
    }

    /// Le coin de la tuile (i, j), au repère du monde.
    pub fn coin(monde: &Monde, (i, j): (i32, i32)) -> [f32; 3] {
        [(i - monde.origine[0]) as f32 * TAILLE, 0.0, (j - monde.origine[1]) as f32 * TAILLE]
    }

    /// Les tuiles autour de `centre` : décharge celles au-delà de `garde` (en écrivant
    /// d'abord celles qui ont changé), charge au plus `limite` tuiles manquantes, les plus
    /// proches d'abord. Renvoie combien il en manque encore.
    pub fn suivre(&mut self, monde: &mut Monde, maillages: &mut Maillages, centre: (i32, i32), limite: usize) -> usize {
        let loin_de = |&(i, j): &(i32, i32), r: i32| (i - centre.0).abs().max((j - centre.1).abs()) > r;
        let partir: Vec<_> = self.chargees.keys().copied().filter(|k| loin_de(k, self.garde)).collect();
        for &k in &partir {
            if self.sales.contains_key(&k) {
                self.ecrire_tuile(monde, k);
            }
            let c = self.chargees.remove(&k).unwrap();
            let id = ident(k.0, k.1);
            maillages.ote(&id);
            maillages.ote(&format!("{id}/loin"));
            monde.detruire(c.terrain);
            for e in self.objets_de(monde, k) {
                monde.detruire(e);
            }
        }
        if !partir.is_empty() {
            balayer(monde, maillages);
        }
        let mut manque: Vec<(i32, i32)> = Vec::new();
        for j in centre.1 - self.rayon..=centre.1 + self.rayon {
            for i in centre.0 - self.rayon..=centre.0 + self.rayon {
                if !self.chargees.contains_key(&(i, j)) {
                    manque.push((i, j));
                }
            }
        }
        manque.sort_by_key(|&(i, j)| (i - centre.0).pow(2) + (j - centre.1).pow(2));
        let n = manque.len().min(limite);
        for &(i, j) in &manque[..n] {
            let t0 = Instant::now();
            let (c, lue) = match self.depot.lire_tuile(i, j) {
                Ok(Some(c)) => (c, true),
                Ok(None) => (self.source.tuile(i, j), false),
                Err(e) => {
                    self.erreur = Some(e);
                    (self.source.tuile(i, j), false)
                }
            };
            self.poser(monde, maillages, i, j, c, lue);
            self.derniere_ms = t0.elapsed().as_secs_f32() * 1000.0;
        }
        // Le terrain complet près du joueur, le simplifié au-delà
        for (k, c) in &self.chargees {
            let voulu = match c.loin {
                Some(l) if loin_de(k, PROCHE) => l,
                _ => c.proche,
            };
            if let Some(r) = monde.rendu.get_mut(c.terrain) {
                r.maillage = voulu;
            }
        }
        manque.len() - n
    }

    fn poser(&mut self, monde: &mut Monde, maillages: &mut Maillages, i: i32, j: i32, c: Contenu, lue: bool) {
        let id = ident(i, j);
        let o = Tuiles::coin(monde, (i, j));
        let t = monde.creer(&id);
        monde.lieu.mettre(t, Lieu::en(o, 0.0));
        let proche = maillages.mettre(&id, Format::Terrain, Arc::new(c.relief.maillage(&c.normales, &c.couleurs)));
        let loin = (c.relief.n % SIMPLIFIE == 0 && c.relief.n > SIMPLIFIE).then(|| maillages.mettre(&format!("{id}/loin"), Format::Terrain, Arc::new(c.relief.maillage_loin(&c.normales, &c.couleurs, SIMPLIFIE))));
        monde.rendu.mettre(t, Rendu { maillage: proche, local: math::scale(1.0) });
        monde.relief.mettre(t, c.relief);
        for el in c.objets {
            let e = monde.creer(&el.id);
            let p = [o[0] + el.pos[0], el.pos[1], o[2] + el.pos[2]];
            placer(monde, maillages, e, el.objet, Lieu::en(p, el.cap));
        }
        self.chargees.insert((i, j), Chargee { terrain: t, couleurs: Arc::new(c.couleurs), proche, loin, lue });
    }

    /// Les objets dont la base est dans la tuile `k`.
    pub fn objets_de(&self, monde: &Monde, k: (i32, i32)) -> Vec<Entite> {
        monde.objet.iter().filter_map(|(e, _)| monde.lieu.get(e).filter(|l| Tuiles::tuile_de(monde, l.pos) == k).map(|_| e)).collect()
    }

    /// Ce qu'il faut écrire de la tuile `k`, telle qu'elle est dans le monde.
    pub fn contenu(&self, monde: &Monde, k: (i32, i32)) -> Option<Contenu> {
        let c = self.chargees.get(&k)?;
        let relief = monde.relief.get(c.terrain)?.clone();
        let o = Tuiles::coin(monde, k);
        let mut objets: Vec<Element> = self
            .objets_de(monde, k)
            .into_iter()
            .filter_map(|e| {
                let (l, x, id) = (monde.lieu.get(e)?, monde.objet.get(e)?, monde.ident.get(e)?);
                Some(Element { id: id.clone(), objet: *x, pos: [l.pos[0] - o[0], l.pos[1], l.pos[2] - o[2]], cap: l.cap })
            })
            .collect();
        objets.sort_by(|a, b| a.id.cmp(&b.id));
        Some(Contenu { relief, normales: Vec::new(), couleurs: c.couleurs.to_vec(), objets })
    }

    /// La tuile `k` a changé : elle sera écrite dans `DELAI` secondes.
    pub fn marquer(&mut self, k: (i32, i32)) {
        if self.chargees.contains_key(&k) {
            self.sales.insert(k, 0.0);
        }
    }

    /// Combien de tuiles attendent d'être écrites.
    pub fn a_ecrire(&self) -> usize {
        self.sales.len() + self.fiche_sale as usize
    }

    /// Un identifiant neuf pour un objet, jamais donné dans ce monde.
    pub fn nouvel_id(&mut self) -> String {
        let id = format!("o/{}", self.fiche.suivant);
        self.fiche.suivant += 1;
        self.fiche_sale = true;
        id
    }

    /// Le temps passe de `dt` : écrit les tuiles laissées tranquilles depuis `DELAI`.
    pub fn vieillir(&mut self, monde: &Monde, dt: f32) {
        let mut dues = Vec::new();
        for (k, age) in self.sales.iter_mut() {
            *age += dt;
            if *age >= DELAI {
                dues.push(*k);
            }
        }
        for k in &dues {
            self.ecrire_tuile(monde, *k);
        }
        if !dues.is_empty() && self.fiche_sale {
            self.ecrire_fiche();
        }
    }

    /// Écrit tout ce qui attend : l'application passe en arrière-plan.
    pub fn tout_ecrire(&mut self, monde: &Monde) {
        let k: Vec<_> = self.sales.keys().copied().collect();
        for k in k {
            self.ecrire_tuile(monde, k);
        }
        self.ecrire_fiche();
    }

    fn ecrire_tuile(&mut self, monde: &Monde, k: (i32, i32)) {
        self.sales.remove(&k);
        let Some(c) = self.contenu(monde, k) else { return };
        match self.depot.ecrire_tuile(k.0, k.1, &c) {
            Ok(()) => {
                self.ecrites += 1;
                if let Some(t) = self.chargees.get_mut(&k) {
                    t.lue = true;
                }
            }
            Err(e) => self.erreur = Some(format!("{} : {e}", ident(k.0, k.1))),
        }
    }

    fn ecrire_fiche(&mut self) {
        match self.depot.ecrire_fiche(&self.fiche) {
            Ok(()) => self.fiche_sale = false,
            Err(e) => self.erreur = Some(format!("monde.json : {e}")),
        }
    }

    /// Change l'objet `e` : ses réglages, son lieu. Les tuiles qu'il quitte et qu'il
    /// rejoint sont à réécrire.
    pub fn modifier(&mut self, monde: &mut Monde, maillages: &mut Maillages, e: Entite, objet: Objet, lieu: Lieu) {
        if let Some(l) = monde.lieu.get(e) {
            let k = Tuiles::tuile_de(monde, l.pos);
            self.marquer(k);
        }
        placer(monde, maillages, e, objet, lieu);
        let k = Tuiles::tuile_de(monde, lieu.pos);
        self.marquer(k);
        balayer(monde, maillages);
    }

    /// Un objet neuf, sous un identifiant neuf.
    pub fn ajouter(&mut self, monde: &mut Monde, maillages: &mut Maillages, objet: Objet, lieu: Lieu) -> Entite {
        let id = self.nouvel_id();
        let e = monde.creer(&id);
        self.modifier(monde, maillages, e, objet, lieu);
        e
    }

    /// Ôte l'objet `e` du monde.
    pub fn oter(&mut self, monde: &mut Monde, maillages: &mut Maillages, e: Entite) {
        if let Some(l) = monde.lieu.get(e) {
            let k = Tuiles::tuile_de(monde, l.pos);
            self.marquer(k);
        }
        monde.detruire(e);
        balayer(monde, maillages);
    }
}

/// Donne à `e` l'objet `objet` en `lieu` : son maillage (partagé par les mêmes
/// réglages), ses obstacles à son cap.
pub fn placer(monde: &mut Monde, maillages: &mut Maillages, e: Entite, objet: Objet, lieu: Lieu) {
    let cle = objet.cle();
    let k = maillages.chercher(&cle).unwrap_or_else(|| maillages.mettre(&cle, Format::Statique, Arc::new(objet.maillage())));
    monde.lieu.mettre(e, lieu);
    monde.rendu.mettre(e, Rendu { maillage: k, local: math::scale(1.0) });
    monde.obstacles.mettre(e, objet.obstacles(lieu.cap));
    monde.objet.mettre(e, objet);
}

/// Ôte les maillages d'objets que plus rien ne dessine.
pub fn balayer(monde: &Monde, maillages: &mut Maillages) {
    let vus: std::collections::HashSet<MaillageId> = monde.rendu.iter().map(|(_, r)| r.maillage).collect();
    for (k, m) in maillages.tous() {
        if m.id.starts_with("objet/") && !vus.contains(&k) {
            maillages.ote(&m.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Un plan incliné, partout le même : h = x absolu / 10.
    struct Pente(Arc<AtomicUsize>);

    impl Source for Pente {
        fn tuile(&self, i: i32, _j: i32) -> Contenu {
            self.0.fetch_add(1, Ordering::Relaxed);
            let n = 8;
            let p = TAILLE / n as f32;
            let h: Vec<f32> = (0..=n).flat_map(|_b| (0..=n).map(move |a| (i as f32 * TAILLE + a as f32 * p) / 10.0)).collect();
            let relief = Relief { n, h };
            let k = (n + 1) * (n + 1);
            let objets = if (i, _j) == (0, 0) { vec![Element { id: "tuile/0/0/bloc/0".into(), objet: Objet::neuf(crate::moteur::Genre::Bloc), pos: [10.0, 0.0, 10.0], cap: 0.0 }] } else { vec![] };
            Contenu { relief, normales: vec![[0.0, 1.0, 0.0]; k], couleurs: vec![[0.5; 3]; k], objets }
        }
    }

    #[test]
    fn le_relief_suit_ses_triangles() {
        let r = Relief { n: 1, h: vec![0.0, 1.0, 2.0, 4.0] };
        assert_eq!(r.hauteur(0.0, 0.0), 0.0);
        assert_eq!(r.hauteur(TAILLE, TAILLE), 4.0);
        // Sur la diagonale, les deux triangles s'accordent
        assert!((r.hauteur(32.0, 32.0) - 2.0).abs() < 1e-5);
        assert!((r.hauteur(TAILLE, 0.0) - 1.0).abs() < 1e-3);
        let m = r.maillage(&[[0.0, 1.0, 0.0]; 4], &[[1.0; 3]; 4]);
        assert_eq!(m.len(), 6 * 9);
        // Chaque triangle regarde vers le haut
        for t in m.chunks(27) {
            let p = |k: usize| [t[k * 9], t[k * 9 + 1], t[k * 9 + 2]];
            let (a, b, c) = (p(0), p(1), p(2));
            let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
            assert!(u[2] * v[0] - u[0] * v[2] > 0.0, "normale vers +Y");
        }
    }

    #[test]
    fn le_terrain_simplifie_garde_les_bords_et_le_lointain_le_prend() {
        let n = 8;
        let h: Vec<f32> = (0..(n + 1) * (n + 1)).map(|k| (k % 7) as f32 * 0.3).collect();
        let r = Relief { n, h };
        let k = (n + 1) * (n + 1);
        let (nm, c) = (vec![[0.0, 1.0, 0.0]; k], vec![[1.0; 3]; k]);
        let plein = r.maillage(&nm, &c);
        let loin = r.maillage_loin(&nm, &c, 4);
        // 2 × 2 carrés de 2 triangles, et 4 bords de 2 segments à 4 triangles
        assert_eq!(loin.len() / 27, 8 + 32);
        assert!(loin.len() < plein.len());
        // Ses points du dessus sont sur le relief
        for t in loin.chunks(9) {
            let (x, y, z) = (t[0], t[1], t[2]);
            let jupe = (r.hauteur(x, z) - y - JUPE).abs() < 1e-4;
            assert!((r.hauteur(x, z) - y).abs() < 1e-4 || jupe, "({x}, {y}, {z})");
        }
        // Les tuiles : complètes près du joueur, simplifiées au-delà
        let mut t = Tuiles::new(Box::new(Pente(Arc::new(AtomicUsize::new(0)))), 2, 2);
        let (mut m, mut mr) = (Monde::default(), Maillages::default());
        t.suivre(&mut m, &mut mr, (0, 0), usize::MAX);
        let vu = |t: &Tuiles, m: &Monde, mr: &Maillages, k: (i32, i32)| mr.get(m.rendu.get(t.chargees[&k].terrain).unwrap().maillage).unwrap().id.clone();
        assert_eq!(vu(&t, &m, &mr, (1, 1)), "tuile/1/1");
        assert_eq!(vu(&t, &m, &mr, (2, 0)), "tuile/2/0/loin");
        t.suivre(&mut m, &mut mr, (1, 0), usize::MAX);
        assert_eq!(vu(&t, &m, &mr, (2, 0)), "tuile/2/0");
        assert_eq!(vu(&t, &m, &mr, (-1, 0)), "tuile/-1/0/loin");
    }

    #[test]
    fn les_tuiles_suivent_le_joueur_avec_une_marge() {
        let compte = Arc::new(AtomicUsize::new(0));
        let mut t = Tuiles::new(Box::new(Pente(compte.clone())), 1, 2);
        let (mut m, mut r) = (Monde::default(), Maillages::default());
        assert_eq!(t.suivre(&mut m, &mut r, (0, 0), 4), 5, "4 chargées, 5 attendent");
        assert_eq!(t.chargees.len(), 4);
        assert!(t.chargees.contains_key(&(0, 0)), "la plus proche d'abord");
        assert_eq!(t.suivre(&mut m, &mut r, (0, 0), usize::MAX), 0);
        assert_eq!(t.chargees.len(), 9);
        assert_eq!(m.nombre(), 10, "9 terrains et un objet");
        // Un pas à côté : rien ne part, trois arrivent
        t.suivre(&mut m, &mut r, (1, 0), usize::MAX);
        assert_eq!(t.chargees.len(), 12);
        // Loin : tout part, les maillages avec
        t.suivre(&mut m, &mut r, (10, 0), usize::MAX);
        assert_eq!(t.chargees.len(), 9);
        assert_eq!(m.nombre(), 9);
        assert_eq!(r.tous().len(), 18, "terrain complet et simplifié de chaque tuile, plus d'objet");
        assert_eq!(compte.load(Ordering::Relaxed), 21);
        assert!(m.trouver(&ident(10, 0)).is_some());
    }

    #[test]
    fn une_tuile_modifiee_s_ecrit_et_se_relit_au_retour() {
        use crate::moteur::{Forme, Genre};
        let mut t = Tuiles::new(Box::new(Pente(Arc::new(AtomicUsize::new(0)))), 1, 1);
        let (mut m, mut r) = (Monde::default(), Maillages::default());
        t.suivre(&mut m, &mut r, (0, 0), usize::MAX);
        // Le bloc passe dans la tuile voisine, tourné et élargi ; un poteau arrive
        let e = m.trouver("tuile/0/0/bloc/0").unwrap();
        let mut l = *m.lieu.get(e).unwrap();
        l.pos[0] += TAILLE + 5.0;
        l.cap = 0.4;
        let x = Objet { dim: [3.0, 1.0, 1.0], ..*m.objet.get(e).unwrap() };
        t.modifier(&mut m, &mut r, e, x, l);
        assert!(matches!(m.obstacles.get(e).unwrap()[..], [Forme::Triangles(_)]), "de travers : des triangles");
        let p = t.ajouter(&mut m, &mut r, Objet::neuf(Genre::Poteau), Lieu::en([30.0, 0.0, 30.0], 0.0));
        assert_eq!(m.ident.get(p).unwrap(), "o/0");
        assert_eq!(t.a_ecrire(), 3, "deux tuiles et la fiche");
        t.vieillir(&m, 1.0);
        assert_eq!(t.ecrites, 0, "pas avant le délai");
        t.vieillir(&m, 1.5);
        assert_eq!((t.ecrites, t.a_ecrire()), (2, 0));
        // Loin, puis de retour : les tuiles écrites sont relues, pas générées
        t.suivre(&mut m, &mut r, (10, 0), usize::MAX);
        assert!(m.trouver("o/0").is_none() && m.objet.iter().count() == 0);
        assert!(r.tous().iter().all(|(_, x)| !x.id.starts_with("objet/")), "les maillages d'objets partent avec");
        t.suivre(&mut m, &mut r, (0, 0), usize::MAX);
        let e = m.trouver("tuile/0/0/bloc/0").unwrap();
        let l = m.lieu.get(e).unwrap();
        assert_eq!(Tuiles::tuile_de(&m, l.pos), (1, 0));
        assert!((l.cap - 0.4).abs() < 1e-4);
        assert_eq!(m.objet.get(e).unwrap().dim[0], 3.0);
        assert!(m.trouver("o/0").is_some());
        assert!(t.chargees[&(0, 0)].lue && t.chargees[&(1, 0)].lue && !t.chargees[&(-1, 0)].lue);
        assert_eq!(m.objet.iter().count(), 2, "rien en double");
        // Ôté : son maillage aussi, que plus rien ne dessine
        let p = m.trouver("o/0").unwrap();
        t.oter(&mut m, &mut r, p);
        assert!(r.tous().iter().all(|(_, x)| !x.id.starts_with("objet/poteau")));
        assert_eq!(t.a_ecrire(), 1);
    }
}
