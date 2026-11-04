//! Le temps de la simulation : des pas fixes, quelle que soit la cadence d'affichage.

/// Durée d'un pas de simulation, en secondes.
pub const PAS_FIXE: f32 = 1.0 / 60.0;
/// Au-delà, le retard est abandonné plutôt que rattrapé (appareil figé, mise en veille).
const PAS_MAX: u32 = 8;

#[derive(Default)]
pub struct Horloge {
    reste: f32,
    /// Pas de simulation depuis le début.
    pub pas: u64,
}

impl Horloge {
    /// Le temps écoulé `dt` : combien de pas faire, et où en est l'image entre le dernier
    /// pas et le suivant (de 0 à 1), pour interpoler.
    pub fn avancer(&mut self, dt: f32) -> (u32, f32) {
        self.reste += dt.max(0.0);
        let mut n = (self.reste / PAS_FIXE) as u32;
        self.reste -= n as f32 * PAS_FIXE;
        if n > PAS_MAX {
            n = PAS_MAX;
            self.reste = 0.0;
        }
        self.pas += n as u64;
        (n, (self.reste / PAS_FIXE).clamp(0.0, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_simulation_ne_depend_pas_de_la_cadence() {
        let (mut a, mut b) = (Horloge::default(), Horloge::default());
        for _ in 0..30 {
            a.avancer(1.0 / 30.0);
        }
        for _ in 0..120 {
            b.avancer(1.0 / 120.0);
        }
        assert!(a.pas.abs_diff(60) <= 1 && b.pas.abs_diff(60) <= 1, "{} {}", a.pas, b.pas);
    }

    #[test]
    fn un_long_arret_n_est_pas_rattrape() {
        let mut h = Horloge::default();
        let (n, u) = h.avancer(5.0);
        assert_eq!(n, PAS_MAX);
        assert_eq!(u, 0.0);
        let (n, u) = h.avancer(PAS_FIXE * 0.5);
        assert_eq!(n, 0);
        assert!((u - 0.5).abs() < 1e-3);
    }
}
