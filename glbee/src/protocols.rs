use std::collections::HashMap;
use std::env;
use std::io::{self, Write};

const ESC: &str = "\x1b";
const ST: &str = "\x1b\\";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[derive(Debug)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<Option<Rgb>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Protocol {
    Auto,
    Kitty,
    Iterm2,
    Sixel,
    Blocks,
}

impl Protocol {
    pub fn name(self) -> &'static str {
        match self {
            Protocol::Auto => "auto",
            Protocol::Kitty => "kitty",
            Protocol::Iterm2 => "iterm2",
            Protocol::Sixel => "sixel",
            Protocol::Blocks => "blocks",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub x: usize,
    pub y: usize,
    pub columns: usize,
    pub rows: usize,
}

pub fn write_image(
    image: &Image,
    protocol: Protocol,
    max_colors: usize,
    placement: Option<Placement>,
) -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    write_image_to(&mut stdout, image, protocol, max_colors, placement)?;
    stdout.flush().map_err(|err| err.to_string())
}

pub fn write_image_to<W: Write>(
    output: &mut W,
    image: &Image,
    protocol: Protocol,
    max_colors: usize,
    placement: Option<Placement>,
) -> Result<(), String> {
    let text = match protocol {
        Protocol::Kitty => pixels_to_kitty(image, placement),
        Protocol::Iterm2 => pixels_to_iterm2(image, placement)?,
        Protocol::Sixel => pixels_to_sixel(image, max_colors)?,
        Protocol::Blocks => pixels_to_blocks(image, placement),
        Protocol::Auto => unreachable!("protocol was resolved"),
    };
    let text = wrap_for_tmux_if_needed(protocol, text);
    output
        .write_all(text.as_bytes())
        .map_err(|err| err.to_string())
}

fn wrap_for_tmux_if_needed(protocol: Protocol, text: String) -> String {
    if env::var_os("TMUX").is_none() || matches!(protocol, Protocol::Blocks) {
        return text;
    }

    let mut wrapped = String::with_capacity(text.len() + 16);
    wrapped.push_str(ESC);
    wrapped.push_str("Ptmux;");
    for byte in text.bytes() {
        if byte == 0x1b {
            wrapped.push_str(ESC);
            wrapped.push_str(ESC);
        } else {
            wrapped.push(byte as char);
        }
    }
    wrapped.push_str(ST);
    wrapped
}

fn pixels_to_kitty(image: &Image, placement: Option<Placement>) -> String {
    let mut rgb = Vec::with_capacity(image.width * image.height * 3);
    for pixel in &image.pixels {
        match pixel {
            Some(color) => rgb.extend_from_slice(&[color.r, color.g, color.b]),
            None => rgb.extend_from_slice(&[0, 0, 0]),
        }
    }
    let encoded = base64_encode(&rgb);
    let placement = placement
        .map(|area| format!(",c={},r={}", area.columns, area.rows))
        .unwrap_or_default();
    format!(
        "{ESC}_Ga=T,f=24,s={},v={},m=0{placement};{encoded}{ESC}\\",
        image.width, image.height
    )
}

fn pixels_to_iterm2(image: &Image, placement: Option<Placement>) -> Result<String, String> {
    let png = pixels_to_png(image)?;
    let encoded = base64_encode(&png);
    let (width, height) = if placement.is_some() {
        (format!("{}px", image.width), format!("{}px", image.height))
    } else {
        (format!("{}px", image.width), format!("{}px", image.height))
    };
    Ok(format!(
        "{ESC}]1337;File=inline=1;width={width};height={height};preserveAspectRatio=0:{encoded}\x07"
    ))
}

fn pixels_to_png(image: &Image) -> Result<Vec<u8>, String> {
    let mut data = Vec::with_capacity(image.width * image.height * 4);
    for pixel in &image.pixels {
        match pixel {
            Some(color) => data.extend_from_slice(&[color.r, color.g, color.b, 255]),
            None => data.extend_from_slice(&[0, 0, 0, 0]),
        }
    }
    let mut encoded = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut encoded, image.width as u32, image.height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|err| err.to_string())?;
        writer
            .write_image_data(&data)
            .map_err(|err| err.to_string())?;
    }
    Ok(encoded)
}

fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        output.push(TABLE[(b0 >> 2) as usize] as char);
        output.push(TABLE[(((b0 & 0b11) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            output.push(TABLE[(((b1 & 0b1111) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(TABLE[(b2 & 0b11_1111) as usize] as char);
        } else {
            output.push('=');
        }
    }
    output
}

fn pixels_to_blocks(image: &Image, placement: Option<Placement>) -> String {
    let mut output = String::with_capacity(image.width * image.height * 16);
    let row_count = image.height.div_ceil(2);
    for (row_index, y) in (0..image.height).step_by(2).enumerate() {
        if let Some(area) = placement {
            push_cursor_move(&mut output, area.x, area.y + row_index);
        }
        for x in 0..image.width {
            let top = image.pixels[y * image.width + x];
            let bottom = if y + 1 < image.height {
                image.pixels[(y + 1) * image.width + x]
            } else {
                None
            };
            push_block_pixel(&mut output, top, bottom);
        }
        output.push_str(ESC);
        output.push_str("[0m");
        if row_index + 1 < row_count {
            output.push('\n');
        }
    }
    output
}

fn push_cursor_move(output: &mut String, x: usize, y: usize) {
    output.push_str(&format!("{ESC}[{};{}H", y + 1, x + 1));
}

fn push_block_pixel(output: &mut String, top: Option<Rgb>, bottom: Option<Rgb>) {
    match (top, bottom) {
        (Some(fg), Some(bg)) => {
            push_fg(output, fg);
            push_bg(output, bg);
            output.push('▀');
        }
        (Some(fg), None) => {
            push_reset_bg(output);
            push_fg(output, fg);
            output.push('▀');
        }
        (None, Some(fg)) => {
            push_reset_bg(output);
            push_fg(output, fg);
            output.push('▄');
        }
        (None, None) => {
            push_reset(output);
            output.push(' ');
        }
    }
}

fn push_fg(output: &mut String, color: Rgb) {
    output.push_str(&format!("{ESC}[38;2;{};{};{}m", color.r, color.g, color.b));
}

fn push_bg(output: &mut String, color: Rgb) {
    output.push_str(&format!("{ESC}[48;2;{};{};{}m", color.r, color.g, color.b));
}

fn push_reset_bg(output: &mut String) {
    output.push_str(ESC);
    output.push_str("[49m");
}

fn push_reset(output: &mut String) {
    output.push_str(ESC);
    output.push_str("[0m");
}

fn pixels_to_sixel(image: &Image, max_colors: usize) -> Result<String, String> {
    if image.width == 0 || image.height == 0 {
        return Err("image dimensions must be positive".into());
    }
    if image.pixels.len() != image.width * image.height {
        return Err("pixel count does not match dimensions".into());
    }

    let (palette, indexed) = index_pixels(&image.pixels, max_colors);
    let palette = if palette.is_empty() {
        vec![Rgb { r: 0, g: 0, b: 0 }]
    } else {
        palette
    };

    let mut parts = String::new();
    parts.push_str(ESC);
    parts.push_str("P0;1;0q");
    parts.push_str(&format!("\"1;1;{};{}", image.width, image.height));
    for (index, color) in palette.iter().enumerate() {
        parts.push_str(&format!(
            "#{};2;{};{};{}",
            index + 1,
            color.r as usize * 100 / 255,
            color.g as usize * 100 / 255,
            color.b as usize * 100 / 255
        ));
    }

    for y0 in (0..image.height).step_by(6) {
        let mut masks = vec![vec![0u8; image.width]; palette.len()];
        let mut any = false;
        for dy in 0..6 {
            let y = y0 + dy;
            if y >= image.height {
                break;
            }
            let bit = 1u8 << dy;
            for x in 0..image.width {
                if let Some(color_index) = indexed[y * image.width + x] {
                    masks[color_index - 1][x] |= bit;
                    any = true;
                }
            }
        }
        if !any {
            parts.push('-');
            continue;
        }
        for (color_index, row_masks) in masks.iter().enumerate() {
            if row_masks.iter().all(|bits| *bits == 0) {
                continue;
            }
            let row = row_masks
                .iter()
                .map(|bits| (63 + bits) as char)
                .collect::<String>();
            parts.push_str(&format!("#{}", color_index + 1));
            parts.push_str(&rle_sixel(&row));
            parts.push('$');
        }
        if parts.ends_with('$') {
            parts.pop();
        }
        parts.push('-');
    }
    if parts.ends_with('-') {
        parts.pop();
    }
    parts.push_str(ST);
    Ok(parts)
}

fn index_pixels(pixels: &[Option<Rgb>], max_colors: usize) -> (Vec<Rgb>, Vec<Option<usize>>) {
    let mut counts: HashMap<Rgb, usize> = HashMap::new();
    for pixel in pixels.iter().flatten() {
        *counts.entry(*pixel).or_insert(0) += 1;
    }

    let mut colors = counts.into_iter().collect::<Vec<(Rgb, usize)>>();
    colors.sort_by(|a, b| b.1.cmp(&a.1));

    let palette = if colors.len() <= max_colors {
        colors.iter().map(|(color, _)| *color).collect::<Vec<_>>()
    } else {
        quantized_palette(&colors, max_colors)
    };

    let mut exact = HashMap::new();
    for (index, color) in palette.iter().enumerate() {
        exact.insert(*color, index + 1);
    }

    let mut nearest_cache = HashMap::new();
    let indexed = pixels
        .iter()
        .map(|pixel| {
            let color = (*pixel)?;
            if let Some(index) = exact.get(&color) {
                return Some(*index);
            }
            if let Some(index) = nearest_cache.get(&color) {
                return Some(*index);
            }
            let nearest = nearest_palette_index(color, &palette);
            nearest_cache.insert(color, nearest);
            Some(nearest)
        })
        .collect();

    (palette, indexed)
}

fn quantized_palette(colors: &[(Rgb, usize)], max_colors: usize) -> Vec<Rgb> {
    let levels = (max_colors as f64).cbrt().floor().max(2.0) as usize;
    let bucket_count = levels * levels * levels;
    let mut buckets = vec![(0usize, 0usize, 0usize, 0usize); bucket_count];
    for (color, count) in colors {
        let ri = color.r as usize * levels / 256;
        let gi = color.g as usize * levels / 256;
        let bi = color.b as usize * levels / 256;
        let index = (ri * levels + gi) * levels + bi;
        let bucket = &mut buckets[index.min(bucket_count - 1)];
        bucket.0 += color.r as usize * count;
        bucket.1 += color.g as usize * count;
        bucket.2 += color.b as usize * count;
        bucket.3 += count;
    }

    let mut palette = buckets
        .into_iter()
        .filter(|bucket| bucket.3 > 0)
        .map(|bucket| {
            (
                Rgb {
                    r: (bucket.0 / bucket.3) as u8,
                    g: (bucket.1 / bucket.3) as u8,
                    b: (bucket.2 / bucket.3) as u8,
                },
                bucket.3,
            )
        })
        .collect::<Vec<_>>();
    palette.sort_by(|a, b| b.1.cmp(&a.1));
    palette.truncate(max_colors);
    palette.into_iter().map(|(color, _)| color).collect()
}

fn nearest_palette_index(color: Rgb, palette: &[Rgb]) -> usize {
    let mut best_index = 1usize;
    let mut best_distance = u32::MAX;
    for (index, candidate) in palette.iter().enumerate() {
        let dr = color.r as i32 - candidate.r as i32;
        let dg = color.g as i32 - candidate.g as i32;
        let db = color.b as i32 - candidate.b as i32;
        let distance = (dr * dr + dg * dg + db * db) as u32;
        if distance < best_distance {
            best_distance = distance;
            best_index = index + 1;
        }
    }
    best_index
}

fn rle_sixel(row: &str) -> String {
    let mut output = String::new();
    let mut chars = row.chars();
    let Some(mut current) = chars.next() else {
        return output;
    };
    let mut count = 1usize;
    for ch in chars {
        if ch == current {
            count += 1;
        } else {
            push_run(&mut output, current, count);
            current = ch;
            count = 1;
        }
    }
    push_run(&mut output, current, count);
    output
}

fn push_run(output: &mut String, ch: char, count: usize) {
    if count >= 4 {
        output.push('!');
        output.push_str(&count.to_string());
        output.push(ch);
    } else {
        for _ in 0..count {
            output.push(ch);
        }
    }
}
