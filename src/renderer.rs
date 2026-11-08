//! Tout OpenGL est ici. Le moteur de rendu ne lit que la `Scene` du moteur : il charge les
//! maillages et les textures nouveaux ou changés, libère ceux qui ont disparu, puis :
//!
//! 1. dessine la carte d'ombre, la profondeur vue du soleil, du décor et des personnages ;
//! 2. dessine chaque partie de chaque objet, triées par programme puis par texture, les
//!    opaques d'abord ; les parties mêlées viennent après, du plus loin au plus proche,
//!    sans écrire la profondeur ;
//! 3. dessine l'interface par-dessus.

use crate::draw::{DrawList, Vertex};
use crate::gles::GlApi;
use crate::moteur::ressources::Maillage3d;
use crate::moteur::{Alpha, Format, MaillageId, Partie, Scene, TextureId};
use crate::personnage::OS_MAX;
use std::collections::HashMap;
use crate::text::{Text, ATLAS};
use std::ffi::{c_char, c_void, CString};
use std::mem::{offset_of, size_of};

const TRIANGLES: u32 = 4;
const FLOAT: u32 = 0x1406;
const UBYTE: u32 = 0x1401;
const ARRAY_BUFFER: u32 = 0x8892;
const STATIC_DRAW: u32 = 0x88E4;
const TEXTURE_2D: u32 = 0x0DE1;
const RED: u32 = 0x1903;
const BLEND: u32 = 0x0BE2;
const DEPTH_TEST: u32 = 0x0B71;
const CULL_FACE: u32 = 0x0B44;
const BACK: u32 = 0x0405;
const SCISSOR: u32 = 0x0C11;
const COLOR_BIT: u32 = 0x4000;
const DEPTH_BIT: u32 = 0x0100;
const FRAMEBUFFER: u32 = 0x8D40;
const DEPTH_ATTACHMENT: u32 = 0x8D00;
const POLYGON_OFFSET_FILL: u32 = 0x8037;

const UI_VS: &str = "#version 300 es
precision highp float;
layout(location=0) in vec2 a_pos;
layout(location=1) in vec2 a_local;
layout(location=2) in vec4 a_shape;
layout(location=3) in vec2 a_uv;
layout(location=4) in vec4 a_color;
uniform vec2 u_size;
out vec2 v_local; out vec4 v_shape; out vec2 v_uv; out vec4 v_color;
void main() {
  v_local = a_local; v_shape = a_shape; v_uv = a_uv; v_color = a_color;
  vec2 p = a_pos / u_size * 2.0 - 1.0;
  gl_Position = vec4(p.x, -p.y, 0.0, 1.0);
}";

// highp : en mediump (16 bits sur Mali), les distances en pixels manquent de précision.
const UI_FS: &str = "#version 300 es
precision highp float;
in vec2 v_local; in vec4 v_shape; in vec2 v_uv; in vec4 v_color;
uniform sampler2D u_atlas;
out vec4 o_color;
void main() {
  // Une image : ses couleurs telles quelles
  if (v_shape.z < -1.5) {
    vec4 t = texture(u_atlas, v_uv);
    o_color = vec4(t.rgb * t.a * v_color.a, t.a * v_color.a);
    return;
  }
  float a;
  if (v_shape.z < 0.0) {
    a = texture(u_atlas, v_uv).r;
  } else {
    vec2 q = abs(v_local) - v_shape.xy + v_shape.z;
    float d = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - v_shape.z;
    float s = max(v_shape.w, 0.6);
    a = 1.0 - smoothstep(-s, s, d);
  }
  a *= v_color.a;
  o_color = vec4(v_color.rgb * a, a);
}";

/// Éclairage commun : un soleil et ses ombres portées, le ciel, et le brouillard qui fond le
/// lointain dans le ciel.
///
/// L'ombre se lit dans la carte de profondeur vue du soleil, en 3 × 3 lectures comparées
/// (chacune déjà filtrée par le processeur graphique). Le point est d'abord décalé le long
/// de sa normale, pour qu'une surface ne s'ombre pas elle-même. Vers le bord de la carte,
/// l'ombre s'efface.
const LUMIERE: &str = "
uniform vec3 u_oeil;
uniform vec3 u_ciel;
uniform float u_brouillard;
uniform vec3 u_soleil;
uniform vec3 u_lumiere;
uniform vec3 u_ambiance;
uniform highp sampler2DShadow u_ombre;
uniform mat4 u_lvp;
uniform float u_ombres;
float ombre(vec3 p, vec3 n) {
  if (u_ombres <= 0.0) return 1.0;
  vec4 l = u_lvp * vec4(p + n * 0.03, 1.0);
  vec3 c = l.xyz / l.w * 0.5 + 0.5;
  float bord = max(abs(c.x - 0.5), abs(c.y - 0.5)) * 2.0;
  if (bord >= 1.0 || c.z >= 1.0) return 1.0;
  float t = 1.0 / #CARTE.0;
  float s = 0.0;
  for (int i = -1; i <= 1; i++)
    for (int j = -1; j <= 1; j++)
      s += texture(u_ombre, vec3(c.xy + vec2(float(i), float(j)) * t, c.z - 0.0002));
  return mix(s / 9.0, 1.0, smoothstep(0.85, 1.0, bord));
}
vec3 eclairer(vec3 base, vec3 n, vec3 p) {
  float d = max(dot(n, u_soleil), 0.0);
  if (d > 0.0 && u_lumiere.r > 0.0) d *= ombre(p, n);
  float ciel = 0.5 + 0.5 * n.y;
  vec3 c = base * ((0.30 + 0.25 * ciel) * u_ambiance + 0.60 * d * u_lumiere);
  // Le lointain se fond dans le ciel : à u_brouillard, il n'en reste que 2 %
  float loin = 2.0 * distance(p, u_oeil) / u_brouillard;
  float f = 1.0 - exp(-loin * loin);
  return mix(c, u_ciel, clamp(f, 0.0, 1.0));
}";

