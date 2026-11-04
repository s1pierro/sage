//! Intégration Android : NativeActivity, thread de rendu, fichiers.
//!
//! Le thread principal ne fait que transmettre les événements et les fichiers reçus ;
//! le jeu vit sur le thread de rendu. Le sélecteur de fichiers et « Ouvrir avec » sont
//! ceux de Rustcraft : `Activite.java` reçoit le document, Rust le lit par JNI.

#![allow(non_camel_case_types)]

use crate::jeu::App;
use crate::manette::{Evenement, AXES};
use crate::draw::{DrawList, Rect};
use crate::gles::{load_gl, Egl};
use crate::renderer::Renderer;
use crate::text::Text;
use crate::ui::{Event, Ui};
use crate::widgets::Cx;
use std::ffi::{c_void, CString};
use std::path::PathBuf;
use std::ptr::null_mut;
use std::sync::{Condvar, Mutex, Once, OnceLock};
use std::time::{Duration, Instant};

type ANativeWindow = c_void;
type AInputQueue = c_void;
type AInputEvent = c_void;
type ALooper = c_void;

#[repr(C)]
pub struct ANativeActivity {
    callbacks: *mut ANativeActivityCallbacks,
    vm: *mut c_void,
    env: *mut c_void,
    clazz: *mut c_void,
    internal_data_path: *const i8,
    external_data_path: *const i8,
    sdk_version: i32,
    instance: *mut c_void,
    asset_manager: *mut c_void,
    obb_path: *const i8,
}

type Cb = Option<unsafe extern "C" fn(*mut ANativeActivity)>;
type CbWindow = Option<unsafe extern "C" fn(*mut ANativeActivity, *mut ANativeWindow)>;
type CbQueue = Option<unsafe extern "C" fn(*mut ANativeActivity, *mut AInputQueue)>;

#[repr(C)]
pub struct ANativeActivityCallbacks {
    on_start: Cb,
    on_resume: Cb,
    on_save_instance_state: Option<unsafe extern "C" fn(*mut ANativeActivity, *mut usize) -> *mut c_void>,
    on_pause: Cb,
    on_stop: Cb,
    on_destroy: Cb,
    on_window_focus_changed: Option<unsafe extern "C" fn(*mut ANativeActivity, i32)>,
    on_native_window_created: CbWindow,
    on_native_window_resized: CbWindow,
    on_native_window_redraw_needed: CbWindow,
    on_native_window_destroyed: CbWindow,
    on_input_queue_created: CbQueue,
    on_input_queue_destroyed: CbQueue,
    on_content_rect_changed: Option<unsafe extern "C" fn(*mut ANativeActivity, *const c_void)>,
    on_configuration_changed: Cb,
    on_low_memory: Cb,
}

const AINPUT_EVENT_TYPE_KEY: i32 = 1;
const AINPUT_EVENT_TYPE_MOTION: i32 = 2;
const SOURCE_GAMEPAD: i32 = 0x0000_0401;
const SOURCE_JOYSTICK: i32 = 0x0100_0010;
/// La liste des manettes se relit toutes les tant de secondes : celles qui ne répondent
/// plus sont parties.
const RELIRE_MANETTES: f32 = 2.0;
const ACTION_DOWN: i32 = 0;
const ACTION_UP: i32 = 1;
const ACTION_MOVE: i32 = 2;
const ACTION_CANCEL: i32 = 3;
const ACTION_POINTER_DOWN: i32 = 5;
const ACTION_POINTER_UP: i32 = 6;
const ACTION_MASK: i32 = 0xff;
const ACTION_POINTER_INDEX_MASK: i32 = 0xff00;
const ACTION_POINTER_INDEX_SHIFT: i32 = 8;

type ALooperCallback = unsafe extern "C" fn(fd: i32, events: i32, data: *mut c_void) -> i32;

