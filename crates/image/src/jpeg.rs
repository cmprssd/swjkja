//! JPEG decoding as libjpeg 9a does it, the version OpenJK bundles (`lib/jpeg-9a`) and
//! `LoadJPG` calls with its defaults: Huffman-coded baseline and progressive scans
//! (`jdhuff.c`), the integer inverse DCT (`jidctint.c`), and YCbCr to RGB (`jdcolor.c`),
//! ported so the pixels match bit for bit.
//!
//! Where 9a differs from libjpeg 6b (and zune-jpeg or libjpeg-turbo), it is in the
//! chroma: a component sampled at half the luma's rate both ways is decoded with a
//! 16x16 inverse DCT (`jpeg_idct_16x16`, chosen in `jdmaster.c`) instead of an 8x8 one
//! followed by an upsampling filter.
//!
//! Not supported, each an error: arithmetic coding, lossless and 12-bit JPEG, other
//! sampling ratios (9a would pick a 16x8 or 8x16 inverse DCT, or upsample), CMYK, and
//! scans that need a Huffman table the file never defines (9a substitutes the
//! standard tables). Like 9a, data that ends early decodes as zeros.

use crate::Error;

/// Zigzag position to row-major position, with 16 extra entries so a corrupt run
/// lands on the last coefficient, as `jpeg_natural_order` does.
const NATURAL_ORDER: [usize; 80] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63,
    63, 63, 63, 63, 63, 63,
];

fn err(message: &str) -> Error {
    Error(format!("JPEG: {message}"))
}

pub fn decode(bytes: &[u8]) -> Result<crate::Image, Error> {
    let mut d = Decoder::default();
    d.read(bytes)?;
    d.output()
}

#[derive(Default, Clone)]
struct Component {
    id: u8,
    h: usize,
    v: usize,
    quant_index: usize,
    /// Latched when the component's first scan starts, as `latch_quant_tables` does.
    quant: Option<[u16; 64]>,
    /// Blocks per row and rows of blocks, padded to whole MCUs.
    stride: usize,
    rows: usize,
    width_in_blocks: usize,
    height_in_blocks: usize,
    coefficients: Vec<[i16; 64]>,
}

#[derive(Default)]
struct Decoder {
    width: usize,
    height: usize,
    progressive: bool,
    components: Vec<Component>,
    quant: [Option<[u16; 64]>; 4],
    /// Raw DHT contents (BITS, HUFFVAL) for DC tables 0-3, then AC tables 0-3.
    huffman: [Option<([u8; 17], Vec<u8>)>; 8],
    restart_interval: usize,
    jfif: bool,
    adobe_transform: Option<u8>,
    max_h: usize,
    max_v: usize,
    scanned: bool,
}

/// Reads one marker segment's length and returns its contents.
fn segment<'a>(data: &'a [u8], pos: &mut usize) -> Result<&'a [u8], Error> {
    let len = data
        .get(*pos..*pos + 2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]) as usize)
        .ok_or_else(|| err("premature end of file"))?;
    let body = len
        .checked_sub(2)
        .and_then(|n| data.get(*pos + 2..*pos + 2 + n))
        .ok_or_else(|| err("bogus marker length"))?;
    *pos += len;
    Ok(body)
}

/// The next marker at or after `pos` (`next_marker`: garbage and fill bytes skipped),
/// with `pos` moved past it; None at the end of the data.
fn next_marker(data: &[u8], pos: &mut usize) -> Option<u8> {
    loop {
        while *data.get(*pos)? != 0xFF {
            *pos += 1;
        }
        while *data.get(*pos)? == 0xFF {
            *pos += 1;
        }
        let c = data[*pos];
        *pos += 1;
        if c != 0 {
            return Some(c);
        }
    }
}

