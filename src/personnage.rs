//! Le personnage en jeu : relu d'un .glb pour S.A.G.E. par la crate `gda`, mis à plat pour
//! le processeur graphique (triangles, os et poids par sommet), et ses matrices d'os pour
//! une pose donnée.
//!
//! Le mannequin intégré passe par le même chemin qu'un fichier : il est construit avec
//! `gda`, écrit en .glb, puis relu.

use crate::math::Mat4;
use gda::animation::{slerp, Animation};
use gda::armature::{Armature, Ecart, Pose};
use gda::maillage::{Face, Maillage, MatMtl, P3};
use gda::peau::{Peau, INFLUENCES};
use gda::quat::{qaxe, qinv, qmul, qnorm, Q};
use gda::sage::{self, Piece};
use std::sync::Arc;

/// Os au plus : autant de matrices dans le nuanceur de la peau.
pub const OS_MAX: usize = 48;
/// Flottants par sommet : position, normale, os (4), poids (4), couleur, coordonnées de
/// texture.
pub const FLOTTANTS: usize = 19;

/// Un matériau du personnage : sa texture décodée, son opacité, son mode d'alpha.
#[derive(Clone, Debug, PartialEq)]
pub struct MateriauModele {
    pub nom: String,
    pub image: Option<Arc<crate::image::Image>>,
    pub opacite: f32,
    pub alpha: crate::moteur::Alpha,
}

/// Une suite de sommets d'un même matériau : `nombre` à partir de `premier`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartieModele {
    pub premier: u32,
    pub nombre: u32,
    pub materiau: usize,
}

/// Un personnage prêt à dessiner et à animer.
pub struct Modele {
    pub id: String,
    pub nom: String,
    pub armature: Armature,
    pub animations: Vec<Animation>,
    /// Triangles à plat, `FLOTTANTS` par sommet, au repère de l'armature, rangés par
    /// matériau.
    pub sommets: Arc<Vec<f32>>,
    pub parties: Vec<PartieModele>,
    pub materiaux: Vec<MateriauModele>,
    /// Échelle de l'armature dans son projet, et hauteur des pieds au repos.
    pub echelle: f32,
    pub sol: f32,
    /// Hauteur du personnage au repos, échelle comprise.
    pub hauteur: f32,
    /// Inverse du repère de chaque os au repos, dans l'ordre des os.
    repos_inv: Vec<[f64; 16]>,
}

/// sRVB depuis le linéaire du glTF.
fn srvb(v: f64) -> f32 {
    let v = v.clamp(0.0, 1.0);
    (if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }) as f32
}