#[link(name = "android")]
extern "C" {
    fn ALooper_forThread() -> *mut ALooper;
    fn AInputQueue_attachLooper(q: *mut AInputQueue, l: *mut ALooper, ident: i32, cb: Option<ALooperCallback>, data: *mut c_void);
    fn AInputQueue_detachLooper(q: *mut AInputQueue);
    fn AInputQueue_getEvent(q: *mut AInputQueue, out: *mut *mut AInputEvent) -> i32;
    fn AInputQueue_preDispatchEvent(q: *mut AInputQueue, e: *mut AInputEvent) -> i32;
    fn AInputQueue_finishEvent(q: *mut AInputQueue, e: *mut AInputEvent, handled: i32);
    fn AInputEvent_getType(e: *const AInputEvent) -> i32;
    fn AMotionEvent_getAction(e: *const AInputEvent) -> i32;
    fn AMotionEvent_getPointerCount(e: *const AInputEvent) -> usize;
    fn AMotionEvent_getPointerId(e: *const AInputEvent, pointer_index: usize) -> i32;
    fn AMotionEvent_getX(e: *const AInputEvent, pointer_index: usize) -> f32;
    fn AMotionEvent_getY(e: *const AInputEvent, pointer_index: usize) -> f32;
    fn AMotionEvent_getAxisValue(e: *const AInputEvent, axis: i32, pointer_index: usize) -> f32;
    fn AInputEvent_getSource(e: *const AInputEvent) -> i32;
    fn AInputEvent_getDeviceId(e: *const AInputEvent) -> i32;
    fn AKeyEvent_getAction(e: *const AInputEvent) -> i32;
    fn AKeyEvent_getKeyCode(e: *const AInputEvent) -> i32;
    fn AKeyEvent_getRepeatCount(e: *const AInputEvent) -> i32;
    fn AConfiguration_new() -> *mut c_void;
    fn AConfiguration_fromAssetManager(c: *mut c_void, am: *mut c_void);
    fn AConfiguration_getDensity(c: *const c_void) -> i32;
    fn AConfiguration_delete(c: *mut c_void);
}

#[link(name = "log")]
extern "C" {
    fn __android_log_write(prio: i32, tag: *const i8, text: *const i8) -> i32;
}

fn log(s: &str) {
    let text = CString::new(s).unwrap_or_default();
    unsafe {
        __android_log_write(6, c"SAGE".as_ptr() as *const _, text.as_ptr() as *const _);
    }
}

// ---------------------------------------------------------------------------
// État partagé entre le thread principal et le thread de rendu
// ---------------------------------------------------------------------------

struct Shared {
    window: *mut ANativeWindow,
    window_changed: bool,
    /// Le rendu a lâché l'ancienne fenêtre : Android peut la détruire.
    released: bool,
    redraw: bool,
    events: Vec<(Event, f64)>,
    dp: f32,
    /// Document reçu (Ouvrir avec, sélecteur) : nom et contenu.
    fichier: Option<(String, Vec<u8>)>,
    /// Sélection en cours de réception.
    lot: Vec<(String, Vec<u8>)>,
    /// Dossier privé de l'application.
    interne: Option<PathBuf>,
    /// Ce qu'envoient les manettes de jeu.
    manette: Vec<Evenement>,
    /// Les manettes déjà présentées à l'application.
    appareils: Vec<i32>,
}
unsafe impl Send for Shared {}

static SHARED: Mutex<Shared> = Mutex::new(Shared {
    window: null_mut(),
    window_changed: false,
    released: true,
    redraw: false,
    events: Vec::new(),
    dp: 1.0,
    fichier: None,
    lot: Vec::new(),
    interne: None,
    manette: Vec::new(),
    appareils: Vec::new(),
});
static WAKE: Condvar = Condvar::new();
/// Machine virtuelle et activité courante, pour ouvrir le sélecteur depuis le fil de rendu.
static ACTIVITE: Mutex<Option<(usize, usize)>> = Mutex::new(None);
/// Le dernier personnage ouvert, relu au lancement.
const GARDE: &str = "personnage.glb";
static START: OnceLock<Instant> = OnceLock::new();

