# Terminal GeoGebra

An interactive math graphing playground that runs entirely inside the terminal.

It renders mathematical formulas with Matplotlib, captures the figure as raw
RGBA pixels, and streams those pixels into the high-performance Rust terminal
image backend [`sixrs`](../sixrs/README.md). The result is a simple terminal UI
with a large `GRAPH` viewport, editable formula input, rendered formula
previews, and persistent history.

![Terminal GeoGebra demo](asset/Screen_Recording.gif)

## Features

- Interactive 3D surface plotting with mouse drag, keyboard rotation, zoom, pan,
  and automatic rotation.
- 2D explicit plots such as `y=x+1`.
- 2D implicit equations such as `x^2+y^2=1`.
- Flexible SymPy-backed parsing; formulas are not limited to `x`, `y`, and `z`.
- LaTeX-style input and rendered formula previews.
- Persistent formula history saved in `.history.json`.
- Clickable and scrollable `HISTORY` panel with the active formula highlighted.
- Terminal-aware layout with tmux handling and Sixel image refresh support.
- Graph images use the terminal background color when available.

## Architecture

The app is split into a few small pieces:

- `main.py` drives the terminal event loop and application state.
- `formula.py` parses formulas with SymPy and turns them into NumPy evaluators.
- `render_pipeline.py` renders Matplotlib figures to RGBA and encodes them
  through `sixrs`.
- `ui.py` owns the terminal layout, frames, GRAPH/HISTORY/INPUT panels, preview
  images, and footer.
- `graph.py` stores view state and mouse/keyboard graph controls.
- `history.py` manages persistent formula history.
- `../sixrs/` contains the Rust image backend used for raw RGBA to terminal
  image output.

Rendering flow:

```text
formula input -> SymPy parser -> NumPy sampler -> Matplotlib Agg RGBA
              -> sixrs --raw-rgba WIDTH HEIGHT -> terminal Sixel/blocks
```

## Requirements

- Python 3.11+
- Rust/Cargo, for building `sixrs`
- A terminal with Sixel support for best results
- tmux works, but make sure your terminal itself supports Sixel

Python packages:

```bash
python3 -m pip install matplotlib numpy sympy
```

Optional, for broader SymPy LaTeX parsing support:

```bash
python3 -m pip install antlr4-python3-runtime
```

Install the local `sixrs` backend from the repository root:

```bash
cargo install --path sixrs
```

Confirm it is available:

```bash
sixrs -h
```

## Run

From the repository root:

```bash
python3 geogebra/main.py
```

Or from this directory:

```bash
python3 main.py
```

The default graph is:

```text
z=\cos(\sqrt{x^2+y^2})e^{\frac{-(x^2+y^2)}{18}}
```

## Controls

| Action | Control |
| --- | --- |
| Edit formula | Click the `INPUT` prompt and type |
| Submit formula | `Enter` |
| Move input cursor | Left / Right while INPUT is focused |
| Select old formula | Click an item in `HISTORY` |
| Scroll history | Mouse wheel over `HISTORY` |
| Rotate graph | Drag in `GRAPH` or use arrow keys while GRAPH is focused |
| Pan graph | Right/middle drag in `GRAPH` |
| Zoom | Mouse wheel while `GRAPH` is focused |
| Pause/resume auto rotation | `Space` |
| Quit | `Esc` |

## Formula Examples

3D explicit surfaces:

```text
z=sin(x)*cos(y)
a=b^2+c^2
cos(sqrt(x^2+y^2))
```

2D explicit curves:

```text
y=x+1
f=t^2-3*t
sin(x)
```

2D implicit equations:

```text
x^2+y^2=1
a^2+b^2=4
sin(x)+cos(y)=0
```

## Notes

The project name is descriptive of the app's graphing goal; it is not affiliated
with GeoGebra.

`sixrs` can also be used independently as a fast terminal image encoder. See
[`../sixrs/README.md`](../sixrs/README.md) for its raw RGB/RGBA and image-file
usage.
