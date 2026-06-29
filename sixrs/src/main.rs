use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;

const ESC: &str = "\x1b";
const ST: &str = "\x1b\\";
const FALLBACK_CELL_HEIGHT: usize = 16;
const MIN_CELL_HEIGHT: usize = 7;
const MAX_CELL_HEIGHT: usize = 64;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct Rgb {
    r: u8,
    g: u8,
    b: u8,
}

#[derive(Debug)]
struct Image {
    width: usize,
    height: usize,
    pixels: Vec<Option<Rgb>>,
}

#[derive(Debug)]
enum InputMode {
    Ppm(PathBuf),
    RawRgb { width: usize, height: usize },
    RawRgba { width: usize, height: usize },
}

#[derive(Debug)]
struct Config {
    input: InputMode,
    max_width: Option<usize>,
    max_height: Option<usize>,
    max_colors: usize,
    protocol: Protocol,
    newline: bool,
    cursor_mode: CursorMode,
    cell_height: Option<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Protocol {
    Auto,
    Sixel,
    Blocks,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CursorMode {
    None,
    Newline,
    Restore,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("sixrs: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = parse_args(env::args().skip(1).collect())?;
    let mut image = load_image(&config.input)?;
    let is_terminal = io::stdout().is_terminal();
    let protocol = resolve_protocol(config.protocol, is_terminal);
    let max_width = effective_max_width(config.max_width, protocol, is_terminal);
    fit_image(&mut image, max_width, config.max_height);
    let output = match protocol {
        Protocol::Sixel => pixels_to_sixel(&image, config.max_colors)?,
        Protocol::Blocks => pixels_to_blocks(&image),
        Protocol::Auto => unreachable!("protocol was resolved"),
    };
    let (cursor_prefix, cursor_suffix) = if is_terminal && protocol == Protocol::Sixel {
        cursor_sequences(config.cursor_mode, image.height, config.cell_height)
    } else {
        (String::new(), String::new())
    };
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(cursor_prefix.as_bytes())
        .map_err(|err| err.to_string())?;
    stdout
        .write_all(output.as_bytes())
        .map_err(|err| err.to_string())?;
    stdout
        .write_all(cursor_suffix.as_bytes())
        .map_err(|err| err.to_string())?;
    if config.newline {
        stdout.write_all(b"\n").map_err(|err| err.to_string())?;
    }
    stdout.flush().map_err(|err| err.to_string())?;
    Ok(())
}

fn parse_args(args: Vec<String>) -> Result<Config, String> {
    let mut input_path: Option<PathBuf> = None;
    let mut raw_rgb: Option<(usize, usize)> = None;
    let mut raw_rgba: Option<(usize, usize)> = None;
    let mut max_width = None;
    let mut max_height = None;
    let mut max_colors = 96usize;
    let mut protocol = Protocol::Auto;
    let mut newline = false;
    let mut cursor_mode = CursorMode::None;
    let mut cell_height = parse_cell_height_env()?;

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--input" | "-i" => {
                index += 1;
                input_path = Some(PathBuf::from(
                    args.get(index).ok_or("--input needs a path")?,
                ));
            }
            "--raw-rgb" => {
                let width = parse_next_usize(&args, &mut index, "--raw-rgb width")?;
                let height = parse_next_usize(&args, &mut index, "--raw-rgb height")?;
                raw_rgb = Some((width, height));
            }
            "--raw-rgba" => {
                let width = parse_next_usize(&args, &mut index, "--raw-rgba width")?;
                let height = parse_next_usize(&args, &mut index, "--raw-rgba height")?;
                raw_rgba = Some((width, height));
            }
            "--max-width" => max_width = Some(parse_next_usize(&args, &mut index, "--max-width")?),
            "--max-height" => {
                max_height = Some(parse_next_usize(&args, &mut index, "--max-height")?)
            }
            "--max-colors" => {
                max_colors = parse_next_usize(&args, &mut index, "--max-colors")?.clamp(2, 256)
            }
            "--protocol" => {
                index += 1;
                protocol = parse_protocol(args.get(index).ok_or("--protocol needs a value")?)?;
            }
            "--newline" => newline = true,
            "--cursor-mode" => {
                index += 1;
                cursor_mode =
                    parse_cursor_mode(args.get(index).ok_or("--cursor-mode needs a value")?)?;
            }
            "--no-cursor-fix" => cursor_mode = CursorMode::None,
            "--cell-height" => {
                cell_height = Some(parse_next_cell_height(&args, &mut index, "--cell-height")?)
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            value if !value.starts_with('-') && input_path.is_none() => {
                input_path = Some(PathBuf::from(value));
            }
            other => return Err(format!("unknown argument: {other}")),
        }
        index += 1;
    }

    let modes = input_path.is_some() as u8 + raw_rgb.is_some() as u8 + raw_rgba.is_some() as u8;
    if modes != 1 {
        return Err("choose exactly one input mode: --input, --raw-rgb, or --raw-rgba".into());
    }

    let input = if let Some(path) = input_path {
        InputMode::Ppm(path)
    } else if let Some((width, height)) = raw_rgb {
        InputMode::RawRgb { width, height }
    } else if let Some((width, height)) = raw_rgba {
        InputMode::RawRgba { width, height }
    } else {
        unreachable!("input mode count was validated");
    };

    Ok(Config {
        input,
        max_width,
        max_height,
        max_colors,
        protocol,
        newline,
        cursor_mode,
        cell_height,
    })
}

fn parse_next_usize(args: &[String], index: &mut usize, label: &str) -> Result<usize, String> {
    *index += 1;
    args.get(*index)
        .ok_or_else(|| format!("{label} needs a value"))?
        .parse::<usize>()
        .map_err(|_| format!("{label} must be a positive integer"))
}

fn parse_next_cell_height(
    args: &[String],
    index: &mut usize,
    label: &str,
) -> Result<usize, String> {
    let height = parse_next_usize(args, index, label)?;
    validate_cell_height(height).ok_or_else(|| {
        format!("{label} must be between {MIN_CELL_HEIGHT} and {MAX_CELL_HEIGHT} pixels")
    })
}

fn parse_cell_height_env() -> Result<Option<usize>, String> {
    let Ok(value) = env::var("SIXRS_CELL_HEIGHT") else {
        return Ok(None);
    };
    if value.trim().is_empty() {
        return Ok(None);
    }
    let height = value
        .parse::<usize>()
        .map_err(|_| "SIXRS_CELL_HEIGHT must be a positive integer")?;
    validate_cell_height(height).map(Some).ok_or_else(|| {
        format!("SIXRS_CELL_HEIGHT must be between {MIN_CELL_HEIGHT} and {MAX_CELL_HEIGHT} pixels")
    })
}

fn validate_cell_height(height: usize) -> Option<usize> {
    (MIN_CELL_HEIGHT..=MAX_CELL_HEIGHT)
        .contains(&height)
        .then_some(height)
}

fn parse_cursor_mode(value: &str) -> Result<CursorMode, String> {
    match value {
        "none" => Ok(CursorMode::None),
        "newline" => Ok(CursorMode::Newline),
        "restore" => Ok(CursorMode::Restore),
        other => Err(format!(
            "unknown cursor mode: {other} (expected none, newline, or restore)"
        )),
    }
}

fn parse_protocol(value: &str) -> Result<Protocol, String> {
    match value {
        "auto" => Ok(Protocol::Auto),
        "sixel" => Ok(Protocol::Sixel),
        "blocks" => Ok(Protocol::Blocks),
        other => Err(format!(
            "unknown protocol: {other} (expected auto, sixel, or blocks)"
        )),
    }
}

fn print_help() {
    println!(
        "Usage:
  sixrs --input image.jpg [--max-width N] [--max-height N] [--max-colors N]
  sixrs image.png [--max-width N] [--max-height N] [--max-colors N]
  sixrs --raw-rgb WIDTH HEIGHT < frame.rgb
  sixrs --raw-rgba WIDTH HEIGHT < frame.rgba

Options:
  -i, --input PATH     Read an image from PATH (JPEG, PNG, GIF, BMP, TIFF, WebP, PPM)
      --raw-rgb W H    Read W x H raw RGB bytes from stdin
      --raw-rgba W H   Read W x H raw RGBA bytes from stdin
      --max-width N    Downscale to fit within N columns of pixels
      --max-height N   Downscale to fit within N rows of pixels
      --max-colors N   Palette size, clamped to 2..256 (default: 96)
      --protocol P     Output protocol: auto, sixel, blocks (default: auto)
      --newline        Print a trailing newline after output
      --cursor-mode M  Cursor handling: none, newline, restore (default: none)
      --no-cursor-fix  Alias for --cursor-mode none
      --cell-height N  Cell height for --cursor-mode restore
  -h, --help           Show this help

Encodes images for terminal display."
    );
}

fn load_image(input: &InputMode) -> Result<Image, String> {
    match input {
        InputMode::Ppm(path) => {
            let data = fs::read(path).map_err(|err| err.to_string())?;
            decode_image(&data)
        }
        InputMode::RawRgb { width, height } => {
            let mut data = Vec::new();
            io::stdin()
                .read_to_end(&mut data)
                .map_err(|err| err.to_string())?;
            raw_rgb_to_image(&data, *width, *height)
        }
        InputMode::RawRgba { width, height } => {
            let mut data = Vec::new();
            io::stdin()
                .read_to_end(&mut data)
                .map_err(|err| err.to_string())?;
            raw_rgba_to_image(&data, *width, *height)
        }
    }
}

fn decode_image(data: &[u8]) -> Result<Image, String> {
    match decode_ppm(data) {
        Ok(image) => Ok(image),
        Err(ppm_err) => decode_with_image_crate(data).map_err(|image_err| {
            format!("unsupported or invalid image ({image_err}; PPM decoder said: {ppm_err})")
        }),
    }
}

fn decode_with_image_crate(data: &[u8]) -> Result<Image, String> {
    let decoded = image::load_from_memory(data).map_err(|err| err.to_string())?;
    let rgba = decoded.to_rgba8();
    let width = usize::try_from(rgba.width()).map_err(|_| "image width is too large")?;
    let height = usize::try_from(rgba.height()).map_err(|_| "image height is too large")?;
    raw_rgba_to_image(rgba.as_raw(), width, height)
}

fn raw_rgb_to_image(data: &[u8], width: usize, height: usize) -> Result<Image, String> {
    let expected = width * height * 3;
    if data.len() != expected {
        return Err(format!(
            "raw RGB input expected {expected} bytes, got {}",
            data.len()
        ));
    }
    let pixels = data
        .chunks_exact(3)
        .map(|chunk| {
            Some(Rgb {
                r: chunk[0],
                g: chunk[1],
                b: chunk[2],
            })
        })
        .collect();
    Ok(Image {
        width,
        height,
        pixels,
    })
}

fn raw_rgba_to_image(data: &[u8], width: usize, height: usize) -> Result<Image, String> {
    let expected = width * height * 4;
    if data.len() != expected {
        return Err(format!(
            "raw RGBA input expected {expected} bytes, got {}",
            data.len()
        ));
    }
    let pixels = data
        .chunks_exact(4)
        .map(|chunk| {
            if chunk[3] == 0 {
                None
            } else {
                Some(Rgb {
                    r: chunk[0],
                    g: chunk[1],
                    b: chunk[2],
                })
            }
        })
        .collect();
    Ok(Image {
        width,
        height,
        pixels,
    })
}

fn decode_ppm(data: &[u8]) -> Result<Image, String> {
    let mut cursor = 0usize;
    let magic = next_token(data, &mut cursor).ok_or("PPM is missing magic")?;
    let width = parse_token_usize(next_token(data, &mut cursor), "width")?;
    let height = parse_token_usize(next_token(data, &mut cursor), "height")?;
    let max_value = parse_token_usize(next_token(data, &mut cursor), "max value")?;
    if max_value == 0 || max_value > 255 {
        return Err("only 1-byte PPM samples are supported".into());
    }

    while cursor < data.len() && data[cursor].is_ascii_whitespace() {
        cursor += 1;
    }

    match magic.as_slice() {
        b"P6" => {
            let expected = width * height * 3;
            if data.len().saturating_sub(cursor) < expected {
                return Err("truncated P6 raster".into());
            }
            let pixels = data[cursor..cursor + expected]
                .chunks_exact(3)
                .map(|chunk| {
                    Some(Rgb {
                        r: scale_sample(chunk[0], max_value),
                        g: scale_sample(chunk[1], max_value),
                        b: scale_sample(chunk[2], max_value),
                    })
                })
                .collect();
            Ok(Image {
                width,
                height,
                pixels,
            })
        }
        b"P3" => {
            let mut pixels = Vec::with_capacity(width * height);
            for _ in 0..width * height {
                let r = parse_token_usize(next_token(data, &mut cursor), "red")?;
                let g = parse_token_usize(next_token(data, &mut cursor), "green")?;
                let b = parse_token_usize(next_token(data, &mut cursor), "blue")?;
                pixels.push(Some(Rgb {
                    r: scale_sample(r as u8, max_value),
                    g: scale_sample(g as u8, max_value),
                    b: scale_sample(b as u8, max_value),
                }));
            }
            Ok(Image {
                width,
                height,
                pixels,
            })
        }
        _ => Err("only PPM P6/P3 is supported".into()),
    }
}

fn next_token(data: &[u8], cursor: &mut usize) -> Option<Vec<u8>> {
    loop {
        while *cursor < data.len() && data[*cursor].is_ascii_whitespace() {
            *cursor += 1;
        }
        if *cursor < data.len() && data[*cursor] == b'#' {
            while *cursor < data.len() && data[*cursor] != b'\n' {
                *cursor += 1;
            }
            continue;
        }
        break;
    }
    if *cursor >= data.len() {
        return None;
    }
    let start = *cursor;
    while *cursor < data.len() && !data[*cursor].is_ascii_whitespace() {
        *cursor += 1;
    }
    Some(data[start..*cursor].to_vec())
}

fn parse_token_usize(token: Option<Vec<u8>>, label: &str) -> Result<usize, String> {
    let token = token.ok_or_else(|| format!("PPM is missing {label}"))?;
    let text = String::from_utf8(token).map_err(|_| format!("invalid {label} token"))?;
    text.parse::<usize>()
        .map_err(|_| format!("invalid {label} token"))
}

fn scale_sample(value: u8, max_value: usize) -> u8 {
    if max_value == 255 {
        value
    } else {
        ((value as usize * 255 + max_value / 2) / max_value) as u8
    }
}

fn fit_image(image: &mut Image, max_width: Option<usize>, max_height: Option<usize>) {
    let Some(scale) = fit_scale(image.width, image.height, max_width, max_height) else {
        return;
    };
    if scale >= 1.0 {
        return;
    }
    let new_width = ((image.width as f64 * scale).floor() as usize).max(1);
    let new_height = ((image.height as f64 * scale).floor() as usize).max(1);
    let mut resized = Vec::with_capacity(new_width * new_height);
    for y in 0..new_height {
        let source_y = (y * image.height / new_height).min(image.height - 1);
        for x in 0..new_width {
            let source_x = (x * image.width / new_width).min(image.width - 1);
            resized.push(image.pixels[source_y * image.width + source_x]);
        }
    }
    image.width = new_width;
    image.height = new_height;
    image.pixels = resized;
}

fn fit_scale(
    width: usize,
    height: usize,
    max_width: Option<usize>,
    max_height: Option<usize>,
) -> Option<f64> {
    match (max_width, max_height) {
        (None, None) => None,
        (Some(max_w), None) => Some(max_w as f64 / width as f64),
        (None, Some(max_h)) => Some(max_h as f64 / height as f64),
        (Some(max_w), Some(max_h)) => {
            Some((max_w as f64 / width as f64).min(max_h as f64 / height as f64))
        }
    }
}

fn cursor_sequences(
    mode: CursorMode,
    image_height: usize,
    configured_cell_height: Option<usize>,
) -> (String, String) {
    match mode {
        CursorMode::None => (String::new(), String::new()),
        CursorMode::Newline => (String::new(), "\n".to_string()),
        CursorMode::Restore => {
            let cell_height = configured_cell_height
                .or_else(terminal_cell_height)
                .unwrap_or(FALLBACK_CELL_HEIGHT);
            let terminal_rows = image_height.div_ceil(cell_height).max(1);
            (format!("{ESC}7"), format!("{ESC}8{ESC}[{terminal_rows}B\r"))
        }
    }
}

fn resolve_protocol(protocol: Protocol, is_terminal: bool) -> Protocol {
    match protocol {
        Protocol::Auto if is_terminal && !terminal_likely_supports_sixel() => Protocol::Blocks,
        Protocol::Auto => Protocol::Sixel,
        explicit => explicit,
    }
}

fn effective_max_width(
    configured_max_width: Option<usize>,
    protocol: Protocol,
    is_terminal: bool,
) -> Option<usize> {
    if configured_max_width.is_some() || protocol != Protocol::Blocks || !is_terminal {
        return configured_max_width;
    }
    terminal_columns().map(|columns| (columns / 2).max(1))
}

fn terminal_likely_supports_sixel() -> bool {
    if env_flag("SIXRS_FORCE_SIXEL") {
        return true;
    }
    if env_flag("SIXRS_NO_SIXEL") {
        return false;
    }

    let term_program = env::var("TERM_PROGRAM").unwrap_or_default();
    if matches!(term_program.as_str(), "Apple_Terminal" | "vscode") {
        return false;
    }
    if matches!(
        term_program.as_str(),
        "iTerm.app" | "WezTerm" | "mintty" | "rio" | "ghostty" | "Windows Terminal"
    ) {
        return true;
    }
    if env::var_os("WEZTERM_EXECUTABLE").is_some()
        || env::var_os("KITTY_WINDOW_ID").is_some()
        || env::var_os("KONSOLE_VERSION").is_some()
        || env::var_os("WT_SESSION").is_some()
    {
        return true;
    }

    let term = env::var("TERM").unwrap_or_default().to_ascii_lowercase();
    if term == "dumb" || term.contains("linux") {
        return false;
    }
    term.contains("sixel")
        || term.contains("xterm")
        || term.contains("mlterm")
        || term.contains("foot")
}

fn env_flag(name: &str) -> bool {
    matches!(
        env::var(name).as_deref(),
        Ok("1") | Ok("true") | Ok("yes") | Ok("on")
    )
}

#[cfg(unix)]
fn terminal_cell_height() -> Option<usize> {
    let mut size = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let result = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut size) };
    if result != 0 || size.ws_row == 0 || size.ws_ypixel == 0 {
        return None;
    }
    let rows = usize::from(size.ws_row);
    let height = usize::from(size.ws_ypixel);
    validate_cell_height((height / rows).max(1))
}

