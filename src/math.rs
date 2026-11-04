//! Matrices 4×4 par colonnes, comme les attend OpenGL.

pub type Mat4 = [f32; 16];
pub type Vec3 = [f32; 3];

pub fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut m = [0.0; 16];
    for c in 0..4 {
        for r in 0..4 {
            m[c * 4 + r] = (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum();
        }
    }
    m
}

pub fn scale(s: f32) -> Mat4 {
    [s, 0., 0., 0., 0., s, 0., 0., 0., 0., s, 0., 0., 0., 0., 1.]
}

pub fn perspective(fovy: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let f = 1.0 / (fovy / 2.0).tan();
    let d = 1.0 / (near - far);
    [f / aspect, 0., 0., 0., 0., f, 0., 0., 0., 0., (far + near) * d, -1., 0., 0., 2.0 * far * near * d, 0.]
}

/// Projection orthographique de la boîte `l`–`r`, `b`–`t`, `n`–`f` (distances devant l'œil).
pub fn ortho(l: f32, r: f32, b: f32, t: f32, n: f32, f: f32) -> Mat4 {
    [2.0 / (r - l), 0., 0., 0., 0., 2.0 / (t - b), 0., 0., 0., 0., -2.0 / (f - n), 0., -(r + l) / (r - l), -(t + b) / (t - b), -(f + n) / (f - n), 1.]
}

/// `m` appliquée au point `p` : x, y, z et w.
pub fn appliquer(m: &Mat4, p: Vec3) -> [f32; 4] {
    [0, 1, 2, 3].map(|r| m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r])
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub fn normalize(a: Vec3) -> Vec3 {
    let l = dot(a, a).sqrt().max(1e-6);
    [a[0] / l, a[1] / l, a[2] / l]
}

pub fn look_at(eye: Vec3, target: Vec3, up: Vec3) -> Mat4 {
    let f = normalize(sub(target, eye));
    let s = normalize(cross(f, up));
    let u = cross(s, f);
    [s[0], u[0], -f[0], 0., s[1], u[1], -f[1], 0., s[2], u[2], -f[2], 0., -dot(s, eye), -dot(u, eye), dot(f, eye), 1.]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identite_et_camera() {
        let id = scale(1.0);
        assert_eq!(mul(&id, &id), id);
        let v = look_at([0., 0., 5.], [0., 0., 0.], [0., 1., 0.]);
        assert_eq!(v[14], -5.0); // l'origine est 5 unités devant la caméra
    }
}
