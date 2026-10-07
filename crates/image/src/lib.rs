//! Textures: TGA, JPEG and PNG decoded to RGBA8, rows top to bottom, with the same
//! acceptance rules and name resolution as the original renderer (OpenJK
//! `rd-common/tr_image_*.cpp`).
//!
//! [`load`] resolves a name the way `R_LoadImage` does: the name as given first, then
//! the same name with `.jpg`, `.png` and `.tga`. A file that exists but fails to decode
//! is skipped like a missing one, and the failure is reported.

use swjkja_fs::Vfs;
use zune_core::bytestream::ZCursor;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// `width * height` RGBA pixels, top row first.
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Jpg,
    Png,
    Tga,
}

impl Format {
    /// The resolution order after the name's own extension (`R_ImageLoader_Init`).
    pub const ORDER: [Format; 3] = [Format::Jpg, Format::Png, Format::Tga];

    pub fn extension(self) -> &'static str {
        match self {
            Format::Jpg => "jpg",
            Format::Png => "png",
            Format::Tga => "tga",
        }
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        Self::ORDER
            .into_iter()
            .find(|f| f.extension().eq_ignore_ascii_case(ext))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

macro_rules! bail {
    ($($t:tt)*) => { return Err(Error(format!($($t)*))) };
}

pub fn decode(format: Format, bytes: &[u8]) -> Result<Image, Error> {
    match format {
        Format::Jpg => decode_jpg(bytes),
        Format::Png => decode_png(bytes),
        Format::Tga => decode_tga(bytes),
    }
}

/// The result of resolving a texture name.
#[derive(Debug)]
pub struct Loaded {
    /// The file that decoded, e.g. `textures/kejim/floor.jpg` for `textures/kejim/floor.tga`.
    pub path: String,
    pub image: Image,
    /// Candidates that existed but failed to decode, in the order tried.
    pub failures: Vec<(String, Error)>,
}

/// Resolves and decodes a texture name like the original `R_LoadImage`. The error
/// lists the candidates that existed but failed to decode; it is empty when none of
/// them exist.
pub fn load(vfs: &Vfs, name: &str) -> Result<Loaded, Vec<(String, Error)>> {
    let (stem, own) = match name.rsplit_once('.') {
        Some((stem, ext)) if !ext.contains('/') => (stem, Format::from_extension(ext)),
        _ => (name, None),
    };
    let candidates = own.map(|f| (name.to_owned(), f)).into_iter().chain(
        Format::ORDER
            .into_iter()
            .filter(|f| Some(*f) != own)
            .map(|f| (format!("{stem}.{}", f.extension()), f)),
    );
    let mut failures = Vec::new();
    for (path, format) in candidates {
        let Ok(bytes) = vfs.read(&path) else { continue };
        match decode(format, &bytes) {
            Ok(image) => {
                return Ok(Loaded {
                    path,
                    image,
                    failures,
                });
            }
            Err(e) => failures.push((path, e)),
        }
    }
    Err(failures)
}

/// TGA types 2 (true colour), 3 (grey) and 10 (RLE true colour), as `LoadTGA`
/// accepts them. Quirks kept from the original, since they decide how textures look:
/// - the right-to-left flag (descriptor bit 0x10) is ignored: rows are never mirrored;
/// - 8-bit pixels are grey in type 2 as well, and 24/32-bit pixels are colour in type 3;
/// - type 10 must be bottom-to-top, 24 or 32 bits.
pub fn decode_tga(bytes: &[u8]) -> Result<Image, Error> {
    if bytes.len() < 18 {
        bail!("TGA: header truncated");
    }
    let u16_at = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    let (id_len, colormap_type, image_type) = (bytes[0], bytes[1], bytes[2]);
    let (first_entry, colormap_len, colormap_entry) = (u16_at(3), u16_at(5), bytes[7]);
    let (width, height, bits, descriptor) = (u16_at(12), u16_at(14), bytes[16], bytes[17]);

    if colormap_type != 0 || first_entry != 0 {
        bail!("TGA: colourmaps not supported");
    }
    if !matches!(image_type, 2 | 3 | 10) {
        bail!("TGA: type {image_type} not supported (only 2, 3 and 10)");
    }
    if colormap_len != 0 && colormap_len != 256 {
        bail!("TGA: colourmap length {colormap_len} must be 0 or 256");
    }
    if colormap_entry != 0 && colormap_entry != 24 {
        bail!("TGA: colourmap entry size {colormap_entry} must be 0 or 24");
    }
    let channels = match bits {
        8 if image_type != 10 => 1,
        24 => 3,
        32 => 4,
        _ => bail!("TGA: {bits}-bit type {image_type} images not supported"),
    };
    let top_down = descriptor & 0x20 != 0;
    if image_type == 10 && descriptor & 0x30 != 0 {
        bail!("TGA: RLE images (type 10) must be bottom to top");
    }

    let (w, h) = (usize::from(width), usize::from(height));
    let mut pixels = &bytes[18 + usize::from(id_len).min(bytes.len() - 18)..];
    let mut rgba = vec![0u8; w * h * 4];
    let mut put = |i: usize, p: &[u8]| {
        // Pixel i in file order; rows are stored bottom first unless top_down.
        let (row, col) = (i / w, i % w);
        let y = if top_down { row } else { h - 1 - row };
        let out = &mut rgba[(y * w + col) * 4..][..4];
        match *p {
            [g] => out.copy_from_slice(&[g, g, g, 255]),
            [b, g, r] => out.copy_from_slice(&[r, g, b, 255]),
            [b, g, r, a] => out.copy_from_slice(&[r, g, b, a]),
            _ => unreachable!(),
        }
    };
    let truncated = || Error("TGA: pixel data truncated".into());
    if image_type == 10 {
        let mut i = 0;
        while i < w * h {
            let (&header, rest) = pixels.split_first().ok_or_else(truncated)?;
            let count = usize::from(header & 0x7f) + 1;
            let n = count.min(w * h - i);
            if header & 0x80 != 0 {
                let p = rest.get(..channels).ok_or_else(truncated)?;
                for k in 0..n {
                    put(i + k, p);
                }
                pixels = &rest[channels..];
            } else {
                let run = rest.get(..count * channels).ok_or_else(truncated)?;
                for (k, p) in run.chunks_exact(channels).take(n).enumerate() {
                    put(i + k, p);
                }
                pixels = &rest[count * channels..];
            }
            i += n;
        }
    } else {
        let data = pixels.get(..w * h * channels).ok_or_else(truncated)?;
        for (i, p) in data.chunks_exact(channels).enumerate() {
            put(i, p);
        }
    }
    Ok(Image {
        width: width.into(),
        height: height.into(),
        rgba,
    })
}

/// Baseline and progressive JPEG, decoded to RGB (grey expands to RGB) with alpha 255,
/// as `LoadJPG` asks libjpeg for.
pub fn decode_jpg(bytes: &[u8]) -> Result<Image, Error> {
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    let rgba = decoder
        .decode()
        .map_err(|e| Error(format!("JPEG: {e:?}")))?;
    let info = decoder
        .info()
        .ok_or_else(|| Error("JPEG: no image info".into()))?;
    image(info.width as usize, info.height as usize, rgba)
}

/// 8-bit RGB or RGBA PNGs with power-of-two sides; RGB gets alpha 255. `LoadPNG`
/// rejects everything else (grey, palette, other depths), and so does this.
pub fn decode_png(bytes: &[u8]) -> Result<Image, Error> {
    let options = DecoderOptions::default().png_set_add_alpha_channel(true);
    let mut decoder = zune_png::PngDecoder::new_with_options(ZCursor::new(bytes), options);
    decoder
        .decode_headers()
        .map_err(|e| Error(format!("PNG: {e:?}")))?;
    let info = decoder
        .info()
        .ok_or_else(|| Error("PNG: no header".into()))?;
    let (w, h) = (info.width, info.height);
    if !w.is_power_of_two() || !h.is_power_of_two() {
        bail!("PNG: {w}x{h} is not a power of two");
    }
    // IHDR is always the first chunk: signature (8), length (4), "IHDR" (4), width (4),
    // height (4), then bit depth and colour type (2 = RGB, 6 = RGBA).
    let (depth, colour) = (bytes[24], bytes[25]);
    if depth != 8 || !matches!(colour, 2 | 6) {
        bail!("PNG: only 8-bit RGB and RGBA are supported (depth {depth}, colour type {colour})");
    }
    let rgba = decoder
        .decode_raw()
        .map_err(|e| Error(format!("PNG: {e:?}")))?;
    image(w, h, rgba)
}

fn image(w: usize, h: usize, rgba: Vec<u8>) -> Result<Image, Error> {
    if rgba.len() != w * h * 4 {
        bail!("decoded {} bytes for {w}x{h} RGBA", rgba.len());
    }
    Ok(Image {
        width: w as u32,
        height: h as u32,
        rgba,
    })
}
