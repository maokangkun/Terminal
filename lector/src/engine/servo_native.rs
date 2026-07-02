//! In-process Servo backend.

use crate::engine::{BrowserChrome, BrowserEngine, BrowserTab, EngineEvent};
#[cfg(feature = "servo-native")]
use crate::engine::{EngineKey, PointerButton};
use crate::graphics::frame::{Frame, Rgba};

pub struct ServoNativeEngine {
    url: String,
    #[cfg(feature = "servo-native")]
    adapter: Option<lector_servo_native::NativeServoAdapter>,
    #[cfg(feature = "servo-native")]
    adapter_size: Option<(u32, u32)>,
    #[cfg(feature = "servo-native")]
    last_error: Option<String>,
    #[cfg(feature = "servo-native")]
    loading_started_at: std::time::Instant,
    #[cfg(feature = "servo-native")]
    cached_frame: Option<Frame>,
}

impl ServoNativeEngine {
    pub fn new(url: String) -> Self {
        Self {
            url,
            #[cfg(feature = "servo-native")]
            adapter: None,
            #[cfg(feature = "servo-native")]
            adapter_size: None,
            #[cfg(feature = "servo-native")]
            last_error: None,
            #[cfg(feature = "servo-native")]
            loading_started_at: std::time::Instant::now(),
            #[cfg(feature = "servo-native")]
            cached_frame: None,
        }
    }
}

impl BrowserEngine for ServoNativeEngine {
    fn handle_event(&mut self, event: EngineEvent) {
        #[cfg(feature = "servo-native")]
        match event {
            EngineEvent::Resize { width, height } => {
                if self.adapter_size != Some((width, height)) {
                    self.adapter_size = Some((width, height));
                    self.cached_frame = None;
                    if let Some(adapter) = self.adapter.as_ref() {
                        adapter.resize(width, height);
                    }
                }
            }
            EngineEvent::PointerMove { x, y } => {
                if let Some(adapter) = self.adapter.as_ref() {
                    adapter.mouse_move(x, y);
                }
            }
            EngineEvent::PointerDown { x, y, button } => {
                if let Some(adapter) = self.adapter.as_ref() {
                    adapter.mouse_button(
                        lector_servo_native::NativeMouseButtonAction::Down,
                        native_button(button),
                        x,
                        y,
                    );
                }
            }
            EngineEvent::PointerUp { x, y, button } => {
                if let Some(adapter) = self.adapter.as_ref() {
                    adapter.mouse_button(
                        lector_servo_native::NativeMouseButtonAction::Up,
                        native_button(button),
                        x,
                        y,
                    );
                }
            }
            EngineEvent::Wheel { x, y, dx, dy } => {
                if let Some(adapter) = self.adapter.as_ref() {
                    adapter.wheel(dx, dy, x, y);
                }
            }
            EngineEvent::Text(text) => {
                if let Some(adapter) = self.adapter.as_ref() {
                    adapter.insert_text(&text);
                }
            }
            EngineEvent::KeyPress(key) => {
                if let Some(adapter) = self.adapter.as_ref() {
                    adapter.key_press(native_key(key));
                }
            }
            EngineEvent::Navigate(url) => {
                if let Some(adapter) = self.adapter.as_ref() {
                    if let Err(error) = adapter.load_url(&url) {
                        self.last_error = Some(error);
                    } else {
                        self.url = url;
                        self.cached_frame = None;
                        self.loading_started_at = std::time::Instant::now();
                    }
                }
            }
            EngineEvent::SwitchTab(index) => {
                if let Some(adapter) = self.adapter.as_ref() {
                    adapter.switch_tab(index);
                    self.cached_frame = None;
                }
            }
            EngineEvent::NewTab => {
                if let Some(adapter) = self.adapter.as_ref() {
                    adapter.new_tab("about:blank");
                    self.cached_frame = None;
                }
            }
            EngineEvent::CloseTab => {
                if let Some(adapter) = self.adapter.as_ref() {
                    adapter.close_active_tab();
                    self.cached_frame = None;
                }
            }
            EngineEvent::Scroll { .. } | EngineEvent::Click { .. } | EngineEvent::Drag { .. } => {}
        }

        #[cfg(not(feature = "servo-native"))]
        let _ = event;
    }

