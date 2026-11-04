//! Dessine le jeu sur le vrai processeur graphique, hors écran, et écrit des images PPM.
//! Sert à valider nuanceurs, peau et mise en page sans installer l'APK.
//!
//!   cargo run --example apercu -- <dossier> [personnage.glb]

use sage::draw::{DrawList, Rect};
use sage::gles::{load_gl, Egl};
use sage::jeu::App;
use sage::renderer::Renderer;
use sage::text::Text;
use sage::ui::{Event, Ui};
use sage::widgets::Cx;

struct Shot {
    name: &'static str,
    w: i32,
    h: i32,
    dp: f32,
    /// Images simulées, à 1/30 s.
    images: usize,
    /// Déplacement du doigt sur la manette, en dp (aucun : au repos).
    manette: Option<(f32, f32)>,
    /// Le moniteur ouvert.
    panneau: bool,
    /// Le joueur part à 3 m du pied de la rampe la plus proche, tourné vers elle.
    rampe: bool,
    /// Le joueur debout au bord d'une dalle, un pied dessus, vu de face et de près.
    dalle: bool,
    /// La caméra derrière le joueur, tournée de cet angle (radians), et son tangage.
    vue: Option<(f32, f32)>,
    /// La caméra tourne autour du joueur à cette vitesse (rad/s) : il suit en virage.
    tourne: f32,
    /// L'éditeur ouvert, l'objet le plus proche choisi et tourné de biais.
    editeur: bool,
}

/// Une manette simulée, le stick gauche poussé, A enfoncé : l'onglet Manette ouvert.
fn manette_simulee(app: &mut App) {
    use sage::manette::{axe, touche, Evenement};
    app.manette(Evenement::Appareil { id: 5, nom: "Manette sans fil".into(), descripteur: "apercu".into(), axes: vec![] });
    app.manette(Evenement::Axes { id: 5, valeurs: vec![(axe::X, 0.5), (axe::Y, -0.6)] });
    app.manette(Evenement::Touche { id: 5, code: touche::A, bas: true });
    app.onglet = 2;
    app.manettes.capture = Some("approcher");
}

/// Place le joueur au bord de la dalle la plus proche : le pied droit dessus, le gauche
/// dans le vide, vu de face.
fn au_bord_d_une_dalle(app: &mut App) {
    let moi = app.lieu().pos;
    let mut mieux: Option<(f32, [f32; 3])> = None;
    for (e, formes) in app.monde.obstacles.iter() {
        let o = app.monde.lieu.get(e).map_or([0.0; 3], |l| l.pos);
        for f in formes {
            let sage::moteur::Forme::Boite(b) = f else { continue };
            if b.demi[0] < 0.4 || b.demi[2] < 0.4 {
                continue;
            }
            // Le bord +x, au milieu
            let p = [o[0] + b.centre[0] + b.demi[0], 0.0, o[2] + b.centre[2]];
            let haut = o[1] + b.centre[1] + b.demi[1];
            let sol = sage::moteur::systemes::hauteur_sol(&app.monde, p[0] + 0.3, p[2]).unwrap_or(haut);
            if !(0.08..0.34).contains(&(haut - sol)) {
                continue;
            }
            let d = (p[0] - moi[0]).hypot(p[2] - moi[2]);
            if mieux.is_none_or(|m| d < m.0) {
                mieux = Some((d, p));
            }
        }
    }
    let Some((_, p)) = mieux else { return };
    let l = app.monde.lieu.get_mut(app.joueur).unwrap();
    l.pos = [p[0] - 0.01, moi[1], p[2]];
    l.cap = 0.0;
    sage::moteur::physique::poser(&mut app.monde);
    app.monde.lieu.get_mut(app.joueur).unwrap().retenir();
    app.cam = (0.0, 0.3, 3.0);
}

