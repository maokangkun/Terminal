# Native Servo Integration

Lector uses `servo-native` as its Servo path. The `servo` engine name is kept as
an alias, but it also selects the in-process native Servo adapter.

Native integration cannot be built by copying only Servo's HTML/layout code.
Servo's renderer depends on the full workspace: script, style, layout,
constellation, compositor, WebRender/surfman, SpiderMonkey, resources, and
platform glue.

## Current Status

Done:

- Servo source is expected under `vendor/servo`.
- `crates/lector-servo-native` depends on Servo through path dependencies.
- The adapter uses Servo's `SoftwareRenderingContext`.
- The adapter captures RGBA frames with `RenderingContext::read_to_image`.
- The main app can pass those frames into the Sixel pipeline with
  `cargo run --features servo-native -- URL`.

Next:

- Map Lector keyboard, mouse, scroll, drag, and resize events into Servo
  embedder events.
- Add navigation/reload lifecycle handling.
- Tune frame scheduling so Servo repaint notifications drive terminal redraws.

## Fetching Servo

```sh
./scripts/fetch-servo.sh
```

If the partial clone fails during checkout, retry on a more stable network or
use a normal clone:

```sh
git clone https://github.com/servo/servo.git vendor/servo
```

## Why This Is Separate

The terminal and Sixel pipeline should stay small and testable while Servo
integration evolves. The native adapter will be the only layer that knows about
Servo's constellation, compositor, and embedder APIs.

## Adapter Experiment

The first native adapter lives in:

```text
crates/lector-servo-native
```

It uses Servo's public API:

- `ServoBuilder`
- `WebViewBuilder`
- `SoftwareRenderingContext`
- `RenderingContext::read_to_image`

Try building it separately from the main Lector binary:

```sh
cd crates/lector-servo-native
cargo check
```

Build the main Lector binary with the native adapter:

```sh
cargo check --features servo-native
cargo run --features servo-native -- https://servo.org
```

Try rendering one frame to a raw RGBA file:

```sh
cd crates/lector-servo-native
cargo run --bin render_once -- https://servo.org /tmp/servo-frame.rgba
```

The adapter is optional in the main build because Servo brings a large
dependency graph and platform toolchain requirements. Build with
`--features servo-native` when working on the native browser path.
