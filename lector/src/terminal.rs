use std::fs::File;
use std::io::{self, Read, Write};
#[cfg(unix)]
use std::os::raw::{c_int, c_ulong};
use std::process::{Command, Stdio};
use std::sync::Once;

use crate::event::{Event, Key, MouseButton, MouseEvent, MouseEventKind};

#[derive(Debug, Clone, Copy)]
pub struct TerminalSize {
    pub rows: u16,
    pub columns: u16,
    pub pixel_width: Option<u16>,
    pub pixel_height: Option<u16>,
}

pub struct Terminal {
    size: TerminalSize,
    saved_state: String,
}

pub struct TerminalGuard {
    tmux_status: Option<String>,
    tmux_allow_passthrough: Option<String>,
    saved_state: String,
}

impl Terminal {
    pub fn new() -> io::Result<Self> {
        let saved_state = command_output("stty", &["-g"])?;
        let size = terminal_size().unwrap_or(TerminalSize {
            rows: 40,
            columns: 100,
            pixel_width: None,
            pixel_height: None,
        });

        Ok(Self { size, saved_state })
    }

    pub fn size(&self) -> TerminalSize {
        self.size
    }

    pub fn refresh_size(&mut self) -> io::Result<()> {
        if let Some(size) = terminal_size() {
            self.size = size;
        }
        Ok(())
    }

    pub fn read_event(&mut self) -> io::Result<Event> {
        let mut stdin = io::stdin().lock();
        let mut first = [0_u8; 1];
        if stdin.read(&mut first)? == 0 {
            return Ok(Event::Unknown);
        }
        parse_event(first[0], &mut stdin)
    }
}

impl TerminalGuard {
    pub fn enter(terminal: &mut Terminal) -> io::Result<Self> {
        install_panic_terminal_restore_hook();
        let tmux_status = hide_tmux_status();
        let tmux_allow_passthrough = enable_tmux_passthrough();
        let _ = stty_status(&["raw", "-echo", "min", "0", "time", "1"])?;
        let mut out = io::stdout().lock();
        write!(
            out,
            "\x1b7\x1b[?1049h\x1b[?25l\x1b[?7l\x1b[?1000h\x1b[?1002h\x1b[?1006h"
        )?;
        out.flush()?;
        terminal.refresh_size()?;
        Ok(Self {
            tmux_status,
            tmux_allow_passthrough,
            saved_state: terminal.saved_state.clone(),
        })
    }
}

fn install_panic_terminal_restore_hook() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let mut out = io::stdout().lock();
            let _ = write!(out, "{}", terminal_restore_sequence());
            let _ = out.flush();
            previous(info);
        }));
    });
}

fn terminal_restore_sequence() -> &'static str {
    "\x1b[?2026l\x1b[?1006l\x1b[?1002l\x1b[?1000l\x1b[?7h\x1b[?25h\x1b[0m\x1b[?1049l"
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut out = io::stdout().lock();
        let _ = write!(out, "{}\x1b8", terminal_restore_sequence());
        let _ = out.flush();
        if !self.saved_state.trim().is_empty() {
            let _ = stty_status(&[self.saved_state.trim()]);
        }
        restore_tmux_passthrough(self.tmux_allow_passthrough.take());
        restore_tmux_status(self.tmux_status.take());
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if !self.saved_state.trim().is_empty() {
            let _ = stty_status(&[self.saved_state.trim()]);
        }
    }
}

fn parse_event(first: u8, input: &mut dyn Read) -> io::Result<Event> {
    match first {
        b'\r' | b'\n' => Ok(Event::Key(Key::Enter)),
        1..=26 => Ok(Event::Key(Key::Ctrl((b'a' + first - 1) as char))),
        127 => Ok(Event::Key(Key::Backspace)),
        27 => parse_escape(input),
        byte if byte.is_ascii_control() => Ok(Event::Unknown),
        byte => Ok(Event::Key(Key::Char(byte as char))),
    }
}

fn parse_escape(input: &mut dyn Read) -> io::Result<Event> {
    let mut buf = [0_u8; 1];
    if input.read(&mut buf)? == 0 {
        return Ok(Event::Key(Key::Esc));
    }
    if buf[0] != b'[' {
        return Ok(Event::Key(Key::Esc));
    }

    let mut seq = Vec::new();
    loop {
        let mut byte = [0_u8; 1];
        if input.read(&mut byte)? == 0 {
            return Ok(Event::Unknown);
        }
        seq.push(byte[0]);
        if byte[0].is_ascii_alphabetic() || byte[0] == b'~' {
            break;
        }
    }

    match seq.as_slice() {
        b"A" => Ok(Event::Key(Key::Up)),
        b"B" => Ok(Event::Key(Key::Down)),
        b"C" => Ok(Event::Key(Key::Right)),
        b"D" => Ok(Event::Key(Key::Left)),
        b"5~" => Ok(Event::Key(Key::PageUp)),
        b"6~" => Ok(Event::Key(Key::PageDown)),
        _ if seq.first() == Some(&b'<') => parse_sgr_mouse(&seq),
        _ => Ok(Event::Unknown),
    }
}

