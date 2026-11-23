//! Les transports d'un lieu réel : les tunnels des routes, les voies ferrées (trains,
//! métros, trams), les arrêts et les lignes (bus, trams, métros, trains).
//!
//! - Un tunnel est une route (`Route`) dont la chaussée passe sous le relief : sa
//!   hauteur rejoint le sol à ses entrées et descend dessous, jusqu'à 6 m.
//! - Une voie ferrée (`Rail`) : un mode, au sol, sur un pont ou en tunnel, son niveau
//!   (`layer` d'OSM : −1, −2 sous terre) et sa ligne (x, z du jeu, et la hauteur des
//!   rails). Un tunnel descend d'autant plus que son niveau est bas (`profondeur`).
//! - Un arrêt (`Arret`) : sa place (x, z du jeu), les modes qui le desservent, son nom,
//!   s'il a un abri, son niveau (0 en surface, −1, −2 sous terre).
//! - Une ligne (`Ligne`) : un mode, son numéro, son nom, sa couleur, ses arrêts dans
//!   l'ordre (des indices dans `arrets`) et son tracé, en morceaux (`Morceau`) au sol,
//!   sur un pont ou en tunnel.
//!
//! Sources : © les contributeurs OpenStreetMap (ODbL), comme le reste du lieu.

/// Les modes : chacun sa couleur.
pub const BUS: u8 = 0;
pub const TRAM: u8 = 1;
/// Le métro, souterrain ou léger.
pub const METRO: u8 = 2;
pub const TRAIN: u8 = 3;
/// Funiculaires, monorails…
pub const AUTRE: u8 = 4;
pub const MODES: u8 = 5;
pub const NOMS_MODES: [&str; MODES as usize] = ["Bus", "Tram", "Métro", "Train", "Autre"];

/// Où passe une voie ferrée.
pub const SOL: u8 = 0;
pub const PONT: u8 = 1;
pub const TUNNEL: u8 = 2;

/// La couleur d'un mode (pour une ligne sans couleur dite).
pub fn teinte(mode: u8) -> [f32; 3] {
    match mode {
        BUS => [0.80, 0.33, 0.20],
        TRAM => [0.20, 0.55, 0.75],
        METRO => [0.55, 0.30, 0.70],
        TRAIN => [0.30, 0.30, 0.34],
        _ => [0.45, 0.45, 0.30],
    }
}

/// La profondeur visée d'un tunnel de niveau `niveau` (`layer` d'OSM, 0 : non dit) :
/// 8 m au niveau −1, 6 m de plus par niveau, au plus 26 m.
pub fn profondeur(niveau: i8) -> f32 {
    (2.0 + 6.0 * (-(niveau as f32)).max(1.0)).min(26.0)
}

