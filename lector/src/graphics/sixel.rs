use std::io::{self, Write};

use crate::graphics::frame::{Frame, Rgba};

const LUT_SIZE: usize = 32 * 32 * 32;

pub struct SixelBackend {
    palette: Vec<Rgba>,
    palette_lut: Box<[u8; LUT_SIZE]>,
    encoded: Vec<u8>,
    indexed: Vec<u8>,
    band_colors: Vec<u8>,
    tmux_mode: Option<TmuxSixelMode>,
}

impl SixelBackend {
    pub fn new() -> Self {
        let palette = build_palette();
        let palette_lut = build_palette_lut(&palette);
        Self {
            palette,
            palette_lut,
            encoded: Vec::new(),
            indexed: Vec::new(),
            band_colors: Vec::new(),
            tmux_mode: detect_tmux_sixel_mode(),
        }
    }

    pub fn render(&mut self, frame: &Frame, out: &mut dyn Write) -> io::Result<()> {
        self.encoded.clear();
        self.render_raw(frame)?;
        match self.tmux_mode {
            Some(TmuxSixelMode::Passthrough) => write_tmux_passthrough(&self.encoded, out),
            Some(TmuxSixelMode::Native) | None => out.write_all(&self.encoded),
        }
    }

    fn render_raw(&mut self, frame: &Frame) -> io::Result<()> {
        let out = &mut self.encoded;
        write!(out, "\x1bP0;1;0q\"1;1;{};{}", frame.width(), frame.height())?;
        for (index, color) in self.palette.iter().enumerate() {
            write!(
                out,
                "#{};2;{};{};{}",
                index + 1,
                color.r as u16 * 100 / 255,
                color.g as u16 * 100 / 255,
                color.b as u16 * 100 / 255
            )?;
        }

        let width = frame.width() as usize;
        let height = frame.height() as usize;
        quantize(frame, &self.palette_lut, &mut self.indexed);

        for band_y in (0..height).step_by(6) {
            collect_band_colors(
                &self.indexed,
                width,
                height,
                band_y,
                self.palette.len(),
                &mut self.band_colors,
            );

            for &color_index in &self.band_colors {
                write!(out, "#{}", color_index as usize + 1)?;
                let mut run_char = 0_u8;
                let mut run_len = 0_usize;

                for x in 0..width {
                    let mut bits = 0_u8;
                    for bit in 0..6 {
                        let y = band_y + bit;
                        if y < height && self.indexed[y * width + x] == color_index {
                            bits |= 1 << bit;
                        }
                    }
                    let ch = 63 + bits;
                    write_sixel_run(out, &mut run_char, &mut run_len, ch)?;
                }
                flush_sixel_run(out, run_char, run_len)?;
                write!(out, "$")?;
            }
            if band_y + 6 < height {
                write!(out, "-")?;
            }
        }

        write!(out, "\x1b\\")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TmuxSixelMode {
    Native,
    Passthrough,
}

fn detect_tmux_sixel_mode() -> Option<TmuxSixelMode> {
    std::env::var_os("TMUX")?;

    match std::env::var("LECTOR_TMUX_SIXEL")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "native" | "raw" => return Some(TmuxSixelMode::Native),
        "passthrough" | "pass-through" | "wrap" => return Some(TmuxSixelMode::Passthrough),
        "off" | "none" => return None,
        _ => {}
    }

    Some(TmuxSixelMode::Passthrough)
}

fn quantize(frame: &Frame, palette_lut: &[u8; LUT_SIZE], indexed: &mut Vec<u8>) {
    indexed.clear();
    indexed.reserve(frame.pixels().len());
    indexed.extend(
        frame
            .pixels()
            .iter()
            .map(|pixel| palette_lut[palette_lut_key(*pixel)]),
    );
}

fn collect_band_colors(
    indexed: &[u8],
    width: usize,
    height: usize,
    band_y: usize,
    palette_len: usize,
    colors: &mut Vec<u8>,
) {
    colors.clear();
    let mut seen = [false; 256];
    let band_height = (height - band_y).min(6);
    for y in band_y..band_y + band_height {
        let row = &indexed[y * width..(y + 1) * width];
        for &color in row {
            let index = color as usize;
            if index < palette_len && !seen[index] {
                seen[index] = true;
                colors.push(color);
            }
        }
    }
    colors.sort_unstable();
}

fn write_tmux_passthrough(bytes: &[u8], out: &mut dyn Write) -> io::Result<()> {
    out.write_all(b"\x1bPtmux;")?;
    for byte in bytes {
        if *byte == 0x1b {
            out.write_all(b"\x1b\x1b")?;
        } else {
            out.write_all(&[*byte])?;
        }
    }
    out.write_all(b"\x1b\\")
}

fn build_palette() -> Vec<Rgba> {
    let mut palette = Vec::with_capacity(240);
    for r in 0..6 {
        for g in 0..6 {
            for b in 0..6 {
                palette.push(Rgba::rgb(
                    cube_component(r),
                    cube_component(g),
                    cube_component(b),
                ));
            }
        }
    }
    for index in 0..24 {
        let value = 8 + index * 10;
        palette.push(Rgba::rgb(value, value, value));
    }
    palette
}

fn cube_component(value: u8) -> u8 {
    if value == 0 { 0 } else { 55 + value * 40 }
}

fn build_palette_lut(palette: &[Rgba]) -> Box<[u8; LUT_SIZE]> {
    let mut lut = Box::new([0; LUT_SIZE]);
    for r in 0..32 {
        for g in 0..32 {
            for b in 0..32 {
                let pixel = Rgba::rgb((r * 8 + 4) as u8, (g * 8 + 4) as u8, (b * 8 + 4) as u8);
                lut[(r << 10) | (g << 5) | b] = nearest_palette_index(pixel, palette);
            }
        }
    }
    lut
}

fn palette_lut_key(pixel: Rgba) -> usize {
    ((pixel.r as usize >> 3) << 10) | ((pixel.g as usize >> 3) << 5) | (pixel.b as usize >> 3)
}

fn nearest_palette_index(pixel: Rgba, palette: &[Rgba]) -> u8 {
    let mut best_index = 0;
    let mut best_distance = u32::MAX;

    for (index, color) in palette.iter().enumerate() {
        let dr = pixel.r as i32 - color.r as i32;
        let dg = pixel.g as i32 - color.g as i32;
        let db = pixel.b as i32 - color.b as i32;
        let distance = (dr * dr + dg * dg + db * db) as u32;
        if distance < best_distance {
            best_distance = distance;
            best_index = index;
        }
    }

    best_index as u8
}

fn write_sixel_run(
    out: &mut dyn Write,
    run_char: &mut u8,
    run_len: &mut usize,
    ch: u8,
) -> io::Result<()> {
    if *run_len == 0 {
        *run_char = ch;
        *run_len = 1;
        return Ok(());
    }

    if *run_char == ch {
        *run_len += 1;
        return Ok(());
    }

    flush_sixel_run(out, *run_char, *run_len)?;
    *run_char = ch;
    *run_len = 1;
    Ok(())
}

fn flush_sixel_run(out: &mut dyn Write, run_char: u8, run_len: usize) -> io::Result<()> {
    if run_len == 0 {
        return Ok(());
    }
    if run_len > 3 {
        write!(out, "!{}{}", run_len, run_char as char)
    } else {
        for _ in 0..run_len {
            write!(out, "{}", run_char as char)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::SixelBackend;
    use crate::graphics::frame::{Frame, Rgba};

    #[test]
    fn emits_sixel_device_control_string() {
        let frame = Frame::new(2, 2, Rgba::rgb(255, 255, 255));
        let mut backend = SixelBackend::new();
        let mut output = Vec::new();

        backend.render(&frame, &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();

        assert!(output.starts_with("\x1bP0;1;0q"));
        assert!(output.contains("\"1;1;2;2"));
        assert!(output.ends_with("\x1b\\"));
    }
}
