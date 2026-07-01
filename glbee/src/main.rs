mod cli;
mod math;
mod model;
mod protocols;
mod renderer;
mod terminal;

use std::env;
use std::io::{self, IsTerminal};

use cli::parse_args;
use model::load_model;
use protocols::write_image;
use renderer::{DEFAULT_BACKGROUND, render_model};
use terminal::{interactive_loop, render_target, resolve_protocol};

fn main() {
    if let Err(err) = run() {
        eprintln!("glbee: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = parse_args(env::args().skip(1).collect())?;
    let model = load_model(&config.path)?;
    let protocol = resolve_protocol(config.protocol, io::stdout().is_terminal());
    let view = renderer::View {
        yaw: -0.65,
        pitch: 0.35,
        distance: 2.85,
    };

    if config.static_frame || !io::stdout().is_terminal() {
        let target = render_target(config.width, config.height, protocol);
        let image = render_model(
            &model,
            view,
            target.width,
            target.height,
            DEFAULT_BACKGROUND,
        );
        write_image(&image, protocol, config.max_colors, target.placement)?;
        println!();
        return Ok(());
    }

    interactive_loop(
        &model,
        protocol,
        config.width,
        config.height,
        config.max_colors,
    )
}
