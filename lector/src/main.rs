mod app;
mod cli;
mod engine;
mod event;
mod graphics;
mod terminal;

fn main() {
    let config = cli::Config::parse();
    if let Err(error) = app::run(config) {
        eprintln!("lector: {error}");
        std::process::exit(1);
    }
}