fn moins(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn vectoriel(a: P3, b: P3) -> P3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn unitaire(a: P3) -> P3 {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    if l < 1e-12 { [0.0, 1.0, 0.0] } else { [a[0] / l, a[1] / l, a[2] / l] }
}

fn mul4(a: &[f64; 16], b: &[f64; 16]) -> [f64; 16] {
    let mut m = [0.0; 16];
    for c in 0..4 {
        for r in 0..4 {
            m[c * 4 + r] = (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum();
        }
    }
    m
}

impl Modele {
    pub fn depuis_glb(d: &[u8]) -> Result<Modele, String> {
        let p = sage::lire(d)?;
        if p.armature.os.len() > OS_MAX {
            return Err(format!("{} os : S.A.G.E. en accepte {OS_MAX} au plus", p.armature.os.len()));
        }
        if p.pieces.iter().all(|x| x.maillage.faces.is_empty()) {
            return Err("personnage sans maillage".into());
        }
        let rang = |id: &str| p.armature.os.iter().position(|b| b.id == id);
        // Les matériaux : la couleur, et la texture décodée s'il y en a une
        let mut materiaux: Vec<MateriauModele> = p
            .materiaux
            .iter()
            .map(|m| {
                let image = m.image.as_ref().and_then(|(_, o)| crate::image::decoder(o).ok()).map(Arc::new);
                let opacite = m.couleur[3].clamp(0.0, 1.0) as f32;
                let alpha = if opacite < 1.0 {
                    crate::moteur::Alpha::Melange
                } else if image.as_ref().is_some_and(|i| i.transparente()) {
                    crate::moteur::Alpha::Masque
                } else {
                    crate::moteur::Alpha::Opaque
                };
                MateriauModele { nom: m.nom.clone(), image, opacite, alpha }
            })
            .collect();
        let couleurs: Vec<[f32; 3]> = p.materiaux.iter().map(|m| [srvb(m.couleur[0]), srvb(m.couleur[1]), srvb(m.couleur[2])]).collect();
        // Un seau de triangles par matériau ; un nom inconnu prend un matériau gris
        let mut seaux: Vec<Vec<f32>> = vec![Vec::new(); materiaux.len()];
        let indice = |nom: &str, materiaux: &mut Vec<MateriauModele>, seaux: &mut Vec<Vec<f32>>| match materiaux.iter().position(|m| m.nom == nom) {
            Some(k) => k,
            None => {
                materiaux.push(MateriauModele { nom: nom.to_string(), image: None, opacite: 1.0, alpha: crate::moteur::Alpha::Opaque });
                seaux.push(Vec::new());
                materiaux.len() - 1
            }
        };
        let (mut bas, mut haut) = (f64::INFINITY, f64::NEG_INFINITY);
        for piece in &p.pieces {
            let m = &piece.maillage;
            for s in &m.sommets {
                bas = bas.min(s[1]);
                haut = haut.max(s[1]);
            }
            // Os de la peau → rang dans l'armature
            let os: Vec<f32> = piece.peau.ids.iter().map(|i| rang(i).unwrap_or(0) as f32).collect();
            // Triangles un par un : chaque coin garde la normale de sa face, lissée
            let normales = normales_lissees(m, 50.0);
            for f in 0..m.faces.len() {
                let face = &m.faces[f];
                let k = indice(&face.m, &mut materiaux, &mut seaux);
                let c = couleurs.get(k).copied().unwrap_or([0.8, 0.8, 0.8]);
                let v = &mut seaux[k];
                let s = &face.s;
                for t in 1..s.len().saturating_sub(1) {
                    for &q in &[0, t, t + 1] {
                        let i = s[q];
                        let p3 = m.sommets[i];
                        let n = normales[f][q];
                        v.extend([p3[0] as f32, p3[1] as f32, p3[2] as f32, n[0] as f32, n[1] as f32, n[2] as f32]);
                        for r in 0..INFLUENCES {
                            let j = piece.peau.indices.get(i * INFLUENCES + r).copied().unwrap_or(0) as usize;
                            v.push(os.get(j).copied().unwrap_or(0.0));
                        }
                        let mut w: Vec<f32> = (0..INFLUENCES).map(|r| piece.peau.poids.get(i * INFLUENCES + r).copied().unwrap_or(0.0)).collect();
                        if w.iter().sum::<f32>() <= 0.0 {
                            w[0] = 1.0;
                        }
                        v.extend(w);
                        v.extend(c);
                        v.extend(face.uv.get(q).copied().unwrap_or([0.0, 0.0]));
                    }
                }
            }
        }
        // Les seaux bout à bout, opaques d'abord : une partie par matériau employé
        let mut ordre: Vec<usize> = (0..materiaux.len()).filter(|k| !seaux[*k].is_empty()).collect();
        ordre.sort_by_key(|k| materiaux[*k].alpha);
        let mut v = Vec::new();
        let mut parties = Vec::new();
        for k in ordre {
            let premier = (v.len() / FLOTTANTS) as u32;
            v.extend_from_slice(&seaux[k]);
            parties.push(PartieModele { premier, nombre: (seaux[k].len() / FLOTTANTS) as u32, materiau: k });
        }
        let repos = p.armature.repos();
        let repos_inv = p.armature.os.iter().map(|b| {
            let (t, q) = repos[&b.id];
            sage::inverse_rigide(t, q)
        }).collect();
        let echelle = p.place.2 as f32;
        Ok(Modele {
            id: p.id,
            nom: p.nom,
            animations: p.armature.animations(),
            armature: p.armature,
            sommets: Arc::new(v),
            parties,
            materiaux,
            echelle,
            sol: bas as f32,
            hauteur: ((haut - bas) as f32 * echelle).max(0.1),
            repos_inv,
        })
    }

    /// Un personnage sans maillage, d'une armature seule : pour animer sans dessiner. Le
    /// sol est le plus bas des bouts de ses os au repos.
    pub fn depuis_armature(id: &str, armature: Armature) -> Modele {
        let repos = armature.repos();
        let (mut bas, mut haut) = (f64::INFINITY, f64::NEG_INFINITY);
        for b in &armature.os {
            let (t, q) = repos[&b.id];
            let d = gda::quat::qappliquer(q, [0.0, b.longueur, 0.0]);
            for y in [t[1], t[1] + d[1]] {
                bas = bas.min(y);
                haut = haut.max(y);
            }
        }
        let repos_inv = armature.os.iter().map(|b| {
            let (t, q) = repos[&b.id];
            sage::inverse_rigide(t, q)
        }).collect();
        Modele {
            id: id.into(),
            nom: id.into(),
            animations: armature.animations(),
            armature,
            sommets: Arc::new(Vec::new()),
            parties: Vec::new(),
            materiaux: Vec::new(),
            echelle: 1.0,
            sol: bas as f32,
            hauteur: (haut - bas).max(0.1) as f32,
            repos_inv,
        }
    }

    /// Matrice de chaque os pour la pose : du repos à la pose, au repère de l'armature.
    pub fn matrices(&self, pose: &Pose) -> Vec<Mat4> {
        let mut a = self.armature.clone();
        a.pose = pose.clone();
        let posee = a.matrices_pose();
        a.os.iter().zip(&self.repos_inv).map(|(b, inv)| {
            let (t, q) = posee[&b.id];
            mul4(&sage::matrice(t, q), inv).map(|x| x as f32)
        }).collect()
    }

    /// L'animation dont le nom contient l'un des mots, sans tenir compte de la casse.
    pub fn animation(&self, mots: &[&str]) -> Option<usize> {
        self.animations.iter().position(|a| {
            let n = a.nom.to_lowercase();
            mots.iter().any(|m| n.contains(m))
        })
    }
}

/// Normale de chaque coin de chaque face : la moyenne des faces voisines au sommet qui
/// font avec elle un angle de moins de `seuil` degrés. Les arêtes vives le restent.
fn normales_lissees(m: &Maillage, seuil: f64) -> Vec<Vec<P3>> {
    let nf: Vec<P3> = m.faces.iter().map(|f| {
        // Newell : juste aussi pour un polygone non plan
        let mut n = [0.0; 3];
        for k in 0..f.s.len() {
            let (a, b) = (m.sommets[f.s[k]], m.sommets[f.s[(k + 1) % f.s.len()]]);
            n[0] += (a[1] - b[1]) * (a[2] + b[2]);
            n[1] += (a[2] - b[2]) * (a[0] + b[0]);
            n[2] += (a[0] - b[0]) * (a[1] + b[1]);
        }
        n
    }).collect();
    let mut autour = vec![Vec::new(); m.sommets.len()];
    for (k, f) in m.faces.iter().enumerate() {
        for &i in &f.s {
            autour[i].push(k);
        }
    }
    let cos = seuil.to_radians().cos();
    m.faces.iter().enumerate().map(|(k, f)| {
        let u = unitaire(nf[k]);
        f.s.iter().map(|&i| {
            let mut s = [0.0; 3];
            for &g in &autour[i] {
                let w = unitaire(nf[g]);
                if u[0] * w[0] + u[1] * w[1] + u[2] * w[2] >= cos {
                    s = [s[0] + nf[g][0], s[1] + nf[g][1], s[2] + nf[g][2]];
                }
            }
            unitaire(s)
        }).collect()
    }).collect()
}

/// Deux poses mêlées : `u` = 0 donne `a`, 1 donne `b`. Un os absent est au repos.
pub fn melanger(a: &Pose, b: &Pose, u: f64) -> Pose {
    let repos = Ecart { r: [0.0, 0.0, 0.0, 1.0], t: [0.0; 3] };
    let mut r = Pose::new();
    for id in a.keys().chain(b.keys()) {
        if r.contains_key(id) {
            continue;
        }
        let (x, y) = (a.get(id).copied().unwrap_or(repos), b.get(id).copied().unwrap_or(repos));
        r.insert(id.clone(), Ecart { r: slerp(x.r, y.r, u), t: [0, 1, 2].map(|c| x.t[c] + (y.t[c] - x.t[c]) * u) });
    }
    r
}

// ----- Mannequin intégré ---------------------------------------------------

/// Écart d'un os qui le tourne de `r`, rotation exprimée au monde dans la position de
/// repos et appliquée avant celles de ses parents.
fn ecart_monde(w: Q, r: Q) -> Ecart {
    Ecart { r: qnorm(qmul(qinv(w), qmul(r, w))), t: [0.0; 3] }
}

fn deg(d: f64) -> f64 {
    d.to_radians()
}

/// Le mannequin : l'humanoïde de `gda`, un pavé par os, et deux animations (« repos » et
/// « marche »). Écrit en .glb, comme l'exporterait Rustcraft.
pub fn mannequin_glb() -> Vec<u8> {
    let a = Armature::humanoide();
    let segs = a.segments();
    let id = |n: &str| a.os.iter().find(|b| b.nom == n).map(|b| b.id.clone()).unwrap();
    let mut m = Maillage::default();
    let mut peau = Peau { ids: a.os.iter().map(|b| b.id.clone()).collect(), indices: Vec::new(), poids: Vec::new() };
    let matiere = |n: &str| -> &'static str {
        match n {
            "tete" | "cou" | "main.G" | "main.D" => "peau",
            "bassin" | "cuisse.G" | "cuisse.D" | "jambe.G" | "jambe.D" => "pantalon",
            "pied.G" | "pied.D" => "chaussure",
            _ => "veste",
        }
    };
    for (k, b) in a.os.iter().enumerate() {
        let s = segs[&b.id];
        let base = b.nom.split('.').next().unwrap_or("");
        // Demi-largeurs, en travers de l'os (x) et d'avant en arrière (z)
        let (lx, lz) = match base {
            "tete" => (0.085, 0.095),
            "cou" => (0.04, 0.04),
            "thorax" => (0.16, 0.1),
            "colonne" => (0.14, 0.09),
            "bassin" => (0.15, 0.09),
            "clavicule" => (0.045, 0.045),
            "bras" => (0.05, 0.05),
            "avant-bras" => (0.042, 0.042),
            "main" => (0.035, 0.045),
            "cuisse" => (0.075, 0.075),
            "jambe" => (0.055, 0.055),
            "pied" => (0.045, 0.04),
            _ => (0.05, 0.05),
        };
        let l = moins(s.queue, s.tete);
        let longueur = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
        // Le repère du pavé : l'axe de l'os, un travers pris sur X (sur Z pour un os couché
        // le long de X, comme les bras), et le troisième
        let y = unitaire(l);
        let r = if y[0].abs() > 0.9 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] };
        let z = unitaire(vectoriel(r, y));
        let x = unitaire(vectoriel(y, z));
        let n0 = m.sommets.len();
        for &(i, j, h) in &[(-1.0, -1.0, 0.0), (1.0, -1.0, 0.0), (1.0, 1.0, 0.0), (-1.0, 1.0, 0.0), (-1.0, -1.0, 1.0), (1.0, -1.0, 1.0), (1.0, 1.0, 1.0), (-1.0, 1.0, 1.0)] {
            let p = [0, 1, 2].map(|c| s.tete[c] + x[c] * i * lx + z[c] * j * lz + y[c] * h * longueur);
            m.sommets.push(p);
            peau.indices.extend([k as u16, 0, 0, 0]);
            peau.poids.extend([1.0, 0.0, 0.0, 0.0]);
        }
        let mat: gda::maillage::Mat = Arc::from(matiere(&b.nom));
        // Six faces, dans le sens trigonométrique vues de dehors
        for q in [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [2, 3, 7, 6], [1, 2, 6, 5], [0, 4, 7, 3]] {
            // Chaque face couvre la texture une fois par 12 cm
            let (tu, tv) = ((lx * 2.0 / 0.12) as f32, (longueur.max(lz * 2.0) / 0.12) as f32);
            let mut f = Face { s: q.iter().map(|c| n0 + c).collect(), m: mat.clone(), uv: vec![[0.0, 0.0], [tu, 0.0], [tu, tv], [0.0, tv]] };
            // Le sens dépend de l'orientation (x, y, z) : on le vérifie sur la normale
            let c = [0, 1, 2].map(|d| f.s.iter().map(|i| m.sommets[*i][d]).sum::<f64>() / 4.0);
            let centre = [0, 1, 2].map(|d| s.tete[d] + y[d] * longueur / 2.0);
            let (p0, p1, p2) = (m.sommets[f.s[0]], m.sommets[f.s[1]], m.sommets[f.s[2]]);
            let n = vectoriel(moins(p1, p0), moins(p2, p0));
            let dehors = moins(c, centre);
            if n[0] * dehors[0] + n[1] * dehors[1] + n[2] * dehors[2] < 0.0 {
                f.s.reverse();
            }
            m.faces.push(f);
        }
    }

    // Les animations, en rotations au monde
    let repos = a.repos();
    let w = |n: &str| repos[&id(n)].1;
    let (x, z) = ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    let bras_bas = |c: &str| qaxe(z, deg(if c == "G" { -75.0 } else { 75.0 }));
    let pose_bras = |p: &mut Pose, c: &str, balancement: f64, coude: f64| {
        let bas = bras_bas(c);
        p.insert(id(&format!("bras.{c}")), ecart_monde(w(&format!("bras.{c}")), qmul(qaxe(x, deg(balancement)), bas)));
        // Le coude plie dans le plan du bras baissé
        let pli = qmul(qinv(bas), qmul(qaxe(x, deg(-coude)), bas));
        p.insert(id(&format!("avant-bras.{c}")), ecart_monde(w(&format!("avant-bras.{c}")), pli));
    };
    let mut repos_an = Animation::neuve("repos");
    repos_an.duree = 3.0;
    for (t, souffle) in [(0.0, 0.0), (1.5, 2.5)] {
        let mut p = Pose::new();
        pose_bras(&mut p, "G", 0.0, 8.0);
        pose_bras(&mut p, "D", 0.0, 8.0);
        p.insert(id("thorax"), ecart_monde(w("thorax"), qaxe(x, deg(-souffle))));
        repos_an.poser_cle(t, p);
    }
    let mut marche = Animation::neuve("marche");
    marche.duree = 1.0;
    // Un pas par demi-seconde : jambe gauche devant, croisement, droite devant, croisement
    for (t, cuisse, genou_g, genou_d, haut) in [(0.0, 28.0, 5.0, 20.0, -0.025), (0.25, 0.0, 0.0, 45.0, 0.015), (0.5, -28.0, 20.0, 5.0, -0.025), (0.75, 0.0, 45.0, 0.0, 0.015)] {
        let mut p = Pose::new();
        // Vers l'avant (+Z) : rotation négative autour de X
        p.insert(id("cuisse.G"), ecart_monde(w("cuisse.G"), qaxe(x, deg(-cuisse))));
        p.insert(id("cuisse.D"), ecart_monde(w("cuisse.D"), qaxe(x, deg(cuisse))));
        p.insert(id("jambe.G"), ecart_monde(w("jambe.G"), qaxe(x, deg(genou_g))));
        p.insert(id("jambe.D"), ecart_monde(w("jambe.D"), qaxe(x, deg(genou_d))));
        pose_bras(&mut p, "G", cuisse * 0.8, 15.0);
        pose_bras(&mut p, "D", -cuisse * 0.8, 15.0);
        let mut b = ecart_monde(w("bassin"), qaxe([0.0, 1.0, 0.0], deg(cuisse * 0.15)));
        b.t = [0.0, haut, 0.0];
        p.insert(id("bassin"), b);
        marche.poser_cle(t, p);
    }
    let mut a = a;
    a.regler_animations(&[repos_an, marche]);
    let mats = [("peau", "#e0b089"), ("veste", "#3f6fb5"), ("pantalon", "#3b3f4c"), ("chaussure", "#5a3a22")].map(|(n, c)| {
        let texture = (n == "veste").then(|| "tissu.png".to_string());
        (n.to_string(), MatMtl { couleur: Some(c.into()), texture, ..Default::default() })
    });
    let piece = Piece { id: "mannequin-corps".into(), nom: "corps".into(), maillage: m, peau };
    sage::glb("mannequin", "mannequin", &a, ([0.0; 3], [0.0, 0.0, 0.0, 1.0], 1.0), &[piece], &mats, &[("tissu.png".into(), Arc::new(tissu()))])
}