fn now() -> f64 {
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

fn push(events: &[Event]) {
    let t = now();
    let mut s = SHARED.lock().unwrap();
    s.events.extend(events.iter().map(|e| (*e, t)));
    WAKE.notify_all();
}

// ---------------------------------------------------------------------------
// Thread de rendu
// ---------------------------------------------------------------------------

fn render_loop() -> Result<(), String> {
    let mut egl = Egl::new(false)?;
    let mut gl = Some(load_gl(false)?);
    let mut renderer: Option<Renderer> = None;
    let mut app: Option<(App, Ui)> = None;
    let mut text = Text::new();
    let mut dl = DrawList::default();
    let mut last = Instant::now();
    let mut again = false; // la dernière image demande à être suivie d'une autre
    let interne = SHARED.lock().unwrap().interne.clone();
    let mut depuis_releve = 0.0f32;

    loop {
        let (window, changed, events, dp, fichier, manette) = {
            let mut s = SHARED.lock().unwrap();
            loop {
                let drawing = !egl.surface.is_null();
                let busy = drawing && (again || app.as_ref().is_some_and(|(a, _)| a.animating()));
                if s.window_changed || s.redraw || busy || (drawing && (!s.events.is_empty() || !s.manette.is_empty() || s.fichier.is_some())) {
                    break;
                }
                s = WAKE.wait(s).unwrap();
            }
            s.redraw = false;
            let changed = std::mem::take(&mut s.window_changed);
            let fichier = if egl.surface.is_null() { None } else { s.fichier.take() };
            (s.window, changed, std::mem::take(&mut s.events), s.dp, fichier, std::mem::take(&mut s.manette))
        };
        again = false;

        if changed {
            if !egl.surface.is_null() {
                egl.detach();
            }
            if window.is_null() {
                // En arrière-plan : le monde s'écrit avant que le système ne puisse arrêter
                // l'application
                if let Some((a, _)) = app.as_mut() {
                    a.enregistrer();
                }
                SHARED.lock().unwrap().released = true;
                WAKE.notify_all();
                continue;
            }
            egl.attach_window(window)?;
            if let Some(api) = gl.take() {
                renderer = Some(Renderer::new(api)?);
            }
            last = Instant::now();
        }
        if egl.surface.is_null() {
            continue;
        }
        let (Some(render), (app, ui)) = (
            renderer.as_mut(),
            app.get_or_insert_with(|| {
                let mut a = App::avec_dossier(dp, interne.clone());
                if let Some(o) = interne.as_ref().and_then(|d| std::fs::read(d.join(GARDE)).ok()) {
                    a.reprendre(&o);
                }
                (a, Ui::new(dp))
            }),
        ) else {
            continue;
        };
        if let Some((nom, contenu)) = fichier {
            app.charger(&nom, &contenu);
        }

        for (e, t) in events {
            ui.event(e, t);
        }
        for e in manette {
            app.manette(e);
        }
        // Les manettes débranchées
        depuis_releve += last.elapsed().as_secs_f32();
        if depuis_releve > RELIRE_MANETTES {
            depuis_releve = 0.0;
            releve_manettes();
        }
        let intervalle = last.elapsed().as_secs_f32();
        let dt = intervalle.min(0.05);
        last = Instant::now();

        let (w, h) = egl.size();
        dl.clear();
        let mut cx = Cx { ui, text: &mut text, dl: &mut dl, dp };
        let t0 = Instant::now();
        let scene = app.frame(&mut cx, Rect::new(0.0, 0.0, w, h), dt);
        ui.end_frame();
        let t1 = Instant::now();
        render.draw(w, h, &scene, &dl, &mut text);
        let t2 = Instant::now();
        egl.swap();
        let t3 = Instant::now();
        // Pour le moniteur ; affichées à l'image suivante
        app.mesurer(intervalle, (t1 - t0).as_secs_f32(), (t2 - t1).as_secs_f32(), (t3 - t2).as_secs_f32());
        if app.prendre_demande() {
            if let Err(e) = selecteur() {
                app.dire(format!("Sélecteur de fichiers indisponible : {e}"));
            }
        }
        // Un contenu vide : revenir au mannequin, et l'oublier
        if let (Some(d), Some(o)) = (&interne, app.prendre_a_garder()) {
            let r = if o.is_empty() {
                std::fs::remove_file(d.join(GARDE)).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) })
            } else {
                std::fs::write(d.join("personnage.tmp"), &o).and_then(|_| std::fs::rename(d.join("personnage.tmp"), d.join(GARDE)))
            };
            if let Err(e) = r {
                log(&format!("personnage gardé : {e}"));
            }
        }
        // Après une rotation, la nouvelle taille n'est connue qu'ici ; et un appui change
        // l'état après le dessin du widget, qu'il faut redessiner à jour.
        again = egl.size() != (w, h) || ui.a_redessiner();
    }
}

