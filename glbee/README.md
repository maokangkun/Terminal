# glbee

`glbee` is a fast terminal 3D model previewer focused on GLB files.

It renders a 3D model directly inside your terminal, with mouse drag rotation,
wheel zoom, keyboard controls, terminal image protocol detection, and a compact
status footer for render diagnostics.

## Features

- Preview `.glb` models in the terminal.
- Rotate with mouse drag or arrow keys.
- Zoom with mouse wheel or `+` / `-`.
- Supports multiple terminal image protocols:
  - Sixel
  - Kitty graphics protocol
  - iTerm2 inline images
  - ANSI truecolor block fallback
- Uses GLB mesh geometry, normals, UVs, material colors, and base color textures.
- Full-screen framed terminal UI with live render/output timing.
- Interactive fast mode while rotating, then a full-quality frame after input settles.

## Installation

```sh
cargo install glbee
```

## Usage

```sh
glbee model.glb
```

Choose a protocol explicitly:

```sh
glbee model.glb --protocol sixel
glbee model.glb --protocol kitty
glbee model.glb --protocol iterm2
glbee model.glb --protocol blocks
```

Render one static frame and exit:

```sh
glbee model.glb --static
```

Override render size:

```sh
glbee model.glb --width 800 --height 600
```

Tune sixel palette size:

```sh
glbee model.glb --protocol sixel --max-colors 128
```

## Controls

| Control | Action |
| --- | --- |
| Left mouse drag | Rotate |
| Mouse wheel | Zoom |
| Arrow keys | Rotate |
| `+` / `=` | Zoom in |
| `-` | Zoom out |
| `r` | Reset view |
| `q`, `Esc`, `Ctrl-C` | Quit |

## Terminal Protocols

By default, `glbee` uses `--protocol auto`.

Auto detection prefers:

1. Kitty graphics protocol when running inside Kitty.
2. iTerm2 inline images when running inside iTerm2.
3. Sixel when the terminal looks sixel-capable.
4. ANSI truecolor blocks as a portable fallback.

For best visual quality and speed, use a terminal with Kitty graphics or sixel
support. The block fallback works nearly everywhere but has lower spatial
resolution.

## Performance Notes

Terminal rendering has two costs:

- Software 3D rasterization.
- Encoding and writing the resulting image through a terminal graphics protocol.

`glbee` reduces interaction latency by rendering a lower-resolution frame while
you are dragging or pressing rotation keys, then replacing it with a full-quality
frame shortly after interaction stops. The footer shows useful timing fields:

- `render_ms`: software render time.
- `output_ms`: protocol encoding and terminal write time.
- `frame_ms`: total frame time.
- `quality`: `fast` while interacting, `full` when settled.

## Environment

If your terminal reports an unrealistic pixel size, you can override cell size:

```sh
GLBEE_CELL_WIDTH=10 GLBEE_CELL_HEIGHT=20 glbee model.glb
```

## Current Scope

GLB is the primary supported format. Other 3D formats can be added later by
converting them into the same internal triangle and texture representation.

## License

Apache-2.0
