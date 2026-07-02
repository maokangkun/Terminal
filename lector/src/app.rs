use std::io::{self, Write};
use std::time::{Duration, Instant};

use crate::cli::Config;
use crate::engine::{
    BrowserChrome, BrowserEngine, DemoEngine, EngineEvent, EngineKey, HtmlEngine, PointerButton,
    ServoNativeEngine,
};
use crate::event::{Event, Key, MouseButton, MouseEventKind};
use crate::graphics::frame::Frame;
use crate::graphics::sixel::SixelBackend;
use crate::terminal::{Terminal, TerminalGuard, TerminalSize};

const FALLBACK_CELL_WIDTH: u32 = 10;
const FALLBACK_CELL_HEIGHT: u32 = 20;
const MIN_REASONABLE_CELL_WIDTH: u32 = 6;
const MIN_REASONABLE_CELL_HEIGHT: u32 = 10;
const CHROME_ROWS: u32 = 2;

pub fn run(config: Config) -> io::Result<()> {
    let mut terminal = Terminal::new()?;
    if config.probe_terminal {
        let size = terminal.size();
        let target = render_target(size);
        println!(
            "columns={} rows={} pixel_width={:?} pixel_height={:?} cell={}x{} viewport={}x{} framebuffer={}x{}",
            size.columns,
            size.rows,
            size.pixel_width,
            size.pixel_height,
            target.cell_width,
            target.cell_height,
            target.image_columns,
            target.image_rows,
            target.image_width,
            target.image_height
        );
        return Ok(());
    }

    let _guard = TerminalGuard::enter(&mut terminal)?;

    let mut engine: Box<dyn BrowserEngine> = match config.engine.as_str() {
        "demo" => Box::new(DemoEngine::new(config.url)),
        "servo" | "servo-native" | "native-servo" => Box::new(ServoNativeEngine::new(config.url)),
        _ => Box::new(HtmlEngine::new(config.url)),
    };
    let mut backend = SixelBackend::new();
    let mut last_draw = Instant::now() - Duration::from_secs(1);
    let mut last_draw_cache = None;
    let mut address_bar = AddressBar::default();

    draw(
        &mut terminal,
        engine.as_mut(),
        &mut backend,
        &address_bar,
        &mut last_draw_cache,
    )?;

    loop {
        if last_draw.elapsed() >= Duration::from_millis(100) {
            draw(
                &mut terminal,
                engine.as_mut(),
                &mut backend,
                &address_bar,
                &mut last_draw_cache,
            )?;
            last_draw = Instant::now();
        }

        match terminal.read_event()? {
            Event::Key(Key::Ctrl('c')) => break,
            Event::Key(Key::Ctrl('n')) => {
                address_bar.focused = false;
                engine.handle_event(EngineEvent::NewTab);
                last_draw_cache = None;
            }
            Event::Key(Key::Ctrl('w')) => {
                address_bar.focused = false;
                engine.handle_event(EngineEvent::CloseTab);
                last_draw_cache = None;
            }
            Event::Key(Key::Esc) => {
                if address_bar.focused {
                    address_bar.focused = false;
                } else {
                    break;
                }
            }
            Event::Key(Key::Enter) if address_bar.focused => {
                let url = address_bar.text.clone();
                address_bar.focused = false;
                engine.handle_event(EngineEvent::Navigate(url));
                last_draw_cache = None;
            }
            Event::Key(Key::Backspace) if address_bar.focused => {
                address_bar.text.pop();
            }
            Event::Key(Key::Char(ch)) if address_bar.focused => {
                address_bar.text.push(ch);
            }
            Event::Key(Key::Up) => {
                engine.handle_event(EngineEvent::Scroll { dx: 0, dy: -48 });
                engine.handle_event(EngineEvent::KeyPress(EngineKey::Up));
            }
            Event::Key(Key::Down) => {
                engine.handle_event(EngineEvent::Scroll { dx: 0, dy: 48 });
                engine.handle_event(EngineEvent::KeyPress(EngineKey::Down));
            }
            Event::Key(Key::Left) => engine.handle_event(EngineEvent::KeyPress(EngineKey::Left)),
            Event::Key(Key::Right) => engine.handle_event(EngineEvent::KeyPress(EngineKey::Right)),
            Event::Key(Key::PageUp) => {
                engine.handle_event(EngineEvent::Scroll { dx: 0, dy: -420 });
                engine.handle_event(EngineEvent::KeyPress(EngineKey::PageUp));
            }
            Event::Key(Key::PageDown) => {
                engine.handle_event(EngineEvent::Scroll { dx: 0, dy: 420 });
                engine.handle_event(EngineEvent::KeyPress(EngineKey::PageDown));
            }
            Event::Key(Key::Enter) => engine.handle_event(EngineEvent::KeyPress(EngineKey::Enter)),
            Event::Key(Key::Backspace) => {
                engine.handle_event(EngineEvent::KeyPress(EngineKey::Backspace));
            }
            Event::Key(Key::Char(ch)) => engine.handle_event(EngineEvent::Text(ch.to_string())),
            Event::Mouse(mouse) => {
                let target = render_target(terminal.size());
                if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                    match chrome_hit(mouse.column, mouse.row, target, &engine.chrome()) {
                        ChromeHit::Tab(index) => {
                            address_bar.focused = false;
                            engine.handle_event(EngineEvent::SwitchTab(index));
                            last_draw_cache = None;
                            continue;
                        }
                        ChromeHit::Address => {
                            let chrome = engine.chrome();
                            address_bar.focused = true;
                            address_bar.text = chrome.active_url;
                            last_draw_cache = None;
                            continue;
                        }
                        ChromeHit::None => {}
                    }
                }
                let point = mouse_to_pixel(mouse.column, mouse.row, target);
                match mouse.kind {
                    MouseEventKind::ScrollUp => {
                        engine.handle_event(EngineEvent::Scroll { dx: 0, dy: -80 });
                        if let Some((x, y)) = point {
                            engine.handle_event(EngineEvent::Wheel {
                                x,
                                y,
                                dx: 0,
                                dy: 80,
                            });
                        }
                    }
                    MouseEventKind::ScrollDown => {
                        engine.handle_event(EngineEvent::Scroll { dx: 0, dy: 80 });
                        if let Some((x, y)) = point {
                            engine.handle_event(EngineEvent::Wheel {
                                x,
                                y,
                                dx: 0,
                                dy: -80,
                            });
                        }
                    }
                    MouseEventKind::Down(button) => {
                        if let Some((x, y)) = point {
                            engine.handle_event(EngineEvent::PointerMove { x, y });
                            engine.handle_event(EngineEvent::PointerDown {
                                x,
                                y,
                                button: pointer_button(button),
                            });
                        }
                        if button == MouseButton::Left {
                            engine.handle_event(EngineEvent::Click {
                                x: mouse.column,
                                y: mouse.row,
                            });
                        }
                    }
                    MouseEventKind::Up(button) => {
                        if let Some((x, y)) = point {
                            engine.handle_event(EngineEvent::PointerMove { x, y });
                            engine.handle_event(EngineEvent::PointerUp {
                                x,
                                y,
                                button: pointer_button(button),
                            });
                        }
                    }
                    MouseEventKind::Drag(button) => {
                        if let Some((x, y)) = point {
                            engine.handle_event(EngineEvent::PointerMove { x, y });
                        }
                        if button == MouseButton::Left {
                            engine.handle_event(EngineEvent::Drag {
                                x: mouse.column,
                                y: mouse.row,
                            });
                        }
                    }
                }
            }
            Event::Unknown => {}
            _ => {}
        }
    }

    Ok(())
}