/// Le terrain : sa couleur et un quadrillage de 2 et de 16 m (des diviseurs de la tuile :
/// il ne saute pas quand l'origine se déplace). Avec `u_isolignes`, des courbes de niveau
/// à la place : un trait tous les `u_isolignes` mètres d'altitude, un plus fort tous les
/// cinq, d'un pixel quelle que soit la distance.
const TERRAIN_FS: &str = "#version 300 es
precision highp float;
in vec3 v_monde; in vec3 v_normale; in vec3 v_couleur;
out vec4 o_color;
uniform float u_isolignes;
uniform vec4 u_trou;
#LUMIERE
float niveau(float y, float pas, float large) {
  float q = y / pas;
  float g = abs(fract(q - 0.5) - 0.5) / max(fwidth(q), 1e-4);
  return 1.0 - clamp(g / large, 0.0, 1.0);
}
float trait(vec2 c, float pas) {
  vec2 q = c / pas;
  vec2 f = fwidth(q);
  vec2 g = abs(fract(q - 0.5) - 0.5) / max(f, vec2(1e-4));
  // Au loin, quand les traits se serrent, ils s'effacent
  return (1.0 - min(min(g.x, g.y), 1.0)) * clamp(1.5 - max(f.x, f.y) * 3.0, 0.0, 1.0);
}
void main() {
  vec2 c = v_monde.xz;
  if (all(greaterThan(c, u_trou.xy)) && all(lessThan(c, u_trou.zw))) discard;
  vec3 base = v_couleur;
  if (u_isolignes > 0.0) {
    float y = v_monde.y;
    base = mix(base, base * 0.55, niveau(y, u_isolignes, 1.0) * 0.75);
    base = mix(base, vec3(0.30, 0.20, 0.12), niveau(y, u_isolignes * 5.0, 1.6) * 0.9);
  } else {
    base = mix(base, base * 0.86, trait(c, 2.0) * 0.5);
    base = mix(base, base * 0.74, trait(c, 16.0) * 0.8);
  }
  o_color = vec4(eclairer(base, normalize(v_normale), v_monde), 1.0);
}";

const BLOC_VS: &str = "#version 300 es
precision highp float;
layout(location=0) in vec3 a_pos;
layout(location=1) in vec3 a_normale;
layout(location=2) in vec3 a_couleur;
uniform mat4 u_place;
uniform mat4 u_vp;
out vec3 v_monde; out vec3 v_normale; out vec3 v_couleur;
void main() {
  vec4 p = u_place * vec4(a_pos, 1.0);
  v_monde = p.xyz; v_normale = mat3(u_place) * a_normale; v_couleur = a_couleur;
  gl_Position = u_vp * p;
}";

const COULEUR_FS: &str = "#version 300 es
precision highp float;
in vec3 v_monde; in vec3 v_normale; in vec3 v_couleur;
out vec4 o_color;
uniform float u_choisi;
#LUMIERE
void main() {
  vec3 c = eclairer(v_couleur, normalize(v_normale), v_monde);
  // Choisi dans l'éditeur : teinté d'orangé, plus clair sur les arêtes vues de biais
  float bord = 1.0 - abs(dot(normalize(v_normale), normalize(u_oeil - v_monde)));
  c = mix(c, vec3(1.0, 0.58, 0.20), u_choisi * (0.22 + 0.38 * bord));
  o_color = vec4(c, 1.0);
}";