fn spawn_render_thread() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        std::thread::spawn(|| {
            if let Err(e) = render_loop() {
                log(&e);
                // Personne ne doit rester bloqué sur une fenêtre à lâcher.
                SHARED.lock().unwrap().released = true;
                WAKE.notify_all();
            }
        });
    });
}

// ---------------------------------------------------------------------------
// Fichiers : JNI, comme dans Rustcraft
// ---------------------------------------------------------------------------

type Jni = *mut *const *const c_void;
type Obj = *mut c_void;

/// Entrée de la table de fonctions JNI (rang fixé par la spécification).
unsafe fn jni_fn<T: Copy>(env: Jni, rang: usize) -> T {
    let f = *(*env).add(rang);
    std::mem::transmute_copy(&f)
}

unsafe fn exception(env: Jni) -> bool {
    let check: unsafe extern "C" fn(Jni) -> u8 = jni_fn(env, 228);
    if check(env) == 0 {
        return false;
    }
    let clear: unsafe extern "C" fn(Jni) = jni_fn(env, 17);
    clear(env);
    true
}

unsafe fn methode(env: Jni, o: Obj, nom: &std::ffi::CStr, sig: &std::ffi::CStr) -> Option<*mut c_void> {
    let classe: unsafe extern "C" fn(Jni, Obj) -> Obj = jni_fn(env, 31);
    let id: unsafe extern "C" fn(Jni, Obj, *const i8, *const i8) -> *mut c_void = jni_fn(env, 33);
    let c = classe(env, o);
    let m = id(env, c, nom.as_ptr() as *const _, sig.as_ptr() as *const _);
    if exception(env) || m.is_null() { None } else { Some(m) }
}

type AppelObj = unsafe extern "C" fn(Jni, Obj, *mut c_void, ...) -> Obj;

unsafe fn objet0(env: Jni, o: Obj, nom: &std::ffi::CStr, sig: &std::ffi::CStr) -> Option<Obj> {
    let m = methode(env, o, nom, sig)?;
    let f: AppelObj = jni_fn(env, 34);
    let r = f(env, o, m);
    if exception(env) || r.is_null() { None } else { Some(r) }
}

unsafe fn chaine(env: Jni, s: &std::ffi::CStr) -> Obj {
    let f: unsafe extern "C" fn(Jni, *const i8) -> Obj = jni_fn(env, 167);
    f(env, s.as_ptr() as *const _)
}

unsafe fn texte(env: Jni, s: Obj) -> Option<String> {
    let lire: unsafe extern "C" fn(Jni, Obj, *mut u8) -> *const i8 = jni_fn(env, 169);
    let lacher: unsafe extern "C" fn(Jni, Obj, *const i8) = jni_fn(env, 170);
    let p = lire(env, s, null_mut());
    if p.is_null() {
        return None;
    }
    let t = std::ffi::CStr::from_ptr(p as *const _).to_string_lossy().into_owned();
    lacher(env, s, p);
    Some(t)
}

/// Nom affiché du document, sinon la fin de son adresse.
unsafe fn nom_document(env: Jni, resolver: Obj, uri: Obj) -> String {
    let query = c"(Landroid/net/Uri;[Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;Ljava/lang/String;)Landroid/database/Cursor;";
    let lu = (|| {
        let m = methode(env, resolver, c"query", query)?;
        let f: AppelObj = jni_fn(env, 34);
        let vide: Obj = null_mut();
        let cur = f(env, resolver, m, uri, vide, vide, vide, vide);
        if exception(env) || cur.is_null() {
            return None;
        }
        let premier = methode(env, cur, c"moveToFirst", c"()Z")?;
        let b: unsafe extern "C" fn(Jni, Obj, *mut c_void, ...) -> u8 = jni_fn(env, 37);
        if b(env, cur, premier) == 0 {
            return None;
        }
        let col = methode(env, cur, c"getColumnIndex", c"(Ljava/lang/String;)I")?;
        let i: unsafe extern "C" fn(Jni, Obj, *mut c_void, ...) -> i32 = jni_fn(env, 49);
        let k = i(env, cur, col, chaine(env, c"_display_name"));
        if exception(env) || k < 0 {
            return None;
        }
        let get = methode(env, cur, c"getString", c"(I)Ljava/lang/String;")?;
        let s = f(env, cur, get, k);
        if exception(env) || s.is_null() {
            return None;
        }
        texte(env, s)
    })();
    lu.or_else(|| {
        let s = objet0(env, uri, c"getLastPathSegment", c"()Ljava/lang/String;")?;
        Some(texte(env, s)?.rsplit(['/', ':']).next()?.to_string())
    })
    .unwrap_or_else(|| "fichier".to_string())
}