fn draw(
    terminal: &mut Terminal,
    engine: &mut dyn BrowserEngine,
    backend: &mut SixelBackend,
    address_bar: &AddressBar,
    last_draw_cache: &mut Option<DrawCache>,
) -> io::Result<()> {
    let target = render_target(terminal.size());
    let (width, height) = (target.image_width, target.image_height);
    engine.handle_event(EngineEvent::Resize { width, height });
    let frame = engine.render(width, height);
    let chrome = engine.chrome();
    let chrome_signature = ChromeSignature::new(&chrome, address_bar);
    let should_clear = last_draw_cache
        .as_ref()
        .map(|last| last.frame.width() != frame.width() || last.frame.height() != frame.height())
        .unwrap_or(false);
    if last_draw_cache
        .as_ref()
        .map(|last| last.frame == frame && last.chrome == chrome_signature)
        .unwrap_or(false)
    {
        return Ok(());
    }
    *last_draw_cache = Some(DrawCache {
        frame: frame.clone(),
        chrome: chrome_signature,
    });

    let mut out = io::stdout().lock();
    let in_tmux = std::env::var_os("TMUX").is_some();
    if in_tmux {
        write!(out, "\x1b[H\x1b[?7l")?;
    } else {
        write!(out, "\x1b[?2026h\x1b[H\x1b[?7l")?;
    }
    if should_clear {
        write!(out, "\x1b[2J")?;
    }
    write!(out, "\x1b[{};{}H", target.image_y + 1, target.image_x + 1)?;
    backend.render(&frame, &mut out)?;
    write!(out, "\x1b[1;1H")?;
    draw_terminal_frame(&mut out, target.columns, target.rows)?;
    draw_browser_chrome(&mut out, target, &chrome, address_bar)?;
    if in_tmux {
        write!(out, "\x1b[1;1H")?;
    } else {
        write!(out, "\x1b[1;1H\x1b[?2026l")?;
    }
    out.flush()
}

