//! La continuité du bitume bordeaux le long des voies de bus : on suit l'axe de chaque
//! arête de bus tous les mètres et on lit la matière vue d'en haut.
//!   cargo run --release --example bus_continu -- <lieu.reel> <carte>
use sage_carte::{Carte, Tuile};
use std::collections::HashMap;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let l = sage_reel::Lieu::depuis_octets(&std::fs::read(&a[1]).unwrap()).unwrap();
    let r = sage_reel::reseau::reseau(&l);
    let d = std::path::PathBuf::from(&a[2]);
    let carte = Carte::ouvrir(&d).unwrap();
    // Échantillons : (arête, x, z), rangés par tuile
    let mut par_tuile: HashMap<usize, Vec<(usize, f32, f32, f32)>> = HashMap::new();
    // TUNNELS=1 : les tunnels au lieu des voies de bus
    let tunnels = std::env::var("TUNNELS").is_ok();
    // PAVES=1 : les voies pavées (hors piétonnes et sentiers)
    let paves = std::env::var("PAVES").is_ok();
    for (k, x) in r.aretes.iter().enumerate().filter(|(_, x)| if paves { x.revetement == sage_reel::PAVES && x.passage == sage_reel::reseau::Passage::Sol && (x.genre <= sage_reel::SERVICE || x.genre == sage_reel::RUE_PIETONNE) } else if tunnels { x.passage == sage_reel::reseau::Passage::Tunnel && x.genre <= sage_reel::SERVICE } else { x.genre == sage_reel::BUS }) {
        for w in x.points.windows(2) {
            let (dx, dz) = (w[1][0] - w[0][0], w[1][1] - w[0][1]);
            let n = dx.hypot(dz).ceil().max(1.0) as usize;
            for i in 0..n {
                let t = i as f32 / n as f32;
                let (px, pz, ph) = (w[0][0] + t * dx, w[0][1] + t * dz, w[0][2] + t * (w[1][2] - w[0][2]));
                if let Some(e) = carte.tuiles.iter().position(|e| px >= e.boite[0] && px < e.boite[2] && pz >= e.boite[1] && pz < e.boite[3]) {
                    par_tuile.entry(e).or_default().push((k, px, pz, ph));
                }
            }
        }
    }
    let mut res: Vec<(usize, f32, f32, String)> = Vec::new();
    for (e, ech) in &par_tuile {
        let t = Tuile::ouvrir(&d.join(carte.tuiles[*e].fichier())).unwrap();
        let o = t.origine;
        let mut haut = vec![(f32::MIN, String::from("rien")); ech.len()];
        let hs: Vec<f32> = ech.iter().map(|e| e.3).collect();
        for p in &t.primitives {
            for i in p.indices.chunks_exact(3) {
                let q = [0, 1, 2].map(|k| { let v = p.positions[i[k] as usize]; [v[0] + o[0], v[1] + o[1], v[2] + o[2]] });
                let dd = (q[1][2] - q[2][2]) * (q[0][0] - q[2][0]) + (q[2][0] - q[1][0]) * (q[0][2] - q[2][2]);
                if dd.abs() < 1e-9 { continue; }
                let (x0, x1) = (q[0][0].min(q[1][0]).min(q[2][0]), q[0][0].max(q[1][0]).max(q[2][0]));
                let (z0, z1) = (q[0][2].min(q[1][2]).min(q[2][2]), q[0][2].max(q[1][2]).max(q[2][2]));
                for (s, &(_, px, pz, _)) in ech.iter().enumerate() {
                    if px < x0 || px > x1 || pz < z0 || pz > z1 { continue; }
                    let l1 = ((q[1][2] - q[2][2]) * (px - q[2][0]) + (q[2][0] - q[1][0]) * (pz - q[2][2])) / dd;
                    let l2 = ((q[2][2] - q[0][2]) * (px - q[2][0]) + (q[0][0] - q[2][0]) * (pz - q[2][2])) / dd;
                    let l3 = 1.0 - l1 - l2;
                    if l1 < 0.0 || l2 < 0.0 || l3 < 0.0 { continue; }
                    let y = l1 * q[0][1] + l2 * q[1][1] + l3 * q[2][1];
                    if y > haut[s].0 && (!tunnels || y < hs[s] + 1.0) { haut[s] = (y, p.matiere.clone()); }
                }
            }
        }
        for (s, &(k, px, pz, _)) in ech.iter().enumerate() { res.push((k, px, pz, haut[s].1.clone())); }
    }
    let mut compte: HashMap<String, usize> = HashMap::new();
    for x in &res { *compte.entry(x.3.clone()).or_default() += 1; }
    println!("{} m d'axe de bus dans la carte : {:?}", res.len(), compte);
    // Trous : cases non bordeaux regroupées par 10 m
    let mut trous: HashMap<(i32, i32), (usize, String)> = HashMap::new();
    for x in res.iter().filter(|x| x.3 != if paves { "chaussee_paves" } else if tunnels { "chaussee_tunnel" } else { "chaussee_bus" }) {
        let c = trous.entry(((x.1 / 10.0).floor() as i32, (x.2 / 10.0).floor() as i32)).or_insert((0, x.3.clone()));
        c.0 += 1;
    }
    let mut v: Vec<_> = trous.into_iter().collect();
    v.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    for ((i, j), (n, m)) in v.iter().take(40) { println!("  ({:5} ; {:5}) {n} m {m}", i * 10 + 5, j * 10 + 5); }
}