    fn render(&mut self, width: u32, height: u32) -> Frame {
        #[cfg(feature = "servo-native")]
        {
            return self.render_with_servo(width, height);
        }

        #[cfg(not(feature = "servo-native"))]
        {
            self.render_feature_disabled(width, height)
        }
    }

    fn chrome(&self) -> BrowserChrome {
        #[cfg(feature = "servo-native")]
        {
            if let Some(adapter) = self.adapter.as_ref() {
                let tabs = adapter.tabs();
                let active_url = tabs
                    .iter()
                    .find(|tab| tab.active)
                    .map(|tab| tab.url.clone())
                    .unwrap_or_default();
                return BrowserChrome {
                    tabs: tabs
                        .into_iter()
                        .map(|tab| BrowserTab {
                            title: tab.title,
                            url: tab.url,
                            active: tab.active,
                        })
                        .collect(),
                    active_url,
                };
            }
        }

        BrowserChrome {
            tabs: vec![BrowserTab {
                title: self.url.clone(),
                url: self.url.clone(),
                active: true,
            }],
            active_url: self.url.clone(),
        }
    }
}

#[cfg(feature = "servo-native")]
fn native_key(key: EngineKey) -> lector_servo_native::NativeKey {
    match key {
        EngineKey::Up => lector_servo_native::NativeKey::Up,
        EngineKey::Down => lector_servo_native::NativeKey::Down,
        EngineKey::Left => lector_servo_native::NativeKey::Left,
        EngineKey::Right => lector_servo_native::NativeKey::Right,
        EngineKey::PageUp => lector_servo_native::NativeKey::PageUp,
        EngineKey::PageDown => lector_servo_native::NativeKey::PageDown,
        EngineKey::Enter => lector_servo_native::NativeKey::Enter,
        EngineKey::Backspace => lector_servo_native::NativeKey::Backspace,
    }
}

#[cfg(feature = "servo-native")]
fn native_button(button: PointerButton) -> lector_servo_native::NativeMouseButton {
    match button {
        PointerButton::Left => lector_servo_native::NativeMouseButton::Left,
        PointerButton::Middle => lector_servo_native::NativeMouseButton::Middle,
        PointerButton::Right => lector_servo_native::NativeMouseButton::Right,
        PointerButton::Other => lector_servo_native::NativeMouseButton::Other,
    }
}

impl ServoNativeEngine {
    #[cfg(feature = "servo-native")]
    fn render_with_servo(&mut self, width: u32, height: u32) -> Frame {
        use std::time::Duration;

        if self.adapter.is_none() {
            let config = lector_servo_native::NativeServoConfig {
                url: self.url.clone(),
                width,
                height,
                settle_timeout: Duration::from_secs(5),
            };
            match lector_servo_native::NativeServoAdapter::new(config) {
                Ok(adapter) => {
                    self.adapter = Some(adapter);
                    self.adapter_size = Some((width, height));
                    self.last_error = None;
                    self.loading_started_at = std::time::Instant::now();
                    self.cached_frame = None;
                }
                Err(error) => {
                    self.last_error = Some(error);
                    return self.render_error(width, height);
                }
            }
        }

        let Some(adapter) = self.adapter.as_ref() else {
            return self.render_error(width, height);
        };

        let timeout = if self.cached_frame.is_some() {
            Duration::from_millis(1)
        } else {
            Duration::from_millis(100)
        };
        let painted = adapter.pump(timeout);
        if !painted {
            if let Some(frame) = &self.cached_frame {
                return frame.clone();
            }
        }

        match adapter.capture() {
            Ok(frame) => {
                let Some(mut frame) =
                    Frame::from_rgba_bytes(frame.width, frame.height, &frame.rgba)
                else {
                    self.last_error = Some("Servo returned an invalid RGBA buffer".to_string());
                    return self.render_error(width, height);
                };
                if mostly_white(&frame) {
                    let active_url = active_adapter_url(adapter);
                    if !active_url.is_empty() {
                        frame = self.render_loading(width, height, &active_url);
                    } else {
                        frame = self.render_blank(width, height);
                    }
                } else {
                    self.loading_started_at = std::time::Instant::now();
                }
                self.cached_frame = Some(frame.clone());
                frame
            }
            Err(error) => {
                self.last_error = Some(error);
                self.render_error(width, height)
            }
        }
    }

