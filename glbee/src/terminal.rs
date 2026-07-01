use std::env;
use std::io::{self, Write};
use std::process::Command;
use std::time::{Duration, Instant};

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton,
    MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};

use crate::model::{Model, ModelMeta};
use crate::protocols::{Image, Placement, Protocol, Rgb, write_image_to};
use crate::renderer::{DEFAULT_BACKGROUND, View, render_model};

const FALLBACK_CELL_WIDTH: usize = 10;
const FALLBACK_CELL_HEIGHT: usize = 20;
const MIN_REASONABLE_CELL_WIDTH: usize = 6;
const MIN_REASONABLE_CELL_HEIGHT: usize = 10;
const FRAME_INTERVAL: Duration = Duration::from_millis(16);
const SETTLE_DELAY: Duration = Duration::from_millis(160);
const INTERACTIVE_SCALE: f32 = 0.45;
const KEY_ROTATION_STEP: f32 = 0.08;
const DRAG_ROTATION_STEP: f32 = 0.025;

pub fn interactive_loop(
    model: &Model,
    protocol: Protocol,
    configured_width: Option<usize>,
    configured_height: Option<usize>,
    max_colors: usize,
) -> Result<(), String> {
    let mut terminal = TerminalSession::enter()?;
    let background = query_terminal_background().unwrap_or(DEFAULT_BACKGROUND);
    let mut stdout = io::stdout().lock();
    let mut view = View {
        yaw: -0.65,
        pitch: 0.35,
        distance: 2.85,
    };
    let mut last_mouse = None::<(u16, u16)>;
    let mut dirty = true;
    let mut refined = false;
    let mut last_draw = Instant::now() - Duration::from_millis(100);
    let mut last_interaction = Instant::now() - SETTLE_DELAY;
    let mut stats = FrameStats::default();

    'outer: loop {
        if !dirty
            && !refined
            && should_refine_after_interaction(protocol, configured_width, configured_height)
            && last_interaction.elapsed() >= SETTLE_DELAY
        {
            dirty = true;
        }

        if dirty && last_draw.elapsed() >= FRAME_INTERVAL {
            let frame_start = Instant::now();
            let base_target = render_target(configured_width, configured_height, protocol);
            let quality = render_quality(
                protocol,
                configured_width,
                configured_height,
                last_interaction,
            );
            let target = target_for_quality(base_target, quality);
            let render_start = Instant::now();
            let image = render_model(model, view, target.width, target.height, background);
            stats.render_ms = render_start.elapsed().as_secs_f32() * 1000.0;
            begin_synchronized_update(&mut stdout)?;
            execute!(stdout, MoveTo(target.image_x as u16, target.image_y as u16))
                .map_err(|err| err.to_string())?;
            let output_start = Instant::now();
            let upscaled = needs_upscaled_output(protocol, quality)
                .then(|| upscale_nearest(&image, base_target.width, base_target.height));
            let output_image = upscaled.as_ref().unwrap_or(&image);
            let output_placement = if upscaled.is_some() {
                base_target.placement
            } else {
                target.placement
            };
            write_image_to(
                &mut stdout,
                output_image,
                protocol,
                max_colors,
                output_placement,
            )?;
            stats.output_ms = output_start.elapsed().as_secs_f32() * 1000.0;
            stats.frame_ms = frame_start.elapsed().as_secs_f32() * 1000.0;
            execute!(stdout, MoveTo(0, 0)).map_err(|err| err.to_string())?;
            draw_frame(
                &mut stdout,
                model,
                &target,
                protocol,
                max_colors,
                view,
                background,
                quality,
                stats,
            )?;
            end_synchronized_update(&mut stdout)?;
            stdout.flush().map_err(|err| err.to_string())?;
            dirty = false;
            refined = quality == RenderQuality::Full
                || !should_refine_after_interaction(protocol, configured_width, configured_height);
            last_draw = Instant::now();
        }

        if !event::poll(Duration::from_millis(12)).map_err(|err| err.to_string())? {
            continue;
        }
        loop {
            let event = event::read().map_err(|err| err.to_string())?;
            match handle_event(event, &mut view, &mut last_mouse) {
                EventAction::Quit => break 'outer,
                EventAction::Changed => {
                    dirty = true;
                    refined = false;
                    last_interaction = Instant::now();
                }
                EventAction::Ignored => {}
            }
            if !event::poll(Duration::ZERO).map_err(|err| err.to_string())? {
                break;
            }
        }
    }
    terminal.leave()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EventAction {
    Ignored,
    Changed,
    Quit,
}

