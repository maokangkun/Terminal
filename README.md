# Terminal

Terminal is a small playground for exploring rich terminal applications. The
repo is less about one polished product and more about testing what modern
terminals can do when graphics, raw input, layout, tmux behavior, and fast
rendering backends are treated as first-class building blocks.

## Experiments

### Terminal GeoGebra

[`geogebra/`](geogebra/) is an interactive math graphing app that runs inside
the terminal.

It combines a Python terminal UI, SymPy formula parsing, Matplotlib rendering,
and the Rust `sixrs` backend to plot 2D curves, implicit equations, and 3D
surfaces as terminal images. It includes a GRAPH viewport, editable formula
input, rendered formula previews, scrollable history, tmux-aware image refresh,
and automatic 3D rotation.

![Terminal GeoGebra demo](geogebra/asset/Screen_Recording.gif)

Read more: [`geogebra/README.md`](geogebra/README.md)

### sixrs

[`sixrs/`](sixrs/) is a fast Rust command-line image renderer for terminals.

It can read common image formats or raw RGB/RGBA bytes and emit Sixel output,
with a Unicode block fallback for terminals without Sixel support. The GeoGebra
experiment uses its raw RGBA path as a low-overhead bridge between Matplotlib
and the terminal.

Read more: [`sixrs/README.md`](sixrs/README.md)

### glbee

[`glbee/`](glbee/) is a fast Rust terminal 3D model previewer, published on
crates.io as `glbee`.

It previews `.glb`, `.gltf`, `.obj`, binary `.fbx`, `.3ds`, `.blend`, and
`.usdz` models directly in the terminal with mouse drag rotation, wheel zoom,
keyboard controls, tmux-aware image output, and automatic protocol detection for
Sixel, Kitty graphics, iTerm2 inline images, and ANSI truecolor blocks.

Install it from crates.io:

https://github.com/user-attachments/assets/7a247eb1-aac9-42cb-a5d6-0dedcb267b94

```bash
cargo install glbee
```

Read more: [`glbee/README.md`](glbee/README.md)

## Themes

This repository is currently exploring:

- High-resolution terminal graphics with Sixel and fallback block rendering.
- Interactive full-screen terminal layouts with mouse and keyboard input.
- tmux behavior around image persistence, status-line redraws, and pixel size
  reporting.
- Using Python for fast iteration on UI and math logic while delegating image
  encoding to Rust.
- Treating terminal apps as rich visual tools rather than plain text streams.

## Repository Layout

```text
.
├── geogebra/   terminal math graphing experiment
├── glbee/      Rust terminal 3D model previewer
├── sixrs/      Rust terminal image backend
└── asset/      shared sample assets
```

## Quick Start

Build the terminal image backend:

```bash
cargo install --path sixrs
```

Run the graphing experiment:

```bash
python3 geogebra/main.py
```

Install the 3D model previewer:

```bash
cargo install glbee
```

See each experiment's README for its own dependencies and details.