/// Fichier transmis à l'activité (Ouvrir avec, Partager), s'il y en a un.
unsafe fn fichier_recu(activity: *mut ANativeActivity) -> Option<(Vec<(String, Vec<u8>)>, i32)> {
    let (env, moi) = ((*activity).env as Jni, (*activity).clazz);
    let intent = objet0(env, moi, c"getIntent", c"()Landroid/content/Intent;")?;
    let uri = objet0(env, intent, c"getData", c"()Landroid/net/Uri;").or_else(|| {
        let m = methode(env, intent, c"getParcelableExtra", c"(Ljava/lang/String;)Landroid/os/Parcelable;")?;
        let f: AppelObj = jni_fn(env, 34);
        let r = f(env, intent, m, chaine(env, c"android.intent.extra.STREAM"));
        if exception(env) || r.is_null() { None } else { Some(r) }
    })?;
    Some((vec![lire_uri(env, moi, uri)?], 0))
}

/// Ouvre le document désigné et le lit en entier.
unsafe fn lire_uri(env: Jni, moi: Obj, uri: Obj) -> Option<(String, Vec<u8>)> {
    use std::io::Read;
    use std::os::fd::FromRawFd;
    let resolver = objet0(env, moi, c"getContentResolver", c"()Landroid/content/ContentResolver;")?;
    let nom = nom_document(env, resolver, uri);
    let m = methode(env, resolver, c"openFileDescriptor", c"(Landroid/net/Uri;Ljava/lang/String;)Landroid/os/ParcelFileDescriptor;")?;
    let f: AppelObj = jni_fn(env, 34);
    let pfd = f(env, resolver, m, uri, chaine(env, c"r"));
    if exception(env) || pfd.is_null() {
        log("fichier reçu illisible");
        return None;
    }
    let get = methode(env, pfd, c"getFd", c"()I")?;
    let i: unsafe extern "C" fn(Jni, Obj, *mut c_void, ...) -> i32 = jni_fn(env, 49);
    let fd = dup(i(env, pfd, get));
    if let Some(close) = methode(env, pfd, c"close", c"()V") {
        let v: unsafe extern "C" fn(Jni, Obj, *mut c_void, ...) = jni_fn(env, 61);
        v(env, pfd, close);
        exception(env);
    }
    if fd < 0 {
        return None;
    }
    let mut contenu = Vec::new();
    std::fs::File::from_raw_fd(fd).read_to_end(&mut contenu).ok()?;
    Some((nom, contenu))
}

extern "C" {
    fn dup(fd: i32) -> i32;
}

/// Demande à l'activité Java d'ouvrir le sélecteur ; le choix revient par `fichier_choisi`.
fn selecteur() -> Result<(), String> {
    let (vm, moi) = ACTIVITE.lock().unwrap().ok_or("activité absente")?;
    let vm = vm as *mut *const *const c_void;
    unsafe {
        let attacher: unsafe extern "C" fn(*mut *const *const c_void, *mut Jni, *mut c_void) -> i32 = std::mem::transmute(*(*vm).add(4));
        let mut env: Jni = null_mut();
        if attacher(vm, &mut env, null_mut()) != 0 || env.is_null() {
            return Err("Java injoignable".into());
        }
        let m = methode(env, moi as Obj, c"choisir", c"(I)V").ok_or("méthode absente")?;
        let v: unsafe extern "C" fn(Jni, Obj, *mut c_void, ...) = jni_fn(env, 61);
        v(env, moi as Obj, m, 1i32);
        if exception(env) {
            return Err("appel refusé".into());
        }
    }
    Ok(())
}