    #[cfg(not(feature = "servo-native"))]
    fn render_feature_disabled(&mut self, width: u32, height: u32) -> Frame {
        let mut frame = Frame::new(width, height, Rgba::rgb(30, 32, 34));
        frame.draw_text(24, 28, "SERVO NATIVE BACKEND", Rgba::rgb(236, 238, 240));
        frame.draw_text(24, 56, &self.url, Rgba::rgb(174, 184, 194));
        frame.draw_text(
            24,
            96,
            "Rebuild Lector with --features servo-native to enable this backend.",
            Rgba::rgb(236, 238, 240),
        );
        frame.draw_text(
            24,
            124,
            "Servo source must be present in vendor/servo.",
            Rgba::rgb(174, 184, 194),
        );
        frame.draw_text(
            24,
            152,
            "See docs/servo-native.md for the integration plan.",
            Rgba::rgb(174, 184, 194),
        );
        frame
    }

    #[cfg(feature = "servo-native")]
    fn render_error(&self, width: u32, height: u32) -> Frame {
        let mut frame = Frame::new(width, height, Rgba::rgb(30, 32, 34));
        frame.draw_text(24, 28, "SERVO NATIVE ERROR", Rgba::rgb(236, 238, 240));
        frame.draw_text(24, 56, &self.url, Rgba::rgb(174, 184, 194));
        if let Some(error) = &self.last_error {
            for (index, line) in wrap_error(error, 88).iter().take(8).enumerate() {
                frame.draw_text(24, 96 + index as i32 * 24, line, Rgba::rgb(236, 190, 160));
            }
        }
        frame
    }

    #[cfg(feature = "servo-native")]
    fn render_loading(&self, width: u32, height: u32, url: &str) -> Frame {
        let mut frame = Frame::new(width, height, Rgba::rgb(30, 32, 34));
        let elapsed = self.loading_started_at.elapsed().as_millis() as u32;
        let progress = ((elapsed / 90) % 100).min(92);
        let panel_width = width.saturating_sub(96).min(760).max(240);
        let x = ((width.saturating_sub(panel_width)) / 2) as i32;
        let y = ((height.saturating_sub(140)) / 2) as i32;
        let bar_width = panel_width.saturating_sub(32);
        let fill_width = bar_width.saturating_mul(progress) / 100;

        frame.draw_text(x, y, "LECTOR", Rgba::rgb(236, 238, 240));
        frame.draw_text(x, y + 28, "LOADING WEB PAGE", Rgba::rgb(174, 184, 194));
        frame.draw_text(x, y + 56, url, Rgba::rgb(174, 184, 194));
        frame.stroke_rect(x, y + 92, bar_width, 18, Rgba::rgb(174, 184, 194));
        frame.fill_rect(
            x + 2,
            y + 94,
            fill_width.saturating_sub(4),
            14,
            Rgba::rgb(46, 105, 118),
        );
        frame.draw_text(
            x,
            y + 124,
            "WAITING FOR SERVO RENDER",
            Rgba::rgb(236, 238, 240),
        );
        frame
    }

    #[cfg(feature = "servo-native")]
    fn render_blank(&self, width: u32, height: u32) -> Frame {
        Frame::new(width, height, Rgba::rgb(30, 32, 34))
    }
}

#[cfg(feature = "servo-native")]
fn mostly_white(frame: &Frame) -> bool {
    let pixels = frame.pixels();
    if pixels.is_empty() {
        return true;
    }

    let white = pixels
        .iter()
        .filter(|pixel| pixel.r >= 248 && pixel.g >= 248 && pixel.b >= 248)
        .count();
    white * 200 >= pixels.len() * 199
}

#[cfg(feature = "servo-native")]
fn active_adapter_url(adapter: &lector_servo_native::NativeServoAdapter) -> String {
    adapter
        .tabs()
        .into_iter()
        .find(|tab| tab.active)
        .map(|tab| tab.url)
        .unwrap_or_default()
}

#[cfg(feature = "servo-native")]
fn wrap_error(error: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();

    for word in error.split_whitespace() {
        if !line.is_empty() && line.len() + word.len() + 1 > width {
            lines.push(line);
            line = String::new();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }

    if !line.is_empty() {
        lines.push(line);
    }

    lines
}