impl Decoder {
    fn read(&mut self, data: &[u8]) -> Result<(), Error> {
        if data.get(..2) != Some(&[0xFF, 0xD8]) {
            return Err(err("not a JPEG file"));
        }
        let mut pos = 2;
        loop {
            // The end of the data reads as EOI, as `jpeg_mem_src` supplies one.
            let marker = next_marker(data, &mut pos).unwrap_or(0xD9);
            match marker {
                0xC0..=0xC2 => {
                    if !self.components.is_empty() {
                        return Err(err("duplicate SOF marker"));
                    }
                    self.progressive = marker == 0xC2;
                    self.frame(segment(data, &mut pos)?)?;
                }
                0xC3 | 0xC5..=0xCB | 0xCD..=0xCF => {
                    return Err(err("unsupported coding process (arithmetic or lossless)"));
                }
                0xC4 => self.define_huffman(segment(data, &mut pos)?)?,
                0xDB => self.define_quant(segment(data, &mut pos)?)?,
                0xDD => {
                    let b = segment(data, &mut pos)?;
                    let b = b.get(..2).ok_or_else(|| err("bogus DRI length"))?;
                    self.restart_interval = u16::from_be_bytes([b[0], b[1]]) as usize;
                }
                0xDA => {
                    if self.components.is_empty() {
                        return Err(err("SOS before SOF"));
                    }
                    let header = segment(data, &mut pos)?;
                    pos = self.scan(header, data, pos)?;
                    self.scanned = true;
                }
                0xD9 => {
                    return if self.scanned {
                        Ok(())
                    } else {
                        Err(err("missing SOS marker"))
                    };
                }
                0xE0 => {
                    let b = segment(data, &mut pos)?;
                    self.jfif |= b.len() >= 14 && b.starts_with(b"JFIF\0");
                }
                0xEE => {
                    let b = segment(data, &mut pos)?;
                    if b.len() >= 12 && b.starts_with(b"Adobe") {
                        self.adobe_transform = Some(b[11]);
                    }
                }
                // Stray restart markers and TEM have no length; APPn, COM and DNL
                // are skipped over.
                0xD0..=0xD7 | 0x01 => {}
                0xE1..=0xED | 0xEF | 0xFE | 0xDC => {
                    segment(data, &mut pos)?;
                }
                0xD8 => return Err(err("duplicate SOI marker")),
                m => return Err(err(&format!("unsupported marker 0x{m:02x}"))),
            }
        }
    }

    fn frame(&mut self, b: &[u8]) -> Result<(), Error> {
        if b.len() < 6 {
            return Err(err("bogus SOF length"));
        }
        if b[0] != 8 {
            return Err(err(&format!("unsupported data precision {}", b[0])));
        }
        self.height = u16::from_be_bytes([b[1], b[2]]) as usize;
        self.width = u16::from_be_bytes([b[3], b[4]]) as usize;
        let n = b[5] as usize;
        if self.width == 0 || self.height == 0 || n == 0 {
            return Err(err("empty image"));
        }
        if n > 10 || b.len() != 6 + 3 * n {
            return Err(err("bogus SOF length"));
        }
        for c in b[6..].as_chunks::<3>().0 {
            let (h, v) = ((c[1] >> 4) as usize, (c[1] & 15) as usize);
            if !(1..=4).contains(&h) || !(1..=4).contains(&v) {
                return Err(err("bogus sampling factors"));
            }
            self.components.push(Component {
                id: c[0],
                h,
                v,
                quant_index: c[2] as usize,
                ..Default::default()
            });
        }
        self.max_h = self.components.iter().map(|c| c.h).max().unwrap();
        self.max_v = self.components.iter().map(|c| c.v).max().unwrap();
        let mcus_x = self.width.div_ceil(8 * self.max_h);
        let mcus_y = self.height.div_ceil(8 * self.max_v);
        for c in &mut self.components {
            c.stride = mcus_x * c.h;
            c.rows = mcus_y * c.v;
            c.width_in_blocks = (self.width * c.h).div_ceil(self.max_h * 8);
            c.height_in_blocks = (self.height * c.v).div_ceil(self.max_v * 8);
            c.coefficients = vec![[0; 64]; c.stride * c.rows];
        }
        Ok(())
    }

    fn define_quant(&mut self, mut b: &[u8]) -> Result<(), Error> {
        while let Some(&pq) = b.first() {
            let wide = pq >> 4 != 0;
            let size = if wide { 128 } else { 64 };
            let values = b.get(1..1 + size).ok_or_else(|| err("bogus DQT length"))?;
            let mut table = [0u16; 64];
            for (k, &position) in NATURAL_ORDER[..64].iter().enumerate() {
                table[position] = if wide {
                    u16::from_be_bytes([values[2 * k], values[2 * k + 1]])
                } else {
                    values[k] as u16
                };
            }
            *self
                .quant
                .get_mut((pq & 15) as usize)
                .ok_or_else(|| err("bogus DQT index"))? = Some(table);
            b = &b[1 + size..];
        }
        Ok(())
    }

    fn define_huffman(&mut self, mut b: &[u8]) -> Result<(), Error> {
        while let Some(&tc) = b.first() {
            let counts = b.get(1..17).ok_or_else(|| err("bogus DHT length"))?;
            let mut bits = [0u8; 17];
            bits[1..].copy_from_slice(counts);
            let total: usize = counts.iter().map(|&n| n as usize).sum();
            if total > 256 {
                return Err(err("bogus Huffman table definition"));
            }
            let values = b
                .get(17..17 + total)
                .ok_or_else(|| err("bogus DHT length"))?;
            if tc & 0xEC != 0 {
                return Err(err("bogus DHT index"));
            }
            let index = (tc & 3) as usize + if tc & 0x10 != 0 { 4 } else { 0 };
            self.huffman[index] = Some((bits, values.to_vec()));
            b = &b[17 + total..];
        }
        Ok(())
    }