/// `Activite.recu`, sur le fil principal : un document choisi, et combien en restent.
/// S'il est illisible, un contenu vide fait dire à l'application pourquoi elle n'a
/// rien ouvert.
unsafe extern "C" fn fichier_choisi(env: Jni, moi: Obj, uri: Obj, _demande: i32, reste: i32) {
    let f = lire_uri(env, moi, uri).unwrap_or_else(|| ("fichier".to_string(), Vec::new()));
    let mut s = SHARED.lock().unwrap();
    s.lot.push(f);
    if reste == 0 {
        // Un seul personnage à la fois : le dernier choisi
        s.fichier = std::mem::take(&mut s.lot).pop();
        WAKE.notify_all();
    }
}

/// Relie `Activite.recu` : la bibliothèque est chargée par NativeActivity, hors de
/// `System.loadLibrary`, et la recherche par nom de symbole ne la trouverait pas.
unsafe fn relier_java(activity: *mut ANativeActivity) {
    #[repr(C)]
    struct Methode {
        nom: *const std::ffi::c_char,
        sig: *const std::ffi::c_char,
        f: *mut c_void,
    }
    let (env, moi) = ((*activity).env as Jni, (*activity).clazz);
    let classe: unsafe extern "C" fn(Jni, Obj) -> Obj = jni_fn(env, 31);
    let inscrire: unsafe extern "C" fn(Jni, Obj, *const Methode, i32) -> i32 = jni_fn(env, 215);
    let m = Methode { nom: c"recu".as_ptr(), sig: c"(Landroid/net/Uri;II)V".as_ptr(), f: fichier_choisi as *mut c_void };
    if inscrire(env, classe(env, moi), &m, 1) != 0 || exception(env) {
        log("Activite.recu non reliée");
    }
    let globale: unsafe extern "C" fn(Jni, Obj) -> Obj = jni_fn(env, 21);
    let lacher: unsafe extern "C" fn(Jni, Obj) = jni_fn(env, 22);
    let nouvelle = ((*activity).vm as usize, globale(env, moi) as usize);
    if let Some((_, ancienne)) = ACTIVITE.lock().unwrap().replace(nouvelle) {
        lacher(env, ancienne as Obj);
    }
}

// ---------------------------------------------------------------------------
// Callbacks de l'activité
// ---------------------------------------------------------------------------

unsafe extern "C" fn on_window_created(_: *mut ANativeActivity, window: *mut ANativeWindow) {
    let mut s = SHARED.lock().unwrap();
    s.window = window;
    s.window_changed = true;
    s.released = false;
    WAKE.notify_all();
}

unsafe extern "C" fn on_window_changed(_: *mut ANativeActivity, _: *mut ANativeWindow) {
    SHARED.lock().unwrap().redraw = true;
    WAKE.notify_all();
}

/// Android détruit la fenêtre au retour : le rendu doit l'avoir lâchée avant.
unsafe extern "C" fn on_window_destroyed(_: *mut ANativeActivity, _: *mut ANativeWindow) {
    let mut s = SHARED.lock().unwrap();
    s.window = null_mut();
    s.window_changed = true;
    WAKE.notify_all();
    let (_guard, _) = WAKE.wait_timeout_while(s, Duration::from_secs(2), |s| !s.released).unwrap();
}

