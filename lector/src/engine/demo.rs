use crate::engine::{BrowserEngine, EngineEvent};
use crate::graphics::frame::{Frame, Rgba};

pub struct DemoEngine {
    url: String,
    scroll_y: i32,
    typed: String,
    cursor: Option<(u16, u16)>,
}

impl DemoEngine {
    pub fn new(url: String) -> Self {
        Self {
            url,
            scroll_y: 0,
            typed: String::new(),
            cursor: None,
        }
    }
}

impl BrowserEngine for DemoEngine {
    fn handle_event(&mut self, event: EngineEvent) {
        match event {
            EngineEvent::Scroll { dx, dy } => {
                self.scroll_y = (self.scroll_y + dy + dx / 4).max(0);
            }
            EngineEvent::Text(text) => {
                self.typed.push_str(&text);
                if self.typed.len() > 48 {
                    let keep_from = self.typed.len() - 48;
                    self.typed = self.typed[keep_from..].to_string();
                }
            }
            EngineEvent::Click { x, y } | EngineEvent::Drag { x, y } => {
                self.cursor = Some((x, y));
            }
            EngineEvent::PointerMove { x, y } => {
                self.cursor = Some((x as u16, y as u16));
            }
            EngineEvent::PointerDown { x, y, button } | EngineEvent::PointerUp { x, y, button } => {
                let _ = button;
                self.cursor = Some((x as u16, y as u16));
            }
            EngineEvent::Wheel { x, y, dx, dy } => {
                let _ = (dx, dy);
                self.cursor = Some((x as u16, y as u16));
            }
            EngineEvent::KeyPress(key) => {
                let _ = key;
            }
            EngineEvent::Navigate(url) => {
                let _ = url;
            }
            EngineEvent::SwitchTab(index) => {
                let _ = index;
            }
            EngineEvent::NewTab | EngineEvent::CloseTab => {}
            EngineEvent::Resize { width, height } => {
                let _ = (width, height);
            }
        }
    }

    fn render(&mut self, width: u32, height: u32) -> Frame {
        let mut frame = Frame::new(width, height, Rgba::rgb(244, 242, 237));
        paint_page(
            &mut frame,
            &self.url,
            self.scroll_y,
            &self.typed,
            self.cursor,
        );
        frame
    }
}

fn paint_page(
    frame: &mut Frame,
    url: &str,
    scroll_y: i32,
    typed: &str,
    cursor: Option<(u16, u16)>,
) {
    let width = frame.width();
    let height = frame.height();

    frame.fill_rect(0, 0, width, 54, Rgba::rgb(28, 32, 36));
    frame.fill_rect(18, 14, width.saturating_sub(36), 26, Rgba::rgb(54, 61, 68));
    frame.draw_text(30, 22, url, Rgba::rgb(222, 231, 235));

    let page_top = 74 - scroll_y;
    frame.draw_text(32, page_top, "Lector", Rgba::rgb(20, 24, 28));
    frame.draw_text(
        32,
        page_top + 28,
        "Graphical terminal browsing, rendered as pixels.",
        Rgba::rgb(63, 71, 79),
    );

    for i in 0..8 {
        let y = page_top + 78 + i * 132;
        let shade = 255_u8.saturating_sub((i as u8) * 9);
        frame.fill_rect(
            32,
            y,
            width.saturating_sub(64),
            96,
            Rgba::rgb(shade, shade, shade),
        );
        frame.stroke_rect(
            32,
            y,
            width.saturating_sub(64),
            96,
            Rgba::rgb(198, 194, 186),
        );
        frame.fill_rect(52, y + 22, 120, 12, Rgba::rgb(46, 105, 118));
        frame.fill_rect(
            52,
            y + 48,
            width.saturating_sub(150),
            9,
            Rgba::rgb(116, 126, 133),
        );
        frame.fill_rect(
            52,
            y + 66,
            width.saturating_sub(220),
            9,
            Rgba::rgb(148, 155, 160),
        );
    }

    let input_y = height as i32 - 62;
    frame.fill_rect(
        24,
        input_y,
        width.saturating_sub(48),
        38,
        Rgba::rgb(255, 255, 255),
    );
    frame.stroke_rect(
        24,
        input_y,
        width.saturating_sub(48),
        38,
        Rgba::rgb(40, 120, 132),
    );
    let label = if typed.is_empty() {
        "TYPE HERE; SERVO INPUT PLUMBING WILL REUSE THIS PATH"
    } else {
        typed
    };
    frame.draw_text(40, input_y + 15, label, Rgba::rgb(38, 43, 48));
    frame.draw_text(
        24,
        height as i32 - 14,
        "Q QUITS | ARROWS/WHEEL SCROLL",
        Rgba::rgb(63, 71, 79),
    );

    if let Some((x, y)) = cursor {
        let px = (x as i32).saturating_mul(8);
        let py = (y as i32).saturating_mul(16);
        frame.stroke_rect(px - 8, py - 8, 24, 24, Rgba::rgb(224, 75, 52));
    }
}