    /// Decodes one scan's entropy-coded data into the coefficients; returns where its
    /// data ends.
    fn scan(&mut self, header: &[u8], data: &[u8], start: usize) -> Result<usize, Error> {
        let n = *header.first().ok_or_else(|| err("bogus SOS length"))? as usize;
        if !(1..=4).contains(&n) || header.len() != 4 + 2 * n {
            return Err(err("bogus SOS length"));
        }
        let mut members = Vec::with_capacity(n);
        let mut dc_tables = Vec::with_capacity(n);
        let mut ac_tables = Vec::with_capacity(n);
        let (ss, se, ah, al) = (
            header[1 + 2 * n] as usize,
            header[2 + 2 * n] as usize,
            (header[3 + 2 * n] >> 4) as u32,
            (header[3 + 2 * n] & 15) as u32,
        );
        if self.progressive {
            let bad = if ss == 0 {
                se != 0
            } else {
                se < ss || se > 63 || n != 1
            };
            if bad || al > 13 || ah > 13 {
                return Err(err("invalid progressive parameters"));
            }
        } else if ss != 0 || se != 63 || ah != 0 || al != 0 {
            return Err(err("unsupported sequential scan parameters"));
        }
        for s in header[1..1 + 2 * n].as_chunks::<2>().0 {
            let ci = self
                .components
                .iter()
                .position(|c| c.id == s[0])
                .ok_or_else(|| err("bad component ID in SOS"))?;
            members.push(ci);
            let c = &mut self.components[ci];
            if c.quant.is_none() {
                c.quant = Some(
                    self.quant
                        .get(c.quant_index)
                        .copied()
                        .flatten()
                        .ok_or_else(|| err("quantization table missing"))?,
                );
            }
            // Only the tables this kind of scan reads have to exist.
            let wants_dc = !self.progressive || (ss == 0 && ah == 0);
            let wants_ac = !self.progressive || ss != 0;
            let table = |number: u8, dc: bool| -> Result<Option<Huffman>, Error> {
                let index = number as usize + if dc { 0 } else { 4 };
                let (bits, values) = (number < 4)
                    .then(|| self.huffman[index].as_ref())
                    .flatten()
                    .ok_or_else(|| err("Huffman table missing"))?;
                Huffman::new(bits, values, dc).map(Some)
            };
            dc_tables.push(if wants_dc {
                table(s[1] >> 4, true)?
            } else {
                None
            });
            ac_tables.push(if wants_ac {
                table(s[1] & 15, false)?
            } else {
                None
            });
        }

        let mut bits = Bits::new(data, start);
        let mut last_dc = [0i32; 4];
        let mut eobrun = 0u32;
        let mut restarts_to_go = self.restart_interval;
        let mut next_restart = 0u8;
        // Blocks of each MCU: (member index, block index in its component).
        let (mcus_x, mcus_y, single) = if n == 1 {
            let c = &self.components[members[0]];
            (c.width_in_blocks, c.height_in_blocks, true)
        } else {
            (
                self.width.div_ceil(8 * self.max_h),
                self.height.div_ceil(8 * self.max_v),
                false,
            )
        };
        let mut blocks = Vec::new();
        for my in 0..mcus_y {
            for mx in 0..mcus_x {
                if self.restart_interval != 0 {
                    if restarts_to_go == 0 {
                        bits.restart(next_restart);
                        next_restart = (next_restart + 1) & 7;
                        last_dc = [0; 4];
                        eobrun = 0;
                        restarts_to_go = self.restart_interval;
                    }
                    restarts_to_go -= 1;
                }
                // Once the data has run out, the rest of the segment stays zero (grey),
                // as `decode_mcu` leaves it.
                if bits.insufficient {
                    continue;
                }
                blocks.clear();
                for (m, &ci) in members.iter().enumerate() {
                    let c = &self.components[ci];
                    if single {
                        blocks.push((m, my * c.stride + mx));
                    } else {
                        for y in 0..c.v {
                            for x in 0..c.h {
                                blocks.push((m, (my * c.v + y) * c.stride + mx * c.h + x));
                            }
                        }
                    }
                }
                for &(m, b) in &blocks {
                    let block = &mut self.components[members[m]].coefficients[b];
                    let dc = dc_tables[m].as_ref();
                    let ac = ac_tables[m].as_ref();
                    if !self.progressive {
                        decode_sequential(
                            &mut bits,
                            block,
                            dc.unwrap(),
                            ac.unwrap(),
                            &mut last_dc[m],
                        );
                    } else if ss == 0 && ah == 0 {
                        let s = dc_difference(&mut bits, dc.unwrap());
                        last_dc[m] = last_dc[m].wrapping_add(s);
                        block[0] = (last_dc[m] as u32).wrapping_shl(al) as i16;
                    } else if ss == 0 {
                        if bits.get(1) != 0 {
                            block[0] |= 1 << al;
                        }
                    } else if ah == 0 {
                        decode_ac_first(&mut bits, block, ac.unwrap(), ss, se, al, &mut eobrun);
                    } else {
                        decode_ac_refine(&mut bits, block, ac.unwrap(), ss, se, al, &mut eobrun);
                    }
                }
            }
        }
        Ok(bits.end())
    }

