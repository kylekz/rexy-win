//! Writes `assets/rexy.ico`, the program's icon on Windows, from Rex's own icon in
//! `assets/windows/rex.png` if that is there, or else from Rexy's drawing:
//! `cargo run --example icon`. Neither the icon nor Rex's is kept in the
//! repository, so `scripts/package.ps1` runs this before every build.
use std::path::Path;

const SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

/// A square RGBA picture, premultiplied, its channels from 0 to 1.
struct Picture {
    side: u32,
    pixels: Vec<[f32; 4]>,
}

fn decode(png: &[u8]) -> Picture {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().expect("a PNG");
    let mut rgba = vec![0; reader.output_buffer_size().expect("a size")];
    let frame = reader.next_frame(&mut rgba).expect("a PNG");
    assert_eq!(frame.color_type, png::ColorType::Rgba, "an RGBA picture");
    assert_eq!(frame.width, frame.height, "a square picture");
    let pixels = rgba[..frame.buffer_size()].chunks(4).map(|pixel| {
        let alpha = f32::from(pixel[3]) / 255.0;
        let channel = |value: u8| f32::from(value) / 255.0 * alpha;
        [channel(pixel[0]), channel(pixel[1]), channel(pixel[2]), alpha]
    });
    Picture { side: frame.width, pixels: pixels.collect() }
}

/// Which source pixels each of `to` takes from `from`, and how much of each: the
/// share of it the smaller pixel covers.
fn spans(from: u32, to: u32) -> Vec<Vec<(usize, f32)>> {
    let ratio = from as f32 / to as f32;
    (0..to)
        .map(|out| {
            let (start, end) = (out as f32 * ratio, (out + 1) as f32 * ratio);
            let first = start.floor() as usize;
            let last = (end.ceil() as usize).min(from as usize);
            (first..last)
                .map(|at| {
                    let covered = end.min(at as f32 + 1.0) - start.max(at as f32);
                    (at, covered / ratio)
                })
                .collect()
        })
        .collect()
}

/// The picture made smaller by averaging every pixel each new one covers, across
/// then down, which keeps its fine lines without the shimmer of sampling.
fn shrink(picture: &Picture, side: u32) -> Picture {
    let spans = spans(picture.side, side);
    let (from, to) = (picture.side as usize, side as usize);
    let mut across = vec![[0.0; 4]; to * from];
    for y in 0..from {
        for (x, span) in spans.iter().enumerate() {
            for &(at, weight) in span {
                let pixel = picture.pixels[y * from + at];
                (0..4).for_each(|c| across[y * to + x][c] += pixel[c] * weight);
            }
        }
    }
    let mut pixels = vec![[0.0; 4]; to * to];
    for (y, span) in spans.iter().enumerate() {
        for x in 0..to {
            for &(at, weight) in span {
                let pixel = across[at * to + x];
                (0..4).for_each(|c| pixels[y * to + x][c] += pixel[c] * weight);
            }
        }
    }
    Picture { side, pixels }
}

/// Straight RGBA bytes, as files hold them.
fn bytes(picture: &Picture) -> Vec<u8> {
    let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    let pixel = |&[r, g, b, a]: &[f32; 4]| {
        let straight = |value: f32| if a > 0.0 { value / a } else { 0.0 };
        [byte(straight(r)), byte(straight(g)), byte(straight(b)), byte(a)]
    };
    picture.pixels.iter().flat_map(pixel).collect()
}

/// A bitmap as an icon holds it: BGRA rows from the bottom up, then a mask of the
/// pixels that are wholly clear, for whatever reads no alpha. Older readers take
/// only these for the smaller sizes.
fn bitmap(side: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mask_row = side.div_ceil(32) * 4;
    let image = side * side * 4 + mask_row * side;
    for field in [40, side, side * 2] {
        out.extend_from_slice(&field.to_le_bytes());
    }
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    for field in [0, image, 0, 0, 0, 0] {
        out.extend_from_slice(&field.to_le_bytes());
    }
    let rows: Vec<&[u8]> = rgba.chunks(side as usize * 4).rev().collect();
    for row in &rows {
        for pixel in row.chunks(4) {
            out.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    for row in &rows {
        let mut mask = vec![0u8; mask_row as usize];
        for (x, pixel) in row.chunks(4).enumerate() {
            if pixel[3] == 0 {
                mask[x / 8] |= 0x80 >> (x % 8);
            }
        }
        out.extend_from_slice(&mask);
    }
    out
}

/// Rex's icon, if it is at hand, or else Rexy's own drawing: a PNG 256 pixels across.
fn picture(root: &Path) -> Vec<u8> {
    if let Ok(png) = std::fs::read(root.join("assets/windows/rex.png")) {
        return png;
    }
    use resvg::{tiny_skia, usvg};
    let svg = std::fs::read_to_string(root.join("assets/dev.rexy.client.svg")).unwrap();
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default()).expect("a sound drawing");
    let mut canvas = tiny_skia::Pixmap::new(256, 256).unwrap();
    let scale = 256.0 / tree.size().width();
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut canvas.as_mut());
    let pixel = |pixel: &tiny_skia::PremultipliedColorU8| {
        let color = pixel.demultiply();
        [color.red(), color.green(), color.blue(), color.alpha()]
    };
    let rgba: Vec<u8> = canvas.pixels().iter().flat_map(pixel).collect();
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, 256, 256);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&rgba).unwrap();
    writer.finish().unwrap();
    png
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let png = picture(root);
    let picture = decode(&png);
    assert_eq!(picture.side, 256, "the icon is 256 pixels across");
    let images: Vec<(u32, Vec<u8>)> = SIZES
        .iter()
        .map(|&side| match side {
            // The largest is the PNG as it is, which Windows has read since Vista.
            256 => (side, png.clone()),
            _ => (side, bitmap(side, &bytes(&shrink(&picture, side)))),
        })
        .collect();
    let mut out = Vec::new();
    for field in [0u16, 1, images.len() as u16] {
        out.extend_from_slice(&field.to_le_bytes());
    }
    let mut offset = 6 + 16 * images.len() as u32;
    for (side, image) in &images {
        // A side of 256 is written as 0.
        let byte = (*side % 256) as u8;
        out.extend_from_slice(&[byte, byte, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(image.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += image.len() as u32;
    }
    for (_, image) in &images {
        out.extend_from_slice(image);
    }
    let path = root.join("assets").join("rexy.ico");
    std::fs::write(&path, out).unwrap();
    println!("{} ({offset} bytes)", path.display());
}
