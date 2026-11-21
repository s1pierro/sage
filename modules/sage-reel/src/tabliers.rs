//! Les tabliers des ponts, en volume : une dalle continue d'un bout à l'autre du pont,
//! sous la chaussée et ses trottoirs, un peu plus large qu'eux, au profil en long lissé.
//!
//! - **Les chaînes** : OpenStreetMap coupe souvent un pont en plusieurs voies mises bout
//!   à bout. Celles qui se suivent (un bout commun, à moins de `ANGLE_SUITE` degrés de
//!   direction) et sont de même sorte (carrossables ou non) se chaînent, en prenant à
//!   chaque bout la suite la plus droite ; une bretelle qui part de côté fait sa
//!   propre chaîne.
//! - **Les voisines** : deux chaînes le sont quand, sur une bonne part de sa longueur,
//!   l'une longe l'autre (direction à moins de `ANGLE` degrés), à moins de `ECART`
//!   mètres de bord à bord, à moins de `DENIVELE` mètres de hauteur. Un trottoir le long
//!   d'une chaussée l'est de plus loin (`ECART_PIETON`) et malgré la hauteur
//!   (`DENIVELE_PIETON`) : ses bouts descendent souvent aux quais, d'où sa hauteur
//!   fausse. Les voisines des voisines vont ensemble.
//! - **Un tablier** suit la plus grande chaîne carrossable du groupe (son épine), de
//!   son premier à son dernier point : tous les `PAS` mètres (et à chacun de ses
//!   sommets), chaque voie du groupe qui passe là dans le même sens donne sa place de
//!   part et d'autre de l'axe ; la dalle va du bord le plus à gauche au plus à droite
//!   (plus `DEBORD`). Là où aucune ne passe, elle s'interrompt.
//! - **Le profil en long** (`profil`) vient des seules chaussées (des trottoirs s'il n'y
//!   en a pas) : leurs hauteurs moyennées par pas, lissées (une gaussienne de
//!   `LISSAGE` mètres), les bouts gardés à leur hauteur, et bombé d'une flèche de
//!   `COURBURE` fois la longueur (au plus `FLECHE_MAX`) : le pont monte doucement vers
//!   son milieu et redescend.
//! - **Les raccords** : les groupes se font du plus grand au plus petit ; un bout de
//!   tablier qui tombe sur un tablier déjà fait (à moins de `RACCORD` mètres, et de
//!   `RACCORD_DENIVELE` mètres de hauteur : sinon il passe dessus ou dessous) prend sa
//!   hauteur là (`Tablier::hauteur_en`), l'écart réparti en pente douce sur toute la
//!   longueur, sans bombement : une bretelle rejoint la chaussée à niveau.
//! - **Le volume** (`monter`) : le dessus à la hauteur du profil, le dessous
//!   `EPAISSEUR` plus bas, les flancs et les deux bouts.

use crate::{carrossable, Route};

/// Ce que le tablier déborde de la chaussée, de chaque côté (m).
pub const DEBORD: f32 = 0.75;
/// L'épaisseur de la dalle (m).
pub const EPAISSEUR: f32 = 1.2;
/// L'écart de bord à bord sous lequel deux ponts parallèles ne font qu'un tablier (m).
pub const ECART: f32 = 6.0;
/// L'écart toléré entre une chaussée et un trottoir le long d'elle (m).
pub const ECART_PIETON: f32 = 10.0;
/// L'écart de direction toléré entre deux ponts parallèles (degrés).
pub const ANGLE: f32 = 12.0;
/// L'écart de direction toléré entre deux voies qui se suivent (degrés).
pub const ANGLE_SUITE: f32 = 35.0;
/// L'écart de hauteur toléré entre deux ponts parallèles (m).
pub const DENIVELE: f32 = 3.0;
/// L'écart de hauteur toléré entre une chaussée et un trottoir le long d'elle (m).
pub const DENIVELE_PIETON: f32 = 8.0;
/// Le pas des coupes le long d'un tablier (m).
pub const PAS: f32 = 4.0;
/// La largeur du lissage du profil en long (m, l'écart type de la gaussienne).
pub const LISSAGE: f32 = 15.0;
/// La flèche du bombement, en part de la longueur du tablier (0 : pas de bombement).
pub const COURBURE: f32 = 0.01;
/// La flèche du bombement au plus (m).
pub const FLECHE_MAX: f32 = 1.5;
/// Un bout de tablier à moins de cette distance d'un autre tablier s'y raccorde (m).
pub const RACCORD: f32 = 3.0;
/// L'écart de hauteur au plus d'un raccord : au-delà, la voie passe dessus ou dessous (m).
pub const RACCORD_DENIVELE: f32 = 5.0;

/// Un tablier : ses coupes successives, chacune le bord gauche et le bord droit du
/// dessus (x, z, hauteur).
#[derive(Clone, Debug, PartialEq)]
pub struct Tablier {
    pub coupes: Vec<([f32; 3], [f32; 3])>,
}

impl Tablier {
    /// Le milieu du tablier (x, z), pour le ranger.
    pub fn centre(&self) -> [f32; 2] {
        let (g, d) = self.coupes[self.coupes.len() / 2];
        [(g[0] + d[0]) / 2.0, (g[1] + d[1]) / 2.0]
    }
}

/// Un axe : une ligne et la longueur cumulée à chaque sommet.
struct Axe<'a> {
    pts: &'a [[f32; 3]],
    cumul: Vec<f32>,
}