    fn output(&self) -> Result<crate::Image, Error> {
        let (w, h) = (self.width, self.height);
        if w * h > 0x1FFF_FFFF {
            return Err(err(&format!("invalid image format: {w}x{h}")));
        }
        // The colour space as `default_decompress_parms` guesses it.
        let ids: Vec<u8> = self.components.iter().map(|c| c.id).collect();
        let rgb = match ids[..] {
            [_] => false,
            [1, 2, 3] => false,
            [0x52, 0x47, 0x42] => true,
            [1, 0x22, 0x23] | [0x72, 0x67, 0x62] => {
                return Err(err("unsupported colour space (big gamut)"));
            }
            [_, _, _] => !self.jfif && self.adobe_transform == Some(0),
            _ => return Err(err("unsupported colour conversion")),
        };
        let planes: Vec<Vec<u8>> = self
            .components
            .iter()
            .map(|c| self.plane(c))
            .collect::<Result<_, _>>()?;
        let mut rgba = vec![255u8; w * h * 4];
        if planes.len() == 1 {
            let (p, pw) = (&planes[0], w.div_ceil(8) * 8);
            for (row, src) in rgba.chunks_exact_mut(w * 4).zip(p.chunks_exact(pw)) {
                for (px, &g) in row.as_chunks_mut::<4>().0.iter_mut().zip(src) {
                    px[..3].fill(g);
                }
            }
        } else {
            let pitch = |i: usize| self.plane_width(&self.components[i]);
            let tables = ColourTables::new();
            let rows = rgba.chunks_exact_mut(w * 4);
            let planes = planes[0]
                .chunks_exact(pitch(0))
                .zip(planes[1].chunks_exact(pitch(1)))
                .zip(planes[2].chunks_exact(pitch(2)));
            for (row, ((p0, p1), p2)) in rows.zip(planes) {
                let samples = p0.iter().zip(p1).zip(p2);
                for (px, ((&a, &b), &c)) in row.as_chunks_mut::<4>().0.iter_mut().zip(samples) {
                    let out = if rgb {
                        [a, b, c]
                    } else {
                        tables.convert(a, b, c)
                    };
                    px[..3].copy_from_slice(&out);
                }
            }
        }
        crate::image(w, h, rgba)
    }

    /// The inverse DCT's output size for a component: 16 where 9a scales it up
    /// (`jdmaster.c`: twice when the component is sampled at half the rate), else 8.
    fn scaled_size(&self, c: &Component) -> Result<usize, Error> {
        let scale = |samp: usize, max: usize| if max.is_multiple_of(samp * 2) { 16 } else { 8 };
        let (sh, sv) = (scale(c.h, self.max_h), scale(c.v, self.max_v));
        if sh != sv || c.h * sh != self.max_h * 8 || c.v * sv != self.max_v * 8 {
            return Err(err(&format!(
                "unsupported sampling {}x{} of {}x{}",
                c.h, c.v, self.max_h, self.max_v
            )));
        }
        Ok(sh)
    }

    fn plane_width(&self, c: &Component) -> usize {
        let size = self.scaled_size(c).unwrap_or(8);
        self.width.div_ceil(size) * size
    }

    /// The component's samples at full resolution, rows `plane_width` long.
    fn plane(&self, c: &Component) -> Result<Vec<u8>, Error> {
        let size = if self.components.len() == 1 {
            8
        } else {
            self.scaled_size(c)?
        };
        let (bw, bh) = (self.width.div_ceil(size), self.height.div_ceil(size));
        let pitch = bw * size;
        let mut plane = vec![0u8; pitch * bh * size];
        let quant = c.quant.unwrap_or([0; 64]);
        for by in 0..bh {
            for bx in 0..bw {
                let block = &c.coefficients[by * c.stride + bx];
                let out = &mut plane[by * size * pitch + bx * size..];
                if size == 8 {
                    idct_8x8(block, &quant, out, pitch);
                } else {
                    idct_16x16(block, &quant, out, pitch);
                }
            }
        }
        Ok(plane)
    }
}