/// Place le joueur devant la rampe la plus proche et tourne la vue dans son sens.
fn devant_la_rampe(app: &mut App) {
    let moi = app.lieu().pos;
    let mut mieux: Option<(f32, [f32; 3], [f32; 2])> = None;
    for (e, formes) in app.monde.obstacles.iter() {
        let o = app.monde.lieu.get(e).map_or([0.0; 3], |l| l.pos);
        for f in formes {
            let sage::moteur::Forme::Triangles(t) = f else { continue };
            let pts: Vec<[f32; 3]> = t.t.iter().flatten().map(|p| [p[0] + o[0], p[1] + o[1], p[2] + o[2]]).collect();
            let bas = pts.iter().copied().fold(pts[0], |a, p| if p[1] < a[1] { p } else { a });
            let haut = pts.iter().copied().fold(pts[0], |a, p| if p[1] > a[1] { p } else { a });
            let (dx, dz) = (haut[0] - bas[0], haut[2] - bas[2]);
            // Le sens : l'axe dominant, du bas vers le haut
            let sens = if dx.abs() > dz.abs() { [dx.signum(), 0.0] } else { [0.0, dz.signum()] };
            // Le pied, au milieu de la largeur : le bord de la boîte englobante côté bas
            let (lo, hi) = pts.iter().fold(([f32::MAX; 2], [f32::MIN; 2]), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[2])], [hi[0].max(p[0]), hi[1].max(p[2])]));
            let (cx, cz) = ((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0);
            let pied = [cx - sens[0] * (hi[0] - lo[0]) / 2.0, bas[1], cz - sens[1] * (hi[1] - lo[1]) / 2.0];
            let d = (pied[0] - moi[0]).hypot(pied[2] - moi[2]);
            if mieux.is_none_or(|m| d < m.0) {
                mieux = Some((d, pied, sens));
            }
        }
    }
    let Some((_, bas, sens)) = mieux else { return };
    let l = app.monde.lieu.get_mut(app.joueur).unwrap();
    l.pos = [bas[0] - sens[0] * 3.0, bas[1], bas[2] - sens[1] * 3.0];
    // La vue derrière lui : vers l'avant, la manette pousse (-sin a, -cos a)
    app.cam.0 = (-sens[0]).atan2(-sens[1]);
    sage::moteur::physique::poser(&mut app.monde);
    app.monde.lieu.get_mut(app.joueur).unwrap().retenir();
}

