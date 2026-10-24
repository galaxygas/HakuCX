/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! The HakuCX icon, drawn in code so every size is sharp: a rounded square with a
//! violet-to-cyan gradient and a white "H", with the bottom-right corner cut (the GX notch).
//! No dependencies, so the build script can embed it in the executable.

/// RGBA pixels (straight alpha), `size` x `size`.
pub fn icon_rgba(size: u32) -> Vec<u8> {
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let s = size as f32;
    let radius = s * 0.22;
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            // Rounded-rect coverage.
            let dx = (radius - fx).max(fx - (s - radius)).max(0.0);
            let dy = (radius - fy).max(fy - (s - radius)).max(0.0);
            let corner = (dx * dx + dy * dy).sqrt();
            let inside = (radius - corner + 0.5).clamp(0.0, 1.0);
            // Notch: cut the bottom-right corner.
            let notch = if fx + fy > s * 1.72 { 0.0 } else { 1.0 };
            let alpha = inside * notch;
            let t = (fx + fy) / (2.0 * s);
            let (r, g, b) = (
                (0x7d as f32 * (1.0 - t) + 0x2e as f32 * t) as u8,
                (0x46 as f32 * (1.0 - t) + 0xc8 as f32 * t) as u8,
                (0xf2 as f32 * (1.0 - t) + 0xff as f32 * t) as u8,
            );
            // White "H": two stems and a bar.
            let u = fx / s;
            let v = fy / s;
            let stem = (0.26..0.40).contains(&u) || (0.60..0.74).contains(&u);
            let bar = (0.46..0.56).contains(&v) && (0.26..0.74).contains(&u);
            let glyph = (stem && (0.24..0.76).contains(&v)) || bar;
            let i = ((y * size + x) * 4) as usize;
            let (r, g, b) = if glyph { (255, 255, 255) } else { (r, g, b) };
            rgba[i] = r;
            rgba[i + 1] = g;
            rgba[i + 2] = b;
            rgba[i + 3] = (alpha * 255.0) as u8;
        }
    }
    rgba
}

/// The sizes Windows uses for program icons.
pub const ICO_SIZES: &[u32] = &[16, 20, 24, 32, 40, 48, 64, 96, 128, 256];

/// A Windows `.ico` with uncompressed 32-bit images at each size.
pub fn ico(sizes: &[u32]) -> Vec<u8> {
    let images: Vec<Vec<u8>> = sizes.iter().map(|&s| dib(s)).collect();
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved
    out.extend_from_slice(&1u16.to_le_bytes()); // type: icon
    out.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (&size, image) in sizes.iter().zip(&images) {
        let dim = if size >= 256 { 0 } else { size as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes()); // planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&(image.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += image.len() as u32;
    }
    for image in images {
        out.extend(image);
    }
    out
}

/// One icon image: BITMAPINFOHEADER, bottom-up BGRA pixels, then an empty AND mask.
fn dib(size: u32) -> Vec<u8> {
    let rgba = icon_rgba(size);
    let mut out = Vec::new();
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(size as i32).to_le_bytes());
    out.extend_from_slice(&((size * 2) as i32).to_le_bytes()); // XOR image + AND mask
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0u8; 24]); // BI_RGB, sizes and palette left zero
    for y in (0..size).rev() {
        for x in 0..size {
            let i = ((y * size + x) * 4) as usize;
            out.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
        }
    }
    let mask_row = (size.div_ceil(32) * 4) as usize;
    out.extend(std::iter::repeat_n(0u8, mask_row * size as usize));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ico_layout() {
        let data = ico(&[16, 256]);
        assert_eq!(&data[..6], &[0, 0, 1, 0, 2, 0]);
        // Second entry: 256 is written as 0; its image starts after the first.
        assert_eq!(data[6 + 16], 0);
        let first_len = u32::from_le_bytes(data[14..18].try_into().unwrap()) as usize;
        let second_offset =
            u32::from_le_bytes(data[6 + 16 + 12..6 + 16 + 16].try_into().unwrap()) as usize;
        assert_eq!(second_offset, 6 + 32 + first_len);
        assert_eq!(first_len, 40 + 16 * 16 * 4 + 4 * 16);
        // The middle of the icon is opaque, the cut corner transparent.
        let px = icon_rgba(64);
        assert_eq!(px[(32 * 64 + 10) * 4 + 3], 255);
        assert_eq!(px[(63 * 64 + 63) * 4 + 3], 0);
    }
}
