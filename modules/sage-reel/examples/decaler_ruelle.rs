//! Martainville est (rouen20) : son départ déplacé de ~8 m vers le nord sur Armand-Carrel
//! (deux T décalés, comme dans la réalité). Usage : decaler_ruelle <lieu.reel> [--ecrire]
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let f = std::path::Path::new(&a[1]);
    let mut l = sage_reel::Lieu::lire(f).expect("lecture");
    let pres = |p: [f32; 3], x: f32, z: f32| (p[0] - x).hypot(p[1] - z) < 1.5;
    let c = (496.0, 366.0);
    let ruelle = l.routes.iter().position(|r| pres(r.points[0], c.0, c.1) && pres(*r.points.last().unwrap(), 535.0, 363.0)).expect("ruelle");
    let carrel = l.routes.iter().position(|r| pres(r.points[0], 455.0, 467.0) && pres(*r.points.last().unwrap(), 531.0, 284.0)).expect("Armand-Carrel");
    let o = l.routes[ruelle].points[0];
    let ac = &l.routes[carrel].points;
    let i0 = ac.iter().position(|p| (p[0] - o[0]).hypot(p[1] - o[1]) < 0.05).expect("croisement sur Armand-Carrel");
    // Vers le nord (z décroissant : le bout (531,284)) : le sommet d'Armand-Carrel le plus proche de 8 m
    let mut l_ = 0.0;
    let mut cible = (i0, 0.0f32);
    for j in i0 + 1..ac.len() {
        l_ += (ac[j][0] - ac[j - 1][0]).hypot(ac[j][1] - ac[j - 1][1]);
        if (l_ - 8.0).abs() < (cible.1 - 8.0).abs() {
            cible = (j, l_);
        }
        if l_ > 14.0 {
            break;
        }
    }
    let n = ac[cible.0];
    println!("ruelle route {ruelle} ({} points), Armand-Carrel route {carrel} ; départ {:?} → {:?} ({:.1} m au nord)", l.routes[ruelle].points.len(), o, n, cible.1);
    let r = &mut l.routes[ruelle];
    // Les premiers sommets de la ruelle, désormais en arrière du nouveau départ, retirés
    let d = |p: [f32; 3]| (p[0] - n[0]).hypot(p[1] - n[1]);
    let fin = *r.points.last().unwrap();
    let dfin = d(fin);
    let reste: Vec<[f32; 3]> = r.points.iter().skip(1).copied().filter(|p| d(*p) > 3.0 && (p[0] - fin[0]).hypot(p[1] - fin[1]) < dfin).collect();
    r.points = std::iter::once(n).chain(reste).collect();
    if r.points.len() < 2 {
        r.points.push(fin);
    }
    println!("nouvelle ruelle : {:?}", r.points.iter().map(|p| (p[0].round(), p[1].round())).collect::<Vec<_>>());
    if a.iter().any(|x| x == "--ecrire") {
        l.ecrire(f).expect("écriture");
        println!("écrit");
    }
}