fn handle_event(event: Event, view: &mut View, last_mouse: &mut Option<(u16, u16)>) -> EventAction {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
            KeyCode::Char('q') | KeyCode::Esc => EventAction::Quit,
            KeyCode::Char('c') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                EventAction::Quit
            }
            KeyCode::Char('r') => {
                *view = View {
                    yaw: -0.65,
                    pitch: 0.35,
                    distance: 2.85,
                };
                EventAction::Changed
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                view.distance = (view.distance * 0.88).max(0.8);
                EventAction::Changed
            }
            KeyCode::Char('-') => {
                view.distance = (view.distance * 1.14).min(12.0);
                EventAction::Changed
            }
            KeyCode::Left => {
                view.yaw += KEY_ROTATION_STEP;
                EventAction::Changed
            }
            KeyCode::Right => {
                view.yaw -= KEY_ROTATION_STEP;
                EventAction::Changed
            }
            KeyCode::Up => {
                view.pitch = (view.pitch + KEY_ROTATION_STEP).min(1.45);
                EventAction::Changed
            }
            KeyCode::Down => {
                view.pitch = (view.pitch - KEY_ROTATION_STEP).max(-1.45);
                EventAction::Changed
            }
            _ => EventAction::Ignored,
        },
        Event::Mouse(mouse) => match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                *last_mouse = Some((mouse.column, mouse.row));
                EventAction::Ignored
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some((x, y)) = *last_mouse {
                    let dx = mouse.column as f32 - x as f32;
                    let dy = mouse.row as f32 - y as f32;
                    view.yaw -= dx * DRAG_ROTATION_STEP;
                    view.pitch = (view.pitch - dy * DRAG_ROTATION_STEP).clamp(-1.45, 1.45);
                }
                *last_mouse = Some((mouse.column, mouse.row));
                EventAction::Changed
            }
            MouseEventKind::Up(MouseButton::Left) => {
                *last_mouse = None;
                EventAction::Changed
            }
            MouseEventKind::ScrollUp => {
                view.distance = (view.distance * 0.9).max(0.8);
                EventAction::Changed
            }
            MouseEventKind::ScrollDown => {
                view.distance = (view.distance * 1.1).min(12.0);
                EventAction::Changed
            }
            _ => EventAction::Ignored,
        },
        Event::Resize(_, _) => EventAction::Changed,
        _ => EventAction::Ignored,
    }
}

