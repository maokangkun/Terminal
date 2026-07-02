use std::fs;
use std::time::Duration;

use lector_servo_native::{NativeServoConfig, render_once};

fn main() {
    if let Err(error) = run() {
        eprintln!("lector-servo-native: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let url = args
        .next()
        .unwrap_or_else(|| "https://servo.org".to_string());
    let output = args
        .next()
        .unwrap_or_else(|| "servo-native.rgba".to_string());
    let width = std::env::var("LECTOR_NATIVE_WIDTH")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(800);
    let height = std::env::var("LECTOR_NATIVE_HEIGHT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(600);

    let frame = render_once(NativeServoConfig {
        url,
        width,
        height,
        settle_timeout: Duration::from_secs(8),
    })?;

    fs::write(&output, &frame.rgba)
        .map_err(|error| format!("failed to write {output}: {error}"))?;
    println!(
        "wrote {}x{} RGBA frame to {output}",
        frame.width, frame.height
    );
    Ok(())
}