impl<'a> Axe<'a> {
    fn new(pts: &'a [[f32; 3]]) -> Axe<'a> {
        let mut cumul = vec![0.0];
        for s in pts.windows(2) {
            let l = cumul.last().copied().unwrap_or(0.0);
            cumul.push(l + (s[1][0] - s[0][0]).hypot(s[1][1] - s[0][1]));
        }
        Axe { pts, cumul }
    }

    fn longueur(&self) -> f32 {
        self.cumul.last().copied().unwrap_or(0.0)
    }

    /// Le segment `i` : son départ et sa direction unitaire.
    fn segment(&self, i: usize) -> ([f32; 2], [f32; 2]) {
        let (a, b) = (self.pts[i], self.pts[i + 1]);
        let l = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-6);
        ([a[0], a[1]], [(b[0] - a[0]) / l, (b[1] - a[1]) / l])
    }

    /// (s, t) du point : l'abscisse le long de l'axe (prolongé à ses bouts) et l'écart à
    /// gauche (positif) ou à droite, au plus proche.
    fn projeter(&self, q: [f32; 2]) -> (f32, f32) {
        let n = self.pts.len() - 1;
        let mut mieux = (f32::MAX, 0.0, 0.0);
        for i in 0..n {
            let (a, d) = self.segment(i);
            let l = self.cumul[i + 1] - self.cumul[i];
            let u = (q[0] - a[0]) * d[0] + (q[1] - a[1]) * d[1];
            let u = match (i == 0, i + 1 == n) {
                (true, true) => u,
                (true, false) => u.min(l),
                (false, true) => u.max(0.0),
                _ => u.clamp(0.0, l),
            };
            let (px, pz) = (a[0] + d[0] * u, a[1] + d[1] * u);
            let dist = (q[0] - px).hypot(q[1] - pz);
            if dist < mieux.0 {
                // À gauche dans le sens de la marche : la normale (−dz, dx)
                let t = (q[0] - px) * -d[1] + (q[1] - pz) * d[0];
                mieux = (dist, self.cumul[i] + u, t);
            }
        }
        (mieux.1, mieux.2)
    }

    /// Le point d'abscisse `s` (prolongé à ses bouts) et la direction là.
    fn point(&self, s: f32) -> ([f32; 2], [f32; 2]) {
        let n = self.pts.len() - 1;
        let i = (0..n).find(|&i| s <= self.cumul[i + 1]).unwrap_or(n - 1);
        let (a, d) = self.segment(i);
        let u = s - self.cumul[i];
        ([a[0] + d[0] * u, a[1] + d[1] * u], d)
    }
}

/// Les échantillons d'une voie le long d'un axe : (s, t, hauteur), triés par s, un tous
/// les `PAS` mètres au plus ; seulement là où elle va dans le sens de l'axe (à moins de
/// `ANGLE_SUITE` degrés), pas là où elle s'en écarte (une bretelle).
fn echantillons(axe: &Axe, p: &Route) -> Vec<(f32, f32, f32)> {
    let cos = ANGLE_SUITE.to_radians().cos();
    let mut v = Vec::new();
    for (k, s) in p.points.windows(2).enumerate() {
        let l = (s[1][0] - s[0][0]).hypot(s[1][1] - s[0][1]);
        if l < 1e-3 {
            continue;
        }
        let dp = [(s[1][0] - s[0][0]) / l, (s[1][1] - s[0][1]) / l];
        let n = (l / PAS).ceil().max(1.0) as usize;
        for i in 0..=n {
            if i == 0 && k > 0 {
                continue;
            }
            let f = i as f32 / n as f32;
            let q = [s[0][0] + (s[1][0] - s[0][0]) * f, s[0][1] + (s[1][1] - s[0][1]) * f];
            let (ss, t) = axe.projeter(q);
            let (_, da) = axe.point(ss);
            if (da[0] * dp[0] + da[1] * dp[1]).abs() >= cos {
                v.push((ss, t, s[0][2] + (s[1][2] - s[0][2]) * f));
            }
        }
    }
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v
}

/// (t, hauteur) des échantillons en s, si la voie passe là.
fn a_l_abscisse(e: &[(f32, f32, f32)], s: f32) -> Option<(f32, f32)> {
    let (a, b) = (e.first()?, e.last()?);
    if s < a.0 - 1e-3 || s > b.0 + 1e-3 {
        return None;
    }
    let i = e.windows(2).position(|w| s <= w[1].0).unwrap_or(0);
    let (p, q) = (e[i], e[(i + 1).min(e.len() - 1)]);
    let f = if q.0 > p.0 { ((s - p.0) / (q.0 - p.0)).clamp(0.0, 1.0) } else { 0.0 };
    Some((p.1 + (q.1 - p.1) * f, p.2 + (q.2 - p.2) * f))
}

