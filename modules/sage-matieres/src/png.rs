//! Écriture d'images PNG, sans dépendance : chaque ligne passe par le filtre « Paeth »
//! puis par un deflate simple (codes de Huffman fixes, répétitions trouvées par une
//! table de hachage), assez pour réduire nettement les textures.

/// Le PNG d'une image RVBA de `l` × `h` pixels, lignes depuis le haut.
pub fn encoder(l: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    assert_eq!(rgba.len(), (l * h * 4) as usize);
    let mut brut = Vec::with_capacity(rgba.len() + h as usize);
    let ligne = l as usize * 4;
    for y in 0..h as usize {
        brut.push(4); // Paeth
        let courante = &rgba[y * ligne..][..ligne];
        for i in 0..ligne {
            let a = if i >= 4 { courante[i - 4] } else { 0 };
            let (b, c) = if y > 0 {
                let haut = &rgba[(y - 1) * ligne..][..ligne];
                (haut[i], if i >= 4 { haut[i - 4] } else { 0 })
            } else {
                (0, 0)
            };
            brut.push(courante[i].wrapping_sub(paeth(a, b, c)));
        }
    }
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&l.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8 bits, RVBA
    morceau(&mut out, b"IHDR", &ihdr);
    morceau(&mut out, b"IDAT", &zlib(&brut));
    morceau(&mut out, b"IEND", &[]);
    out
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let (pa, pb, pc) = ((p - a as i16).abs(), (p - b as i16).abs(), (p - c as i16).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn morceau(out: &mut Vec<u8>, genre: &[u8; 4], donnees: &[u8]) {
    out.extend_from_slice(&(donnees.len() as u32).to_be_bytes());
    let debut = out.len();
    out.extend_from_slice(genre);
    out.extend_from_slice(donnees);
    let crc = crc32(&out[debut..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

fn crc32(d: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (n, t) in table.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *t = c;
    }
    let mut c = 0xFFFF_FFFFu32;
    for &b in d {
        c = table[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

fn adler32(d: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for bloc in d.chunks(5552) {
        for &x in bloc {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

/// Écrit des bits, du poids faible au poids fort, comme deflate les attend.
struct Bits {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Bits {
    fn mettre(&mut self, v: u32, n: u32) {
        self.acc |= (v as u64) << self.n;
        self.n += n;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    /// Un code de Huffman : il s'écrit du bit de poids fort au plus faible.
    fn code(&mut self, code: u32, n: u32) {
        let mut r = 0;
        for i in 0..n {
            r |= ((code >> i) & 1) << (n - 1 - i);
        }
        self.mettre(r, n);
    }

    fn finir(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

/// Le code fixe d'un symbole littéral ou de longueur (0 à 287).
fn litteral(b: &mut Bits, s: u32) {
    match s {
        0..=143 => b.code(0x30 + s, 8),
        144..=255 => b.code(0x190 + s - 144, 9),
        256..=279 => b.code(s - 256, 7),
        _ => b.code(0xC0 + s - 280, 8),
    }
}

const LONGUEURS: [(u16, u32); 29] = [
    (3, 0), (4, 0), (5, 0), (6, 0), (7, 0), (8, 0), (9, 0), (10, 0), (11, 1), (13, 1), (15, 1), (17, 1), (19, 2), (23, 2), (27, 2), (31, 2),
    (35, 3), (43, 3), (51, 3), (59, 3), (67, 4), (83, 4), (99, 4), (115, 4), (131, 5), (163, 5), (195, 5), (227, 5), (258, 0),
];
const DISTANCES: [(u32, u32); 30] = [
    (1, 0), (2, 0), (3, 0), (4, 0), (5, 1), (7, 1), (9, 2), (13, 2), (17, 3), (25, 3), (33, 4), (49, 4), (65, 5), (97, 5), (129, 6), (193, 6),
    (257, 7), (385, 7), (513, 8), (769, 8), (1025, 9), (1537, 9), (2049, 10), (3073, 10), (4097, 11), (6145, 11), (8193, 12), (12289, 12), (16385, 13), (24577, 13),
];

fn zlib(d: &[u8]) -> Vec<u8> {
    let mut b = Bits { out: vec![0x78, 0x01], acc: 0, n: 0 };
    b.mettre(1, 1); // dernier bloc
    b.mettre(1, 2); // Huffman fixe
    const FENETRE: usize = 32768;
    let mut tete = vec![usize::MAX; 1 << 15];
    let mut prec = vec![usize::MAX; FENETRE];
    let cle = |i: usize| ((d[i] as usize) << 10 ^ (d[i + 1] as usize) << 5 ^ d[i + 2] as usize) & 0x7FFF;
    let mut i = 0;
    let inserer = |i: usize, tete: &mut Vec<usize>, prec: &mut Vec<usize>| {
        if i + 2 < d.len() {
            let k = cle(i);
            prec[i % FENETRE] = tete[k];
            tete[k] = i;
        }
    };
    while i < d.len() {
        let (mut meilleure, mut dist) = (0usize, 0usize);
        if i + 2 < d.len() {
            let mut j = tete[cle(i)];
            let mut essais = 32;
            while j != usize::MAX && i - j <= FENETRE && essais > 0 {
                let max = (d.len() - i).min(258);
                let mut n = 0;
                while n < max && d[j + n] == d[i + n] {
                    n += 1;
                }
                if n > meilleure {
                    meilleure = n;
                    dist = i - j;
                    if n == max {
                        break;
                    }
                }
                let suivant = prec[j % FENETRE];
                if suivant == usize::MAX || suivant >= j {
                    break;
                }
                j = suivant;
                essais -= 1;
            }
        }
        if meilleure >= 3 {
            let k = LONGUEURS.iter().rposition(|(l, _)| *l as usize <= meilleure).unwrap_or(0);
            litteral(&mut b, 257 + k as u32);
            b.mettre((meilleure - LONGUEURS[k].0 as usize) as u32, LONGUEURS[k].1);
            let k = DISTANCES.iter().rposition(|(l, _)| *l as usize <= dist).unwrap_or(0);
            b.code(k as u32, 5);
            b.mettre((dist - DISTANCES[k].0 as usize) as u32, DISTANCES[k].1);
            for p in i..i + meilleure {
                inserer(p, &mut tete, &mut prec);
            }
            i += meilleure;
        } else {
            litteral(&mut b, d[i] as u32);
            inserer(i, &mut tete, &mut prec);
            i += 1;
        }
    }
    litteral(&mut b, 256);
    let mut out = b.finir();
    out.extend_from_slice(&adler32(d).to_be_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un inflate minimal (Huffman fixe seulement), pour relire ce qu'on écrit.
    fn inflate(z: &[u8]) -> Vec<u8> {
        let d = &z[2..z.len() - 4];
        let mut pos = 0usize;
        let mut bit = |n: u32| {
            let mut v = 0;
            for i in 0..n {
                v |= (((d[pos / 8] >> (pos % 8)) & 1) as u32) << i;
                pos += 1;
            }
            v
        };
        assert_eq!(bit(3), 0b011);
        let mut out: Vec<u8> = Vec::new();
        loop {
            // Lit un symbole fixe, bit de poids fort d'abord
            let mut code = 0;
            let mut n = 0;
            let s = loop {
                code = (code << 1) | bit(1);
                n += 1;
                match n {
                    7 if code <= 0x17 => break code + 256,
                    8 if (0x30..=0xBF).contains(&code) => break code - 0x30,
                    8 if (0xC0..=0xC7).contains(&code) => break code - 0xC0 + 280,
                    9 if code >= 0x190 => break code - 0x190 + 144,
                    _ => {}
                }
            };
            match s {
                0..=255 => out.push(s as u8),
                256 => break,
                _ => {
                    let (base, extra) = LONGUEURS[(s - 257) as usize];
                    let l = base as usize + bit(extra) as usize;
                    let mut k = 0;
                    for _ in 0..5 {
                        k = (k << 1) | bit(1);
                    }
                    let (base, extra) = DISTANCES[k as usize];
                    let dist = base as usize + bit(extra) as usize;
                    for _ in 0..l {
                        out.push(out[out.len() - dist]);
                    }
                }
            }
        }
        out
    }

    #[test]
    fn le_deflate_se_relit() {
        let mut d: Vec<u8> = (0..5000u32).map(|i| ((i * 7) % 13) as u8).collect();
        d.extend(std::iter::repeat_n(9u8, 700));
        d.extend((0..300u32).map(|i| (i * i % 251) as u8));
        assert_eq!(inflate(&zlib(&d)), d);
        assert_eq!(adler32(&d).to_be_bytes(), zlib(&d)[zlib(&d).len() - 4..]);
    }

    #[test]
    fn un_png_porte_ses_morceaux() {
        let p = encoder(2, 2, &[255; 16]);
        assert_eq!(&p[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        assert_eq!(&p[12..16], b"IHDR");
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
    }
}
