mod demo;
mod html;
mod servo_native;

pub use demo::DemoEngine;
pub use html::HtmlEngine;
pub use servo_native::ServoNativeEngine;

use crate::graphics::frame::Frame;

pub trait BrowserEngine {
    fn handle_event(&mut self, event: EngineEvent);
    fn render(&mut self, width: u32, height: u32) -> Frame;
    fn chrome(&self) -> BrowserChrome {
        BrowserChrome::default()
    }
}

#[derive(Debug, Clone)]
pub enum EngineEvent {
    Scroll {
        dx: i32,
        dy: i32,
    },
    Click {
        x: u16,
        y: u16,
    },
    Drag {
        x: u16,
        y: u16,
    },
    PointerMove {
        x: u32,
        y: u32,
    },
    PointerDown {
        x: u32,
        y: u32,
        button: PointerButton,
    },
    PointerUp {
        x: u32,
        y: u32,
        button: PointerButton,
    },
    Wheel {
        x: u32,
        y: u32,
        dx: i32,
        dy: i32,
    },
    KeyPress(EngineKey),
    Text(String),
    Navigate(String),
    SwitchTab(usize),
    NewTab,
    CloseTab,
    Resize {
        width: u32,
        height: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerButton {
    Left,
    Middle,
    Right,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineKey {
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Enter,
    Backspace,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BrowserChrome {
    pub tabs: Vec<BrowserTab>,
    pub active_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserTab {
    pub title: String,
    pub url: String,
    pub active: bool,
}