/// Bits of one scan's entropy-coded data, as `jpeg_fill_bit_buffer` reads them:
/// stuffed zero bytes dropped, and zeros once a marker or the end is reached.
struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    acc: u64,
    count: u32,
    /// How many of `count` bits came from the data; zeros follow them.
    real: u32,
    /// At a marker (or the end): only zeros follow until a restart moves past it.
    stopped: bool,
    /// More bits were read than the data had (9a's `insufficient_data`).
    insufficient: bool,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8], pos: usize) -> Self {
        Self {
            data,
            pos,
            acc: 0,
            count: 0,
            real: 0,
            stopped: false,
            insufficient: false,
        }
    }

    fn fill(&mut self) {
        while self.count <= 56 {
            let mut byte = 0;
            if !self.stopped {
                match self.data.get(self.pos) {
                    None => self.stopped = true,
                    Some(&0xFF) => {
                        let mut p = self.pos + 1;
                        while self.data.get(p) == Some(&0xFF) {
                            p += 1;
                        }
                        if self.data.get(p) == Some(&0) {
                            byte = 0xFF;
                            self.pos = p + 1;
                        } else {
                            self.stopped = true;
                        }
                    }
                    Some(&b) => {
                        byte = b;
                        self.pos += 1;
                    }
                }
            }
            if !self.stopped {
                self.real += 8;
            }
            self.acc |= (byte as u64) << (56 - self.count);
            self.count += 8;
        }
    }

    fn peek(&mut self, n: u32) -> u32 {
        if self.count < n {
            self.fill();
        }
        (self.acc >> (64 - n)) as u32
    }

    fn skip(&mut self, n: u32) {
        self.acc <<= n;
        self.count -= n;
        match self.real.checked_sub(n) {
            Some(real) => self.real = real,
            None => {
                self.real = 0;
                self.insufficient = true;
            }
        }
    }

    fn get(&mut self, n: u32) -> i32 {
        if n == 0 {
            return 0;
        }
        let v = self.peek(n);
        self.skip(n);
        v as i32
    }

    /// `process_restart`: the buffered bits are dropped and the expected RSTn
    /// consumed. Any other marker stays put, and the data reads as zeros.
    fn restart(&mut self, number: u8) {
        self.acc = 0;
        self.count = 0;
        self.real = 0;
        let mut p = self.pos;
        if next_marker(self.data, &mut p) == Some(0xD0 + number) {
            self.pos = p;
            self.stopped = false;
            self.insufficient = false;
        } else {
            self.stopped = true;
        }
    }

    /// Where the scan's data ends: at the marker that stopped it.
    fn end(&self) -> usize {
        self.pos
    }
}

/// A Huffman table as `jpeg_make_d_derived_tbl` derives it, with a 9-bit lookahead.
struct Huffman {
    /// For each 9-bit prefix: code length (0 if longer) and symbol.
    lookahead: [(u8, u8); 512],
    maxcode: [i32; 18],
    valoffset: [i32; 18],
    values: [u8; 256],
}

impl Huffman {
    fn new(bits: &[u8; 17], values: &[u8], dc: bool) -> Result<Self, Error> {
        let bad = || err("bogus Huffman table definition");
        let mut sizes = Vec::with_capacity(257);
        for (l, &n) in bits.iter().enumerate().skip(1) {
            sizes.extend(std::iter::repeat_n(l, n as usize));
        }
        let mut codes = Vec::with_capacity(sizes.len());
        let (mut code, mut si, mut p) = (0i32, sizes.first().copied().unwrap_or(0), 0);
        while p < sizes.len() {
            while p < sizes.len() && sizes[p] == si {
                codes.push(code);
                code += 1;
                p += 1;
            }
            if code >= 1 << si {
                return Err(bad());
            }
            code <<= 1;
            si += 1;
        }
        let mut h = Self {
            lookahead: [(0, 0); 512],
            maxcode: [-1; 18],
            valoffset: [0; 18],
            values: [0; 256],
        };
        h.values[..values.len()].copy_from_slice(values);
        let mut p = 0;
        for (l, &n) in bits.iter().enumerate().skip(1) {
            let n = n as usize;
            if n != 0 {
                h.valoffset[l] = p as i32 - codes[p];
                p += n;
                h.maxcode[l] = codes[p - 1];
            }
        }
        h.maxcode[17] = 0xFFFFF;
        for (i, (&size, &code)) in sizes.iter().zip(&codes).enumerate() {
            if size <= 9 {
                let first = (code as usize) << (9 - size);
                for slot in &mut h.lookahead[first..first + (1 << (9 - size))] {
                    *slot = (size as u8, values[i]);
                }
            }
        }
        if dc && values.iter().any(|&v| v > 15) {
            return Err(bad());
        }
        Ok(h)
    }

    /// The next symbol; a code longer than 16 bits decodes as 0 after 17 bits, as
    /// `jpeg_huff_decode` does.
    fn decode(&self, bits: &mut Bits) -> u8 {
        let (len, symbol) = self.lookahead[bits.peek(9) as usize];
        if len != 0 {
            bits.skip(len as u32);
            return symbol;
        }
        let code = bits.peek(17) as i32;
        for l in 10..=16 {
            let c = code >> (17 - l);
            if c <= self.maxcode[l] {
                bits.skip(l as u32);
                return self.values[((c + self.valoffset[l]) & 0xFF) as usize];
            }
        }
        bits.skip(17);
        0
    }
}