/// La peau : chaque sommet suit jusqu'à quatre os, mêlés selon ses poids.
const PEAU_VS: &str = "#version 300 es
precision highp float;
layout(location=0) in vec3 a_pos;
layout(location=1) in vec3 a_normale;
layout(location=2) in vec4 a_os;
layout(location=3) in vec4 a_poids;
layout(location=4) in vec3 a_couleur;
layout(location=5) in vec2 a_uv;
uniform mat4 u_os[#OS];
uniform mat4 u_place;
uniform mat4 u_vp;
out vec3 v_monde; out vec3 v_normale; out vec3 v_couleur; out vec2 v_uv;
void main() {
  mat4 s = a_poids.x * u_os[int(a_os.x + 0.5)] + a_poids.y * u_os[int(a_os.y + 0.5)]
         + a_poids.z * u_os[int(a_os.z + 0.5)] + a_poids.w * u_os[int(a_os.w + 0.5)];
  mat4 m = u_place * s;
  vec4 p = m * vec4(a_pos, 1.0);
  v_monde = p.xyz;
  v_normale = mat3(m) * a_normale;
  v_couleur = a_couleur;
  v_uv = a_uv;
  gl_Position = u_vp * p;
}";

/// Un modèle importé : statique, avec ses coordonnées de texture.
const TEXTURE_VS: &str = "#version 300 es
precision highp float;
layout(location=0) in vec3 a_pos;
layout(location=1) in vec3 a_normale;
layout(location=2) in vec3 a_couleur;
layout(location=3) in vec2 a_uv;
uniform mat4 u_place;
uniform mat4 u_vp;
out vec3 v_monde; out vec3 v_normale; out vec3 v_couleur; out vec2 v_uv;
void main() {
  vec4 p = u_place * vec4(a_pos, 1.0);
  v_monde = p.xyz; v_normale = mat3(u_place) * a_normale; v_couleur = a_couleur; v_uv = a_uv;
  gl_Position = u_vp * p;
}";

/// Un matériau : la texture (blanche s'il n'en a pas) teintée de la couleur des sommets,
/// son opacité, et la coupure nette à mi-opacité d'un matériau masqué.
const MATIERE_FS: &str = "#version 300 es
precision highp float;
in vec3 v_monde; in vec3 v_normale; in vec3 v_couleur; in vec2 v_uv;
uniform sampler2D u_texture;
uniform float u_opacite;
uniform float u_masque;
uniform float u_choisi;
out vec4 o_color;
#LUMIERE
void main() {
  vec4 t = texture(u_texture, v_uv);
  float a = t.a * u_opacite;
  if (u_masque > 0.0 && a < 0.5) discard;
  vec3 n = normalize(v_normale);
  if (!gl_FrontFacing) n = -n;
  vec3 c = eclairer(v_couleur * t.rgb, n, v_monde);
  float bord = 1.0 - abs(dot(n, normalize(u_oeil - v_monde)));
  c = mix(c, vec3(1.0, 0.58, 0.20), u_choisi * (0.22 + 0.38 * bord));
  o_color = vec4(c, u_masque > 0.0 ? 1.0 : a);
}";

/// La mer : un carré à plat autour de l'œil, jusqu'à l'horizon.
const EAU_VS: &str = "#version 300 es
precision highp float;
layout(location=0) in vec2 a_coin;
uniform mat4 u_vp;
uniform vec3 u_centre;
uniform float u_etendue;
out vec3 v_monde;
void main() {
  vec3 p = vec3(u_centre.x + a_coin.x * u_etendue, u_centre.y, u_centre.z + a_coin.y * u_etendue);
  v_monde = p;
  gl_Position = u_vp * vec4(p, 1.0);
}";

/// La houle (quelques ondes qui penchent la normale, effacées au loin), le ciel qui s'y
/// reflète d'autant plus qu'on la voit de biais, l'éclat du soleil ; plus transparente
/// vue d'en haut.
const EAU_FS: &str = "#version 300 es
precision highp float;
in vec3 v_monde;
uniform float u_temps;
out vec4 o_color;
#LUMIERE
void main() {
  vec2 p = v_monde.xz;
  vec2 g = vec2(0.0);
  vec4 ondes[4] = vec4[4](vec4(0.8, 0.6, 23.0, 0.10), vec4(-0.5, 0.86, 11.0, 0.05), vec4(0.2, -0.98, 5.3, 0.025), vec4(-0.9, -0.4, 2.1, 0.01));
  for (int i = 0; i < 4; i++) {
    vec2 k = ondes[i].xy * (6.2832 / ondes[i].z);
    float w = sqrt(9.81 * length(k));
    g += ondes[i].w * length(k) * cos(dot(k, p) - w * u_temps) * ondes[i].xy;
  }
  float d = distance(v_monde, u_oeil);
  g *= 1.0 - smoothstep(60.0, 600.0, d);
  vec3 n = normalize(vec3(-g.x, 1.0, -g.y));
  vec3 v = normalize(u_oeil - v_monde);
  float f = 0.02 + 0.98 * pow(1.0 - max(dot(n, v), 0.0), 5.0);
  vec3 c = eclairer(vec3(0.10, 0.30, 0.36), n, v_monde);
  c = mix(c, u_ciel * 1.05, f);
  vec3 r = reflect(-v, n);
  c += vec3(1.0, 0.95, 0.85) * u_lumiere * pow(max(dot(r, u_soleil), 0.0), 180.0) * 1.5;
  o_color = vec4(c, mix(0.62, 0.97, f));
}";

/// La carte d'ombre : seule la profondeur compte.
const OMBRE_BLOC_VS: &str = "#version 300 es
precision highp float;
layout(location=0) in vec3 a_pos;
uniform mat4 u_place;
uniform mat4 u_vp;
void main() {
  gl_Position = u_vp * (u_place * vec4(a_pos, 1.0));
}";

const OMBRE_PEAU_VS: &str = "#version 300 es
precision highp float;
layout(location=0) in vec3 a_pos;
layout(location=2) in vec4 a_os;
layout(location=3) in vec4 a_poids;
uniform mat4 u_os[#OS];
uniform mat4 u_place;
uniform mat4 u_vp;
void main() {
  mat4 s = a_poids.x * u_os[int(a_os.x + 0.5)] + a_poids.y * u_os[int(a_os.y + 0.5)]
         + a_poids.z * u_os[int(a_os.z + 0.5)] + a_poids.w * u_os[int(a_os.w + 0.5)];
  gl_Position = u_vp * (u_place * s * vec4(a_pos, 1.0));
}";

const OMBRE_FS: &str = "#version 300 es
precision mediump float;
out vec4 o_color;
void main() { o_color = vec4(1.0); }";

/// Côté de la carte d'ombre, en texels.
pub const CARTE: u32 = 2048;

struct Tampon {
    vao: u32,
    vbo: u32,
    n: i32,
}

/// La carte d'ombre : sa texture de profondeur et son tampon d'image ; absente si le
/// processeur graphique la refuse.
struct Carte {
    texture: u32,
    fbo: u32,
}

pub struct Renderer {
    gl: GlApi,
    ui_program: u32,
    u_size: i32,
    ui_vao: u32,
    ui_vbo: u32,
    atlas: u32,
    terrain: u32,
    bloc: u32,
    peau: u32,
    /// Les modèles importés, texturés.
    matiere: u32,
    ombre_bloc: u32,
    ombre_peau: u32,
    /// La mer, et son carré.
    eau: u32,
    eau_vao: u32,
    blanc: u32,
    carte: Option<Carte>,
    /// Les maillages et les textures chargés, avec leur version.
    maillages: HashMap<MaillageId, (u64, Format, Tampon)>,
    textures: HashMap<TextureId, (u64, u32)>,
    lieux: std::cell::RefCell<HashMap<(u32, &'static str), i32>>,
    /// Ce que la dernière image a dessiné : changements de programme, de texture, parties.
    pub bilan: Bilan,
}

/// Ce qu'une image a coûté au processeur graphique.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bilan {
    pub parties: usize,
    pub programmes: usize,
    pub textures: usize,
    pub ombres: usize,
}

/// Une partie à dessiner : l'objet, la partie, son programme, sa texture, son alpha et sa
/// distance à l'œil.
#[derive(Clone, Debug)]
struct Appel {
    dessin: usize,
    partie: Partie,
    programme: u32,
    texture: u32,
    alpha: Alpha,
    distance: f32,
}

/// Les appels, triés : opaques et masqués par programme, puis texture ; les mêlés ensuite,
/// du plus loin au plus proche.
fn trier(l: &mut [Appel]) {
    l.sort_by(|a, b| {
        let (ma, mb) = (a.alpha == Alpha::Melange, b.alpha == Alpha::Melange);
        ma.cmp(&mb).then_with(|| if ma { b.distance.total_cmp(&a.distance) } else { (a.programme, a.texture, a.alpha).cmp(&(b.programme, b.texture, b.alpha)) })
    });
}

fn cstr(s: &str) -> CString {
    CString::new(s).unwrap()
}

unsafe fn compile(gl: &GlApi, kind: u32, src: &str) -> Result<u32, String> {
    let s = (gl.glCreateShader)(kind);
    let c = cstr(src);
    let p = c.as_ptr();
    (gl.glShaderSource)(s, 1, &p, std::ptr::null());
    (gl.glCompileShader)(s);
    let mut ok = 0;
    (gl.glGetShaderiv)(s, 0x8B81, &mut ok);
    if ok == 0 {
        let mut log = [0u8; 1024];
        (gl.glGetShaderInfoLog)(s, 1024, std::ptr::null_mut(), log.as_mut_ptr() as *mut c_char);
        return Err(String::from_utf8_lossy(&log).trim_end_matches('\0').to_string());
    }
    Ok(s)
}

unsafe fn program(gl: &GlApi, vs: &str, fs: &str) -> Result<u32, String> {
    let vs = vs.replace("#OS", &OS_MAX.to_string());
    let fs = fs.replace("#LUMIERE", LUMIERE).replace("#CARTE", &CARTE.to_string());
    let p = (gl.glCreateProgram)();
    (gl.glAttachShader)(p, compile(gl, 0x8B31, &vs)?);
    (gl.glAttachShader)(p, compile(gl, 0x8B30, &fs)?);
    (gl.glLinkProgram)(p);
    let mut ok = 0;
    (gl.glGetProgramiv)(p, 0x8B82, &mut ok);
    if ok == 0 {
        let mut log = [0u8; 1024];
        (gl.glGetProgramInfoLog)(p, 1024, std::ptr::null_mut(), log.as_mut_ptr() as *mut c_char);
        return Err(String::from_utf8_lossy(&log).trim_end_matches('\0').to_string());
    }
    Ok(p)
}

/// Un tampon de sommets à plat : `attributs` donne, pour chaque emplacement, son nombre
/// de flottants.
unsafe fn tampon(gl: &GlApi, v: &[f32], attributs: &[i32]) -> Tampon {
    let stride: i32 = attributs.iter().sum();
    let (mut vao, mut vbo) = (0, 0);
    (gl.glGenVertexArrays)(1, &mut vao);
    (gl.glGenBuffers)(1, &mut vbo);
    (gl.glBindVertexArray)(vao);
    (gl.glBindBuffer)(ARRAY_BUFFER, vbo);
    (gl.glBufferData)(ARRAY_BUFFER, (v.len() * 4) as isize, v.as_ptr() as *const c_void, STATIC_DRAW);
    let mut off = 0;
    for (i, n) in attributs.iter().enumerate() {
        (gl.glEnableVertexAttribArray)(i as u32);
        (gl.glVertexAttribPointer)(i as u32, *n, FLOAT, 0, stride * 4, (off * 4) as *const c_void);
        off += n;
    }
    (gl.glBindVertexArray)(0);
    Tampon { vao, vbo, n: v.len() as i32 / stride }
}

unsafe fn liberer(gl: &GlApi, t: &Tampon) {
    (gl.glDeleteVertexArrays)(1, &t.vao);
    (gl.glDeleteBuffers)(1, &t.vbo);
}

unsafe fn texture_rgba(gl: &GlApi, l: i32, h: i32, rgba: &[u8]) -> u32 {
    let mut t = 0;
    (gl.glGenTextures)(1, &mut t);
    (gl.glBindTexture)(TEXTURE_2D, t);
    (gl.glPixelStorei)(0x0CF5, 1);
    (gl.glTexImage2D)(TEXTURE_2D, 0, 0x8058, l, h, 0, 0x1908, UBYTE, rgba.as_ptr() as *const c_void);
    (gl.glGenerateMipmap)(TEXTURE_2D);
    // Filtrage trilinéaire, et la texture se répète
    for (k, v) in [(0x2801, 0x2703), (0x2800, 0x2601), (0x2802, 0x2901), (0x2803, 0x2901)] {
        (gl.glTexParameteri)(TEXTURE_2D, k, v);
    }
    t
}

/// La carte d'ombre : une texture de profondeur comparée, et son tampon d'image.
unsafe fn carte(gl: &GlApi) -> Option<Carte> {
    let mut texture = 0;
    (gl.glGenTextures)(1, &mut texture);
    (gl.glBindTexture)(TEXTURE_2D, texture);
    // DEPTH_COMPONENT24, DEPTH_COMPONENT, UNSIGNED_INT
    (gl.glTexImage2D)(TEXTURE_2D, 0, 0x81A6, CARTE as i32, CARTE as i32, 0, 0x1902, 0x1405, std::ptr::null());
    // Filtrage linéaire des comparaisons, bords étirés, comparaison « plus près ou égal »
    for (k, v) in [(0x2801, 0x2601), (0x2800, 0x2601), (0x2802, 0x812F), (0x2803, 0x812F), (0x884C, 0x884E), (0x884D, 0x0203)] {
        (gl.glTexParameteri)(TEXTURE_2D, k, v);
    }
    let mut fbo = 0;
    (gl.glGenFramebuffers)(1, &mut fbo);
    (gl.glBindFramebuffer)(FRAMEBUFFER, fbo);
    (gl.glFramebufferTexture2D)(FRAMEBUFFER, DEPTH_ATTACHMENT, TEXTURE_2D, texture, 0);
    let aucun = 0u32;
    (gl.glDrawBuffers)(1, &aucun);
    (gl.glReadBuffer)(0);
    let complet = (gl.glCheckFramebufferStatus)(FRAMEBUFFER) == 0x8CD5;
    (gl.glBindFramebuffer)(FRAMEBUFFER, 0);
    if complet {
        Some(Carte { texture, fbo })
    } else {
        (gl.glDeleteTextures)(1, &texture);
        None
    }
}

impl Renderer {
    /// À créer avec un contexte courant.
    pub fn new(gl: GlApi) -> Result<Renderer, String> {
        unsafe {
            let ui_program = program(&gl, UI_VS, UI_FS)?;
            let terrain = program(&gl, BLOC_VS, TERRAIN_FS).map_err(|e| format!("terrain : {e}"))?;
            let bloc = program(&gl, BLOC_VS, COULEUR_FS).map_err(|e| format!("décor : {e}"))?;
            let peau = program(&gl, PEAU_VS, MATIERE_FS).map_err(|e| format!("peau : {e}"))?;
            let matiere = program(&gl, TEXTURE_VS, MATIERE_FS).map_err(|e| format!("modèle : {e}"))?;
            let ombre_bloc = program(&gl, OMBRE_BLOC_VS, OMBRE_FS).map_err(|e| format!("ombre : {e}"))?;
            let ombre_peau = program(&gl, OMBRE_PEAU_VS, OMBRE_FS).map_err(|e| format!("ombre de la peau : {e}"))?;
            let eau = program(&gl, EAU_VS, EAU_FS).map_err(|e| format!("mer : {e}"))?;
            let eau_vao = tampon(&gl, &[-1.0, -1.0, 1.0, 1.0, 1.0, -1.0, -1.0, -1.0, -1.0, 1.0, 1.0, 1.0], &[2]).vao;
            let u_size = (gl.glGetUniformLocation)(ui_program, cstr("u_size").as_ptr());

            let (mut ui_vao, mut ui_vbo) = (0, 0);
            (gl.glGenVertexArrays)(1, &mut ui_vao);
            (gl.glGenBuffers)(1, &mut ui_vbo);
            (gl.glBindVertexArray)(ui_vao);
            (gl.glBindBuffer)(ARRAY_BUFFER, ui_vbo);
            let stride = size_of::<Vertex>() as i32;
            let attrs: [(u32, i32, u32, u8, usize); 5] = [
                (0, 2, FLOAT, 0, offset_of!(Vertex, pos)),
                (1, 2, FLOAT, 0, offset_of!(Vertex, local)),
                (2, 4, FLOAT, 0, offset_of!(Vertex, shape)),
                (3, 2, FLOAT, 0, offset_of!(Vertex, uv)),
                (4, 4, UBYTE, 1, offset_of!(Vertex, color)),
            ];
            for (i, n, t, norm, off) in attrs {
                (gl.glEnableVertexAttribArray)(i);
                (gl.glVertexAttribPointer)(i, n, t, norm, stride, off as *const c_void);
            }
            (gl.glBindVertexArray)(0);

            let mut atlas = 0;
            (gl.glGenTextures)(1, &mut atlas);
            (gl.glBindTexture)(TEXTURE_2D, atlas);
            (gl.glPixelStorei)(0x0CF5, 1);
            (gl.glTexImage2D)(TEXTURE_2D, 0, 0x8229, ATLAS as i32, ATLAS as i32, 0, RED, UBYTE, std::ptr::null());
            for (k, v) in [(0x2801, 0x2601), (0x2800, 0x2601), (0x2802, 0x812F), (0x2803, 0x812F)] {
                (gl.glTexParameteri)(TEXTURE_2D, k, v);
            }
            let blanc = texture_rgba(&gl, 1, 1, &[255, 255, 255, 255]);
            let carte = carte(&gl);

            Ok(Renderer {
                gl,
                ui_program,
                u_size,
                ui_vao,
                ui_vbo,
                atlas,
                terrain,
                bloc,
                peau,
                matiere,
                ombre_bloc,
                ombre_peau,
                eau,
                eau_vao,
                blanc,
                carte,
                maillages: HashMap::new(),
                textures: HashMap::new(),
                lieux: Default::default(),
                bilan: Bilan::default(),
            })
        }
    }

    pub fn gl(&self) -> &GlApi {
        &self.gl
    }

    /// Les ombres portées sont dessinées.
    pub fn a_des_ombres(&self) -> bool {
        self.carte.is_some()
    }

    unsafe fn loc(&self, p: u32, n: &'static str) -> i32 {
        *self.lieux.borrow_mut().entry((p, n)).or_insert_with(|| (self.gl.glGetUniformLocation)(p, cstr(n).as_ptr()))
    }

    unsafe fn communs(&self, p: u32, s: &Scene, vp: &[f32; 16], lvp: &[f32; 16]) {
        let gl = &self.gl;
        let o = s.camera.oeil;
        let d = s.soleil.direction;
        (gl.glUseProgram)(p);
        (gl.glUniformMatrix4fv)(self.loc(p, "u_vp"), 1, 0, vp.as_ptr());
        (gl.glUniform3f)(self.loc(p, "u_oeil"), o[0], o[1], o[2]);
        let (c, l, a) = (s.soleil.ciel, s.soleil.lumiere, s.soleil.ambiance);
        (gl.glUniform3f)(self.loc(p, "u_ciel"), c[0], c[1], c[2]);
        (gl.glUniform3f)(self.loc(p, "u_lumiere"), l[0], l[1], l[2]);
        (gl.glUniform3f)(self.loc(p, "u_ambiance"), a[0], a[1], a[2]);
        (gl.glUniform1f)(self.loc(p, "u_brouillard"), s.brouillard.max(1.0));
        (gl.glUniform1f)(self.loc(p, "u_isolignes"), s.isolignes.max(0.0));
        (gl.glUniform3f)(self.loc(p, "u_soleil"), d[0], d[1], d[2]);
        (gl.glUniformMatrix4fv)(self.loc(p, "u_lvp"), 1, 0, lvp.as_ptr());
        (gl.glUniform1f)(self.loc(p, "u_ombres"), if self.carte.is_some() { 1.0 } else { 0.0 });
        (gl.glUniform1i)(self.loc(p, "u_ombre"), 1);
        (gl.glUniform1i)(self.loc(p, "u_texture"), 0);
    }

    /// Les maillages et les textures nouveaux ou changés repartent au processeur
    /// graphique ; ceux qui ont disparu sont libérés.
    unsafe fn synchroniser(&mut self, scene: &Scene) {
        let vivants: HashMap<MaillageId, u64> = scene.maillages.iter().map(|(k, m)| (*k, m.version)).collect();
        let gl = &self.gl;
        self.maillages.retain(|k, (v, _, t)| {
            let garde = vivants.get(k) == Some(v);
            if !garde {
                liberer(gl, t);
            }
            garde
        });
        for (k, m) in &scene.maillages {
            if !self.maillages.contains_key(k) {
                self.maillages.insert(*k, (m.version, m.format, tampon(gl, &m.sommets, m.format.attributs())));
            }
        }
        let vivantes: HashMap<TextureId, u64> = scene.textures.iter().map(|(k, t)| (*k, t.version)).collect();
        self.textures.retain(|k, (v, t)| {
            let garde = vivantes.get(k) == Some(v);
            if !garde {
                (gl.glDeleteTextures)(1, t);
            }
            garde
        });
        for (k, t) in &scene.textures {
            if !self.textures.contains_key(k) {
                let i = &t.image;
                self.textures.insert(*k, (t.version, texture_rgba(gl, i.l as i32, i.h as i32, &i.rgba)));
            }
        }
    }

    unsafe fn os(&self, p: u32, d: &crate::moteur::Dessin) {
        let os: Vec<f32> = d.os.iter().take(OS_MAX).flatten().copied().collect();
        if !os.is_empty() {
            (self.gl.glUniformMatrix4fv)(self.loc(p, "u_os"), (os.len() / 16) as i32, 0, os.as_ptr());
        }
    }

    /// La carte d'ombre : le décor et les personnages, vus du soleil.
    unsafe fn dessiner_ombres(&mut self, scene: &Scene, lvp: &[f32; 16]) {
        let Some(c) = &self.carte else { return };
        let gl = &self.gl;
        (gl.glBindFramebuffer)(FRAMEBUFFER, c.fbo);
        (gl.glViewport)(0, 0, CARTE as i32, CARTE as i32);
        (gl.glDisable)(SCISSOR);
        (gl.glDepthMask)(1);
        (gl.glClear)(DEPTH_BIT);
        (gl.glEnable)(DEPTH_TEST);
        (gl.glDisable)(CULL_FACE);
        (gl.glEnable)(POLYGON_OFFSET_FILL);
        (gl.glPolygonOffset)(1.5, 3.0);
        let mut n = 0;
        // Ce qui est hors du carré des ombres n'y porte rien
        let (cx, cz, r) = (scene.soleil.centre[0], scene.soleil.centre[2], scene.soleil.rayon * 1.5 + 20.0);
        for format in [Format::Statique, Format::Texture, Format::Peau] {
            let p = if format == Format::Peau { self.ombre_peau } else { self.ombre_bloc };
            (gl.glUseProgram)(p);
            (gl.glUniformMatrix4fv)(self.loc(p, "u_vp"), 1, 0, lvp.as_ptr());
            for d in &scene.dessins {
                let Some((_, f, t)) = self.maillages.get(&d.maillage) else { continue };
                if *f != format || t.n == 0 {
                    continue;
                }
                if format == Format::Peau && ((d.place[12] - cx).abs() > r || (d.place[14] - cz).abs() > r) {
                    continue;
                }
                (gl.glUniformMatrix4fv)(self.loc(p, "u_place"), 1, 0, d.place.as_ptr());
                if format == Format::Peau {
                    self.os(p, d);
                }
                (gl.glBindVertexArray)(t.vao);
                (gl.glDrawArrays)(TRIANGLES, 0, t.n);
                n += 1;
            }
        }
        (gl.glDisable)(POLYGON_OFFSET_FILL);
        (gl.glBindFramebuffer)(FRAMEBUFFER, 0);
        self.bilan.ombres = n;
    }

    pub fn draw(&mut self, w: f32, h: f32, scene: &Scene, dl: &DrawList, text: &mut Text) {
        unsafe {
            self.synchroniser(scene);
            self.bilan = Bilan::default();
            let lvp = scene.soleil.matrice(CARTE);
            self.dessiner_ombres(scene, &lvp);

            let gl = &self.gl;
            (gl.glDisable)(SCISSOR);
            (gl.glViewport)(0, 0, w as i32, h as i32);
            (gl.glClearColor)(1.0, 1.0, 1.0, 1.0);
            (gl.glDepthMask)(1);
            (gl.glClear)(COLOR_BIT | DEPTH_BIT);
            // La 3D, dans sa fenêtre seulement ; le reste est à l'interface
            let f = scene.camera.fenetre;
            let (x, y, vw, vh) = (f[0].round() as i32, (h - f[1] - f[3]).round() as i32, f[2].round().max(0.0) as i32, f[3].round().max(0.0) as i32);
            (gl.glEnable)(SCISSOR);
            (gl.glScissor)(x, y, vw, vh);
            (gl.glViewport)(x, y, vw, vh);
            let c = scene.soleil.ciel;
            (gl.glClearColor)(c[0], c[1], c[2], 1.0);
            (gl.glClear)(COLOR_BIT);
            (gl.glEnable)(DEPTH_TEST);
            (gl.glDisable)(BLEND);
            (gl.glEnable)(CULL_FACE);
            (gl.glCullFace)(BACK);
            let vp = crate::math::mul(&scene.camera.proj, &scene.camera.vue);
            (gl.glActiveTexture)(0x84C1);
            (gl.glBindTexture)(TEXTURE_2D, self.carte.as_ref().map_or(0, |c| c.texture));
            (gl.glActiveTexture)(0x84C0);

            // Chaque partie de chaque objet, triées
            let o = scene.camera.oeil;
            let mut appels = Vec::new();
            let de_la_scene: HashMap<MaillageId, &Maillage3d> = scene.maillages.iter().map(|(k, m)| (*k, m)).collect();
            for (i, d) in scene.dessins.iter().enumerate() {
                let Some(m) = de_la_scene.get(&d.maillage).copied() else { continue };
                let Some((_, format, t)) = self.maillages.get(&d.maillage) else { continue };
                if t.n == 0 {
                    continue;
                }
                let programme = match format {
                    Format::Terrain => self.terrain,
                    Format::Statique => self.bloc,
                    Format::Peau => self.peau,
                    Format::Texture => self.matiere,
                };
                let distance = ((d.place[12] - o[0]).powi(2) + (d.place[13] - o[1]).powi(2) + (d.place[14] - o[2]).powi(2)).sqrt();
                for partie in m.parties() {
                    let texture = partie.materiau.texture.and_then(|k| self.textures.get(&k)).map_or(self.blanc, |x| x.1);
                    appels.push(Appel { dessin: i, alpha: partie.materiau.alpha, partie, programme, texture, distance });
                }
            }
            trier(&mut appels);
            let (mut programme, mut texture, mut mele) = (u32::MAX, u32::MAX, false);
            for a in &appels {
                if a.programme != programme {
                    self.communs(a.programme, scene, &vp, &lvp);
                    programme = a.programme;
                    self.bilan.programmes += 1;
                }
                if a.texture != texture {
                    (gl.glBindTexture)(TEXTURE_2D, a.texture);
                    texture = a.texture;
                    self.bilan.textures += 1;
                }
                if (a.alpha == Alpha::Melange) != mele {
                    mele = a.alpha == Alpha::Melange;
                    if mele {
                        (gl.glEnable)(BLEND);
                        (gl.glBlendFunc)(0x0302, 0x0303);
                        (gl.glDepthMask)(0);
                    }
                }
                let d = &scene.dessins[a.dessin];
                (gl.glUniformMatrix4fv)(self.loc(programme, "u_place"), 1, 0, d.place.as_ptr());
                if programme == self.terrain {
                    let t = match &scene.trou {
                        Some((k, t)) if k.contains(&d.maillage) => *t,
                        _ => [1.0, 1.0, -1.0, -1.0],
                    };
                    (gl.glUniform4f)(self.loc(programme, "u_trou"), t[0], t[1], t[2], t[3]);
                }
                if programme == self.bloc || programme == self.matiere || programme == self.peau {
                    (gl.glUniform1f)(self.loc(programme, "u_choisi"), if d.choisi { 1.0 } else { 0.0 });
                }
                if programme == self.peau || programme == self.matiere {
                    if programme == self.peau {
                        self.os(programme, d);
                    }
                    (gl.glUniform1f)(self.loc(programme, "u_opacite"), a.partie.materiau.opacite);
                    (gl.glUniform1f)(self.loc(programme, "u_masque"), if a.alpha == Alpha::Masque { 1.0 } else { 0.0 });
                    // Un matériau masqué, mêlé ou à deux côtés se voit des deux côtés
                    if a.alpha == Alpha::Opaque && !a.partie.materiau.deux_cotes {
                        (gl.glEnable)(CULL_FACE);
                    } else {
                        (gl.glDisable)(CULL_FACE);
                    }
                }
                let t = &self.maillages[&d.maillage].2;
                (gl.glBindVertexArray)(t.vao);
                (gl.glDrawArrays)(TRIANGLES, a.partie.premier as i32, (a.partie.nombre as i32).min(t.n - a.partie.premier as i32));
                self.bilan.parties += 1;
            }
            // La mer, par-dessus le fond, mêlée, des deux côtés
            if let Some(niveau) = scene.mer {
                self.communs(self.eau, scene, &vp, &lvp);
                (gl.glUniform3f)(self.loc(self.eau, "u_centre"), o[0], niveau, o[2]);
                (gl.glUniform1f)(self.loc(self.eau, "u_etendue"), 12_000.0);
                (gl.glUniform1f)(self.loc(self.eau, "u_temps"), scene.temps);
                (gl.glEnable)(BLEND);
                (gl.glBlendFunc)(0x0302, 0x0303);
                (gl.glDepthMask)(0);
                (gl.glDisable)(CULL_FACE);
                (gl.glBindVertexArray)(self.eau_vao);
                (gl.glDrawArrays)(TRIANGLES, 0, 6);
                self.bilan.parties += 1;
            }
            (gl.glDepthMask)(1);
            (gl.glDisable)(BLEND);
            (gl.glDisable)(CULL_FACE);
            (gl.glDisable)(SCISSOR);
            (gl.glViewport)(0, 0, w as i32, h as i32);

            // Interface, sur tout l'écran
            (gl.glDisable)(DEPTH_TEST);
            (gl.glEnable)(BLEND);
            (gl.glBlendFunc)(1, 0x0303); // couleurs prémultipliées
            (gl.glUseProgram)(self.ui_program);
            (gl.glUniform2f)(self.u_size, w, h);
            (gl.glActiveTexture)(0x84C0);
            (gl.glBindTexture)(TEXTURE_2D, self.atlas);
            if text.dirty {
                (gl.glTexSubImage2D)(TEXTURE_2D, 0, 0, 0, ATLAS as i32, ATLAS as i32, RED, UBYTE, text.atlas.as_ptr() as *const c_void);
                text.dirty = false;
            }
            (gl.glBindVertexArray)(self.ui_vao);
            (gl.glBindBuffer)(ARRAY_BUFFER, self.ui_vbo);
            (gl.glBufferData)(ARRAY_BUFFER, (dl.vertices.len() * size_of::<Vertex>()) as isize, dl.vertices.as_ptr() as *const c_void, 0x88E0);
            for c in &dl.cmds {
                match c.clip {
                    Some(r) => {
                        let (x, y, cw, ch) = (r.x.round() as i32, (h - r.bottom()).round() as i32, r.w.round().max(0.0) as i32, r.h.round().max(0.0) as i32);
                        (gl.glEnable)(SCISSOR);
                        (gl.glScissor)(x, y, cw, ch);
                    }
                    None => (gl.glDisable)(SCISSOR),
                }
                // Une image se lit dans sa texture, le reste dans l'atlas
                let t = c.image.and_then(|k| self.textures.get(&crate::moteur::TextureId(k))).map_or(self.atlas, |x| x.1);
                (gl.glBindTexture)(TEXTURE_2D, t);
                (gl.glDrawArrays)(TRIANGLES, c.first as i32, c.count as i32);
            }
            (gl.glBindVertexArray)(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::moteur::Materiau;

    fn appel(programme: u32, texture: u32, alpha: Alpha, distance: f32) -> Appel {
        Appel { dessin: 0, partie: Partie { premier: 0, nombre: 3, materiau: Materiau::default() }, programme, texture, alpha, distance }
    }

    #[test]
    fn les_parties_se_trient_par_programme_et_texture_et_les_melees_de_loin_en_pres() {
        let mut l = vec![
            appel(2, 5, Alpha::Melange, 3.0),
            appel(2, 7, Alpha::Opaque, 1.0),
            appel(1, 9, Alpha::Opaque, 9.0),
            appel(2, 5, Alpha::Opaque, 4.0),
            appel(2, 5, Alpha::Melange, 8.0),
            appel(1, 9, Alpha::Masque, 2.0),
        ];
        trier(&mut l);
        let r: Vec<(u32, u32, Alpha, f32)> = l.iter().map(|a| (a.programme, a.texture, a.alpha, a.distance)).collect();
        assert_eq!(
            r,
            vec![(1, 9, Alpha::Opaque, 9.0), (1, 9, Alpha::Masque, 2.0), (2, 5, Alpha::Opaque, 4.0), (2, 7, Alpha::Opaque, 1.0), (2, 5, Alpha::Melange, 8.0), (2, 5, Alpha::Melange, 3.0)]
        );
    }
}
