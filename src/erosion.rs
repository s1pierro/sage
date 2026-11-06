//! L'érosion hydraulique : des gouttes de pluie tombent sur l'île, coulent selon la pente,
//! arrachent de la terre là où elles vont vite et la déposent là où elles ralentissent.
//! Les vallées se creusent, les ravines marquent les pentes, les plaines se comblent au
//! pied des montagnes, et la terre emportée finit en deltas au bord de la mer.
//!
//! Le calcul se fait une fois par graine, à « Régénérer » (ou au premier lancement), sur
//! une grille de `PAS` mètres qui couvre toute la carte. Il en reste deux champs :
//!
//! - `ecart` : ce que l'érosion a changé à la hauteur, en mètres ; la campagne l'ajoute à
//!   son relief, interpolé en douceur (Catmull-Rom) ;
//! - `eau` : l'eau passée en chaque point, de 0 à 1 (échelle logarithmique) : les fonds
//!   de vallée, plus tard les rivières.
//!
//! Le calcul est déterministe : les gouttes partent de points tirés de la graine et se
//! suivent une à une ; seules les hauteurs de départ se calculent sur plusieurs fils.

use crate::moteur::tuiles::DEMI_CARTE;
use crate::moteur::TAILLE;

/// Le pas de la grille, en mètres.
pub const PAS: f64 = 16.0;
/// Le format du fichier d'érosion.
const FORMAT: u32 = 1;
/// Les réglages du calcul ; `Reglage::default()` est celui du jeu.
#[derive(Clone, Copy, Debug)]
pub struct Reglage {
    /// Gouttes par point de terre.
    pub densite: f64,
    /// Pas d'une goutte, au plus.
    pub vie: usize,
    /// Part de sa direction qu'une goutte garde d'un pas à l'autre.
    pub inertie: f32,
    /// Terre emportée : capacité, vitesse d'arrachage, de dépôt.
    pub capacite: f32,
    pub arrache: f32,
    pub depose: f32,
    /// Pente minimale prise pour la capacité : l'eau lente emporte encore un peu.
    pub pente_min: f32,
    pub evaporation: f32,
    pub gravite: f32,
    /// Le rayon du pinceau qui arrache, en points de grille.
    pub rayon: i32,
    /// Une goutte n'arrache pas plus que tant de mètres en un point, en tout.
    pub creuse_max: f32,
    /// Les hauteurs comptent pour tant dans les pentes : moins, l'eau voit un relief plus
    /// doux et l'érosion est plus lente.
    pub echelle: f32,
}

impl Default for Reglage {
    fn default() -> Reglage {
        Reglage { densite: 0.35, vie: 80, inertie: 0.3, capacite: 2.0, arrache: 0.12, depose: 0.05, pente_min: 0.01, evaporation: 0.02, gravite: 4.0, rayon: 2, creuse_max: 40.0, echelle: 0.25 }
    }
}

/// Le coin de la grille (absolu), en mètres : le coin de la première tuile de la carte.
pub fn origine() -> f64 {
    -(DEMI_CARTE as f64) * TAILLE as f64
}

/// Points de la grille par côté.
pub fn cote() -> usize {
    ((2 * DEMI_CARTE + 1) as f64 * TAILLE as f64 / PAS).round() as usize + 1
}

#[derive(Clone, Debug, PartialEq)]
pub struct Erosion {
    pub graine: u32,
    pub cote: usize,
    pub ecart: Vec<f32>,
    pub eau: Vec<f32>,
}