pub fn resolve_protocol(protocol: Protocol, is_terminal: bool) -> Protocol {
    match protocol {
        Protocol::Auto if !is_terminal => Protocol::Blocks,
        Protocol::Auto if env::var_os("KITTY_WINDOW_ID").is_some() => Protocol::Kitty,
        Protocol::Auto if env::var("TERM_PROGRAM").as_deref() == Ok("iTerm.app") => {
            Protocol::Iterm2
        }
        Protocol::Auto if terminal_likely_supports_sixel() => Protocol::Sixel,
        Protocol::Auto => Protocol::Blocks,
        explicit => explicit,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RenderTarget {
    pub width: usize,
    pub height: usize,
    pub placement: Option<Placement>,
    pub term_columns: usize,
    pub term_rows: usize,
    pub cell_width: usize,
    pub cell_height: usize,
    pub pixel_source: PixelSource,
    pub image_x: usize,
    pub image_y: usize,
    pub image_columns: usize,
    pub image_rows: usize,
}

pub fn render_target(
    width: Option<usize>,
    height: Option<usize>,
    protocol: Protocol,
) -> RenderTarget {
    let (columns, rows, px_w, px_h) = terminal_size_pixels();
    let term_columns = columns.map(usize::from).unwrap_or(96).max(8);
    let term_rows = rows.map(usize::from).unwrap_or(32).max(8);
    let (cell_width, cell_height, pixel_source) = resolve_cell_size(
        px_w.map(usize::from),
        px_h.map(usize::from),
        term_columns,
        term_rows,
    );
    let image_x = 1usize;
    let image_y = 1usize;
    let image_columns = term_columns.saturating_sub(2).max(1);
    let image_rows = term_rows.saturating_sub(5).max(1);
    let image_px_w = image_columns * cell_width;
    let image_px_h = image_rows * cell_height;

    let default_width = match protocol {
        Protocol::Blocks => image_columns,
        _ => image_px_w,
    };
    let default_height = match protocol {
        Protocol::Blocks => image_rows.saturating_mul(2),
        _ => image_px_h,
    };
    let w = width.unwrap_or(default_width).clamp(24, 4096);
    let h = height.unwrap_or(default_height).clamp(16, 4096);
    let placement = match protocol {
        Protocol::Kitty | Protocol::Iterm2 | Protocol::Blocks
            if width.is_none() && height.is_none() =>
        {
            Some(Placement {
                x: image_x,
                y: image_y,
                columns: image_columns,
                rows: image_rows,
            })
        }
        _ => None,
    };
    RenderTarget {
        width: w,
        height: h,
        placement,
        term_columns,
        term_rows,
        cell_width,
        cell_height,
        pixel_source,
        image_x,
        image_y,
        image_columns,
        image_rows,
    }
}

#[derive(Clone, Copy, Debug)]
pub enum PixelSource {
    Terminal,
    Tmux,
    Fallback,
    Manual,
}

impl PixelSource {
    fn name(self) -> &'static str {
        match self {
            PixelSource::Terminal => "terminal",
            PixelSource::Tmux => "tmux",
            PixelSource::Fallback => "fallback",
            PixelSource::Manual => "manual",
        }
    }
}

fn resolve_cell_size(
    pixel_width: Option<usize>,
    pixel_height: Option<usize>,
    columns: usize,
    rows: usize,
) -> (usize, usize, PixelSource) {
    let manual_width = env::var("GLBEE_CELL_WIDTH")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| (4..=64).contains(value));
    let manual_height = env::var("GLBEE_CELL_HEIGHT")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| (6..=96).contains(value));

    if manual_width.is_some() || manual_height.is_some() {
        return (
            manual_width.unwrap_or(FALLBACK_CELL_WIDTH),
            manual_height.unwrap_or(FALLBACK_CELL_HEIGHT),
            PixelSource::Manual,
        );
    }

    if let (Some(width), Some(height)) = (pixel_width, pixel_height) {
        let cell_width = width as f64 / columns.max(1) as f64;
        let cell_height = height as f64 / rows.max(1) as f64;
        if cell_width >= MIN_REASONABLE_CELL_WIDTH as f64
            && cell_height >= MIN_REASONABLE_CELL_HEIGHT as f64
        {
            return (
                (width as f64 / columns.max(1) as f64).round().max(1.0) as usize,
                (height as f64 / rows.max(1) as f64).round().max(1.0) as usize,
                PixelSource::Terminal,
            );
        }
    }

    if let Some((cell_width, cell_height)) = tmux_client_cell_size() {
        return (cell_width, cell_height, PixelSource::Tmux);
    }

    (
        FALLBACK_CELL_WIDTH,
        FALLBACK_CELL_HEIGHT,
        PixelSource::Fallback,
    )
}

