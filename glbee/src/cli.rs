use std::path::PathBuf;

use crate::protocols::Protocol;

#[derive(Debug)]
pub struct Config {
    pub path: PathBuf,
    pub protocol: Protocol,
    pub width: Option<usize>,
    pub height: Option<usize>,
    pub max_colors: usize,
    pub static_frame: bool,
}

pub fn parse_args(args: Vec<String>) -> Result<Config, String> {
    let mut path = None;
    let mut protocol = Protocol::Auto;
    let mut width = None;
    let mut height = None;
    let mut max_colors = 96usize;
    let mut static_frame = false;

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--protocol" => {
                index += 1;
                protocol = parse_protocol(args.get(index).ok_or("--protocol needs a value")?)?;
            }
            "--width" | "-w" => width = Some(parse_next_usize(&args, &mut index, "--width")?),
            "--height" | "-h" => height = Some(parse_next_usize(&args, &mut index, "--height")?),
            "--max-colors" => {
                max_colors = parse_next_usize(&args, &mut index, "--max-colors")?.clamp(2, 256)
            }
            "--static" => static_frame = true,
            "--help" => {
                print_help();
                std::process::exit(0);
            }
            value if !value.starts_with('-') && path.is_none() => path = Some(PathBuf::from(value)),
            other => return Err(format!("unknown argument: {other}")),
        }
        index += 1;
    }

    Ok(Config {
        path: path.ok_or("usage: glbee MODEL [--protocol auto|kitty|iterm2|sixel|blocks]")?,
        protocol,
        width,
        height,
        max_colors,
        static_frame,
    })
}

fn parse_next_usize(args: &[String], index: &mut usize, label: &str) -> Result<usize, String> {
    *index += 1;
    args.get(*index)
        .ok_or_else(|| format!("{label} needs a value"))?
        .parse::<usize>()
        .map_err(|_| format!("{label} must be a positive integer"))
}

fn parse_protocol(value: &str) -> Result<Protocol, String> {
    match value {
        "auto" => Ok(Protocol::Auto),
        "kitty" => Ok(Protocol::Kitty),
        "iterm2" | "iterm" => Ok(Protocol::Iterm2),
        "sixel" => Ok(Protocol::Sixel),
        "blocks" => Ok(Protocol::Blocks),
        other => Err(format!(
            "unknown protocol: {other} (expected auto, kitty, iterm2, sixel, or blocks)"
        )),
    }
}

fn print_help() {
    println!(
        "Usage:
  glbee MODEL [--protocol auto|kitty|iterm2|sixel|blocks]

Options:
      --protocol P    Terminal image protocol (default: auto)
  -w, --width N       Render width in pixels
  -h, --height N      Render height in pixels
      --max-colors N  Sixel palette size, clamped to 2..256 (default: 96)
      --static        Render one frame and exit
      --help          Show this help

Controls:
  Drag left mouse     Rotate
  Wheel               Zoom
  +/-                 Zoom
  Arrows              Rotate
  r                   Reset view
  q/Esc/Ctrl-C        Quit"
    );
}
