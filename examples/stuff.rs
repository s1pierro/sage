//! L'outil `stuff` : affiche l'en-tête, les tampons et le manifeste d'un fichier `.stuff`.
//!
//!   cargo run --example stuff -- <fichier.stuff>

fn main() {
    let Some(f) = std::env::args().nth(1) else {
        eprintln!("usage : stuff <fichier.stuff>");
        std::process::exit(2);
    };
    let o = std::fs::read(&f).unwrap_or_else(|e| panic!("{f} : {e}"));
    match sage::moteur::stuff::resume(&o) {
        Ok(s) => print!("{s}"),
        Err(e) => {
            eprintln!("{f} : {e}");
            std::process::exit(1);
        }
    }
}