/// La ligne `b` longe-t-elle `a` sur une bonne part de sa longueur, à moins de `ecart`
/// mètres de bord à bord et de `denivele` mètres de hauteur ?
fn longe(a: &Route, b: &Route, ecart: f32, denivele: f32) -> bool {
    if a.points.len() < 2 || b.points.len() < 2 {
        return false;
    }
    let axe = Axe::new(&a.points);
    let ea = echantillons(&axe, a);
    let marge = (a.largeur + b.largeur) / 2.0 + ecart;
    let cos = ANGLE.to_radians().cos();
    let mut bons = 0;
    let mut tous = 0;
    for s in b.points.windows(2) {
        let l = (s[1][0] - s[0][0]).hypot(s[1][1] - s[0][1]);
        if l < 1e-3 {
            continue;
        }
        let db = [(s[1][0] - s[0][0]) / l, (s[1][1] - s[0][1]) / l];
        let n = (l / PAS).ceil().max(1.0) as usize;
        for i in 0..n {
            let f = (i as f32 + 0.5) / n as f32;
            let q = [s[0][0] + (s[1][0] - s[0][0]) * f, s[0][1] + (s[1][1] - s[0][1]) * f];
            let h = s[0][2] + (s[1][2] - s[0][2]) * f;
            tous += 1;
            let (ss, t) = axe.projeter(q);
            let Some((ta, ha)) = a_l_abscisse(&ea, ss) else { continue };
            let (_, da) = axe.point(ss);
            if (t - ta).abs() <= marge && (h - ha).abs() <= denivele && (da[0] * db[0] + da[1] * db[1]).abs() >= cos {
                bons += 1;
            }
        }
    }
    tous > 0 && bons * 10 >= tous * 3
}

/// Une chaîne de voies qui se suivent : ses voies, sa ligne (bout à bout), si elle est
/// carrossable.
struct Chaine {
    membres: Vec<usize>,
    ligne: Route,
    carrossable: bool,
}

/// La direction unitaire (x, z) de `a` vers `b`.
fn direction(a: [f32; 3], b: [f32; 3]) -> [f32; 2] {
    let l = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-6);
    [(b[0] - a[0]) / l, (b[1] - a[1]) / l]
}

/// Les chaînes : chaque voie, de la plus grande (largeur × longueur : la chaussée
/// principale avant une longue bretelle étroite) à la plus petite, prolongée à ses deux
/// bouts par la suite la plus droite, de même sorte, pas encore prise.
fn chaines(ponts: &[Route]) -> Vec<Chaine> {
    let aire = |p: &Route| Axe::new(&p.points).longueur() * p.largeur;
    let mut ordre: Vec<usize> = (0..ponts.len()).filter(|&i| ponts[i].points.len() >= 2).collect();
    ordre.sort_by(|&a, &b| aire(&ponts[b]).total_cmp(&aire(&ponts[a])));
    let mut pris = vec![false; ponts.len()];
    let cos = ANGLE_SUITE.to_radians().cos();
    let mut v = Vec::new();
    for &graine in &ordre {
        if pris[graine] {
            continue;
        }
        pris[graine] = true;
        let sorte = carrossable(ponts[graine].genre);
        let mut membres = vec![graine];
        let mut pts = ponts[graine].points.clone();
        let mut largeur = ponts[graine].largeur;
        // En avant, puis en arrière (la ligne retournée, puis remise dans son sens)
        for _ in 0..2 {
            loop {
                let n = pts.len();
                let (fin, d) = (pts[n - 1], direction(pts[n - 2], pts[n - 1]));
                let mut mieux: Option<(f32, usize, bool)> = None;
                for &j in &ordre {
                    let q = &ponts[j].points;
                    if pris[j] || carrossable(ponts[j].genre) != sorte {
                        continue;
                    }
                    for envers in [false, true] {
                        let (a, b) = if envers { (q[q.len() - 1], q[q.len() - 2]) } else { (q[0], q[1]) };
                        if (a[0] - fin[0]).hypot(a[1] - fin[1]) > 0.5 {
                            continue;
                        }
                        let dd = direction(a, b);
                        let c = d[0] * dd[0] + d[1] * dd[1];
                        if c >= cos && mieux.is_none_or(|m| c > m.0) {
                            mieux = Some((c, j, envers));
                        }
                    }
                }
                let Some((_, j, envers)) = mieux else { break };
                pris[j] = true;
                membres.push(j);
                largeur = largeur.max(ponts[j].largeur);
                let mut q = ponts[j].points.clone();
                if envers {
                    q.reverse();
                }
                pts.extend_from_slice(&q[1..]);
            }
            pts.reverse();
        }
        v.push(Chaine { membres, ligne: Route { revetement: ponts[graine].revetement, genre: ponts[graine].genre, largeur, points: pts }, carrossable: sorte });
    }
    v
}

