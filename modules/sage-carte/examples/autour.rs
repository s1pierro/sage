//! Les triangles non bâtis autour d'un point : cargo run --example autour -- <carte> x z r
use sage_carte::{Carte, Tuile};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = std::path::PathBuf::from(&a[1]);
    let (x, z, r): (f32, f32, f32) = (a[2].parse().unwrap(), a[3].parse().unwrap(), a[4].parse().unwrap());
    let c = Carte::ouvrir(&d).unwrap();
    for e in c.tuiles_autour(x, z, r) {
        let t = Tuile::ouvrir(&d.join(e.fichier())).unwrap();
        let o = t.origine;
        for p in &t.primitives {
            if p.matiere.starts_with("bati") || p.matiere == "eau" || p.matiere == "sol" { continue; }
            for i in p.indices.chunks_exact(3) {
                let q = [0, 1, 2].map(|k| { let v = p.positions[i[k] as usize]; [v[0] + o[0], v[1] + o[1], v[2] + o[2]] });
                if q.iter().all(|v| (v[0] - x).hypot(v[2] - z) > r) { continue; }
                let (u, w) = ([q[1][0]-q[0][0], q[1][1]-q[0][1], q[1][2]-q[0][2]], [q[2][0]-q[0][0], q[2][1]-q[0][1], q[2][2]-q[0][2]]);
                let n = [u[1]*w[2]-u[2]*w[1], u[2]*w[0]-u[0]*w[2], u[0]*w[1]-u[1]*w[0]];
                let l = (n[0]*n[0]+n[1]*n[1]+n[2]*n[2]).sqrt();
                println!("{:8} {:?} n=({:.2},{:.2},{:.2}) tuile {:?}", p.matiere, q.map(|v| [(v[0]*100.0).round()/100.0, (v[1]*100.0).round()/100.0, (v[2]*100.0).round()/100.0]), n[0]/l, n[1]/l, n[2]/l, e.cle);
            }
        }
    }
}