#[derive(Debug, Clone, Default)]
struct AddressBar {
    focused: bool,
    text: String,
}

#[derive(Debug, Clone)]
struct DrawCache {
    frame: Frame,
    chrome: ChromeSignature,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChromeSignature {
    chrome: BrowserChrome,
    address_focused: bool,
    address_text: String,
}

impl ChromeSignature {
    fn new(chrome: &BrowserChrome, address_bar: &AddressBar) -> Self {
        Self {
            chrome: chrome.clone(),
            address_focused: address_bar.focused,
            address_text: address_bar.text.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct RenderTarget {
    columns: u32,
    rows: u32,
    cell_width: u32,
    cell_height: u32,
    image_x: u32,
    image_y: u32,
    image_columns: u32,
    image_rows: u32,
    image_width: u32,
    image_height: u32,
}

fn render_target(size: TerminalSize) -> RenderTarget {
    let columns = size.columns.max(40) as u32;
    let rows = size.rows.max(10 + CHROME_ROWS as u16) as u32;
    let (cell_width, cell_height) = cell_size(size, columns, rows);
    let image_x = 1;
    let image_y = 1 + CHROME_ROWS;
    let image_columns = columns.saturating_sub(2).max(1);
    let image_rows = rows.saturating_sub(2 + CHROME_ROWS).max(1);

    RenderTarget {
        columns,
        rows,
        cell_width,
        cell_height,
        image_x,
        image_y,
        image_columns,
        image_rows,
        image_width: image_columns * cell_width,
        image_height: image_rows * cell_height,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChromeHit {
    Tab(usize),
    Address,
    None,
}

fn chrome_hit(column: u16, row: u16, target: RenderTarget, chrome: &BrowserChrome) -> ChromeHit {
    let column = column as u32;
    let row = row as u32;
    if row == 2 {
        for item in tab_layout(target.columns, chrome) {
            if column >= item.start && column < item.start + item.width as u32 {
                return ChromeHit::Tab(item.index);
            }
        }
    }
    if row == 3 && column > 2 && column < target.columns {
        return ChromeHit::Address;
    }
    ChromeHit::None
}

fn draw_browser_chrome(
    out: &mut dyn Write,
    target: RenderTarget,
    chrome: &BrowserChrome,
    address_bar: &AddressBar,
) -> io::Result<()> {
    draw_tab_line(out, 2, target.columns, chrome)?;

    let url = if address_bar.focused {
        format!("> {}", address_bar.text)
    } else {
        format!("> {}", chrome.active_url)
    };
    write_padded_line(out, 3, target.columns, &url, AnsiStyle::Chrome)
}

fn draw_tab_line(
    out: &mut dyn Write,
    row: u32,
    columns: u32,
    chrome: &BrowserChrome,
) -> io::Result<()> {
    write!(out, "\x1b[{};2H{}", row, AnsiStyle::Reset.prefix())?;
    let inner = columns.saturating_sub(2) as usize;
    let mut used = 0_usize;
    let tabs = visible_tabs(chrome);
    let layout = tab_layout(columns, chrome);

    for item in layout {
        let Some(tab) = tabs.get(item.index) else {
            continue;
        };
        let style = if tab.active {
            AnsiStyle::ActiveTab
        } else {
            AnsiStyle::Reset
        };
        write!(out, "{}{}", style.prefix(), item.label)?;
        used += item.width;
        if used >= inner {
            break;
        }
    }

    if used < inner {
        write!(
            out,
            "{}{}",
            AnsiStyle::Reset.prefix(),
            " ".repeat(inner - used)
        )?;
    }
    write!(
        out,
        "{}\x1b[{};{}H│",
        AnsiStyle::Reset.prefix(),
        row,
        columns
    )
}

fn write_padded_line(
    out: &mut dyn Write,
    row: u32,
    columns: u32,
    text: &str,
    style: AnsiStyle,
) -> io::Result<()> {
    let inner = columns.saturating_sub(2) as usize;
    let mut content = truncate_chars(text, inner);
    let pad = inner.saturating_sub(display_width(&content));
    content.push_str(&" ".repeat(pad));
    write!(
        out,
        "\x1b[{};2H{}{}{}",
        row,
        style.prefix(),
        content,
        AnsiStyle::Reset.prefix()
    )
}

fn visible_tabs(chrome: &BrowserChrome) -> Vec<crate::engine::BrowserTab> {
    if chrome.tabs.is_empty() {
        vec![crate::engine::BrowserTab {
            title: "Lector".to_string(),
            url: String::new(),
            active: true,
        }]
    } else {
        chrome.tabs.clone()
    }
}

#[derive(Debug, Clone)]
struct TabLayoutItem {
    index: usize,
    start: u32,
    width: usize,
    label: String,
}

fn tab_layout(columns: u32, chrome: &BrowserChrome) -> Vec<TabLayoutItem> {
    let tabs = visible_tabs(chrome);
    let inner = columns.saturating_sub(2) as usize;
    let title_width = adaptive_tab_title_width(columns, tabs.len());
    let mut items = Vec::with_capacity(tabs.len());
    let mut used = 0_usize;
    for (index, tab) in tabs.iter().enumerate() {
        let label = format!(" {} ", tab_label(tab, index, title_width));
        let width = display_width(&label);
        if width == 0 || used + width > inner {
            break;
        }
        items.push(TabLayoutItem {
            index,
            start: 2 + used as u32,
            width,
            label,
        });
        used += width;
    }
    items
}

fn adaptive_tab_title_width(columns: u32, tab_count: usize) -> usize {
    if tab_count == 0 {
        return 18;
    }
    let inner = columns.saturating_sub(2) as usize;
    let per_tab = inner / tab_count;
    let max_title = match tab_count {
        0..=3 => 18,
        4 => 14,
        5 => 12,
        _ => 10,
    };
    per_tab
        .saturating_sub(index_width(tab_count - 1) + 3)
        .clamp(3, max_title)
}

fn tab_label(tab: &crate::engine::BrowserTab, index: usize, title_width: usize) -> String {
    let title = if tab.title.trim().is_empty() {
        if tab.url.trim().is_empty() {
            "New tab"
        } else {
            tab.url.as_str()
        }
    } else {
        tab.title.as_str()
    };
    format!("{} {}", index + 1, truncate_chars(title, title_width))
}

#[derive(Debug, Clone, Copy)]
enum AnsiStyle {
    Chrome,
    ActiveTab,
    Reset,
}

impl AnsiStyle {
    fn prefix(self) -> &'static str {
        match self {
            AnsiStyle::Chrome => "\x1b[48;2;30;32;34m\x1b[38;2;236;238;240m",
            AnsiStyle::ActiveTab => "\x1b[48;2;30;32;34m\x1b[38;2;255;255;255m",
            AnsiStyle::Reset => "\x1b[0m",
        }
    }
}

fn truncate_chars(text: &str, max: usize) -> String {
    if display_width(text) <= max {
        return text.to_string();
    }
    let keep = max.saturating_sub(1);
    let mut output = String::new();
    let mut width = 0_usize;
    for ch in text.chars() {
        let ch_width = char_width(ch);
        if width + ch_width > keep {
            break;
        }
        output.push(ch);
        width += ch_width;
    }
    output.push('~');
    output
}

fn display_width(text: &str) -> usize {
    text.chars().map(char_width).sum()
}

fn char_width(ch: char) -> usize {
    if ch.is_ascii() {
        return 1;
    }
    match ch as u32 {
        0x1100..=0x115f
        | 0x2e80..=0xa4cf
        | 0xac00..=0xd7a3
        | 0xf900..=0xfaff
        | 0xfe10..=0xfe19
        | 0xfe30..=0xfe6f
        | 0xff00..=0xff60
        | 0xffe0..=0xffe6 => 2,
        _ => 1,
    }
}

fn index_width(index: usize) -> usize {
    (index + 1).to_string().len()
}

fn mouse_to_pixel(column: u16, row: u16, target: RenderTarget) -> Option<(u32, u32)> {
    let x_cell = (column as u32)
        .checked_sub(1)?
        .checked_sub(target.image_x)?;
    let y_cell = (row as u32).checked_sub(1)?.checked_sub(target.image_y)?;
    if x_cell >= target.image_columns || y_cell >= target.image_rows {
        return None;
    }

    let x = (x_cell * target.cell_width + target.cell_width / 2)
        .min(target.image_width.saturating_sub(1));
    let y = (y_cell * target.cell_height + target.cell_height / 2)
        .min(target.image_height.saturating_sub(1));
    Some((x, y))
}

fn pointer_button(button: MouseButton) -> PointerButton {
    match button {
        MouseButton::Left => PointerButton::Left,
        MouseButton::Middle => PointerButton::Middle,
        MouseButton::Right => PointerButton::Right,
        MouseButton::Other => PointerButton::Other,
    }
}

fn cell_size(size: TerminalSize, columns: u32, rows: u32) -> (u32, u32) {
    if let Some(cell) = manual_cell_size() {
        return cell;
    }

    if let (Some(pixel_width), Some(pixel_height)) = (size.pixel_width, size.pixel_height) {
        let cell_width = (pixel_width as f64 / columns.max(1) as f64).round() as u32;
        let cell_height = (pixel_height as f64 / rows.max(1) as f64).round() as u32;
        if (MIN_REASONABLE_CELL_WIDTH..=64).contains(&cell_width)
            && (MIN_REASONABLE_CELL_HEIGHT..=96).contains(&cell_height)
        {
            return (cell_width, cell_height);
        }
    }

    tmux_cell_size().unwrap_or((FALLBACK_CELL_WIDTH, FALLBACK_CELL_HEIGHT))
}

fn draw_terminal_frame(out: &mut dyn Write, columns: u32, rows: u32) -> io::Result<()> {
    if columns < 2 || rows < 2 {
        return Ok(());
    }

    let top = titled_rule(columns, "Lector Browser");
    let horizontal = "─".repeat(columns.saturating_sub(2) as usize);
    write!(out, "\x1b[1;1H{top}")?;
    for row in 2..rows {
        write!(out, "\x1b[{};1H│\x1b[{};{}H│", row, row, columns)?;
    }
    write!(out, "\x1b[{};1H└{}┘", rows, horizontal)?;
    Ok(())
}

fn titled_rule(columns: u32, title: &str) -> String {
    if columns <= 2 {
        return String::new();
    }

    let inner = columns.saturating_sub(2) as usize;
    let title = format!(" {title} ");
    if title.len() >= inner {
        return format!("┌{}┐", title.chars().take(inner).collect::<String>());
    }

    let left = (inner - title.len()) / 2;
    let right = inner - title.len() - left;
    format!("┌{}{}{}┐", "─".repeat(left), title, "─".repeat(right))
}

fn manual_cell_size() -> Option<(u32, u32)> {
    let width = std::env::var("LECTOR_CELL_WIDTH")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| (MIN_REASONABLE_CELL_WIDTH..=64).contains(value));
    let height = std::env::var("LECTOR_CELL_HEIGHT")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| (MIN_REASONABLE_CELL_HEIGHT..=96).contains(value));

    match (width, height) {
        (Some(width), Some(height)) => Some((width, height)),
        _ => None,
    }
}

fn tmux_cell_size() -> Option<(u32, u32)> {
    std::env::var_os("TMUX")?;
    let output = std::process::Command::new("tmux")
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
    let width = parts.next()?.parse::<u32>().ok()?;
    let height = parts.next()?.parse::<u32>().ok()?;
    if (MIN_REASONABLE_CELL_WIDTH..=64).contains(&width)
        && (MIN_REASONABLE_CELL_HEIGHT..=96).contains(&height)
    {
        Some((width, height))
    } else {
        None
    }
}