unsafe extern "C" fn on_input(_fd: i32, _events: i32, data: *mut c_void) -> i32 {
    let queue = data as *mut AInputQueue;
    let mut event: *mut AInputEvent = null_mut();
    while AInputQueue_getEvent(queue, &mut event) >= 0 {
        if AInputQueue_preDispatchEvent(queue, event) != 0 {
            continue;
        }
        let mut handled = 0;
        let source = AInputEvent_getSource(event);
        let ty = AInputEvent_getType(event);
        if source & SOURCE_GAMEPAD == SOURCE_GAMEPAD || source & SOURCE_JOYSTICK == SOURCE_JOYSTICK {
            // Une manette : jamais un doigt, et ses touches restent au jeu (B ne fait pas
            // Retour)
            handled = 1;
            let id = AInputEvent_getDeviceId(event);
            presenter(id);
            if ty == AINPUT_EVENT_TYPE_KEY {
                if AKeyEvent_getRepeatCount(event) == 0 {
                    let bas = AKeyEvent_getAction(event) == 0;
                    pousser_manette(Evenement::Touche { id, code: AKeyEvent_getKeyCode(event), bas });
                }
            } else if ty == AINPUT_EVENT_TYPE_MOTION {
                let valeurs = AXES.iter().map(|&a| (a, AMotionEvent_getAxisValue(event, a, 0))).collect();
                pousser_manette(Evenement::Axes { id, valeurs });
            }
        } else if ty == AINPUT_EVENT_TYPE_MOTION {
            handled = 1;
            let action = AMotionEvent_getAction(event);
            let idx = ((action & ACTION_POINTER_INDEX_MASK) >> ACTION_POINTER_INDEX_SHIFT) as usize;
            let id = |i: usize| AMotionEvent_getPointerId(event, i);
            match action & ACTION_MASK {
                ACTION_DOWN | ACTION_POINTER_DOWN => {
                    push(&[Event::Down { id: id(idx), x: AMotionEvent_getX(event, idx), y: AMotionEvent_getY(event, idx) }])
                }
                ACTION_MOVE => {
                    let moves: Vec<Event> = (0..AMotionEvent_getPointerCount(event))
                        .map(|i| Event::Move { id: id(i), x: AMotionEvent_getX(event, i), y: AMotionEvent_getY(event, i) })
                        .collect();
                    push(&moves);
                }
                ACTION_UP | ACTION_POINTER_UP => push(&[Event::Up { id: id(idx) }]),
                ACTION_CANCEL => push(&[Event::Cancel]),
                _ => handled = 0,
            }
        }
        AInputQueue_finishEvent(queue, event, handled);
    }
    1
}

fn pousser_manette(e: Evenement) {
    let mut s = SHARED.lock().unwrap();
    s.manette.push(e);
    WAKE.notify_all();
}

/// L'environnement Java du fil courant, qui s'y attache au besoin.
unsafe fn env_java() -> Option<Jni> {
    let (vm, _) = (*ACTIVITE.lock().unwrap())?;
    let vm = vm as *mut *const *const c_void;
    let attacher: unsafe extern "C" fn(*mut *const *const c_void, *mut Jni, *mut c_void) -> i32 = std::mem::transmute(*(*vm).add(4));
    let mut env: Jni = null_mut();
    (attacher(vm, &mut env, null_mut()) == 0 && !env.is_null()).then_some(env)
}

/// `InputDevice.getDevice(id)`, dans un cadre de références locales que l'appelant ferme.
unsafe fn appareil_java(env: Jni, id: i32) -> Option<Obj> {
    let trouver: unsafe extern "C" fn(Jni, *const i8) -> Obj = jni_fn(env, 6);
    let statique: unsafe extern "C" fn(Jni, Obj, *const i8, *const i8) -> *mut c_void = jni_fn(env, 113);
    let appeler: unsafe extern "C" fn(Jni, Obj, *mut c_void, ...) -> Obj = jni_fn(env, 114);
    let c = trouver(env, c"android/view/InputDevice".as_ptr() as *const _);
    if exception(env) || c.is_null() {
        return None;
    }
    let m = statique(env, c, c"getDevice".as_ptr() as *const _, c"(I)Landroid/view/InputDevice;".as_ptr() as *const _);
    if exception(env) || m.is_null() {
        return None;
    }
    let d = appeler(env, c, m, id);
    if exception(env) || d.is_null() { None } else { Some(d) }
}

/// Le nom, le descripteur et les axes de la manette `id`.
unsafe fn decrire(env: Jni, id: i32) -> Option<(String, String, Vec<i32>)> {
    let d = appareil_java(env, id)?;
    let nom = objet0(env, d, c"getName", c"()Ljava/lang/String;").and_then(|s| texte(env, s)).unwrap_or_else(|| format!("Manette {id}"));
    let descripteur = objet0(env, d, c"getDescriptor", c"()Ljava/lang/String;").and_then(|s| texte(env, s)).unwrap_or_else(|| format!("appareil-{id}"));
    let mut axes = Vec::new();
    if let Some(m) = methode(env, d, c"getMotionRange", c"(I)Landroid/view/InputDevice$MotionRange;") {
        let f: AppelObj = jni_fn(env, 34);
        for &a in AXES.iter() {
            let r = f(env, d, m, a);
            if !exception(env) && !r.is_null() {
                axes.push(a);
            }
        }
    }
    Some((nom, descripteur, axes))
}

