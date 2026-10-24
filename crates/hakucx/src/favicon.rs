/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Favicons from Servo's raster images to small PNG data URLs for the UI.

use base64::Engine;
use image::{ImageBuffer, Rgba, RgbaImage, imageops};
use servo::{Image, PixelFormat};

const SIZE: u32 = 32;

pub fn to_data_url(image: &Image) -> Option<String> {
    let (w, h) = (image.width, image.height);
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return None;
    }
    let data = image.data();
    let mut rgba: Vec<u8> = Vec::with_capacity((w * h * 4) as usize);
    match image.format {
        PixelFormat::RGBA8 => rgba.extend_from_slice(data.get(..(w * h * 4) as usize)?),
        PixelFormat::BGRA8 => {
            for px in data.get(..(w * h * 4) as usize)?.as_chunks::<4>().0 {
                rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
            }
        }
        PixelFormat::RGB8 => {
            for px in data.get(..(w * h * 3) as usize)?.as_chunks::<3>().0 {
                rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
        }
        PixelFormat::K8 => {
            for &k in data.get(..(w * h) as usize)? {
                rgba.extend_from_slice(&[k, k, k, 255]);
            }
        }
        PixelFormat::KA8 => {
            for px in data.get(..(w * h * 2) as usize)?.as_chunks::<2>().0 {
                rgba.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
            }
        }
    }
    let img: RgbaImage = ImageBuffer::<Rgba<u8>, _>::from_raw(w, h, rgba)?;
    let img = if w > SIZE || h > SIZE {
        imageops::resize(&img, SIZE, SIZE, imageops::FilterType::Triangle)
    } else {
        img
    };
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    ))
}
