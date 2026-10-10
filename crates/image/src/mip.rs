//! Mipmaps as the original makes them on the CPU (OpenJK `rd-vanilla/tr_image.cpp`
//! `Upload32` with `r_simpleMipMaps 1`, the default): each level is the previous one
//! halved by `R_MipMap`, which averages 2 x 2 blocks with truncation, or pairs once
//! one side has reached 1 pixel.

/// `R_MipMap` with `r_simpleMipMaps 1`: `src` (`width x height` RGBA pixels, not
/// both 1) halved into `dst`. The original works in place; for power-of-two sides it
/// never reads a pixel it has already overwritten, so this is the same.
fn mip_map(src: &[u8], width: usize, height: usize, dst: &mut [u8]) {
    let row = width * 4;
    let (w, h) = (width >> 1, height >> 1);
    if w == 0 || h == 0 {
        for (i, out) in dst
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .take(w + h)
            .enumerate()
        {
            let p = &src[i * 8..i * 8 + 8];
            for c in 0..4 {
                out[c] = ((u32::from(p[c]) + u32::from(p[4 + c])) >> 1) as u8;
            }
        }
        return;
    }
    for (i, out_row) in dst.chunks_exact_mut(w * 4).take(h).enumerate() {
        let top = &src[2 * i * row..2 * i * row + row];
        let bottom = &src[(2 * i + 1) * row..(2 * i + 2) * row];
        for (j, out) in out_row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let (t, b) = (&top[j * 8..j * 8 + 8], &bottom[j * 8..j * 8 + 8]);
            for c in 0..4 {
                let sum =
                    u32::from(t[c]) + u32::from(t[4 + c]) + u32::from(b[c]) + u32::from(b[4 + c]);
                out[c] = (sum >> 2) as u8;
            }
        }
    }
}

/// The bytes of every mip level of a `width x height` RGBA image together.
pub fn chain_size(width: u32, height: u32) -> usize {
    let (mut w, mut h, mut size) = (width as usize, height as usize, 0);
    loop {
        size += w * h * 4;
        if w == 1 && h == 1 {
            return size;
        }
        w = (w >> 1).max(1);
        h = (h >> 1).max(1);
    }
}

/// Writes every mip level of a `width x height` RGBA image, largest first, down to
/// 1 x 1, one after another into `out` (`chain_size` bytes): `Upload32`'s loop, whose
/// levels clamp each side at 1.
pub fn mip_chain_into(rgba: &[u8], width: u32, height: u32, out: &mut [u8]) {
    let (mut w, mut h) = (width as usize, height as usize);
    out[..rgba.len()].copy_from_slice(rgba);
    let mut at = 0;
    while w > 1 || h > 1 {
        let size = w * h * 4;
        let (done, rest) = out[at..].split_at_mut(size);
        mip_map(done, w, h, rest);
        w = (w >> 1).max(1);
        h = (h >> 1).max(1);
        at += size;
    }
}

/// Every mip level, largest first (`mip_chain_into`, one level per vector).
pub fn mip_chain(rgba: &[u8], width: u32, height: u32) -> Vec<Vec<u8>> {
    let mut all = vec![0; chain_size(width, height)];
    mip_chain_into(rgba, width, height, &mut all);
    let (mut w, mut h, mut at) = (width as usize, height as usize, 0);
    let mut levels = Vec::new();
    loop {
        levels.push(all[at..at + w * h * 4].to_vec());
        at += w * h * 4;
        if w == 1 && h == 1 {
            return levels;
        }
        w = (w >> 1).max(1);
        h = (h >> 1).max(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_average_2x2_blocks_with_truncation() {
        // 2 x 2: (1 + 2 + 3 + 5) >> 2 = 2 (11 / 4 truncated) in every channel.
        let px = |v: u8| [v, v, v, v];
        let image: Vec<u8> = [px(1), px(2), px(3), px(5)].concat();
        let chain = mip_chain(&image, 2, 2);
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[1], vec![2, 2, 2, 2]);
    }

    #[test]
    fn non_square_images_average_pairs_once_a_side_is_one() {
        // 4 x 1: halving gives 2 x 1 by pairs, then 1 x 1.
        let image: Vec<u8> = [[0u8; 4], [10; 4], [20; 4], [255; 4]].concat();
        let chain = mip_chain(&image, 4, 1);
        assert_eq!(chain.len(), 3);
        assert_eq!(chain[1], [[5u8; 4], [137; 4]].concat()); // (20 + 255) >> 1 = 137
        assert_eq!(chain[2], vec![71; 4]); // (5 + 137) >> 1
        // 4 x 4 makes 3 levels, the last one pixel.
        assert_eq!(
            mip_chain(&[0; 64], 4, 4)
                .iter()
                .map(Vec::len)
                .collect::<Vec<_>>(),
            [64, 16, 4]
        );
    }
}