/// `HUFF_EXTEND`: `s` bits read as a signed value.
fn extend(bits: &mut Bits, s: u8) -> i32 {
    let s = s as u32;
    let r = bits.get(s);
    if s != 0 && r < 1 << (s - 1) {
        r - (1 << s) + 1
    } else {
        r
    }
}

fn dc_difference(bits: &mut Bits, dc: &Huffman) -> i32 {
    let s = dc.decode(bits);
    extend(bits, s)
}

fn decode_sequential(
    bits: &mut Bits,
    block: &mut [i16; 64],
    dc: &Huffman,
    ac: &Huffman,
    last_dc: &mut i32,
) {
    *last_dc = last_dc.wrapping_add(dc_difference(bits, dc));
    block[0] = *last_dc as i16;
    let mut k = 1;
    while k < 64 {
        let rs = ac.decode(bits);
        let (r, s) = ((rs >> 4) as usize, rs & 15);
        if s != 0 {
            k += r;
            block[NATURAL_ORDER[k]] = extend(bits, s) as i16;
        } else if r != 15 {
            break;
        } else {
            k += 15;
        }
        k += 1;
    }
}

fn decode_ac_first(
    bits: &mut Bits,
    block: &mut [i16; 64],
    ac: &Huffman,
    ss: usize,
    se: usize,
    al: u32,
    eobrun: &mut u32,
) {
    if *eobrun > 0 {
        *eobrun -= 1;
        return;
    }
    let mut k = ss;
    while k <= se {
        let rs = ac.decode(bits);
        let (r, s) = ((rs >> 4) as u32, rs & 15);
        if s != 0 {
            k += r as usize;
            block[NATURAL_ORDER[k]] = (extend(bits, s) as u32).wrapping_shl(al) as i16;
        } else if r != 15 {
            *eobrun = (1 << r) + bits.get(r) as u32 - 1;
            break;
        } else {
            k += 15;
        }
        k += 1;
    }
}

fn decode_ac_refine(
    bits: &mut Bits,
    block: &mut [i16; 64],
    ac: &Huffman,
    ss: usize,
    se: usize,
    al: u32,
    eobrun: &mut u32,
) {
    let p1 = 1i16 << al;
    let m1 = (-1i16).wrapping_shl(al);
    let refine = |bits: &mut Bits, coef: &mut i16| {
        if *coef != 0 && bits.get(1) != 0 && *coef & p1 == 0 {
            *coef = coef.wrapping_add(if *coef >= 0 { p1 } else { m1 });
        }
    };
    let mut k = ss;
    if *eobrun == 0 {
        while k <= se {
            let rs = ac.decode(bits);
            let (mut r, s) = ((rs >> 4) as i32, rs & 15);
            let mut value = 0;
            if s != 0 {
                value = if bits.get(1) != 0 { p1 } else { m1 };
            } else if r != 15 {
                *eobrun = (1 << r) + bits.get(r as u32) as u32;
                break;
            }
            while k <= se {
                let coef = &mut block[NATURAL_ORDER[k]];
                if *coef != 0 {
                    refine(bits, coef);
                } else {
                    r -= 1;
                    if r < 0 {
                        break;
                    }
                }
                k += 1;
            }
            if value != 0 {
                block[NATURAL_ORDER[k]] = value;
            }
            k += 1;
        }
    }
    if *eobrun > 0 {
        while k <= se {
            refine(bits, &mut block[NATURAL_ORDER[k]]);
            k += 1;
        }
        *eobrun -= 1;
    }
}

/// The post-IDCT range limit (`prepare_range_limit_table`): the value plus 128,
/// clamped, after masking to 10 bits, so wildly corrupt values wrap instead.
fn limit(x: i64) -> u8 {
    static TABLE: [u8; 1024] = {
        let mut t = [0u8; 1024];
        let mut i = 0;
        while i < 1024 {
            t[i] = match i {
                0..128 => (i + 128) as u8,
                128..512 => 255,
                512..896 => 0,
                _ => (i - 896) as u8,
            };
            i += 1;
        }
        t
    };
    TABLE[(x & 1023) as usize]
}

const CONST_BITS: u32 = 13;
const PASS1_BITS: u32 = 2;