/// Les tabliers des ponts : un par groupe de chaînes voisines (ou par morceau, là où le
/// groupe s'interrompt).
pub fn tabliers(ponts: &[Route]) -> Vec<Tablier> {
    let ch = chaines(ponts);
    let n = ch.len();
    // Les boîtes, pour ne comparer que les chaînes proches
    let boites: Vec<[f32; 4]> = ch
        .iter()
        .map(|c| {
            let m = c.ligne.largeur / 2.0 + ECART_PIETON;
            c.ligne.points.iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, q| [b[0].min(q[0] - m), b[1].min(q[1] - m), b[2].max(q[0] + m), b[3].max(q[1] + m)])
        })
        .collect();
    let mut chef: Vec<usize> = (0..n).collect();
    fn racine(c: &mut [usize], mut i: usize) -> usize {
        while c[i] != i {
            c[i] = c[c[i]];
            i = c[i];
        }
        i
    }
    for i in 0..n {
        for j in i + 1..n {
            let (a, b) = (boites[i], boites[j]);
            if a[0] > b[2] || b[0] > a[2] || a[1] > b[3] || b[1] > a[3] {
                continue;
            }
            // Un trottoir le long d'une chaussée : de plus loin, malgré la hauteur
            let (ecart, denivele) = if ch[i].carrossable != ch[j].carrossable { (ECART_PIETON, DENIVELE_PIETON) } else { (ECART, DENIVELE) };
            if longe(&ch[i].ligne, &ch[j].ligne, ecart, denivele) || longe(&ch[j].ligne, &ch[i].ligne, ecart, denivele) {
                let (ri, rj) = (racine(&mut chef, i), racine(&mut chef, j));
                chef[ri] = rj;
            }
        }
    }
    let mut groupes: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for i in 0..n {
        let r = racine(&mut chef, i);
        groupes.entry(r).or_default().push(i);
    }
    // Les plus grands d'abord : une bretelle se raccorde au tablier déjà fait de la
    // chaussée qu'elle rejoint
    let aire = |c: &Chaine| Axe::new(&c.ligne.points).longueur() * c.ligne.largeur;
    let mut ordre: Vec<&Vec<usize>> = groupes.values().collect();
    ordre.sort_by(|a, b| {
        let m = |g: &Vec<usize>| g.iter().map(|&c| aire(&ch[c])).fold(0.0, f32::max);
        m(b).total_cmp(&m(a))
    });
    let mut v = Vec::new();
    for membres in ordre {
        let t = tablier_du_groupe(ponts, &ch, membres, &v);
        v.extend(t);
    }
    v
}

impl Tablier {
    /// La hauteur du dessus en (x, z), si le point est sur le tablier (ou à moins de
    /// `marge` mètres de son bord) : interpolée entre les deux coupes qui l'encadrent.
    /// Avec la distance au tablier (0 dessus).
    pub fn hauteur_en(&self, q: [f32; 2], marge: f32) -> Option<(f32, f32)> {
        let mut mieux: Option<(f32, f32)> = None;
        for w in self.coupes.windows(2) {
            let ((g0, d0), (g1, d1)) = (w[0], w[1]);
            let quad = [g0, d0, d1, g1];
            // Dedans (le signe des côtés), sinon la distance aux côtés
            let mut signe = 0i32;
            let mut dedans = true;
            let mut dist = f32::MAX;
            for k in 0..4 {
                let (a, b) = (quad[k], quad[(k + 1) % 4]);
                let (ex, ez) = (b[0] - a[0], b[1] - a[1]);
                let c = ex * (q[1] - a[1]) - ez * (q[0] - a[0]);
                let sg = if c > 0.0 { 1 } else if c < 0.0 { -1 } else { 0 };
                if sg != 0 {
                    if signe == 0 {
                        signe = sg;
                    } else if sg != signe {
                        dedans = false;
                    }
                }
                let l2 = (ex * ex + ez * ez).max(1e-9);
                let u = (((q[0] - a[0]) * ex + (q[1] - a[1]) * ez) / l2).clamp(0.0, 1.0);
                dist = dist.min((q[0] - a[0] - ex * u).hypot(q[1] - a[1] - ez * u));
            }
            let dist = if dedans { 0.0 } else { dist };
            if dist > marge || mieux.is_some_and(|m| m.1 <= dist) {
                continue;
            }
            // La place entre les deux coupes, le long de leurs milieux
            let (m0, m1) = ([(g0[0] + d0[0]) / 2.0, (g0[1] + d0[1]) / 2.0], [(g1[0] + d1[0]) / 2.0, (g1[1] + d1[1]) / 2.0]);
            let (ex, ez) = (m1[0] - m0[0], m1[1] - m0[1]);
            let u = (((q[0] - m0[0]) * ex + (q[1] - m0[1]) * ez) / (ex * ex + ez * ez).max(1e-9)).clamp(0.0, 1.0);
            mieux = Some((g0[2] + (g1[2] - g0[2]) * u, dist));
        }
        mieux
    }
}

/// La hauteur des tabliers déjà faits en (x, z), là où l'un d'eux passe (le plus proche).
fn hauteur_sur(faits: &[Tablier], q: [f32; 2]) -> Option<f32> {
    faits.iter().filter_map(|t| t.hauteur_en(q, RACCORD)).min_by(|a, b| a.1.total_cmp(&b.1)).map(|m| m.0)
}

/// Le profil en long : la hauteur lissée et bombée aux abscisses `abscisses` (croissantes),
/// d'après les échantillons (s, hauteur) des voies qui le portent, sur [s0, s1].
pub fn profil(echantillons: &[(f32, f32)], s0: f32, s1: f32, abscisses: &[f32]) -> Vec<f32> {
    profil_raccorde(echantillons, s0, s1, abscisses, [None, None])
}