/// L'écartement qu'on dessine d'une voie ferrée de ce mode (m).
pub fn largeur(mode: u8) -> f32 {
    match mode {
        TRAIN => 3.0,
        METRO => 2.8,
        TRAM => 2.6,
        _ => 2.0,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Rail {
    pub mode: u8,
    /// `SOL`, `PONT` ou `TUNNEL`.
    pub ou: u8,
    /// Le niveau (`layer` d'OSM) : négatif sous terre.
    pub niveau: i8,
    /// x, z du jeu, et la hauteur des rails.
    pub points: Vec<[f32; 3]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Arret {
    /// Un bit par mode (`1 << BUS`…).
    pub modes: u8,
    pub x: f32,
    pub z: f32,
    pub nom: String,
    pub abri: bool,
    /// 0 en surface ; négatif sous terre (−1, −2…).
    pub niveau: i8,
}

impl Arret {
    /// Le premier mode qui le dessert, du plus lourd au plus léger.
    pub fn mode(&self) -> u8 {
        [TRAIN, METRO, TRAM, BUS, AUTRE].into_iter().find(|m| self.modes & (1 << m) != 0).unwrap_or(BUS)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ligne {
    pub mode: u8,
    /// Le numéro (`ref` d'OSM), peut-être vide.
    pub numero: String,
    pub nom: String,
    pub couleur: Option<[u8; 3]>,
    /// Les arrêts dans l'ordre : des indices dans `Transports::arrets`.
    pub arrets: Vec<u32>,
    /// Le tracé, en morceaux.
    pub trace: Vec<Morceau>,
}

/// Un morceau du tracé d'une ligne : au sol, sur un pont ou en tunnel (`SOL`, `PONT`,
/// `TUNNEL`), une ligne x, z du jeu.
#[derive(Clone, Debug, PartialEq)]
pub struct Morceau {
    pub ou: u8,
    pub points: Vec<[f32; 2]>,
}

impl Ligne {
    /// Sa couleur, ou celle de son mode.
    pub fn teinte(&self) -> [f32; 3] {
        self.couleur.map_or(teinte(self.mode), |c| c.map(|v| v as f32 / 255.0))
    }

    /// « Bus 6 », « Métro », …
    pub fn titre(&self) -> String {
        let m = NOMS_MODES[self.mode.min(MODES - 1) as usize];
        if self.numero.is_empty() { m.to_string() } else { format!("{m} {}", self.numero) }
    }
}

/// Tout ce qui sert à se déplacer, hors des routes et des ponts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transports {
    pub tunnels: Vec<crate::Route>,
    pub rails: Vec<Rail>,
    pub arrets: Vec<Arret>,
    pub lignes: Vec<Ligne>,
}

impl Transports {
    pub fn est_vide(&self) -> bool {
        self.tunnels.is_empty() && self.rails.is_empty() && self.arrets.is_empty() && self.lignes.is_empty()
    }

    /// Réduit à ce qui est dans `dans` : lignes coupées à son bord, arrêts dehors ôtés
    /// (et retirés des lignes).
    pub fn couper(&self, dans: &dyn Fn(f32, f32) -> bool) -> Transports {
        fn morceaux<P: Copy>(pts: &[P], dans: impl Fn(&P) -> bool) -> Vec<Vec<P>> {
            let mut v = Vec::new();
            let mut m = Vec::new();
            for p in pts {
                if dans(p) {
                    m.push(*p);
                } else if m.len() > 1 {
                    v.push(std::mem::take(&mut m));
                } else {
                    m.clear();
                }
            }
            if m.len() > 1 {
                v.push(m);
            }
            v
        }
        let tunnels = self.tunnels.iter().flat_map(|r| morceaux(&r.points, |p| dans(p[0], p[1])).into_iter().map(|points| crate::Route { niveau: r.niveau, revetement: r.revetement, files: r.files, genre: r.genre, largeur: r.largeur, points })).collect();
        let rails = self.rails.iter().flat_map(|r| morceaux(&r.points, |p| dans(p[0], p[1])).into_iter().map(|points| Rail { mode: r.mode, ou: r.ou, niveau: r.niveau, points })).collect();
        let mut nouveau = vec![u32::MAX; self.arrets.len()];
        let mut arrets = Vec::new();
        for (k, a) in self.arrets.iter().enumerate() {
            if dans(a.x, a.z) {
                nouveau[k] = arrets.len() as u32;
                arrets.push(a.clone());
            }
        }
        let lignes = self
            .lignes
            .iter()
            .map(|l| Ligne {
                arrets: l.arrets.iter().filter_map(|&k| nouveau.get(k as usize).copied().filter(|&n| n != u32::MAX)).collect(),
                trace: l.trace.iter().flat_map(|t| morceaux(&t.points, |p| dans(p[0], p[1])).into_iter().map(|points| Morceau { ou: t.ou, points })).collect(),
                ..l.clone()
            })
            .filter(|l| !l.arrets.is_empty() || !l.trace.is_empty())
            .collect();
        Transports { tunnels, rails, arrets, lignes }
    }

    pub(crate) fn ecrire(&self, o: &mut Vec<u8>) {
        let u32_ = |o: &mut Vec<u8>, v: usize| o.extend_from_slice(&(v as u32).to_le_bytes());
        let texte = |o: &mut Vec<u8>, s: &str| {
            o.extend_from_slice(&(s.len() as u32).to_le_bytes());
            o.extend_from_slice(s.as_bytes());
        };
        crate::ecrire_routes(o, &self.tunnels);
        u32_(o, self.rails.len());
        for r in &self.rails {
            o.extend_from_slice(&[r.mode, r.ou, r.niveau as u8]);
            u32_(o, r.points.len());
            for v in r.points.iter().flatten() {
                o.extend_from_slice(&v.to_le_bytes());
            }
        }
        u32_(o, self.arrets.len());
        for a in &self.arrets {
            o.extend_from_slice(&[a.modes, a.abri as u8, a.niveau as u8]);
            o.extend_from_slice(&a.x.to_le_bytes());
            o.extend_from_slice(&a.z.to_le_bytes());
            texte(o, &a.nom);
        }
        u32_(o, self.lignes.len());
        for l in &self.lignes {
            o.push(l.mode);
            match l.couleur {
                Some(c) => {
                    o.push(1);
                    o.extend_from_slice(&c);
                }
                None => o.extend_from_slice(&[0; 4]),
            }
            texte(o, &l.numero);
            texte(o, &l.nom);
            u32_(o, l.arrets.len());
            for k in &l.arrets {
                o.extend_from_slice(&k.to_le_bytes());
            }
            u32_(o, l.trace.len());
            for t in &l.trace {
                o.push(t.ou);
                u32_(o, t.points.len());
                for v in t.points.iter().flatten() {
                    o.extend_from_slice(&v.to_le_bytes());
                }
            }
        }
    }

    /// Relu depuis `prendre` (qui rend `n` octets de plus) ; rien si c'est tronqué ou
    /// incohérent.
    pub(crate) fn lire(prendre: &mut dyn FnMut(usize) -> Option<Vec<u8>>) -> Option<Transports> {
        let u32_ = |p: &mut dyn FnMut(usize) -> Option<Vec<u8>>| -> Option<usize> { Some(u32::from_le_bytes(p(4)?.try_into().ok()?) as usize) };
        let f32_ = |c: &[u8]| f32::from_le_bytes(c.try_into().unwrap());
        let texte = |p: &mut dyn FnMut(usize) -> Option<Vec<u8>>| -> Option<String> {
            let n = u32::from_le_bytes(p(4)?.try_into().ok()?) as usize;
            String::from_utf8(p(n)?).ok()
        };
        let mut tunnels = Vec::new();
        for _ in 0..u32_(prendre)? {
            let genre = prendre(1)?[0];
            let largeur = f32_(&prendre(4)?);
            let m = u32_(prendre)?;
            let points = prendre(m * 12)?.chunks(12).map(|c| [f32_(&c[0..4]), f32_(&c[4..8]), f32_(&c[8..12])]).collect();
            tunnels.push(crate::Route { niveau: 0, revetement: 0, files: crate::Files::deduire(genre, largeur, 0), genre, largeur, points });
        }
        let mut rails = Vec::new();
        for _ in 0..u32_(prendre)? {
            let t = prendre(3)?;
            let m = u32_(prendre)?;
            let points = prendre(m * 12)?.chunks(12).map(|c| [f32_(&c[0..4]), f32_(&c[4..8]), f32_(&c[8..12])]).collect();
            rails.push(Rail { mode: t[0], ou: t[1], niveau: t[2] as i8, points });
        }
        let mut arrets = Vec::new();
        for _ in 0..u32_(prendre)? {
            let t = prendre(11)?;
            let nom = texte(prendre)?;
            arrets.push(Arret { modes: t[0], abri: t[1] != 0, niveau: t[2] as i8, x: f32_(&t[3..7]), z: f32_(&t[7..11]), nom });
        }
        let mut lignes = Vec::new();
        for _ in 0..u32_(prendre)? {
            let t = prendre(5)?;
            let couleur = (t[1] != 0).then(|| [t[2], t[3], t[4]]);
            let numero = texte(prendre)?;
            let nom = texte(prendre)?;
            let m = u32_(prendre)?;
            let la: Vec<u32> = prendre(m * 4)?.chunks(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
            if la.iter().any(|&k| k as usize >= arrets.len()) {
                return None;
            }
            let mut trace = Vec::new();
            for _ in 0..u32_(prendre)? {
                let ou = prendre(1)?[0];
                let m = u32_(prendre)?;
                trace.push(Morceau { ou, points: prendre(m * 8)?.chunks(8).map(|c| [f32_(&c[0..4]), f32_(&c[4..8])]).collect() });
            }
            lignes.push(Ligne { mode: t[0], numero, nom, couleur, arrets: la, trace });
        }
        Some(Transports { tunnels, rails, arrets, lignes })
    }
}

/// Une couleur d'OSM (`colour`) : `#rrggbb`, `#rgb` ou quelques noms.
pub fn couleur(s: &str) -> Option<[u8; 3]> {
    let s = s.trim();
    if let Some(h) = s.strip_prefix('#') {
        let v = |a: &str| u8::from_str_radix(a, 16).ok();
        return match h.len() {
            6 => Some([v(&h[0..2])?, v(&h[2..4])?, v(&h[4..6])?]),
            3 => Some([v(&h[0..1])? * 17, v(&h[1..2])? * 17, v(&h[2..3])? * 17]),
            _ => None,
        };
    }
    Some(match s.to_lowercase().as_str() {
        "red" => [220, 30, 30],
        "green" => [30, 150, 50],
        "blue" => [30, 80, 200],
        "yellow" => [240, 200, 20],
        "orange" => [240, 130, 20],
        "purple" => [130, 40, 150],
        "pink" => [230, 100, 160],
        "brown" => [130, 80, 40],
        "black" => [20, 20, 20],
        "grey" | "gray" => [128, 128, 128],
        "white" => [240, 240, 240],
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_couleurs_d_osm_se_lisent() {
        assert_eq!(couleur("#FF8000"), Some([255, 128, 0]));
        assert_eq!(couleur("#f00"), Some([255, 0, 0]));
        assert_eq!(couleur("Blue"), Some([30, 80, 200]));
        assert_eq!(couleur("bof"), None);
    }

    #[test]
    fn couper_renumerote_les_arrets() {
        let t = Transports {
            tunnels: vec![],
            rails: vec![Rail { mode: METRO, ou: TUNNEL, niveau: -1, points: vec![[0.0, 0.0, 1.0], [10.0, 0.0, 1.0], [30.0, 0.0, 1.0]] }],
            arrets: vec![
                Arret { modes: 1 << BUS, x: 50.0, z: 0.0, nom: "Loin".into(), abri: false, niveau: 0 },
                Arret { modes: 1 << BUS, x: 5.0, z: 0.0, nom: "Près".into(), abri: true, niveau: 0 },
            ],
            lignes: vec![Ligne { mode: BUS, numero: "6".into(), nom: "".into(), couleur: None, arrets: vec![0, 1], trace: vec![Morceau { ou: TUNNEL, points: vec![[0.0, 0.0], [5.0, 0.0], [50.0, 0.0]] }] }],
        };
        let c = t.couper(&|x, _| x <= 20.0);
        assert_eq!(c.rails[0].points.len(), 2);
        assert_eq!(c.arrets.len(), 1);
        assert_eq!(c.lignes[0].arrets, vec![0]);
        assert_eq!(c.lignes[0].trace[0], Morceau { ou: TUNNEL, points: vec![[0.0, 0.0], [5.0, 0.0]] });
    }
}