/// `jpeg_idct_islow`: dequantizes and inverse transforms one block to 8x8 samples.
/// Integer arithmetic is 64-bit, which gives libjpeg's 32-bit results wherever those
/// do not overflow (they do only on corrupt data).
fn idct_8x8(block: &[i16; 64], quant: &[u16; 64], out: &mut [u8], pitch: usize) {
    let dq = |i: usize| block[i] as i64 * quant[i] as i64;
    let mut ws = [0i64; 64];
    for col in 0..8 {
        if (1..8).all(|r| block[r * 8 + col] == 0) {
            let dc = dq(col) << PASS1_BITS;
            for r in 0..8 {
                ws[r * 8 + col] = dc;
            }
            continue;
        }
        let z2 = dq(16 + col);
        let z3 = dq(48 + col);
        let z1 = (z2 + z3) * 4433;
        let tmp2 = z1 + z2 * 6270;
        let tmp3 = z1 - z3 * 15137;
        let z2 = (dq(col) << CONST_BITS) + (1 << (CONST_BITS - PASS1_BITS - 1));
        let z3 = dq(32 + col) << CONST_BITS;
        let even = even_8(z2, z3, tmp2, tmp3);
        let odd = odd_8(dq(56 + col), dq(40 + col), dq(24 + col), dq(8 + col));
        for (r, v) in butterfly_8(even, odd).into_iter().enumerate() {
            ws[r * 8 + col] = v >> (CONST_BITS - PASS1_BITS);
        }
    }
    for row in 0..8 {
        let w = &ws[row * 8..row * 8 + 8];
        let o = &mut out[row * pitch..row * pitch + 8];
        if w[1..].iter().all(|&v| v == 0) {
            let dc = limit((w[0] + (1 << (PASS1_BITS + 2))) >> (PASS1_BITS + 3));
            o.fill(dc);
            continue;
        }
        let (z2, z3) = (w[2], w[6]);
        let z1 = (z2 + z3) * 4433;
        let tmp2 = z1 + z2 * 6270;
        let tmp3 = z1 - z3 * 15137;
        let z2 = w[0] + (1 << (PASS1_BITS + 2));
        let z3 = w[4];
        let even = even_8(z2 << CONST_BITS, z3 << CONST_BITS, tmp2, tmp3);
        let odd = odd_8(w[7], w[5], w[3], w[1]);
        for (c, v) in butterfly_8(even, odd).into_iter().enumerate() {
            o[c] = limit(v >> (CONST_BITS + PASS1_BITS + 3));
        }
    }
}

/// The even part's last step: (tmp10, tmp11, tmp12, tmp13) from the scaled DC and
/// fourth inputs and the rotated second and sixth.
fn even_8(z2: i64, z3: i64, tmp2: i64, tmp3: i64) -> [i64; 4] {
    let (tmp0, tmp1) = (z2 + z3, z2 - z3);
    [tmp0 + tmp2, tmp1 + tmp3, tmp1 - tmp3, tmp0 - tmp2]
}

/// The odd part from inputs 7, 5, 3, 1: (tmp0, tmp1, tmp2, tmp3).
fn odd_8(tmp0: i64, tmp1: i64, tmp2: i64, tmp3: i64) -> [i64; 4] {
    let z2 = tmp0 + tmp2;
    let z3 = tmp1 + tmp3;
    let z1 = (z2 + z3) * 9633;
    let z2 = z2 * -16069 + z1;
    let z3 = z3 * -3196 + z1;
    let z1 = (tmp0 + tmp3) * -7373;
    let t0 = tmp0 * 2446 + z1 + z2;
    let t3 = tmp3 * 12299 + z1 + z3;
    let z1 = (tmp1 + tmp2) * -20995;
    let t1 = tmp1 * 16819 + z1 + z3;
    let t2 = tmp2 * 25172 + z1 + z2;
    [t0, t1, t2, t3]
}

/// Outputs 0-7 from the even (tmp10, tmp11, tmp12, tmp13) and odd parts.
fn butterfly_8([t10, t11, t12, t13]: [i64; 4], [t0, t1, t2, t3]: [i64; 4]) -> [i64; 8] {
    [
        t10 + t3,
        t11 + t2,
        t12 + t1,
        t13 + t0,
        t13 - t0,
        t12 - t1,
        t11 - t2,
        t10 - t3,
    ]
}

/// `jpeg_idct_16x16`: one block of coefficients to 16x16 samples.
fn idct_16x16(block: &[i16; 64], quant: &[u16; 64], out: &mut [u8], pitch: usize) {
    let dq = |i: usize| block[i] as i64 * quant[i] as i64;
    let mut ws = [0i64; 128];
    for col in 0..8 {
        let x: [i64; 8] = std::array::from_fn(|r| dq(r * 8 + col));
        let dc = (x[0] << CONST_BITS) + (1 << (CONST_BITS - PASS1_BITS - 1));
        for (r, v) in kernel_16(dc, x).into_iter().enumerate() {
            ws[r * 8 + col] = v >> (CONST_BITS - PASS1_BITS);
        }
    }
    for row in 0..16 {
        let w: [i64; 8] = ws[row * 8..row * 8 + 8].try_into().unwrap();
        let dc = (w[0] + (1 << (PASS1_BITS + 2))) << CONST_BITS;
        let o = &mut out[row * pitch..row * pitch + 16];
        for (c, v) in kernel_16(dc, w).into_iter().enumerate() {
            o[c] = limit(v >> (CONST_BITS + PASS1_BITS + 3));
        }
    }
}