/// Le profil en long, ses bouts posés aux hauteurs `bouts` quand elles sont données (une
/// bretelle sur le tablier qu'elle rejoint) : l'écart se répartit en pente douce sur toute
/// la longueur, et un profil raccordé n'est pas bombé (c'est une rampe).
pub fn profil_raccorde(echantillons: &[(f32, f32)], s0: f32, s1: f32, abscisses: &[f32], bouts: [Option<f32>; 2]) -> Vec<f32> {
    if echantillons.is_empty() || s1 <= s0 {
        return vec![echantillons.first().map_or(0.0, |e| e.1); abscisses.len()];
    }
    // Les hauteurs moyennes par pas ; les pas vides, entre leurs voisins
    let k = ((s1 - s0) / PAS).ceil().max(1.0) as usize + 1;
    let mut somme = vec![(0.0f32, 0u32); k];
    for &(s, h) in echantillons {
        let i = (((s - s0) / PAS).round().max(0.0) as usize).min(k - 1);
        somme[i].0 += h;
        somme[i].1 += 1;
    }
    let pleins: Vec<(f32, f32)> = somme.iter().enumerate().filter(|(_, c)| c.1 > 0).map(|(i, c)| (s0 + i as f32 * PAS, c.0 / c.1 as f32)).collect();
    let brut = |s: f32| -> f32 {
        let i = pleins.partition_point(|p| p.0 < s);
        match (i.checked_sub(1).map(|j| pleins[j]), pleins.get(i)) {
            (Some(a), Some(b)) if b.0 > a.0 => a.1 + (b.1 - a.1) * (s - a.0) / (b.0 - a.0),
            (Some(a), _) => a.1,
            (_, Some(b)) => b.1,
            _ => 0.0,
        }
    };
    let grille: Vec<f32> = (0..k).map(|i| brut(s0 + i as f32 * PAS)).collect();
    // Le lissage gaussien, sur les pas
    let sigma = LISSAGE / PAS;
    let r = (sigma * 3.0).ceil() as isize;
    let lisse = |s: f32| -> f32 {
        let c = (s - s0) / PAS;
        let (mut a, mut w) = (0.0, 0.0);
        for d in -r..=r {
            let i = c.round() as isize + d;
            let h = grille[(i.max(0) as usize).min(k - 1)];
            let e = (i as f32 - c) / sigma.max(1e-3);
            let p = (-0.5 * e * e).exp();
            a += h * p;
            w += p;
        }
        a / w
    };
    // Les bouts gardés à leur hauteur (ou à celle du tablier rejoint), et le bombement
    // (un tablier trop haut ou trop bas n'est pas rejoint : il passe dessus ou dessous)
    let bouts = [(bouts[0], grille[0]), (bouts[1], grille[k - 1])].map(|(b, h)| b.filter(|b| (b - h).abs() <= RACCORD_DENIVELE));
    let (d0, d1) = (bouts[0].unwrap_or(grille[0]) - lisse(s0), bouts[1].unwrap_or(grille[k - 1]) - lisse(s1));
    let l = s1 - s0;
    let fleche = if bouts.iter().any(|b| b.is_some()) { 0.0 } else { (COURBURE * l).min(FLECHE_MAX) };
    abscisses
        .iter()
        .map(|&s| {
            let u = ((s - s0) / l).clamp(0.0, 1.0);
            lisse(s.clamp(s0, s1)) + d0 + (d1 - d0) * u + fleche * 4.0 * u * (1.0 - u)
        })
        .collect()
}

/// Les tabliers d'un groupe de chaînes voisines, le long de son épine (la plus grande
/// des chaînes carrossables).
/// Un bout qui tombe sur un tablier déjà fait (`faits`) s'y raccorde.
fn tablier_du_groupe(ponts: &[Route], ch: &[Chaine], groupe: &[usize], faits: &[Tablier]) -> Vec<Tablier> {
    let longueur = |c: &Chaine| Axe::new(&c.ligne.points).longueur() * c.ligne.largeur;
    let porteuses: Vec<usize> = if groupe.iter().any(|&c| ch[c].carrossable) { groupe.iter().copied().filter(|&c| ch[c].carrossable).collect() } else { groupe.to_vec() };
    let Some(&epine) = porteuses.iter().max_by(|&&a, &&b| longueur(&ch[a]).total_cmp(&longueur(&ch[b]))) else { return Vec::new() };
    let axe = Axe::new(&ch[epine].ligne.points);
    // Chaque voie du groupe : ses échantillons, sa largeur, si elle porte le profil
    let mut voies: Vec<(Vec<(f32, f32, f32)>, f32, bool)> = Vec::new();
    for &c in groupe {
        for &m in &ch[c].membres {
            voies.push((echantillons(&axe, &ponts[m]), ponts[m].largeur, porteuses.contains(&c)));
        }
    }
    // L'étendue : celle des voies qui portent le profil
    let (s0, s1) = voies.iter().filter(|v| v.2).filter_map(|(e, _, _)| Some((e.first()?.0, e.last()?.0))).fold((f32::MAX, f32::MIN), |a, b| (a.0.min(b.0), a.1.max(b.1)));
    if s1 <= s0 {
        return Vec::new();
    }
    // Les coupes : tous les PAS mètres, aux sommets de l'axe et aux bouts de chaque voie
    let mut abscisses: Vec<f32> = Vec::new();
    let k = ((s1 - s0) / PAS).ceil().max(1.0) as usize;
    abscisses.extend((0..=k).map(|i| s0 + (s1 - s0) * i as f32 / k as f32));
    abscisses.extend(axe.cumul.iter().copied().filter(|s| *s > s0 && *s < s1));
    for (e, _, _) in &voies {
        if let (Some(a), Some(b)) = (e.first(), e.last()) {
            abscisses.extend([a.0, b.0].into_iter().filter(|s| *s > s0 && *s < s1));
        }
    }
    abscisses.sort_by(|a, b| a.total_cmp(b));
    abscisses.dedup_by(|a, b| (*a - *b).abs() < 0.05);
    let ech: Vec<(f32, f32)> = voies.iter().filter(|v| v.2).flat_map(|(e, _, _)| e.iter().map(|x| (x.0, x.2))).collect();
    let bouts = [s0, s1].map(|s| hauteur_sur(faits, axe.point(s).0));
    let hauteurs = profil_raccorde(&ech, s0, s1, &abscisses, bouts);
    let mut v = Vec::new();
    let mut coupes = Vec::new();
    for (&s, &h) in abscisses.iter().zip(&hauteurs) {
        let mut bords: Option<(f32, f32)> = None;
        for (e, l, _) in &voies {
            if let Some((t, _)) = a_l_abscisse(e, s) {
                let w = l / 2.0 + DEBORD;
                bords = Some(bords.map_or((t - w, t + w), |(g, d)| (g.min(t - w), d.max(t + w))));
            }
        }
        match bords {
            Some((droite, gauche)) => {
                let (p, d) = axe.point(s);
                let n = [-d[1], d[0]];
                coupes.push(([p[0] + n[0] * gauche, p[1] + n[1] * gauche, h], [p[0] + n[0] * droite, p[1] + n[1] * droite, h]));
            }
            None => {
                if coupes.len() >= 2 {
                    v.push(Tablier { coupes: std::mem::take(&mut coupes) });
                }
                coupes.clear();
            }
        }
    }
    if coupes.len() >= 2 {
        v.push(Tablier { coupes });
    }
    v
}

