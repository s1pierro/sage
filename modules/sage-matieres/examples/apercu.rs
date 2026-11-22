//! L'aperçu d'une matière : son réglage (.txt) rendu en PNG, répété `n` × `n` fois.
//!   cargo run --release --example apercu -- <matiere.txt> <sortie.png> [n]
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let r = sage_matieres::Reglages::lire(&std::fs::read_to_string(&a[1]).unwrap()).expect("réglage illisible");
    let n: usize = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(2);
    let i = sage_matieres::generer(&r);
    let (l, h) = (i.l as usize, i.h as usize);
    let mut o = vec![0u8; l * n * h * n * 4];
    for y in 0..h * n {
        for x in 0..l * n {
            let s = ((y % h) * l + x % l) * 4;
            let d = (y * l * n + x) * 4;
            o[d..d + 4].copy_from_slice(&i.rgba[s..s + 4]);
        }
    }
    std::fs::write(&a[2], sage_matieres::png::encoder((l * n) as u32, (h * n) as u32, &o)).unwrap();
}