#[cfg(unix)]
fn terminal_columns() -> Option<usize> {
    let mut size = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let result = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut size) };
    if result != 0 || size.ws_col == 0 {
        return None;
    }
    Some(usize::from(size.ws_col))
}

#[cfg(not(unix))]
fn terminal_columns() -> Option<usize> {
    env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|columns| *columns > 0)
}

#[cfg(not(unix))]
fn terminal_cell_height() -> Option<usize> {
    None
}

fn pixels_to_blocks(image: &Image) -> String {
    let mut output = String::new();
    for y in (0..image.height).step_by(2) {
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
        output.push('\n');
    }
    output
}

fn push_block_pixel(output: &mut String, top: Option<Rgb>, bottom: Option<Rgb>) {
    match (top, bottom) {
        (Some(fg), Some(bg)) => {
            push_fg(output, fg);
            push_bg(output, bg);
            output.push('▀');
        }
        (Some(fg), None) => {
            push_fg(output, fg);
            output.push('▀');
        }
        (None, Some(fg)) => {
            push_fg(output, fg);
            output.push('▄');
        }
        (None, None) => output.push(' '),
    }
}

fn push_fg(output: &mut String, color: Rgb) {
    output.push_str(&format!("{ESC}[38;2;{};{};{}m", color.r, color.g, color.b));
}

fn push_bg(output: &mut String, color: Rgb) {
    output.push_str(&format!("{ESC}[48;2;{};{};{}m", color.r, color.g, color.b));
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
    parts.push_str("Pq");
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
            let row: String = row_masks.iter().map(|bits| (63 + bits) as char).collect();
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

    let mut colors: Vec<(Rgb, usize)> = counts.into_iter().collect();
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

    let mut nearest_cache: HashMap<Rgb, usize> = HashMap::new();
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