/// Un nombre de 0 à 1, toujours le même pour (graine, k, sel).
fn hasard(graine: u32, k: u64, sel: u64) -> f32 {
    let mut h = k.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ sel.wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ (graine as u64).wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 31;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 29;
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// Catmull-Rom : la valeur entre `b` et `c` en `t`, sa pente continue d'une maille à
/// l'autre.
fn cr(a: f64, b: f64, c: f64, d: f64, t: f64) -> f64 {
    b + 0.5 * t * (c - a + t * (2.0 * a - 5.0 * b + 4.0 * c - d + t * (3.0 * (b - c) + d - a)))
}

impl Erosion {
    /// Une érosion qui ne change rien.
    pub fn nulle(graine: u32) -> Erosion {
        let n = cote();
        Erosion { graine, cote: n, ecart: vec![0.0; n * n], eau: vec![0.0; n * n] }
    }

    /// L'érosion du relief `hauteur` (absolu, en mètres), la mer à `mer`.
    pub fn calculer(graine: u32, mer: f32, hauteur: &(dyn Fn(f64, f64) -> f64 + Sync)) -> Erosion {
        Erosion::calculer_avec(&Reglage::default(), graine, mer, hauteur)
    }

    pub fn calculer_avec(r: &Reglage, graine: u32, mer: f32, hauteur: &(dyn Fn(f64, f64) -> f64 + Sync)) -> Erosion {
        let r = *r;
        let echelle = (PAS / r.echelle as f64) as f32;
        let n = cote();
        let o = origine();
        // Les hauteurs de départ, en points de grille (un mètre vaut 1/PAS) : les pentes
        // se lisent sans unité
        let mut h = vec![0f32; n * n];
        let fils = std::thread::available_parallelism().map_or(4, |f| f.get()).clamp(1, 8);
        let par = n.div_ceil(fils);
        std::thread::scope(|s| {
            for (k, bande) in h.chunks_mut(par * n).enumerate() {
                s.spawn(move || {
                    for (r, ligne) in bande.chunks_mut(n).enumerate() {
                        let b = k * par + r;
                        for (a, v) in ligne.iter_mut().enumerate() {
                            *v = hauteur(o + a as f64 * PAS, o + b as f64 * PAS) as f32 / echelle;
                        }
                    }
                });
            }
        });
        let depart = h.clone();
        let mer = mer / echelle;
        let mut eau = vec![0f32; n * n];
        let mut creuse = vec![0f32; n * n];

        // Le pinceau : ses points et leurs poids, qui somment à 1
        let mut pinceau: Vec<(i32, i32, f32)> = Vec::new();
        for dz in -r.rayon..=r.rayon {
            for dx in -r.rayon..=r.rayon {
                let d = ((dx * dx + dz * dz) as f32).sqrt();
                if d <= r.rayon as f32 {
                    pinceau.push((dx, dz, 1.0 - d / (r.rayon as f32 + 0.5)));
                }
            }
        }
        let total: f32 = pinceau.iter().map(|p| p.2).sum();
        for p in &mut pinceau {
            p.2 /= total;
        }

        // Les points de terre : les gouttes y tombent
        let terres: Vec<u32> = (0..n * n).filter(|&i| h[i] > mer).map(|i| i as u32).collect();
        let gouttes = (terres.len() as f64 * r.densite) as u64;
        let m = (n - 1) as f32;
        let haut = |h: &[f32], x: f32, z: f32| -> (f32, f32, f32) {
            let (a, b) = (x as usize, z as usize);
            let (u, v) = (x - a as f32, z - b as f32);
            let i = b * n + a;
            let (h00, h10, h01, h11) = (h[i], h[i + 1], h[i + n], h[i + n + 1]);
            let gx = (h10 - h00) * (1.0 - v) + (h11 - h01) * v;
            let gz = (h01 - h00) * (1.0 - u) + (h11 - h10) * u;
            let y = h00 * (1.0 - u) * (1.0 - v) + h10 * u * (1.0 - v) + h01 * (1.0 - u) * v + h11 * u * v;
            (y, gx, gz)
        };
        let deposer = |h: &mut [f32], x: f32, z: f32, q: f32| {
            let (a, b) = (x as usize, z as usize);
            let (u, v) = (x - a as f32, z - b as f32);
            let i = b * n + a;
            h[i] += q * (1.0 - u) * (1.0 - v);
            h[i + 1] += q * u * (1.0 - v);
            h[i + n] += q * (1.0 - u) * v;
            h[i + n + 1] += q * u * v;
        };
        for k in 0..gouttes {
            let t = terres[((hasard(graine, k, 1) * terres.len() as f32) as usize).min(terres.len() - 1)] as usize;
            let (mut x, mut z) = ((t % n) as f32 + hasard(graine, k, 2), (t / n) as f32 + hasard(graine, k, 3));
            let (mut dx, mut dz) = (0f32, 0f32);
            let (mut vitesse, mut eau_g, mut charge) = (1f32, 1f32, 0f32);
            for _ in 0..r.vie {
                if !(x >= 0.0 && z >= 0.0 && x < m && z < m) {
                    break;
                }
                let (y, gx, gz) = haut(&h, x, z);
                dx = dx * r.inertie - gx * (1.0 - r.inertie);
                dz = dz * r.inertie - gz * (1.0 - r.inertie);
                let l = dx.hypot(dz);
                if l < 1e-6 {
                    break;
                }
                (dx, dz) = (dx / l, dz / l);
                let (nx, nz) = (x + dx, z + dz);
                let ici = (z as usize) * n + x as usize;
                eau[ici] += eau_g;
                if !(nx >= 0.0 && nz >= 0.0 && nx < m && nz < m) {
                    break;
                }
                let ny = haut(&h, nx, nz).0;
                let dy = ny - y;
                // À la mer, tout ce qui est emporté se dépose : un delta
                if ny <= mer {
                    deposer(&mut h, x, z, charge * 0.5);
                    deposer(&mut h, nx, nz, charge * 0.5);
                    break;
                }
                let cap = (-dy).max(r.pente_min) * vitesse * eau_g * r.capacite;
                if charge > cap || dy > 0.0 {
                    // Dans un creux, la goutte le comble (sans dépasser), sinon elle dépose
                    // le trop
                    let q = if dy > 0.0 { dy.min(charge) } else { (charge - cap) * r.depose };
                    charge -= q;
                    deposer(&mut h, x, z, q);
                } else {
                    let q = ((cap - charge) * r.arrache).min(-dy);
                    let (a, b) = (x as i32, z as i32);
                    for &(px, pz, w) in &pinceau {
                        let (ca, cb) = (a + px, b + pz);
                        if ca < 0 || cb < 0 || ca >= n as i32 || cb >= n as i32 {
                            continue;
                        }
                        let i = cb as usize * n + ca as usize;
                        let pris = (q * w).min(r.creuse_max / echelle - creuse[i]).max(0.0);
                        h[i] -= pris;
                        creuse[i] += pris;
                        charge += pris;
                    }
                }
                vitesse = (vitesse * vitesse - dy * r.gravite).max(0.0).sqrt();
                eau_g *= 1.0 - r.evaporation;
                (x, z) = (nx, nz);
            }
        }

        // L'écart, adouci d'un flou 3 × 3 : pas de creux d'un seul point
        let brut: Vec<f32> = (0..n * n).map(|i| (h[i] - depart[i]) * echelle).collect();
        let mut ecart = brut.clone();
        for b in 1..n - 1 {
            for a in 1..n - 1 {
                let i = b * n + a;
                let s = brut[i] * 4.0 + (brut[i - 1] + brut[i + 1] + brut[i - n] + brut[i + n]) * 2.0 + brut[i - n - 1] + brut[i - n + 1] + brut[i + n - 1] + brut[i + n + 1];
                ecart[i] = s / 16.0;
            }
        }
        // L'eau, sur une échelle logarithmique : 1 pour les plus grands passages
        let max = eau.iter().cloned().fold(0f32, f32::max).max(1.0);
        let lmax = (1.0 + max).ln();
        let eau = eau.iter().map(|e| (1.0 + e).ln() / lmax).collect();
        Erosion { graine, cote: n, ecart, eau }
    }

    fn lire(&self, champ: &[f32], x: f64, z: f64) -> f64 {
        let n = self.cote as i64;
        let (gx, gz) = ((x - origine()) / PAS, (z - origine()) / PAS);
        let (a, b) = (gx.floor(), gz.floor());
        let (u, v) = (gx - a, gz - b);
        let (a, b) = (a as i64, b as i64);
        let p = |i: i64, j: i64| champ[(j.clamp(0, n - 1) * n + i.clamp(0, n - 1)) as usize] as f64;
        let l = |j: i64| cr(p(a - 1, j), p(a, j), p(a + 1, j), p(a + 2, j), u);
        cr(l(b - 1), l(b), l(b + 1), l(b + 2), v)
    }

    /// Ce que l'érosion change à la hauteur en (x, z) absolu, en mètres.
    pub fn ecart(&self, x: f64, z: f64) -> f64 {
        self.lire(&self.ecart, x, z)
    }

    /// L'eau passée en (x, z), de 0 à 1.
    pub fn eau(&self, x: f64, z: f64) -> f64 {
        self.lire(&self.eau, x, z).clamp(0.0, 1.0)
    }

    /// En octets, pour le dépôt : l'écart au centimètre, l'eau sur 16 bits.
    pub fn octets(&self) -> Vec<u8> {
        let mut o = Vec::with_capacity(12 + self.ecart.len() * 4);
        for v in [FORMAT, self.graine, self.cote as u32] {
            o.extend_from_slice(&v.to_le_bytes());
        }
        for e in &self.ecart {
            o.extend_from_slice(&((e * 100.0).round().clamp(i32::MIN as f32, i32::MAX as f32) as i32).to_le_bytes()[..3]);
        }
        for e in &self.eau {
            o.extend_from_slice(&((e.clamp(0.0, 1.0) * 65535.0).round() as u16).to_le_bytes());
        }
        o
    }

    /// Relue des octets ; rien si le format, la graine ou la taille ne vont pas.
    pub fn depuis_octets(o: &[u8], graine: u32) -> Option<Erosion> {
        let u = |k: usize| Some(u32::from_le_bytes(o.get(k * 4..k * 4 + 4)?.try_into().ok()?));
        let (f, g, n) = (u(0)?, u(1)?, u(2)? as usize);
        if f != FORMAT || g != graine || n != cote() || o.len() != 12 + n * n * 5 {
            return None;
        }
        let (e, w) = o[12..].split_at(n * n * 3);
        let ecart = e.chunks(3).map(|c| (i32::from_le_bytes([c[0], c[1], c[2], if c[2] & 0x80 != 0 { 0xFF } else { 0 }]) as f32) / 100.0).collect();
        let eau = w.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]]) as f32 / 65535.0).collect();
        Some(Erosion { graine, cote: n, ecart, eau })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_grille_couvre_la_carte() {
        let fin = origine() + (cote() - 1) as f64 * PAS;
        assert_eq!(origine(), -(DEMI_CARTE as f64) * TAILLE as f64);
        assert_eq!(fin, (DEMI_CARTE + 1) as f64 * TAILLE as f64);
    }

    #[test]
    fn une_colline_se_ravine_et_se_relit() {
        // Un cône de 300 m au milieu d'une mer
        let colline = |x: f64, z: f64| 300.0 - x.hypot(z) * 0.1;
        // Peu de gouttes : le calcul reste court sans optimisation
        let r = Reglage { densite: 0.05, ..Reglage::default() };
        let e = Erosion::calculer_avec(&r, 5, 0.0, &colline);
        assert_eq!(e, Erosion::calculer_avec(&r, 5, 0.0, &colline), "toujours le même");
        let creuse = e.ecart.iter().cloned().fold(0f32, f32::min);
        let comble = e.ecart.iter().cloned().fold(0f32, f32::max);
        assert!(creuse < -0.2 && comble > 0.1, "{creuse} {comble}");
        assert!(e.eau.iter().any(|&w| w > 0.9));
        // En mer, rien
        assert_eq!(e.ecart(9000.0, 9000.0), 0.0);
        // Relue
        let r = Erosion::depuis_octets(&e.octets(), 5).unwrap();
        assert!(e.ecart.iter().zip(&r.ecart).all(|(a, b)| (a - b).abs() <= 0.006));
        assert!(Erosion::depuis_octets(&e.octets(), 6).is_none());
    }
}