/// Monte le tablier en triangles (x, hauteur, z), chacun avec sa normale et sa couleur :
/// `dessus` pour le dessus, `dessous` pour les flancs et le dessous.
pub fn monter(t: &Tablier, dessus: [f32; 3], dessous: [f32; 3], tri: &mut dyn FnMut([[f32; 3]; 3], [f32; 3], [f32; 3])) {
    let c = &t.coupes;
    if c.len() < 2 {
        return;
    }
    let p = |q: [f32; 3], bas: bool| [q[0], q[2] - if bas { EPAISSEUR } else { 0.0 }, q[1]];
    // Un quadrilatère, tourné vers `n`
    let mut quad = |a: [f32; 3], b: [f32; 3], cc: [f32; 3], d: [f32; 3], n: [f32; 3], coul: [f32; 3]| {
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if l < 1e-6 {
            return;
        }
        let n = [n[0] / l, n[1] / l, n[2] / l];
        for (x, y, z) in [(a, b, cc), (a, cc, d)] {
            let (u, w) = ([y[0] - x[0], y[1] - x[1], y[2] - x[2]], [z[0] - x[0], z[1] - x[1], z[2] - x[2]]);
            let k = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
            if k[0] * n[0] + k[1] * n[1] + k[2] * n[2] >= 0.0 {
                tri([x, y, z], n, coul);
            } else {
                tri([x, z, y], n, coul);
            }
        }
    };
    for w in c.windows(2) {
        let ((g0, d0), (g1, d1)) = (w[0], w[1]);
        quad(p(g0, false), p(d0, false), p(d1, false), p(g1, false), [0.0, 1.0, 0.0], dessus);
        quad(p(g0, true), p(d0, true), p(d1, true), p(g1, true), [0.0, -1.0, 0.0], dessous);
        // Les flancs, vers le dehors
        let (gx, gz) = ((g0[0] + g1[0]) / 2.0 - (d0[0] + d1[0]) / 2.0, (g0[1] + g1[1]) / 2.0 - (d0[1] + d1[1]) / 2.0);
        quad(p(g0, false), p(g1, false), p(g1, true), p(g0, true), [gx, 0.0, gz], dessous);
        quad(p(d0, false), p(d1, false), p(d1, true), p(d0, true), [-gx, 0.0, -gz], dessous);
    }
    // Les bouts
    for k in [0, c.len() - 1] {
        let j = if k == 0 { 1 } else { k - 1 };
        let (g, d) = c[k];
        let (m, mj) = ([(g[0] + d[0]) / 2.0, (g[1] + d[1]) / 2.0], [(c[j].0[0] + c[j].1[0]) / 2.0, (c[j].0[1] + c[j].1[1]) / 2.0]);
        let n = [m[0] - mj[0], 0.0, m[1] - mj[1]];
        quad(p(g, false), p(d, false), p(d, true), p(g, true), n, dessous);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BUS, GRANDE, PIETONNE, ROUTE};

    fn pont(genre: u8, largeur: f32, pts: &[[f32; 3]]) -> Route {
        Route { revetement: 0, genre, largeur, points: pts.to_vec() }
    }

    /// La hauteur du tablier au plus près de x (le long d'un pont vers l'est).
    fn hauteur_en(t: &Tablier, x: f32) -> f32 {
        t.coupes.iter().min_by(|a, b| (a.0[0] - x).abs().total_cmp(&(b.0[0] - x).abs())).unwrap().0[2]
    }

    #[test]
    fn un_pont_seul_a_son_tablier_un_peu_plus_large() {
        let t = tabliers(&[pont(GRANDE, 8.0, &[[0.0, 0.0, 10.0], [100.0, 0.0, 12.0]])]);
        assert_eq!(t.len(), 1);
        let c = &t[0].coupes;
        assert!(c.len() >= 25);
        let (g, d) = c[0];
        // Vers l'est (x croissant), la gauche est vers z croissant : 4 + 0,75 m de part et
        // d'autre
        assert!((g[1] - 4.75).abs() < 1e-3 && (d[1] + 4.75).abs() < 1e-3, "{g:?} {d:?}");
        // Les bouts à leur hauteur ; au milieu, la pente plus le bombement (1 % de 100 m)
        assert!((g[2] - 10.0).abs() < 1e-3 && (c[c.len() - 1].0[2] - 12.0).abs() < 1e-3);
        assert!((hauteur_en(&t[0], 50.0) - 12.0).abs() < 0.05, "{}", hauteur_en(&t[0], 50.0));
    }

    #[test]
    fn le_profil_est_lisse_et_bombe() {
        // Une marche de 2 m au milieu : le profil la fond en pente douce, bouts gardés
        let ech: Vec<(f32, f32)> = (0..=50).map(|i| (i as f32 * 4.0, if i < 25 { 10.0 } else { 12.0 })).collect();
        let s: Vec<f32> = (0..=200).map(|i| i as f32).collect();
        let h = profil(&ech, 0.0, 200.0, &s);
        assert!((h[0] - 10.0).abs() < 1e-3 && (h[200] - 12.0).abs() < 1e-3);
        let pente = h.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
        assert!(pente < 0.08, "{pente}");
        // Sans marche : le bombement seul, 1,5 m au plus au milieu
        let plat: Vec<(f32, f32)> = (0..=50).map(|i| (i as f32 * 4.0, 10.0)).collect();
        let h = profil(&plat, 0.0, 200.0, &s);
        assert!((h[100] - 11.5).abs() < 1e-3 && h[50] > 10.5 && h[50] < h[100]);
    }

    #[test]
    fn les_voies_bout_a_bout_font_un_seul_tablier() {
        // Trois morceaux d'une même chaussée (un retourné), et une bretelle qui part de
        // côté au bout est : un tablier pour la chaussée, sans éventail, un pour la bretelle
        let a = pont(GRANDE, 8.0, &[[0.0, 0.0, 10.0], [60.0, 0.0, 11.0]]);
        let b = pont(GRANDE, 8.0, &[[120.0, 0.0, 12.0], [60.0, 0.0, 11.0]]);
        let c = pont(GRANDE, 8.0, &[[120.0, 0.0, 12.0], [180.0, 0.0, 11.0]]);
        let bretelle = pont(ROUTE, 5.0, &[[180.0, 0.0, 11.0], [220.0, 40.0, 10.0]]);
        let t = tabliers(&[a, b, c, bretelle]);
        assert_eq!(t.len(), 2, "{t:?}");
        let long = t.iter().find(|t| t.coupes.len() > 20).expect("la chaussée");
        let (x0, x1) = (long.coupes[0].0[0], long.coupes[long.coupes.len() - 1].0[0]);
        assert!(x0.min(x1).abs() < 0.1 && (x0.max(x1) - 180.0).abs() < 0.1, "{x0} {x1}");
        assert!(long.coupes.iter().all(|(g, d)| (g[1] - d[1]).abs() < 9.6), "éventail");
    }

    #[test]
    fn une_bretelle_rejoint_la_chaussee_a_niveau() {
        // La chaussée, droite et bombée ; une bretelle qui en part au milieu, de biais, à
        // une hauteur fausse (8 m) qui descend à 6 m
        let route = pont(GRANDE, 10.0, &[[0.0, 0.0, 10.0], [200.0, 0.0, 10.0]]);
        let bretelle = pont(ROUTE, 5.0, &[[100.0, 0.0, 8.0], [130.0, 30.0, 7.0], [160.0, 60.0, 6.0]]);
        let t = tabliers(&[bretelle, route]);
        assert_eq!(t.len(), 2, "{t:?}");
        let (grand, petit) = if t[0].coupes.len() > t[1].coupes.len() { (&t[0], &t[1]) } else { (&t[1], &t[0]) };
        let (h_route, d) = grand.hauteur_en([100.0, 0.0], 0.0).expect("sur la chaussée");
        assert!(d == 0.0 && (h_route - 11.5).abs() < 0.05, "{h_route}");
        // Le bout de la bretelle à la hauteur de la chaussée, l'autre à la sienne, sans bosse
        let (g, dd) = petit.coupes[0];
        let milieu = [(g[0] + dd[0]) / 2.0, (g[1] + dd[1]) / 2.0];
        let (debut, fin) = if milieu[0] < 110.0 { (g[2], petit.coupes[petit.coupes.len() - 1].0[2]) } else { (petit.coupes[petit.coupes.len() - 1].0[2], g[2]) };
        assert!((debut - h_route).abs() < 0.05 && (fin - 6.0).abs() < 1e-3, "{debut} {fin}");
        let h: Vec<f32> = petit.coupes.iter().map(|c| c.0[2]).collect();
        assert!(h.windows(2).all(|w| (w[1] - w[0]).abs() < 0.6), "{h:?}");
        // Une voie 8 m plus bas qui finit sous la chaussée n'y monte pas
        let dessous = pont(PIETONNE, 2.0, &[[100.0, 0.0, 2.0], [100.0, 60.0, 2.0]]);
        let route = pont(GRANDE, 10.0, &[[0.0, 0.0, 10.0], [200.0, 0.0, 10.0]]);
        let t = tabliers(&[dessous, route]);
        let bas = t.iter().find(|t| t.coupes.len() < 30).expect("la voie dessous");
        // (ses bouts à 2 m, et son bombement à elle)
        let n = bas.coupes.len() - 1;
        assert!((bas.coupes[0].0[2] - 2.0).abs() < 1e-3 && (bas.coupes[n].0[2] - 2.0).abs() < 1e-3, "{bas:?}");
        assert!(bas.coupes.iter().all(|c| c.0[2] < 2.0 + FLECHE_MAX), "{bas:?}");
    }

    #[test]
    fn un_trottoir_le_long_de_la_chaussee_est_dans_le_tablier() {
        // La chaussée à 11 m ; le trottoir 9 m à côté, descendu au quai à ses bouts
        let route = pont(ROUTE, 7.5, &[[0.0, 0.0, 11.0], [200.0, 0.0, 11.0]]);
        let trottoir = pont(PIETONNE, 2.0, &[[0.0, 9.0, 4.0], [40.0, 9.0, 9.0], [100.0, 9.0, 11.5], [160.0, 9.0, 9.0], [200.0, 9.0, 4.0]]);
        let t = tabliers(&[route, trottoir]);
        assert_eq!(t.len(), 1, "{t:?}");
        let (g, d) = t[0].coupes[t[0].coupes.len() / 2];
        // De la chaussée (−3,75 − 0,75) au trottoir (9 + 1 + 0,75), à la hauteur de la
        // chaussée (et du bombement)
        assert!((g[1] - 10.75).abs() < 0.05 && (d[1] + 4.5).abs() < 0.05, "{g:?} {d:?}");
        assert!((t[0].coupes[0].0[2] - 11.0).abs() < 1e-3 && (g[2] - 12.5).abs() < 0.05, "{g:?}");
    }

    #[test]
    fn deux_ponts_paralleles_font_un_seul_tablier() {
        // Les deux chaussées d'une voie rapide, à 3 m de bord à bord, dans des sens
        // opposés ; une voie de bus plus loin (20 m) a le sien
        let a = pont(GRANDE, 8.0, &[[0.0, 0.0, 10.0], [200.0, 0.0, 10.0]]);
        let b = pont(GRANDE, 8.0, &[[190.0, 11.0, 10.5], [10.0, 11.0, 10.5]]);
        let c = pont(BUS, 4.0, &[[0.0, 40.0, 10.0], [200.0, 40.0, 10.0]]);
        let t = tabliers(&[a.clone(), b.clone(), c]);
        assert_eq!(t.len(), 2);
        let large = t.iter().find(|t| t.coupes.len() > 10 && (t.coupes[t.coupes.len() / 2].0[1] - t.coupes[t.coupes.len() / 2].1[1]).abs() > 15.0).expect("tablier unifié");
        // Au milieu : du bord de a (z = −4,75) au bord de b (z = 15,75), à la hauteur
        // moyenne des deux, bombée
        let (g, d) = large.coupes[large.coupes.len() / 2];
        let (g, d) = if g[1] > d[1] { (g, d) } else { (d, g) };
        assert!((g[1] - 15.75).abs() < 0.05 && (d[1] + 4.75).abs() < 0.05, "{g:?} {d:?}");
        assert!(g[2] > 10.25 + 1.0 && g[2] < 10.25 + 1.6, "{g:?}");
        // Au bout ouest, seul a passe : sa largeur à lui
        let (g, d) = large.coupes.iter().min_by(|a, b| a.0[0].total_cmp(&b.0[0])).copied().unwrap();
        assert!(((g[1] - d[1]).abs() - 9.5).abs() < 0.05, "{g:?} {d:?}");
        // Trop haut l'un sur l'autre (un pont qui en enjambe un autre) : chacun le sien
        let haut = pont(GRANDE, 8.0, &[[0.0, 11.0, 20.0], [200.0, 11.0, 20.0]]);
        assert_eq!(tabliers(&[a.clone(), haut]).len(), 2);
        // En croix : chacun le sien
        let croix = pont(GRANDE, 8.0, &[[100.0, -100.0, 10.0], [100.0, 100.0, 10.0]]);
        assert_eq!(tabliers(&[a, croix]).len(), 2);
    }

    #[test]
    fn le_volume_est_ferme_et_tourne_vers_le_dehors() {
        let t = &tabliers(&[pont(GRANDE, 8.0, &[[0.0, 0.0, 10.0], [40.0, 0.0, 10.0], [80.0, 30.0, 10.0]])])[0];
        let mut tris = Vec::new();
        monter(t, [1.0; 3], [0.5; 3], &mut |p, n, _| tris.push((p, n)));
        let k = t.coupes.len() - 1;
        // Dessus, dessous, deux flancs par tranche ; deux bouts
        assert_eq!(tris.len(), k * 8 + 4);
        for (p, n) in &tris {
            let (u, w) = ([p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]], [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]]);
            let c = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
            assert!(c[0] * n[0] + c[1] * n[1] + c[2] * n[2] >= 0.0);
        }
        // Le dessus de 10 m (aux bouts) à 10 m plus le bombement, le dessous 1,2 m plus bas
        let haut = |y: f32| (10.0 - 1e-4..=10.0 + FLECHE_MAX).contains(&y);
        assert!(tris.iter().filter(|(_, n)| n[1] > 0.9).all(|(p, _)| p.iter().all(|q| haut(q[1]))));
        assert!(tris.iter().filter(|(_, n)| n[1] < -0.9).all(|(p, _)| p.iter().all(|q| haut(q[1] + EPAISSEUR))));
    }
}