fn tmux_client_cell_size() -> Option<(usize, usize)> {
    if !running_in_tmux() {
        return None;
    }
    let output = Command::new("tmux")
        .args([
            "display-message",
            "-p",
            "#{client_cell_width} #{client_cell_height}",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let mut parts = text.split_whitespace();
    let cell_width = parts.next()?.parse::<usize>().ok()?;
    let cell_height = parts.next()?.parse::<usize>().ok()?;
    if cell_width < MIN_REASONABLE_CELL_WIDTH || cell_height < MIN_REASONABLE_CELL_HEIGHT {
        return None;
    }
    Some((cell_width, cell_height))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RenderQuality {
    Interactive,
    Full,
}

impl RenderQuality {
    fn name(self) -> &'static str {
        match self {
            RenderQuality::Interactive => "fast",
            RenderQuality::Full => "full",
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct FrameStats {
    render_ms: f32,
    output_ms: f32,
    frame_ms: f32,
}

fn render_quality(
    protocol: Protocol,
    configured_width: Option<usize>,
    configured_height: Option<usize>,
    last_interaction: Instant,
) -> RenderQuality {
    if configured_width.is_some() || configured_height.is_some() {
        return RenderQuality::Full;
    }
    if !matches!(
        protocol,
        Protocol::Kitty | Protocol::Iterm2 | Protocol::Sixel
    ) {
        return RenderQuality::Full;
    }
    if last_interaction.elapsed() < SETTLE_DELAY {
        RenderQuality::Interactive
    } else {
        RenderQuality::Full
    }
}

fn should_refine_after_interaction(
    protocol: Protocol,
    configured_width: Option<usize>,
    configured_height: Option<usize>,
) -> bool {
    configured_width.is_none()
        && configured_height.is_none()
        && matches!(
            protocol,
            Protocol::Kitty | Protocol::Iterm2 | Protocol::Sixel
        )
}

fn running_in_tmux() -> bool {
    env::var_os("TMUX").is_some()
}

fn target_for_quality(mut target: RenderTarget, quality: RenderQuality) -> RenderTarget {
    if quality == RenderQuality::Interactive {
        target.width = scaled_dimension(target.width, INTERACTIVE_SCALE, 120);
        target.height = scaled_dimension(target.height, INTERACTIVE_SCALE, 90);
    }
    target
}

fn needs_upscaled_output(protocol: Protocol, quality: RenderQuality) -> bool {
    matches!(protocol, Protocol::Sixel) && quality == RenderQuality::Interactive
}

fn upscale_nearest(image: &Image, width: usize, height: usize) -> Image {
    if image.width == width && image.height == height {
        return Image {
            width,
            height,
            pixels: image.pixels.clone(),
        };
    }

    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        let source_y = y * image.height / height.max(1);
        let row = source_y.min(image.height.saturating_sub(1)) * image.width;
        for x in 0..width {
            let source_x = (x * image.width / width.max(1)).min(image.width.saturating_sub(1));
            pixels.push(image.pixels[row + source_x]);
        }
    }
    Image {
        width,
        height,
        pixels,
    }
}

fn scaled_dimension(value: usize, scale: f32, minimum: usize) -> usize {
    ((value as f32 * scale).round() as usize).clamp(minimum.min(value), value)
}

fn draw_frame<W: Write>(
    output: &mut W,
    model: &Model,
    target: &RenderTarget,
    protocol: Protocol,
    max_colors: usize,
    view: View,
    background: Rgb,
    quality: RenderQuality,
    stats: FrameStats,
) -> Result<(), String> {
    let columns = target.term_columns;
    let rows = target.term_rows;
    if columns < 8 || rows < 7 {
        return Ok(());
    }

    write_at(output, 0, 0, &titled_rule(columns, " 3D Model "))?;
    let footer_rule_y = rows.saturating_sub(4);
    let footer_one_y = rows.saturating_sub(3);
    let footer_two_y = rows.saturating_sub(2);
    let bottom_y = rows.saturating_sub(1);
    let footer = footer_texts(
        target, protocol, max_colors, view, background, quality, stats,
    );

    for y in 1..footer_rule_y {
        write_at(output, 0, y, "│")?;
        write_at(output, columns - 1, y, "│")?;
    }

    draw_model_meta(output, target, &model.meta)?;
    write_at(output, 0, footer_rule_y, &rule(columns, '├', '─', '┤'))?;
    write_at(output, 0, footer_one_y, &framed_line(columns, &footer.0))?;
    write_at(output, 0, footer_two_y, &framed_line(columns, &footer.1))?;
    write_at(output, 0, bottom_y, &rule(columns, '└', '─', '┘'))?;
    Ok(())
}

fn footer_texts(
    target: &RenderTarget,
    protocol: Protocol,
    max_colors: usize,
    view: View,
    background: Rgb,
    quality: RenderQuality,
    stats: FrameStats,
) -> (String, String) {
    (
        format!(
            " protocol={} quality={} render={}x{} viewport={}x{} cells={}x{} cell={}x{} src={} bg=#{:02x}{:02x}{:02x} palette={} ",
            protocol.name(),
            quality.name(),
            target.width,
            target.height,
            target.image_columns,
            target.image_rows,
            target.term_columns,
            target.term_rows,
            target.cell_width,
            target.cell_height,
            target.pixel_source.name(),
            background.r,
            background.g,
            background.b,
            max_colors,
        ),
        format!(
            " render_ms={:.1} output_ms={:.1} frame_ms={:.1} yaw={:.2} pitch={:.2} zoom={:.2} ",
            stats.render_ms, stats.output_ms, stats.frame_ms, view.yaw, view.pitch, view.distance,
        ),
    )
}

fn titled_rule(width: usize, title: &str) -> String {
    if width <= 2 {
        return String::new();
    }
    let inner = width - 2;
    let title = truncate_to_width(title, inner);
    let left = inner.saturating_sub(title.len()) / 2;
    let right = inner.saturating_sub(title.len() + left);
    format!("┌{}{}{}┐", "─".repeat(left), title, "─".repeat(right))
}

fn draw_model_meta<W: Write>(
    output: &mut W,
    target: &RenderTarget,
    meta: &ModelMeta,
) -> Result<(), String> {
    let columns = target.term_columns;
    let footer_rule_y = target.term_rows.saturating_sub(4);
    if columns < 24 || footer_rule_y <= 2 {
        return Ok(());
    }

    let lines = model_meta_lines(meta);
    let width = lines
        .iter()
        .map(|line| line.len())
        .max()
        .unwrap_or(0)
        .min(columns.saturating_sub(4))
        .min(54);
    if width == 0 {
        return Ok(());
    }

    for (index, line) in lines
        .iter()
        .take(footer_rule_y.saturating_sub(1))
        .enumerate()
    {
        let text = truncate_to_width(line, width);
        let x = columns.saturating_sub(text.len() + 2);
        write_at(output, x, 1 + index, &text)?;
    }
    Ok(())
}

fn model_meta_lines(meta: &ModelMeta) -> Vec<String> {
    vec![
        format!("file {}", meta.file_name),
        format!("fmt {}  size {}", meta.format, human_size(meta.file_size)),
        format!(
            "mesh {}  prim {}  mat {}  tex {}",
            meta.meshes, meta.primitives, meta.materials, meta.textures
        ),
        format!(
            "tri {}  vtx {}",
            compact_count(meta.triangles),
            compact_count(meta.vertices)
        ),
        format!(
            "node {}  scene {}  anim {}",
            meta.nodes, meta.scenes, meta.animations
        ),
        format!("radius {:.2}", meta.radius),
    ]
}

fn rule(width: usize, left: char, fill: char, right: char) -> String {
    if width <= 2 {
        return String::new();
    }
    format!("{left}{}{right}", fill.to_string().repeat(width - 2))
}

fn framed_line(width: usize, text: &str) -> String {
    if width <= 2 {
        return String::new();
    }
    let inner = width - 2;
    let text = truncate_to_width(text, inner);
    format!("│{text}{}│", " ".repeat(inner - text.len()))
}

fn truncate_to_width(text: &str, width: usize) -> String {
    if text.len() <= width {
        return text.to_string();
    }
    if width <= 1 {
        return " ".repeat(width);
    }
    let mut truncated = text.chars().take(width - 1).collect::<String>();
    truncated.push('~');
    truncated
}

fn compact_count(value: usize) -> String {
    if value >= 1_000_000 {
        format!("{:.1}m", value as f32 / 1_000_000.0)
    } else if value >= 10_000 {
        format!("{:.1}k", value as f32 / 1_000.0)
    } else {
        value.to_string()
    }
}

fn human_size(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    if bytes as f64 >= MIB {
        format!("{:.1}MiB", bytes as f64 / MIB)
    } else if bytes as f64 >= KIB {
        format!("{:.1}KiB", bytes as f64 / KIB)
    } else {
        format!("{bytes}B")
    }
}

fn write_at<W: Write>(output: &mut W, x: usize, y: usize, text: &str) -> Result<(), String> {
    write!(output, "\x1b[{};{}H{text}", y + 1, x + 1).map_err(|err| err.to_string())
}

fn begin_synchronized_update<W: Write>(output: &mut W) -> Result<(), String> {
    if running_in_tmux() {
        return Ok(());
    }
    output
        .write_all(b"\x1b[?2026h")
        .map_err(|err| err.to_string())
}

fn end_synchronized_update<W: Write>(output: &mut W) -> Result<(), String> {
    if running_in_tmux() {
        return Ok(());
    }
    output
        .write_all(b"\x1b[?2026l")
        .map_err(|err| err.to_string())
}

fn query_terminal_background() -> Option<Rgb> {
    let mut stdout = io::stdout();
    stdout.write_all(b"\x1b]11;?\x1b\\").ok()?;
    stdout.flush().ok()?;
    let data = read_stdin_response(Duration::from_millis(140));
    parse_background_response(&data)
}

#[cfg(unix)]
fn read_stdin_response(timeout: Duration) -> Vec<u8> {
    let fd = libc::STDIN_FILENO;
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Vec::new();
    }
    let _ = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };

    let start = Instant::now();
    let mut data = Vec::with_capacity(128);
    while start.elapsed() < timeout {
        let mut buffer = [0u8; 512];
        let read = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
        if read > 0 {
            data.extend_from_slice(&buffer[..read as usize]);
            if data.contains(&b'\x07') || data.windows(2).any(|item| item == b"\x1b\\") {
                break;
            }
        } else {
            std::thread::sleep(Duration::from_millis(8));
        }
    }

    let _ = unsafe { libc::fcntl(fd, libc::F_SETFL, flags) };
    data
}

#[cfg(not(unix))]
fn read_stdin_response(_timeout: Duration) -> Vec<u8> {
    Vec::new()
}

fn parse_background_response(data: &[u8]) -> Option<Rgb> {
    let text = std::str::from_utf8(data).ok()?;
    let start = text.find("rgb:")? + 4;
    let rest = &text[start..];
    let end = rest
        .find(['\x07', '\x1b'])
        .unwrap_or(rest.len())
        .min(rest.len());
    let mut parts = rest[..end].split('/');
    Some(Rgb {
        r: parse_osc_rgb_component(parts.next()?)?,
        g: parse_osc_rgb_component(parts.next()?)?,
        b: parse_osc_rgb_component(parts.next()?)?,
    })
}

fn parse_osc_rgb_component(raw: &str) -> Option<u8> {
    let clean = raw.trim();
    if clean.is_empty() {
        return None;
    }
    let value = u32::from_str_radix(clean, 16).ok()?;
    let max_value = 16u32
        .checked_pow(clean.len() as u32)?
        .saturating_sub(1)
        .max(1);
    Some(((value * 255 + max_value / 2) / max_value).min(255) as u8)
}

struct TerminalSession {
    active: bool,
    tmux_status: Option<String>,
}

impl TerminalSession {
    fn enter() -> Result<Self, String> {
        terminal::enable_raw_mode().map_err(|err| err.to_string())?;
        let tmux_status = hide_tmux_status();
        if let Err(err) = execute!(
            io::stdout(),
            EnterAlternateScreen,
            Clear(ClearType::All),
            EnableMouseCapture,
            Hide
        ) {
            restore_tmux_status(tmux_status);
            return Err(err.to_string());
        }
        Ok(Self {
            active: true,
            tmux_status,
        })
    }

    fn leave(&mut self) -> Result<(), String> {
        if self.active {
            execute!(
                io::stdout(),
                Show,
                DisableMouseCapture,
                LeaveAlternateScreen
            )
            .map_err(|err| err.to_string())?;
            terminal::disable_raw_mode().map_err(|err| err.to_string())?;
            restore_tmux_status(self.tmux_status.take());
            self.active = false;
        }
        Ok(())
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = self.leave();
    }
}

fn hide_tmux_status() -> Option<String> {
    if !running_in_tmux() || env::var_os("GLBEE_TMUX_KEEP_STATUS").is_some() {
        return None;
    }
    let output = Command::new("tmux")
        .args(["show-option", "-qv", "status"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let original = String::from_utf8(output.stdout)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "on".to_string());
    let status = Command::new("tmux")
        .args(["set-option", "-q", "status", "off"])
        .status()
        .ok()?;
    status.success().then_some(original)
}

fn restore_tmux_status(original: Option<String>) {
    if let Some(value) = original {
        let _ = Command::new("tmux")
            .args(["set-option", "-q", "status", &value])
            .status();
    }
}

fn terminal_likely_supports_sixel() -> bool {
    let term_program = env::var("TERM_PROGRAM").unwrap_or_default();
    if matches!(
        term_program.as_str(),
        "WezTerm" | "mintty" | "rio" | "ghostty" | "Windows Terminal"
    ) {
        return true;
    }
    if env::var_os("WEZTERM_EXECUTABLE").is_some()
        || env::var_os("KONSOLE_VERSION").is_some()
        || env::var_os("WT_SESSION").is_some()
    {
        return true;
    }
    let term = env::var("TERM").unwrap_or_default().to_ascii_lowercase();
    term.contains("sixel")
        || term.contains("xterm")
        || term.contains("mlterm")
        || term.contains("foot")
}

#[cfg(unix)]
fn terminal_size_pixels() -> (Option<u16>, Option<u16>, Option<u16>, Option<u16>) {
    let mut size = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let result = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut size) };
    if result != 0 {
        return (None, None, None, None);
    }
    (
        (size.ws_col > 0).then_some(size.ws_col),
        (size.ws_row > 0).then_some(size.ws_row),
        (size.ws_xpixel > 0).then_some(size.ws_xpixel),
        (size.ws_ypixel > 0).then_some(size.ws_ypixel),
    )
}

#[cfg(not(unix))]
fn terminal_size_pixels() -> (Option<u16>, Option<u16>, Option<u16>, Option<u16>) {
    let size = terminal::size().ok();
    (size.map(|s| s.0), size.map(|s| s.1), None, None)
}