/// L'éditeur ouvert sur l'objet le plus proche du joueur, tourné de 30°, et la vue vers
/// lui.
fn choisir_le_plus_proche(app: &mut App) {
    app.onglet = 1;
    let p = app.lieu().pos;
    let (e, o, l) = app
        .monde
        .objet
        .iter()
        .filter_map(|(e, o)| Some((e, *o, *app.monde.lieu.get(e)?)))
        .min_by(|a, b| {
            let d = |l: &sage::moteur::Lieu| (l.pos[0] - p[0]).hypot(l.pos[2] - p[2]);
            d(&a.2).total_cmp(&d(&b.2))
        })
        .expect("un objet");
    let lieu = sage::moteur::Lieu::en(l.pos, l.cap + 30f32.to_radians());
    app.tuiles.modifier(&mut app.monde, &mut app.maillages, e, o, lieu);
    app.monde.choisie = Some(e);
    app.cam.0 = (-(l.pos[0] - p[0])).atan2(-(l.pos[2] - p[2]));
    println!("choisi {} ({:?}) à {:.1} m", app.monde.ident.get(e).unwrap(), o.genre, (l.pos[0] - p[0]).hypot(l.pos[2] - p[2]));
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("dossier de sortie");
    let glb = args.next().map(|f| (f.clone(), std::fs::read(&f).expect("personnage illisible")));
    let shots = [
        Shot { name: "repos", w: 1920, h: 1200, dp: 1.5, images: 20, manette: None, panneau: false, rampe: false, dalle: false, vue: None, tourne: 0.0, editeur: false },
        Shot { name: "marche", w: 1920, h: 1200, dp: 1.5, images: 50, manette: Some((10.0, -60.0)), panneau: false, rampe: false, dalle: false, vue: None, tourne: 0.0, editeur: false },
        Shot { name: "moniteur", w: 1920, h: 1200, dp: 1.5, images: 50, manette: Some((10.0, -60.0)), panneau: true, rampe: false, dalle: false, vue: None, tourne: 0.0, editeur: false },
        Shot { name: "moniteur-portrait", w: 1200, h: 1920, dp: 1.5, images: 50, manette: Some((-40.0, -40.0)), panneau: true, rampe: false, dalle: false, vue: None, tourne: 0.0, editeur: false },
        Shot { name: "loin", w: 1920, h: 1200, dp: 1.5, images: 900, manette: Some((0.0, -60.0)), panneau: true, rampe: false, dalle: false, vue: None, tourne: 0.0, editeur: false },
        // Les allures : marche à 1,4 m/s, entre marche et course à 2,15 m/s, course à 3,2 m/s
        Shot { name: "allure-marche", w: 1920, h: 1200, dp: 1.5, images: 50, manette: Some((0.0, -28.0)), panneau: true, rampe: false, dalle: false, vue: None, tourne: 0.0, editeur: false },
        Shot { name: "allure-mixte", w: 1920, h: 1200, dp: 1.5, images: 50, manette: Some((0.0, -43.0)), panneau: true, rampe: false, dalle: false, vue: None, tourne: 0.0, editeur: false },
        Shot { name: "allure-course", w: 1920, h: 1200, dp: 1.5, images: 50, manette: Some((0.0, -64.0)), panneau: true, rampe: false, dalle: false, vue: None, tourne: 0.0, editeur: false },
        Shot { name: "rampe-monte", w: 1920, h: 1200, dp: 1.5, images: 150, manette: Some((0.0, -28.0)), panneau: true, rampe: true, dalle: false, vue: None, tourne: 0.0, editeur: false },
        Shot { name: "rampe-haut", w: 1920, h: 1200, dp: 1.5, images: 230, manette: Some((0.0, -28.0)), panneau: true, rampe: true, dalle: false, vue: None, tourne: 0.0, editeur: false },
        Shot { name: "dalle", w: 1920, h: 1200, dp: 1.5, images: 40, manette: None, panneau: false, rampe: false, dalle: true, vue: None, tourne: 0.0, editeur: false },
        // Le regard : la caméra tournée sur le côté, puis vers le haut
        Shot { name: "regard-cote", w: 1920, h: 1200, dp: 1.5, images: 45, manette: None, panneau: true, rampe: false, dalle: false, vue: Some((0.9, 0.35)), tourne: 0.0, editeur: false },
        Shot { name: "regard-haut", w: 1920, h: 1200, dp: 1.5, images: 45, manette: None, panneau: true, rampe: false, dalle: false, vue: Some((0.4, -0.1)), tourne: 0.0, editeur: false },
        // L'inclinaison : un départ en course, puis un virage serré
        Shot { name: "depart", w: 1920, h: 1200, dp: 1.5, images: 7, manette: Some((0.0, -64.0)), panneau: true, rampe: false, dalle: false, vue: Some((1.6, 0.2)), tourne: 0.0, editeur: false },
        Shot { name: "virage", w: 1920, h: 1200, dp: 1.5, images: 75, manette: Some((0.0, -64.0)), panneau: true, rampe: false, dalle: false, vue: Some((0.0, 0.3)), tourne: 1.2, editeur: false },
        Shot { name: "editeur", w: 1920, h: 1200, dp: 1.5, images: 30, manette: None, panneau: true, rampe: false, dalle: false, vue: None, tourne: 0.0, editeur: true },
        Shot { name: "editeur-portrait", w: 1200, h: 1920, dp: 1.5, images: 30, manette: None, panneau: true, rampe: false, dalle: false, vue: None, tourne: 0.0, editeur: true },
        Shot { name: "manette", w: 1920, h: 1200, dp: 1.5, images: 30, manette: None, panneau: true, rampe: false, dalle: false, vue: None, tourne: 0.0, editeur: false },
        Shot { name: "telephone", w: 2400, h: 1080, dp: 2.5, images: 40, manette: Some((60.0, 0.0)), panneau: true, rampe: false, dalle: false, vue: None, tourne: 0.0, editeur: false },
    ];
    let mut egl = Egl::new(true).expect("EGL");
    egl.attach_pbuffer(2400, 2400).expect("pbuffer");
    let mut render = Renderer::new(load_gl(true).expect("GL")).expect("nuanceurs");
    println!("nuanceurs compilés et liés");
    let mut text = Text::new();

    for s in shots {
        let (w, h) = (s.w as f32, s.h as f32);
        let screen = Rect::new(0.0, 0.0, w, h);
        let mut app = App::new(s.dp);
        if let Some((nom, octets)) = &glb {
            app.charger(nom, octets);
        }
        if s.rampe {
            devant_la_rampe(&mut app);
        }
        if s.dalle {
            au_bord_d_une_dalle(&mut app);
        }
        if let Some((lacet, tangage)) = s.vue {
            // Le joueur regarde vers +z : la caméra derrière lui est en -z
            app.cam.0 = std::f32::consts::PI + lacet;
            app.cam.1 = tangage;
        }
        app.shell.start.open = s.panneau;
        if s.editeur {
            choisir_le_plus_proche(&mut app);
        }
        if s.name.starts_with("manette") {
            manette_simulee(&mut app);
        }
        let mut ui = Ui::new(s.dp);
        let mut dl = DrawList::default();
        // Sous le doigt : l'espace de travail une fois le panneau déployé
        let mut coque = sage::shell::Shell::new(s.dp);
        coque.start.open = s.panneau;
        coque.start.anim = if s.panneau { 1.0 } else { 0.0 };
        let l = coque.layout(screen, 0.0);
        let (x0, y0) = (l.workspace.x + l.workspace.w * 0.2, l.workspace.y + l.workspace.h * 0.7);
        let mut scene = None;
        for i in 0..s.images {
            let t = i as f64 / 30.0;
            if let Some((dx, dy)) = s.manette {
                // Vraie séquence tactile : le doigt se pose, puis glisse en quelques images
                if i == 1 {
                    ui.event(Event::Down { id: 0, x: x0, y: y0 }, t);
                } else if i > 1 {
                    let u = ((i - 1) as f32 / 4.0).min(1.0);
                    ui.event(Event::Move { id: 0, x: x0 + dx * s.dp * u, y: y0 + dy * s.dp * u }, t);
                }
            }
            app.cam.0 += s.tourne / 30.0;
            dl.clear();
            let mut cx = Cx { ui: &mut ui, text: &mut text, dl: &mut dl, dp: s.dp };
            scene = Some(app.frame(&mut cx, screen, 1.0 / 30.0));
            ui.end_frame();
        }
        render.draw(w, h, &scene.unwrap(), &dl, &mut text);

        let gl = render.gl();
        let mut px = vec![0u8; (s.w * s.h * 4) as usize];
        unsafe {
            (gl.glReadPixels)(0, 0, s.w, s.h, 0x1908, 0x1401, px.as_mut_ptr() as *mut _);
            let e = (gl.glGetError)();
            if let Some(q) = app.monde.squelette.get(app.joueur) {
                let (b, r) = q.posture.resume();
                println!("{} : pieds {} ; buste {b} ; regard {r}", s.name, q.pieds.resume());
            }
            println!("{} : absolu {:?}, tuiles {}, vitesse {:.2}, erreur GL {e:#x}", s.name, app.monde.absolu(app.lieu().pos), app.tuiles.chargees.len(), app.mobile().vitesse);
        }
        let mut out = format!("P6\n{} {}\n255\n", s.w, s.h).into_bytes();
        for y in (0..s.h as usize).rev() {
            for x in 0..s.w as usize {
                out.extend_from_slice(&px[(y * s.w as usize + x) * 4..][..3]);
            }
        }
        std::fs::write(format!("{dir}/{}.ppm", s.name), out).unwrap();
    }
}
