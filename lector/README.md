# Lector

Lector is an experimental graphical browser for terminals. It renders browser
frames as pixels and sends them to terminals through image protocols, with Sixel
as the first backend.

The project goal is a terminal-native browser that works without a GUI window
and still supports mouse clicks, scrolling, text input, dragging, tabs, and real
web-page rendering.

## Status

Lector is early software. The core terminal shell is usable, and the repository
contains an in-process Servo experiment for real page rendering.

Current highlights:

- fullscreen alternate-screen terminal UI
- Sixel frame output
- tmux-aware Sixel passthrough handling
- mouse click, drag, wheel, and keyboard event plumbing
- tab bar and address bar
- `Ctrl-N` new tab, `Ctrl-W` close tab, `Esc` quit
- native Servo backend when building from this repository with vendored Servo

The recommended build is the precompiled GitHub Release binary. It is built
with the native Servo backend enabled, so it can render real web pages without
requiring Servo to be installed separately.

## Install

### Precompiled Native Servo Build

Download the latest `lector-*-aarch64-apple-darwin.tar.gz` archive from GitHub
Releases, then install the binary:

```sh
tar -xzf lector-0.1.5-aarch64-apple-darwin.tar.gz
sudo install -m 0755 lector-0.1.5-aarch64-apple-darwin/lector /usr/local/bin/lector
lector https://www.google.com.hk
```

The release binary is the distribution path for `servo-native`. It includes the
Lector terminal shell and the in-process Servo adapter in one executable.

### Lightweight Cargo Build

```sh
cargo install lector-browser
```

Run the packaged build:

```sh
lector https://example.com
lector --engine html https://example.com
lector --engine demo
```

The crates.io package contains the terminal shell and protocol backends for
testing and development. It does not include the heavy native Servo build.

## Native Servo

To build the native in-process Servo backend from source:

```sh
./scripts/fetch-servo.sh
cargo run --features servo-native -- https://www.google.com.hk
```

The native adapter lives in `crates/lector-servo-native` and uses Servo's
software rendering context to capture RGBA frames for the Sixel pipeline.

## Controls

- `Esc`: quit, or leave address-bar focus
- `Ctrl-C`: quit
- `Ctrl-N`: new tab
- `Ctrl-W`: close current tab
- `Enter`: navigate when the address bar is focused
- arrow keys / PageUp / PageDown: scroll and key navigation
- mouse wheel: scroll
- left click: click page controls or switch tabs
- drag: send pointer drag events to the page

## tmux

Lector wraps Sixel output for tmux passthrough and temporarily hides the tmux
status line while running. For best results, enable passthrough in tmux:

```tmux
set -g allow-passthrough on
```

Then reload tmux:

```sh
tmux source-file ~/.tmux.conf
```

If the image still does not appear, verify that the outer terminal itself
supports Sixel. Lector also reads tmux client cell-size metadata to size the
rendered framebuffer.

## Terminal Sizing

Inspect detected terminal geometry:

```sh
lector --probe-terminal
```

Override cell size when the terminal does not report usable pixel dimensions:

```sh
LECTOR_CELL_WIDTH=10 LECTOR_CELL_HEIGHT=20 lector https://example.com
```

## Backends

- `servo-native`: default in GitHub Release binaries and source builds made
  with `--features servo-native`
- `html`: default without native Servo; lightweight renderer for terminal/protocol testing
- `demo`: synthetic graphical page for layout and Sixel debugging

## Release Builds

Maintainers can create the same archive locally:

```sh
./scripts/fetch-servo.sh
./scripts/package-release.sh
```

GitHub Actions publishes release archives when a `lector-v*` tag is pushed:

```sh
git tag lector-v0.1.5
git push origin lector-v0.1.5
```

## Architecture

```text
src/
  app.rs              event loop, tabs, address bar, frame scheduling
  cli.rs              command-line parsing
  event.rs            keyboard and mouse event model
  terminal.rs         raw mode, terminal queries, tmux lifecycle
  engine/
    mod.rs            browser engine trait and event types
    html.rs           lightweight built-in renderer
    demo.rs           synthetic layout renderer
    servo_native.rs   feature-gated native Servo adapter
  graphics/
    frame.rs          RGBA frame buffer
    sixel.rs          Sixel encoder and tmux wrapper
```

The engine contract is intentionally small: an engine receives viewport/input
events and returns an RGBA frame. The terminal layer owns protocol output,
chrome, input, and lifecycle cleanup.