/// The 16-point kernel of `jpeg_idct_16x16` (28 multiplications), given the scaled
/// and rounded DC term and the eight inputs (`x[0]` unused).
fn kernel_16(tmp0: i64, x: [i64; 8]) -> [i64; 16] {
    // Even part.
    let z1 = x[4];
    let tmp1 = z1 * 10703;
    let tmp2 = z1 * 4433;
    let tmp10 = tmp0 + tmp1;
    let tmp11 = tmp0 - tmp1;
    let tmp12 = tmp0 + tmp2;
    let tmp13 = tmp0 - tmp2;
    let (z1, z2) = (x[2], x[6]);
    let z3 = z1 - z2;
    let z4 = z3 * 2260;
    let z3 = z3 * 11363;
    let tmp0 = z3 + z2 * 20995;
    let tmp1 = z4 + z1 * 7373;
    let tmp2 = z3 - z1 * 4926;
    let tmp3 = z4 - z2 * 4176;
    let tmp20 = tmp10 + tmp0;
    let tmp27 = tmp10 - tmp0;
    let tmp21 = tmp12 + tmp1;
    let tmp26 = tmp12 - tmp1;
    let tmp22 = tmp13 + tmp2;
    let tmp25 = tmp13 - tmp2;
    let tmp23 = tmp11 + tmp3;
    let tmp24 = tmp11 - tmp3;

    // Odd part.
    let (z1, mut z2, z3, z4) = (x[1], x[3], x[5], x[7]);
    let mut tmp11 = z1 + z3;
    let mut tmp1 = (z1 + z2) * 11086;
    let mut tmp2 = tmp11 * 10217;
    let mut tmp3 = (z1 + z4) * 8956;
    let mut tmp10 = (z1 - z4) * 7350;
    tmp11 *= 5461;
    let mut tmp12 = (z1 - z2) * 3363;
    let tmp0 = tmp1 + tmp2 + tmp3 - z1 * 18730;
    let tmp13 = tmp10 + tmp11 + tmp12 - z1 * 15038;
    let mut zz = (z2 + z3) * 1136;
    tmp1 += zz + z2 * 589;
    tmp2 += zz - z3 * 9222;
    zz = (z3 - z2) * 11529;
    tmp11 += zz - z3 * 6278;
    tmp12 += zz + z2 * 16154;
    z2 += z4;
    zz = z2 * -5461;
    tmp1 += zz;
    tmp3 += zz + z4 * 8728;
    z2 *= -10217;
    tmp10 += z2 + z4 * 25733;
    tmp12 += z2;
    z2 = (z3 + z4) * -11086;
    tmp2 += z2;
    tmp3 += z2;
    z2 = (z4 - z3) * 3363;
    tmp10 += z2;
    tmp11 += z2;

    [
        tmp20 + tmp0,
        tmp21 + tmp1,
        tmp22 + tmp2,
        tmp23 + tmp3,
        tmp24 + tmp10,
        tmp25 + tmp11,
        tmp26 + tmp12,
        tmp27 + tmp13,
        tmp27 - tmp13,
        tmp26 - tmp12,
        tmp25 - tmp11,
        tmp24 - tmp10,
        tmp23 - tmp3,
        tmp22 - tmp2,
        tmp21 - tmp1,
        tmp20 - tmp0,
    ]
}

/// `build_ycc_rgb_table` and `ycc_rgb_convert`, 16-bit fixed point.
struct ColourTables {
    cr_r: [i32; 256],
    cb_b: [i32; 256],
    cr_g: [i32; 256],
    cb_g: [i32; 256],
}

impl ColourTables {
    fn new() -> Self {
        const HALF: i32 = 1 << 15;
        let mut t = Self {
            cr_r: [0; 256],
            cb_b: [0; 256],
            cr_g: [0; 256],
            cb_g: [0; 256],
        };
        for i in 0..256 {
            let x = i as i32 - 128;
            t.cr_r[i] = (91881 * x + HALF) >> 16;
            t.cb_b[i] = (116130 * x + HALF) >> 16;
            t.cr_g[i] = -46802 * x;
            t.cb_g[i] = -22553 * x + HALF;
        }
        t
    }

    fn convert(&self, y: u8, cb: u8, cr: u8) -> [u8; 3] {
        let y = y as i32;
        let (cb, cr) = (cb as usize, cr as usize);
        let clamp = |v: i32| v.clamp(0, 255) as u8;
        [
            clamp(y + self.cr_r[cr]),
            clamp(y + ((self.cb_g[cb] + self.cr_g[cr]) >> 16)),
            clamp(y + self.cb_b[cb]),
        ]
    }
}