fn parse_sgr_mouse(seq: &[u8]) -> io::Result<Event> {
    let final_byte = *seq.last().unwrap_or(&b'M');
    let body = &seq[1..seq.len().saturating_sub(1)];
    let body = std::str::from_utf8(body).unwrap_or("");
    let parts: Vec<&str> = body.split(';').collect();
    if parts.len() != 3 {
        return Ok(Event::Unknown);
    }

    let code = parts[0].parse::<u16>().unwrap_or(0);
    let column = parts[1].parse::<u16>().unwrap_or(1);
    let row = parts[2].parse::<u16>().unwrap_or(1);

    let button = match code & 0b11 {
        0 => MouseButton::Left,
        1 => MouseButton::Middle,
        2 => MouseButton::Right,
        _ => MouseButton::Other,
    };

    let kind = if code & 64 != 0 {
        if code & 1 == 0 {
            MouseEventKind::ScrollUp
        } else {
            MouseEventKind::ScrollDown
        }
    } else if code & 32 != 0 {
        MouseEventKind::Drag(button)
    } else if final_byte == b'm' {
        MouseEventKind::Up(button)
    } else {
        MouseEventKind::Down(button)
    };

    Ok(Event::Mouse(MouseEvent { kind, column, row }))
}

fn terminal_size() -> Option<TerminalSize> {
    if let Some(size) = tmux_pane_size() {
        return Some(size);
    }
    terminal_size_pixels().or_else(query_stty_size)
}

fn tmux_pane_size() -> Option<TerminalSize> {
    std::env::var_os("TMUX")?;
    let output = Command::new("tmux")
        .args(["display-message", "-p", "#{pane_width} #{pane_height}"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let mut parts = text.split_whitespace();
    let columns = parts.next()?.parse::<u16>().ok()?;
    let rows = parts.next()?.parse::<u16>().ok()?;
    if columns == 0 || rows == 0 {
        return None;
    }
    Some(TerminalSize {
        rows,
        columns,
        pixel_width: None,
        pixel_height: None,
    })
}

#[cfg(unix)]
fn terminal_size_pixels() -> Option<TerminalSize> {
    if std::env::var_os("LECTOR_ENABLE_IOCTL").is_none() {
        return None;
    }

    let mut size = Winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let result = unsafe { ioctl(STDOUT_FILENO, TIOCGWINSZ, &mut size as *mut Winsize) };
    if result != 0 || size.ws_row == 0 || size.ws_col == 0 {
        return None;
    }
    Some(TerminalSize {
        rows: size.ws_row,
        columns: size.ws_col,
        pixel_width: (size.ws_xpixel > 0).then_some(size.ws_xpixel),
        pixel_height: (size.ws_ypixel > 0).then_some(size.ws_ypixel),
    })
}

#[cfg(not(unix))]
fn terminal_size_pixels() -> Option<TerminalSize> {
    None
}

#[cfg(unix)]
const STDOUT_FILENO: c_int = 1;

#[cfg(all(unix, target_os = "macos"))]
const TIOCGWINSZ: c_ulong = 0x40087468;

#[cfg(all(unix, not(target_os = "macos")))]
const TIOCGWINSZ: c_ulong = 0x5413;

#[cfg(unix)]
#[repr(C)]
struct Winsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

#[cfg(unix)]
unsafe extern "C" {
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
}

fn query_stty_size() -> Option<TerminalSize> {
    let output = command_output("stty", &["size"]).ok()?;
    let mut parts = output.split_whitespace();
    let rows = parts.next()?.parse().ok()?;
    let columns = parts.next()?.parse().ok()?;
    Some(TerminalSize {
        rows,
        columns,
        pixel_width: None,
        pixel_height: None,
    })
}

fn command_output(command: &str, args: &[&str]) -> io::Result<String> {
    let output = Command::new(command)
        .args(args)
        .stdin(Stdio::from(File::open("/dev/tty")?))
        .output()?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn stty_status(args: &[&str]) -> io::Result<std::process::ExitStatus> {
    Command::new("stty")
        .args(args)
        .stdin(Stdio::from(File::open("/dev/tty")?))
        .status()
}

fn hide_tmux_status() -> Option<String> {
    if std::env::var_os("TMUX").is_none() || std::env::var_os("LECTOR_TMUX_KEEP_STATUS").is_some() {
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

fn enable_tmux_passthrough() -> Option<String> {
    if std::env::var_os("TMUX").is_none() {
        return None;
    }
    let output = Command::new("tmux")
        .args(["show-option", "-p", "-q", "-v", "allow-passthrough"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let original = String::from_utf8(output.stdout)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "off".to_string());
    let status = Command::new("tmux")
        .args(["set-option", "-p", "-q", "allow-passthrough", "all"])
        .status()
        .ok()?;
    status.success().then_some(original)
}

fn restore_tmux_passthrough(original: Option<String>) {
    if let Some(value) = original {
        let _ = Command::new("tmux")
            .args(["set-option", "-p", "-q", "allow-passthrough", &value])
            .status();
    }
}
