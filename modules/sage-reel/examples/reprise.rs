//! La demande qu'on reprendrait pour chaque lieu donné (son .reel, existant ou non).
fn main() {
    for a in std::env::args().skip(1) {
        let d = sage_reel::generer::Demande::reprendre(std::path::Path::new(&a));
        println!("{a} : {}", d.map_or("rien".into(), |d| d.texte().replace('\n', " · ")));
    }
}
