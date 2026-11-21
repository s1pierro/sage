//! Le plan des matières vues d'en haut autour d'un point, un caractère par case :
//! cargo run --example plan_matieres -- <carte> x z r [pas]
//! B chaussee_bus, # chaussee, o beton, r remblai, . sol, H bâti, ~ eau, ? autre
use sage_carte::{Carte, Tuile};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = std::path::PathBuf::from(&a[1]);
    let (x, z, r): (f32, f32, f32) = (a[2].parse().unwrap(), a[3].parse().unwrap(), a[4].parse().unwrap());
    let pas: f32 = a.get(5).map_or(1.0, |s| s.parse().unwrap());
    let n = (2.0 * r / pas) as usize;
    let mut h = vec![f32::MIN; n * n];
    let mut c = vec![' '; n * n];
    let carte = Carte::ouvrir(&d).unwrap();
    for e in carte.tuiles_autour(x, z, r) {
        let t = Tuile::ouvrir(&d.join(e.fichier())).unwrap();
        let o = t.origine;
        for p in &t.primitives {
            let ch = match p.matiere.as_str() { "chaussee_bus" => 'B', "chaussee" => '#', "beton" => 'o', "remblai" => 'r', "sol" => '.', "eau" => '~', m if m.starts_with("bati") => 'H', _ => '?' };
            for i in p.indices.chunks_exact(3) {
                let q = [0, 1, 2].map(|k| { let v = p.positions[i[k] as usize]; [v[0] + o[0], v[1] + o[1], v[2] + o[2]] });
                let (x0, x1) = (q.iter().map(|v| v[0]).fold(f32::MAX, f32::min), q.iter().map(|v| v[0]).fold(f32::MIN, f32::max));
                let (z0, z1) = (q.iter().map(|v| v[2]).fold(f32::MAX, f32::min), q.iter().map(|v| v[2]).fold(f32::MIN, f32::max));
                let i0 = (((x0 - (x - r)) / pas).floor().max(0.0)) as usize;
                let i1 = (((x1 - (x - r)) / pas).ceil().max(0.0) as usize).min(n);
                let j0 = (((z0 - (z - r)) / pas).floor().max(0.0)) as usize;
                let j1 = (((z1 - (z - r)) / pas).ceil().max(0.0) as usize).min(n);
                for j in j0..j1 { for ii in i0..i1 {
                    let (px, pz) = (x - r + (ii as f32 + 0.5) * pas, z - r + (j as f32 + 0.5) * pas);
                    let dd = (q[1][2] - q[2][2]) * (q[0][0] - q[2][0]) + (q[2][0] - q[1][0]) * (q[0][2] - q[2][2]);
                    if dd.abs() < 1e-9 { continue; }
                    let l1 = ((q[1][2] - q[2][2]) * (px - q[2][0]) + (q[2][0] - q[1][0]) * (pz - q[2][2])) / dd;
                    let l2 = ((q[2][2] - q[0][2]) * (px - q[2][0]) + (q[0][0] - q[2][0]) * (pz - q[2][2])) / dd;
                    let l3 = 1.0 - l1 - l2;
                    if l1 < 0.0 || l2 < 0.0 || l3 < 0.0 { continue; }
                    let y = l1 * q[0][1] + l2 * q[1][1] + l3 * q[2][1];
                    let k = j * n + ii;
                    if y > h[k] { h[k] = y; c[k] = ch; }
                }}
            }
        }
    }
    for j in 0..n { println!("{}", c[j * n..(j + 1) * n].iter().collect::<String>()); }
    if std::env::var("HAUTEURS").is_ok() { for j in 0..n { println!("{}", h[j * n..(j + 1) * n].iter().map(|y| if *y == f32::MIN { "  . ".into() } else { format!("{:4.0}", y * 10.0 % 1000.0) }).collect::<String>()); } }
}
