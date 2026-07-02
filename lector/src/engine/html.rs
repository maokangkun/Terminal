use std::process::Command;

use crate::engine::{BrowserEngine, EngineEvent};
use crate::graphics::frame::{Frame, Rgba};

pub struct HtmlEngine {
    url: String,
    state: LoadState,
    scroll_y: i32,
    lines: Vec<String>,
    title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoadState {
    Pending,
    Loaded,
    Failed,
}

impl HtmlEngine {
    pub fn new(url: String) -> Self {
        Self {
            url,
            state: LoadState::Pending,
            scroll_y: 0,
            lines: Vec::new(),
            title: "Loading".to_string(),
        }
    }

    fn ensure_loaded(&mut self) {
        if self.state != LoadState::Pending {
            return;
        }

        match fetch_url(&self.url) {
            Ok(html) => {
                let document = parse_html_text(&html);
                self.title = document.title;
                self.lines = document.lines;
                self.state = LoadState::Loaded;
            }
            Err(error) => {
                self.title = "Load failed".to_string();
                self.lines = vec![
                    format!("Could not load {}", self.url),
                    error,
                    "Lector currently uses curl as a temporary fetcher before Servo is integrated."
                        .to_string(),
                ];
                self.state = LoadState::Failed;
            }
        }
    }
}

impl BrowserEngine for HtmlEngine {
    fn handle_event(&mut self, event: EngineEvent) {
        if let EngineEvent::Scroll { dx, dy } = event {
            self.scroll_y = (self.scroll_y + dy + dx / 4).max(0);
        }
    }

    fn render(&mut self, width: u32, height: u32) -> Frame {
        self.ensure_loaded();

        let mut frame = Frame::new(width, height, Rgba::rgb(247, 247, 244));
        paint_browser_chrome(&mut frame, &self.url, &self.title);
        paint_document(&mut frame, &self.lines, self.scroll_y);
        frame
    }
}

fn fetch_url(url: &str) -> Result<String, String> {
    let output = Command::new("curl")
        .args([
            "-L",
            "--compressed",
            "--max-time",
            "12",
            "-A",
            "Mozilla/5.0 Lector/0.1",
            url,
        ])
        .output()
        .map_err(|err| format!("failed to start curl: {err}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("curl exited with {}: {}", output.status, stderr));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

struct HtmlText {
    title: String,
    lines: Vec<String>,
}

fn parse_html_text(html: &str) -> HtmlText {
    let title = extract_title(html).unwrap_or_else(|| "Untitled page".to_string());
    let body = remove_tag_contents(html, "script");
    let body = remove_tag_contents(&body, "style");
    let body = remove_tag_contents(&body, "noscript");
    let with_breaks = body
        .replace("<br", "\n<br")
        .replace("<p", "\n<p")
        .replace("<div", "\n<div")
        .replace("<li", "\n* <li")
        .replace("<h1", "\n# <h1")
        .replace("<h2", "\n## <h2")
        .replace("<h3", "\n### <h3");
    let text = strip_tags(&with_breaks);
    let text = decode_entities(&text);
    let mut lines = Vec::new();

    for raw in text.lines() {
        let line = collapse_whitespace(raw);
        if line.len() >= 2 {
            wrap_line(&line, 132, &mut lines);
        }
    }

    if lines.is_empty() {
        lines.push("No readable text was found in the fetched HTML.".to_string());
    }

    HtmlText { title, lines }
}

fn extract_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let after_open = lower[start..].find('>')? + start + 1;
    let end = lower[after_open..].find("</title>")? + after_open;
    Some(decode_entities(&collapse_whitespace(
        &html[after_open..end],
    )))
}

fn remove_tag_contents(html: &str, tag: &str) -> String {
    let mut rest = html;
    let mut output = String::with_capacity(html.len());
    let open = format!("<{tag}");
    let close = format!("</{tag}>");

    loop {
        let lower = rest.to_ascii_lowercase();
        let Some(start) = lower.find(&open) else {
            output.push_str(rest);
            break;
        };
        output.push_str(&rest[..start]);
        let Some(end) = lower[start..].find(&close) else {
            break;
        };
        rest = &rest[start + end + close.len()..];
    }

    output
}

fn strip_tags(html: &str) -> String {
    let mut output = String::with_capacity(html.len());
    let mut in_tag = false;

    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                output.push(' ');
            }
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }

    output
}

fn decode_entities(text: &str) -> String {
    text.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn wrap_line(line: &str, width: usize, lines: &mut Vec<String>) {
    let mut current = String::new();
    for word in line.split_whitespace() {
        if !current.is_empty() && current.len() + word.len() + 1 > width {
            lines.push(current);
            current = String::new();
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
}

fn paint_browser_chrome(frame: &mut Frame, url: &str, title: &str) {
    let width = frame.width();
    frame.fill_rect(0, 0, width, 64, Rgba::rgb(30, 34, 38));
    frame.fill_rect(20, 16, width.saturating_sub(40), 28, Rgba::rgb(55, 61, 68));
    frame.draw_text(32, 26, url, Rgba::rgb(236, 238, 240));
    frame.draw_text(32, 52, title, Rgba::rgb(174, 184, 194));
}

fn paint_document(frame: &mut Frame, lines: &[String], scroll_y: i32) {
    let width = frame.width();
    let height = frame.height();
    let content_x = 32;
    let mut y = 92 - scroll_y;

    frame.fill_rect(
        0,
        64,
        width,
        height.saturating_sub(64),
        Rgba::rgb(247, 247, 244),
    );
    for line in lines {
        if y > -12 && y < height as i32 - 20 {
            frame.draw_text(content_x, y, line, Rgba::rgb(38, 43, 48));
        }
        y += 18;
    }
}

#[cfg(test)]
mod tests {
    use super::parse_html_text;

    #[test]
    fn extracts_title_and_body_text() {
        let doc = parse_html_text(
            "<html><head><title>Hello &amp; Test</title><style>x</style></head><body><h1>Hi</h1><p>World</p><script>bad()</script></body></html>",
        );

        assert_eq!(doc.title, "Hello & Test");
        assert!(doc.lines.iter().any(|line| line.contains("Hi")));
        assert!(doc.lines.iter().any(|line| line.contains("World")));
        assert!(!doc.lines.iter().any(|line| line.contains("bad")));
    }
}
