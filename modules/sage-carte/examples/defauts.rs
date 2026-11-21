//! Cherche dans une carte les triangles qui piègent la physique de WhiteRoom
//! (sage-physique `Decor::dedans`) : les « murs » (triangles raides de plus de 30 cm
//! de haut) dont le dessus affleure une surface roulable qui est derrière eux, et les
//! surfaces superposées à moins de 50 cm.
//!
//! cargo run --release --example defauts -- <dossier de la carte> [x z rayon]

use sage_carte::{Carte, Tuile};
use std::collections::HashMap;

type V = [f32; 3];
fn sub(a: V, b: V) -> V { [a[0] - b[0], a[1] - b[1], a[2] - b[2]] }
fn cross(a: V, b: V) -> V { [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]] }
fn unit(a: V) -> V { let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1e-12); [a[0] / l, a[1] / l, a[2] / l] }

fn hauteur(t: &[V; 3], x: f32, z: f32) -> Option<f32> {
    let [a, b, c] = *t;
    let d = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
    if d.abs() < 1e-9 { return None; }
    let l1 = ((b[2] - c[2]) * (x - c[0]) + (c[0] - b[0]) * (z - c[2])) / d;
    let l2 = ((c[2] - a[2]) * (x - c[0]) + (a[0] - c[0]) * (z - c[2])) / d;
    let l3 = 1.0 - l1 - l2;
    (l1 >= -1e-5 && l2 >= -1e-5 && l3 >= -1e-5).then(|| l1 * a[1] + l2 * b[1] + l3 * c[1])
}