/// Le tissu de la veste : un sergé clair, en diagonale, que la couleur du matériau teinte.
pub fn tissu() -> Vec<u8> {
    crate::image::png_de(32, 32, |x, y| {
        let d = (x + y) % 8;
        let v = if d < 3 { 255 } else if d < 5 { 222 } else { 200 };
        let grain = if (x * 7 + y * 13) % 11 == 0 { 18 } else { 0 };
        let v = (v - grain) as u8;
        [v, v, v, 255]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn appliquer(m: &Mat4, p: [f32; 3]) -> [f32; 3] {
        [0, 1, 2].map(|r| m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r])
    }

    #[test]
    fn le_mannequin_se_relit_et_marche() {
        let m = Modele::depuis_glb(&mannequin_glb()).expect("mannequin");
        assert_eq!(m.armature.os.len(), 19);
        assert_eq!(m.sommets.len() % (3 * FLOTTANTS), 0);
        assert!(m.sommets.len() / FLOTTANTS >= 19 * 36, "un pavé par os");
        assert!((m.hauteur - 1.75).abs() < 0.1, "{}", m.hauteur);
        assert!(m.sol.abs() < 0.05, "les pieds au sol : {}", m.sol);
        let (r, w) = (m.animation(&["repos"]).unwrap(), m.animation(&["march", "walk"]).unwrap());
        assert_eq!((m.animations[r].nom.as_str(), m.animations[w].nom.as_str()), ("repos", "marche"));
        // Au repos, chaque matrice est l'identité
        for x in m.matrices(&Pose::new()) {
            for (k, v) in x.iter().enumerate() {
                assert!((v - if k % 5 == 0 { 1.0 } else { 0.0 }).abs() < 1e-5);
            }
        }
        // En marche, le pied gauche passe devant puis derrière
        let a = &m.animations[w];
        let pied = m.armature.os.iter().position(|b| b.nom == "pied.G").unwrap();
        let bout = m.armature.segments()[&m.armature.os[pied].id].queue.map(|c| c as f32);
        let z = |t: f64| appliquer(&m.matrices(&a.pose_a(t))[pied], bout)[2];
        assert!(z(0.0) > bout[2] + 0.1 && z(0.5) < bout[2] - 0.1, "{} {} {}", z(0.0), z(0.5), bout[2]);
        // Et les bras sont baissés
        let main = m.armature.os.iter().position(|b| b.nom == "main.G").unwrap();
        let bout = m.armature.segments()[&m.armature.os[main].id].queue.map(|c| c as f32);
        let y = appliquer(&m.matrices(&a.pose_a(0.25))[main], bout)[1];
        assert!(y < 1.0, "main à {y}");
    }

    #[test]
    fn le_mannequin_a_une_partie_par_materiau_et_sa_veste_une_texture() {
        let m = Modele::depuis_glb(&mannequin_glb()).unwrap();
        let noms: Vec<&str> = m.parties.iter().map(|p| m.materiaux[p.materiau].nom.as_str()).collect();
        assert_eq!(noms.len(), 4, "{noms:?}");
        for n in ["peau", "veste", "pantalon", "chaussure"] {
            assert!(noms.contains(&n), "{n} dans {noms:?}");
        }
        // Les parties se suivent et couvrent tout
        let mut fin = 0;
        for p in &m.parties {
            assert_eq!(p.premier, fin);
            fin += p.nombre;
        }
        assert_eq!(fin as usize, m.sommets.len() / FLOTTANTS);
        let veste = m.materiaux.iter().find(|x| x.nom == "veste").unwrap();
        let i = veste.image.as_ref().expect("la veste a sa texture");
        assert_eq!((i.l, i.h, veste.alpha), (32, 32, crate::moteur::Alpha::Opaque));
        assert!(m.materiaux.iter().filter(|x| x.nom != "veste").all(|x| x.image.is_none()));
        // Les coordonnées de texture de la veste vont au-delà de 1 : elle se répète
        let p = m.parties.iter().find(|p| m.materiaux[p.materiau].nom == "veste").unwrap();
        let umax = (p.premier..p.premier + p.nombre).map(|k| m.sommets[k as usize * FLOTTANTS + 17]).fold(0.0f32, f32::max);
        assert!(umax > 1.5, "{umax}");
    }

    #[test]
    fn les_faces_du_mannequin_regardent_dehors() {
        let m = Modele::depuis_glb(&mannequin_glb()).unwrap();
        // Le pavé du thorax : chaque normale s'éloigne de son centre
        let v = &m.sommets;
        let n = v.len() / FLOTTANTS;
        let mut dehors = 0;
        for k in 0..n / 3 {
            let s = |j: usize, c: usize| v[(k * 3 + j) * FLOTTANTS + c];
            let normale = [s(0, 3), s(0, 4), s(0, 5)];
            let e = [0, 1, 2].map(|c| (s(1, c) - s(0, c), s(2, c) - s(0, c)));
            let geo = [e[1].0 * e[2].1 - e[2].0 * e[1].1, e[2].0 * e[0].1 - e[0].0 * e[2].1, e[0].0 * e[1].1 - e[1].0 * e[0].1];
            if geo[0] * normale[0] + geo[1] * normale[1] + geo[2] * normale[2] > 0.0 {
                dehors += 1;
            }
        }
        assert_eq!(dehors, n / 3, "le sens des triangles suit leur normale");
    }

    #[test]
    fn melanger_va_d_une_pose_a_l_autre() {
        let a = Pose::from([("x".to_string(), Ecart { r: [0.0, 0.0, 0.0, 1.0], t: [0.0; 3] })]);
        let b = Pose::from([("x".to_string(), Ecart { r: qaxe([1.0, 0.0, 0.0], 1.0), t: [1.0, 0.0, 0.0] })]);
        let m = melanger(&a, &b, 0.5);
        assert!((m["x"].t[0] - 0.5).abs() < 1e-12);
        assert!((m["x"].r[0] - (0.25f64).sin()).abs() < 1e-9);
    }
}
