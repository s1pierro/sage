//! Met des routes d'un lieu à double sens (autant de files dans chaque sens, une
//! partagée au milieu si leur nombre est impair). Usage : double_sens <lieu.reel> <route>… [--ecrire]
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let f = std::path::Path::new(&a[1]);
    let mut l = sage_reel::Lieu::lire(f).expect("lecture");
    for s in a[2..].iter().filter(|s| !s.starts_with("--")) {
        let k: usize = s.parse().unwrap();
        let r = &mut l.routes[k];
        let avant = r.files;
        let t = r.files.total.max(2);
        r.files.total = t;
        r.files.avant = t / 2;
        r.files.arriere = t / 2;
        println!("route {k} : {avant:?} → {:?}", r.files);
    }
    if a.iter().any(|x| x == "--ecrire") {
        l.ecrire(f).expect("écriture");
        println!("écrit");
    }
}