struct Tri { p: [V; 3], m: usize }

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let dossier = std::path::PathBuf::from(&a[1]);
    let zone: Option<[f32; 3]> = (a.len() >= 5).then(|| [a[2].parse().unwrap(), a[3].parse().unwrap(), a[4].parse().unwrap()]);
    let carte = Carte::ouvrir(&dossier).expect("carte");
    let mut noms: Vec<String> = Vec::new();
    let mut tris: Vec<Tri> = Vec::new();
    for e in &carte.tuiles {
        if let Some([x, z, r]) = zone {
            let dx = (e.boite[0] - x).max(0.0).max(x - e.boite[2]);
            let dz = (e.boite[1] - z).max(0.0).max(z - e.boite[3]);
            if dx.hypot(dz) > r { continue; }
        }
        let Ok(t) = Tuile::ouvrir(&dossier.join(e.fichier())) else { continue };
        let o = t.origine;
        for p in &t.primitives {
            if p.matiere == sage_carte::EAU { continue; }
            let m = match noms.iter().position(|n| *n == p.matiere) { Some(i) => i, None => { noms.push(p.matiere.clone()); noms.len() - 1 } };
            for i in p.indices.chunks_exact(3) {
                let q = [0, 1, 2].map(|k| { let v = p.positions[i[k] as usize]; [v[0] + o[0], v[1] + o[1], v[2] + o[2]] });
                tris.push(Tri { p: q, m });
            }
        }
    }
    eprintln!("{} triangles, matières {:?}", tris.len(), noms);
    // Grille de 2 m
    let case = |x: f32| (x / 2.0).floor() as i32;
    let mut g: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
    for (i, t) in tris.iter().enumerate() {
        let (x0, x1) = (t.p.iter().map(|p| p[0]).fold(f32::MAX, f32::min), t.p.iter().map(|p| p[0]).fold(f32::MIN, f32::max));
        let (z0, z1) = (t.p.iter().map(|p| p[2]).fold(f32::MAX, f32::min), t.p.iter().map(|p| p[2]).fold(f32::MIN, f32::max));
        for cz in case(z0)..=case(z1) { for cx in case(x0)..=case(x1) { g.entry((cx, cz)).or_default().push(i as u32); } }
    }
    let bati = |m: usize| noms[m].starts_with("bati") || noms[m] == "beton_bati";
    let cos45 = 45f32.to_radians().cos();
    // Les surfaces roulables sous (x, z) : (hauteur, matière)
    let sols = |x: f32, z: f32| -> Vec<(f32, usize)> {
        let mut v = Vec::new();
        if let Some(l) = g.get(&(case(x), case(z))) {
            for &i in l {
                let t = &tris[i as usize];
                let n = unit(cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0])));
                if n[1].abs() < cos45 { continue; }
                if let Some(h) = hauteur(&t.p, x, z) { v.push((h, t.m)); }
            }
        }
        v
    };
    // 1. Les murs bas : raides, > 30 cm de haut, pas un bâtiment ; une surface roulable
    //    0,3 m derrière (côté opposé à la normale du sens direct) dont la hauteur est
    //    sous le dessus du mur, à moins de 50 cm
    let mut murs: HashMap<(usize, usize), (u32, f32, V)> = HashMap::new();
    let mut exemples: Vec<(f32, V, usize, usize, f32)> = Vec::new();
    for t in &tris {
        if bati(t.m) { continue; }
        let n = unit(cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0])));
        if n[1].abs() >= cos45 { continue; }
        let ys = t.p.map(|p| p[1]);
        let (y0, y1) = (ys.iter().cloned().fold(f32::MAX, f32::min), ys.iter().cloned().fold(f32::MIN, f32::max));
        if y1 - y0 < 0.3 { continue; }
        let l = n[0].hypot(n[2]);
        if l < 0.3 { continue; }
        // Le milieu du bord haut : les deux sommets les plus hauts
        let mut k = [0usize, 1, 2];
        k.sort_by(|&a, &b| ys[b].total_cmp(&ys[a]));
        let (h1, h2) = (t.p[k[0]], t.p[k[1]]);
        let mil = [(h1[0] + h2[0]) / 2.0, (h1[1] + h2[1]) / 2.0, (h1[2] + h2[2]) / 2.0];
        for d in [0.15f32, 0.3, 0.6] {
            let (x, z) = (mil[0] - n[0] / l * d, mil[2] - n[2] / l * d);
            for (h, m) in sols(x, z) {
                if bati(m) { continue; }
                let ecart = mil[1] - h;
                // La surface derrière est sous le haut du mur (de moins de 50 cm) : un
                // point de la voiture posé dessus est « derrière le mur »
                if ecart > 0.005 && ecart < 0.5 {
                    let e = murs.entry((t.m, m)).or_insert((0, 0.0, mil));
                    e.0 += 1;
                    e.1 = e.1.max(ecart);
                    exemples.push((ecart, mil, t.m, m, y1 - y0));
                    break;
                }
            }
        }
    }
    println!("== Murs bas (mur, sol derrière) : nombre, dépassement max, exemple");
    let mut v: Vec<_> = murs.iter().collect();
    v.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    for ((mm, ms), (c, e, p)) in v { println!("  {:>10} sur {:>10} : {:6} ; max {:.2} m ; ex. ({:.1}, {:.1}, {:.1})", noms[*mm], noms[*ms], c, e, p[0], p[1], p[2]); }
    exemples.sort_by(|a, b| b.0.total_cmp(&a.0));
    // Les cas les plus francs de chaussée, en dédoublonnant par case de 5 m
    let mut vus = std::collections::HashSet::new();
    println!("== Exemples (dépassement, position, mur / sol, hauteur du mur)");
    let mut n = 0;
    for (e, p, mm, ms, hm) in exemples.iter().filter(|x| noms[x.3] == "chaussee" || noms[x.3] == "beton") {
        if !vus.insert(((p[0] / 5.0) as i32, (p[2] / 5.0) as i32)) { continue; }
        if n < 25 && *e > 0.02 { println!("  {:.2} m en ({:.1}, {:.1}, {:.1}) : {} / {} ; mur de {:.1} m", e, p[0], p[1], p[2], noms[*mm], noms[*ms], hm); n += 1; }
    }
    println!("  ({} lieux distincts)", vus.len());
    // 1 bis. Les lames : deux faces raides (non bâties) en sens opposés, dans le même
    //    plan à 2 cm près, qui se recouvrent : un mur sans épaisseur
    let mut lames: Vec<(V, usize, f32)> = Vec::new();
    for (i, t) in tris.iter().enumerate() {
        if bati(t.m) { continue; }
        let n = unit(cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0])));
        if n[1].abs() >= cos45 || !n[0].is_finite() { continue; }
        let c = [(t.p[0][0] + t.p[1][0] + t.p[2][0]) / 3.0, (t.p[0][1] + t.p[1][1] + t.p[2][1]) / 3.0, (t.p[0][2] + t.p[1][2] + t.p[2][2]) / 3.0];
        let Some(l) = g.get(&(case(c[0]), case(c[2]))) else { continue };
        for &j in l {
            let j = j as usize;
            if j <= i { continue; }
            let u = &tris[j];
            if bati(u.m) { continue; }
            let m = unit(cross(sub(u.p[1], u.p[0]), sub(u.p[2], u.p[0])));
            if n[0] * m[0] + n[1] * m[1] + n[2] * m[2] > -0.95 { continue; }
            let d = sub(c, u.p[0]);
            if (d[0] * m[0] + d[1] * m[1] + d[2] * m[2]).abs() > 0.02 { continue; }
            let cu = [(u.p[0][0] + u.p[1][0] + u.p[2][0]) / 3.0, (u.p[0][1] + u.p[1][1] + u.p[2][1]) / 3.0, (u.p[0][2] + u.p[1][2] + u.p[2][2]) / 3.0];
            if (c[0] - cu[0]).hypot(c[2] - cu[2]) > 1.0 { continue; }
            let ys = t.p.map(|p| p[1]);
            lames.push((c, t.m, ys.iter().cloned().fold(f32::MIN, f32::max) - ys.iter().cloned().fold(f32::MAX, f32::min)));
            break;
        }
    }
    let mut lieux_l = std::collections::HashSet::new();
    let mut par_m: HashMap<usize, u32> = HashMap::new();
    for (c, m, _) in &lames { lieux_l.insert(((c[0] / 5.0).floor() as i32, (c[2] / 5.0).floor() as i32)); *par_m.entry(*m).or_default() += 1; }
    println!("== Lames (faces opposées confondues) : {} triangles, {} lieux distincts", lames.len(), lieux_l.len());
    {
        let hautes: Vec<_> = lames.iter().filter(|l| l.2 >= 0.3).collect();
        let mut lh = std::collections::HashSet::new();
        for (c, _, _) in &hautes { lh.insert(((c[0] / 5.0).floor() as i32, (c[2] / 5.0).floor() as i32)); }
        println!("   dont d'au moins 30 cm (des murs pour WhiteRoom) : {} triangles, {} lieux", hautes.len(), lh.len());
        let mut v2 = std::collections::HashSet::new();
        for (c, m, h) in hautes { if v2.insert(((c[0] / 5.0).floor() as i32, (c[2] / 5.0).floor() as i32)) && v2.len() <= 12 { println!("     ({:.1}, {:.1}, {:.1}) {} {:.2} m", c[0], c[1], c[2], noms[*m], h); } }
    }
    for (m, k) in &par_m { println!("  {} : {}", noms[*m], k); }
    let mut vus2 = std::collections::HashSet::new();
    let mut k = 0;
    for (c, m, h) in &lames {
        if k >= 15 || !vus2.insert(((c[0] / 5.0).floor() as i32, (c[2] / 5.0).floor() as i32)) { continue; }
        println!("  ({:.1}, {:.1}, {:.1}) {} haut {:.2} m", c[0], c[1], c[2], noms[*m], h);
        k += 1;
    }
    // 2. Surfaces roulables superposées à moins de 50 cm (le dessus pris pour un
    //    plafond, scintillement) : échantillon aux centres des triangles de chaussée
    let mut sup: HashMap<(usize, usize), u32> = HashMap::new();
    let mut ex2 = Vec::new();
    for t in &tris {
        if noms[t.m] != "chaussee" { continue; }
        let c = [(t.p[0][0] + t.p[1][0] + t.p[2][0]) / 3.0, (t.p[0][1] + t.p[1][1] + t.p[2][1]) / 3.0, (t.p[0][2] + t.p[1][2] + t.p[2][2]) / 3.0];
        for (h, m) in sols(c[0], c[2]) {
            let d = h - c[1];
            if d > 0.01 && d < 0.5 && !bati(m) {
                *sup.entry((t.m, m)).or_default() += 1;
                ex2.push((d, c, m));
            }
        }
    }
    println!("== Surfaces au-dessus de la chaussée à moins de 50 cm");
    for ((a, b), c) in &sup { println!("  {} sous {} : {}", noms[*a], noms[*b], c); }
    ex2.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (d, c, m) in ex2.iter().take(10) { println!("  +{:.2} m en ({:.1}, {:.1}, {:.1}) : {}", d, c[0], c[1], c[2], noms[*m]); }
}
