//! Les images des textures : PNG ou JPEG, décodées en RVBA 8 bits, réduites de moitié tant
//! qu'un côté dépasse `COTE_MAX`. Comme `texture.rs` de Rustcraft.

/// Le plus grand côté d'une texture envoyée au processeur graphique.
pub const COTE_MAX: u32 = 1024;

/// Une image en RVBA, ligne du haut d'abord.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub l: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    /// Un de ses pixels n'est pas opaque.
    pub fn transparente(&self) -> bool {
        self.rgba.chunks_exact(4).any(|p| p[3] < 255)
    }
}

pub fn decoder(d: &[u8]) -> Result<Image, String> {
    let i = if d.starts_with(&[0x89, b'P', b'N', b'G']) {
        png(d)?
    } else if d.starts_with(&[0xFF, 0xD8]) {
        jpeg(d)?
    } else {
        return Err("image ni PNG ni JPEG".into());
    };
    Ok(reduire(i))
}

fn png(d: &[u8]) -> Result<Image, String> {
    use png::{ColorType, Decoder, Transformations};
    let mut p = Decoder::new(d);
    p.set_transformations(Transformations::EXPAND | Transformations::STRIP_16);
    let mut r = p.read_info().map_err(|e| format!("PNG : {e}"))?;
    let mut b = vec![0; r.output_buffer_size()];
    let i = r.next_frame(&mut b).map_err(|e| format!("PNG : {e}"))?;
    b.truncate(i.buffer_size());
    let rgba = match i.color_type {
        ColorType::Rgba => b,
        ColorType::Rgb => b.chunks_exact(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        ColorType::GrayscaleAlpha => b.chunks_exact(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        ColorType::Grayscale => b.iter().flat_map(|c| [*c, *c, *c, 255]).collect(),
        ColorType::Indexed => return Err("PNG : palette non développée".into()),
    };
    Ok(Image { l: i.width, h: i.height, rgba })
}

fn jpeg(d: &[u8]) -> Result<Image, String> {
    use jpeg_decoder::{Decoder, PixelFormat};
    let mut j = Decoder::new(d);
    let b = j.decode().map_err(|e| format!("JPEG : {e}"))?;
    let info = j.info().ok_or("JPEG sans en-tête")?;
    let rgba = match info.pixel_format {
        PixelFormat::RGB24 => b.chunks_exact(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        PixelFormat::L8 => b.iter().flat_map(|c| [*c, *c, *c, 255]).collect(),
        PixelFormat::L16 => b.chunks_exact(2).flat_map(|c| [c[0], c[0], c[0], 255]).collect(),
        PixelFormat::CMYK32 => b
            .chunks_exact(4)
            .flat_map(|c| {
                let k = c[3] as u32;
                [(c[0] as u32 * k / 255) as u8, (c[1] as u32 * k / 255) as u8, (c[2] as u32 * k / 255) as u8, 255]
            })
            .collect(),
    };
    Ok(Image { l: info.width as u32, h: info.height as u32, rgba })
}

/// Divise par deux tant qu'un côté dépasse `COTE_MAX`.
fn reduire(mut i: Image) -> Image {
    while i.l.max(i.h) > COTE_MAX {
        let (l, h) = ((i.l / 2).max(1), (i.h / 2).max(1));
        let mut r = Vec::with_capacity((l * h * 4) as usize);
        for y in 0..h {
            for x in 0..l {
                for c in 0..4 {
                    let mut s = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (px, py) = ((x * 2 + dx).min(i.l - 1), (y * 2 + dy).min(i.h - 1));
                        s += i.rgba[((py * i.l + px) * 4 + c) as usize] as u32;
                    }
                    r.push((s / 4) as u8);
                }
            }
        }
        i = Image { l, h, rgba: r };
    }
    i
}

/// Une image PNG de `l` × `h`, chaque pixel donné par `f` : pour le mannequin et les essais.
pub fn png_de(l: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let mut o = Vec::new();
    let mut e = ::png::Encoder::new(&mut o, l, h);
    e.set_color(::png::ColorType::Rgba);
    e.set_depth(::png::BitDepth::Eight);
    let mut w = e.write_header().expect("en-tête PNG");
    let d: Vec<u8> = (0..h).flat_map(|y| (0..l).map(move |x| (x, y))).flat_map(|(x, y)| f(x, y)).collect();
    w.write_image_data(&d).expect("image PNG");
    w.finish().expect("fin du PNG");
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_png_se_relit_et_une_grande_image_se_reduit() {
        let d = png_de(3, 2, |x, y| [x as u8 * 80, y as u8 * 200, 7, if x == 2 { 128 } else { 255 }]);
        let i = decoder(&d).unwrap();
        assert_eq!((i.l, i.h), (3, 2));
        assert_eq!(&i.rgba[..8], &[0, 0, 7, 255, 80, 0, 7, 255]);
        assert!(i.transparente());
        let g = decoder(&png_de(3000, 10, |_, _| [9, 9, 9, 255])).unwrap();
        assert_eq!((g.l, g.h), (750, 2));
        assert!(!g.transparente());
        assert!(decoder(b"rien").is_err());
    }
}
