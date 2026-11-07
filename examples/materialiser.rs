//! Matérialise un lieu réel : écrit les régions `.stuff` du monde et sa fiche des sources
//! (voir `docs/format-stuff.md`), comme le fait le jeu après l'import d'un `.reel`.
//!
//!   cargo run --release --example materialiser -- <lieu.reel> <dossier du monde>
//!
//!   cargo run --release --example materialiser -- --lire <dossier du monde>
//!
//! relit un monde matérialisé comme au lancement du jeu, et dit combien de temps cela prend.
//!
//! Le dossier reçoit `regions/<ri>_<rj>.stuff`, `sources.json`, et une copie du lieu
//! (`lieu.reel`) s'il n'y est pas encore. Copié dans le dossier `monde/` de l'application,
//! il s'ouvre dans le jeu sans rien recalculer.

use sage::reel::Reel;
use sage::regions::{self, Avancement};
use std::sync::atomic::Ordering;
use std::sync::Arc;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() == 3 && a[1] == "--lire" {
        let t = std::time::Instant::now();
        let m = regions::Regions::ouvrir(std::path::Path::new(&a[2])).expect("pas un monde matérialisé");
        let s = m.silhouettes();
        println!("{} : {} régions, silhouettes lues en {:.2} s ({} + {} + {} sommets)", m.nom, s.lues, t.elapsed().as_secs_f64(), s.relief.len() / 9, s.bati.len() / 9, s.bois.len() / 9);
        for (ri, rj) in [(0, 0), (-1, -1), (3, 2)] {
            let t = std::time::Instant::now();
            let r = m.region(ri, rj).expect("région");
            println!("région ({ri}, {rj}) : {} bâtiments, {} arbres, relue en {:.0} ms", r.batiments.len(), r.arbres.len(), t.elapsed().as_secs_f64() * 1e3);
        }
        return;
    }
    if a.len() < 3 {
        eprintln!("usage : materialiser <lieu.reel> <dossier du monde>");
        std::process::exit(2);
    }
    let o = std::fs::read(&a[1]).unwrap_or_else(|e| panic!("{} : {e}", a[1]));
    let r = Arc::new(Reel::depuis_octets(&o).expect("ce n'est pas un lieu réel lisible"));
    let d = std::path::PathBuf::from(&a[2]);
    std::fs::create_dir_all(&d).unwrap();
    let lieu = d.join(regions::LIEU);
    if !lieu.exists() {
        std::fs::write(&lieu, &o).unwrap();
    }
    println!("{} : {} régions de {} m", r.nom, regions::toutes().len(), regions::METRES);
    let av = Arc::new(Avancement::default());
    let fils = std::thread::available_parallelism().map_or(2, |n| n.get());
    let t = std::time::Instant::now();
    let a2 = av.clone();
    let suivi = std::thread::spawn(move || loop {
        let (f, n) = (a2.faites.load(Ordering::Relaxed), a2.total.load(Ordering::Relaxed));
        eprint!("\r{f} / {n}");
        if n > 0 && f >= n || a2.arret.load(Ordering::Relaxed) {
            eprintln!();
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    });
    let res = regions::materialiser(r, &d, &av, fils);
    av.arret.store(true, Ordering::Relaxed);
    let _ = suivi.join();
    match res {
        Ok(n) => println!("{:.1} Mo écrits en {:.1} s, sur {fils} fils", n as f64 / 1e6, t.elapsed().as_secs_f64()),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