/// Présente à l'application une manette qu'elle ne connaît pas encore.
unsafe fn presenter(id: i32) {
    if SHARED.lock().unwrap().appareils.contains(&id) {
        return;
    }
    let ev = env_java().and_then(|env| {
        let cadre: unsafe extern "C" fn(Jni, i32) -> i32 = jni_fn(env, 19);
        let fermer: unsafe extern "C" fn(Jni, Obj) -> Obj = jni_fn(env, 20);
        if cadre(env, 32) != 0 {
            return None;
        }
        let r = decrire(env, id);
        fermer(env, null_mut());
        r
    });
    let (nom, descripteur, axes) = ev.unwrap_or_else(|| (format!("Manette {id}"), format!("appareil-{id}"), Vec::new()));
    SHARED.lock().unwrap().appareils.push(id);
    pousser_manette(Evenement::Appareil { id, nom, descripteur, axes });
}

/// Les manettes qu'Android ne connaît plus sont parties.
fn releve_manettes() {
    let ids = SHARED.lock().unwrap().appareils.clone();
    if ids.is_empty() {
        return;
    }
    unsafe {
        let Some(env) = env_java() else { return };
        let cadre: unsafe extern "C" fn(Jni, i32) -> i32 = jni_fn(env, 19);
        let fermer: unsafe extern "C" fn(Jni, Obj) -> Obj = jni_fn(env, 20);
        if cadre(env, 32) != 0 {
            return;
        }
        let partis: Vec<i32> = ids.into_iter().filter(|&i| appareil_java(env, i).is_none()).collect();
        fermer(env, null_mut());
        if !partis.is_empty() {
            SHARED.lock().unwrap().appareils.retain(|i| !partis.contains(i));
            for i in partis {
                pousser_manette(Evenement::Parti(i));
            }
        }
    }
}

unsafe extern "C" fn on_queue_created(_: *mut ANativeActivity, queue: *mut AInputQueue) {
    AInputQueue_attachLooper(queue, ALooper_forThread(), 1, Some(on_input), queue);
}

unsafe extern "C" fn on_queue_destroyed(_: *mut ANativeActivity, queue: *mut AInputQueue) {
    AInputQueue_detachLooper(queue);
}

/// Un pixel indépendant de la densité (dp), en pixels réels.
unsafe fn density(activity: *mut ANativeActivity) -> f32 {
    let c = AConfiguration_new();
    AConfiguration_fromAssetManager(c, (*activity).asset_manager);
    let d = AConfiguration_getDensity(c);
    AConfiguration_delete(c);
    // 0 : non précisée, 0xFFFF / 0xFFFE : valeurs spéciales
    if (1..=1000).contains(&d) {
        d as f32 / 160.0
    } else {
        2.0
    }
}

/// Point d'entrée appelé par `android.app.NativeActivity`.
///
/// # Safety
/// Appelé uniquement par Android, avec une activité valide.
#[no_mangle]
pub unsafe extern "C" fn ANativeActivity_onCreate(activity: *mut ANativeActivity, _: *mut c_void, _: usize) {
    let fichier = fichier_recu(activity);
    relier_java(activity);
    {
        let mut s = SHARED.lock().unwrap();
        s.dp = density(activity);
        let p = (*activity).internal_data_path;
        if !p.is_null() {
            s.interne = Some(PathBuf::from(std::ffi::CStr::from_ptr(p as *const _).to_string_lossy().into_owned()));
        }
        if let Some((mut l, _)) = fichier {
            s.fichier = l.pop();
        }
    }
    let cb = &mut *(*activity).callbacks;
    cb.on_native_window_created = Some(on_window_created);
    cb.on_native_window_redraw_needed = Some(on_window_changed);
    cb.on_native_window_resized = Some(on_window_changed);
    cb.on_native_window_destroyed = Some(on_window_destroyed);
    cb.on_input_queue_created = Some(on_queue_created);
    cb.on_input_queue_destroyed = Some(on_queue_destroyed);
    spawn_render_thread();
}
