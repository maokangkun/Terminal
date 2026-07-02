//! Native Servo adapter experiment for Lector.
//!
//! This crate intentionally lives outside the main `lector` binary dependency
//! graph until the adapter is stable. It exercises Servo's public in-process API
//! with a `SoftwareRenderingContext`, then exposes a simple RGBA frame result
//! that the main Sixel pipeline can eventually consume.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use dpi::PhysicalSize;
use euclid::Scale;
use servo::{
    CompositionEvent, CompositionState, CreateNewWebViewRequest, EventLoopWaker, ImeEvent,
    InputEvent, Key, KeyState, KeyboardEvent, LoadStatus, MouseButton, MouseButtonAction,
    MouseButtonEvent, MouseMoveEvent, NamedKey, Preferences, RenderingContext, Servo,
    ServoBuilder, SoftwareRenderingContext, WebView, WebViewBuilder, WheelDelta, WheelEvent,
    WheelMode,
    UserAgentPlatform,
};
use url::Url;
use webrender_api::units::{DeviceIntPoint, DeviceIntRect, DeviceIntSize, DevicePoint};

#[derive(Debug, Clone)]
pub struct NativeServoFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeTabInfo {
    pub title: String,
    pub url: String,
    pub active: bool,
}

#[derive(Debug, Clone)]
pub struct NativeServoConfig {
    pub url: String,
    pub width: u32,
    pub height: u32,
    pub settle_timeout: Duration,
}

impl Default for NativeServoConfig {
    fn default() -> Self {
        Self {
            url: "about:blank".to_string(),
            width: 800,
            height: 600,
            settle_timeout: Duration::from_secs(5),
        }
    }
}

pub struct NativeServoAdapter {
    servo: Servo,
    tabs: Rc<RefCell<NativeTabs>>,
    rendering_context: Rc<SoftwareRenderingContext>,
    new_frame_ready: Rc<Cell<bool>>,
    load_complete: Rc<Cell<bool>>,
    force_active_screenshot: Rc<Cell<bool>>,
    post_load_repaint_requested: Cell<bool>,
}

struct NativeTabs {
    tabs: Vec<NativeTab>,
    active: usize,
}

struct NativeTab {
    webview: WebView,
    title: String,
    url: String,
}

impl NativeServoAdapter {
    pub fn new(config: NativeServoConfig) -> Result<Self, String> {
        install_crypto_provider();

        let size = PhysicalSize::new(config.width.max(1), config.height.max(1));
        let rendering_context =
            Rc::new(SoftwareRenderingContext::new(size).map_err(|error| {
                format!("failed to create SoftwareRenderingContext: {error:?}")
            })?);
        rendering_context
            .make_current()
            .map_err(|error| format!("failed to make rendering context current: {error:?}"))?;

        let new_frame_ready = Rc::new(Cell::new(false));
        let load_complete = Rc::new(Cell::new(false));
        let force_active_screenshot = Rc::new(Cell::new(false));
        let mut preferences = Preferences::default();
        preferences.network_http_proxy_uri = String::new();
        preferences.network_https_proxy_uri = String::new();
        preferences.user_agent = UserAgentPlatform::Desktop.to_user_agent_string();

        let servo = ServoBuilder::default()
            .preferences(preferences)
            .event_loop_waker(Box::new(NoopWaker))
            .build();
        servo.setup_logging();

        let tabs = Rc::new(RefCell::new(NativeTabs {
            tabs: Vec::new(),
            active: 0,
        }));
        let delegate = Rc::new(TerminalWebViewDelegate {
            tabs: tabs.clone(),
            rendering_context: rendering_context.clone(),
            new_frame_ready: new_frame_ready.clone(),
            load_complete: load_complete.clone(),
            force_active_screenshot: force_active_screenshot.clone(),
        });
        let url = Url::parse(&config.url).map_err(|error| format!("invalid URL: {error}"))?;
        let webview = WebViewBuilder::new(&servo, rendering_context.clone())
            .url(url)
            .hidpi_scale_factor(Scale::new(1.0))
            .delegate(delegate)
            .build();
        webview.show();
        webview.focus();
        webview.resize(size);
        tabs.borrow_mut().tabs.push(NativeTab {
            webview,
            title: config.url.clone(),
            url: config.url.clone(),
        });

        Ok(Self {
            servo,
            tabs,
            rendering_context,
            new_frame_ready,
            load_complete,
            force_active_screenshot,
            post_load_repaint_requested: Cell::new(false),
        })
    }

    pub fn spin_until_frame(&self, timeout: Duration) {
        self.pump(timeout);
    }

    pub fn pump(&self, timeout: Duration) -> bool {
        let start = Instant::now();
        let mut painted = false;
        loop {
            self.servo.spin_event_loop();
            self.request_post_load_repaint();
            if self.new_frame_ready.replace(false) {
                self.paint_and_present();
                painted = true;
            }
            if painted || start.elapsed() >= timeout {
                break;
            }
            std::thread::sleep(Duration::from_millis(8));
        }
        if !painted && timeout > Duration::ZERO {
            self.paint_and_present();
        }
        painted
    }

    pub fn capture(&self) -> Result<NativeServoFrame, String> {
        if self.force_active_screenshot.replace(false) {
            self.request_active_repaint();
            self.wait_for_active_frame(Duration::from_millis(250));
            return self
                .capture_screenshot(Duration::from_millis(500))
                .or_else(|_| self.capture_framebuffer());
        }

        self.capture_framebuffer()
            .or_else(|_| self.capture_screenshot(Duration::from_millis(250)))
    }

    pub fn tabs(&self) -> Vec<NativeTabInfo> {
        let mut tabs = self.tabs.borrow_mut();
        for tab in &mut tabs.tabs {
            refresh_tab_metadata(tab);
        }
        tabs.tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| NativeTabInfo {
                title: tab.title.clone(),
                url: tab.url.clone(),
                active: index == tabs.active,
            })
            .collect()
    }

    pub fn switch_tab(&self, index: usize) {
        let mut tabs = self.tabs.borrow_mut();
        if index >= tabs.tabs.len() || index == tabs.active {
            return;
        }
        activate_tab(&mut tabs, index, self.rendering_context.size());
        drop(tabs);
        self.post_load_repaint_requested.set(false);
        self.force_active_screenshot.set(true);
        self.new_frame_ready.set(false);
        self.request_active_repaint();
    }

    pub fn new_tab(&self, url: &str) {
        match normalize_url(url) {
            Ok(url) => self.create_tab(Some(url)),
            Err(_) => self.create_tab(None),
        }
    }

    pub fn close_active_tab(&self) {
        let should_create_blank = {
            let mut tabs = self.tabs.borrow_mut();
            if tabs.tabs.is_empty() {
                true
            } else if tabs.tabs.len() == 1 {
                tabs.active = 0;
                let tab = &mut tabs.tabs[0];
                tab.title = "New tab".to_string();
                tab.url.clear();
                if let Ok(url) = Url::parse("about:blank") {
                    tab.webview.load(url);
                }
                tab.webview.show();
                tab.webview.focus();
                tab.webview.resize(self.rendering_context.size());
                self.post_load_repaint_requested.set(false);
                self.force_active_screenshot.set(true);
                self.new_frame_ready.set(false);
                false
            } else {
                let active = tabs.active.min(tabs.tabs.len() - 1);
                tabs.tabs[active].webview.hide();
                tabs.tabs.remove(active);
                if tabs.tabs.is_empty() {
                    true
                } else {
                    let next = active.min(tabs.tabs.len() - 1);
                    activate_tab(&mut tabs, next, self.rendering_context.size());
                    drop(tabs);
                    self.post_load_repaint_requested.set(false);
                    self.force_active_screenshot.set(true);
                    self.new_frame_ready.set(false);
                    self.request_active_repaint();
                    false
                }
            }
        };
        if should_create_blank {
            self.create_tab(None);
        }
        self.load_complete.set(false);
        self.post_load_repaint_requested.set(false);
    }

    pub fn load_url(&self, url: &str) -> Result<(), String> {
        let url = normalize_url(url)?;
        {
            let mut tabs = self.tabs.borrow_mut();
            let active = tabs.active;
            tabs.tabs[active].url = url.to_string();
            tabs.tabs[active].title = url.to_string();
            tabs.tabs[active].webview.load(url);
        }
        self.load_complete.set(false);
        self.post_load_repaint_requested.set(false);
        self.force_active_screenshot.set(true);
        Ok(())
    }

    fn create_tab(&self, url: Option<Url>) {
        let size = self.rendering_context.size();
        let delegate = self.webview().delegate();
        let mut builder = WebViewBuilder::new(&self.servo, self.rendering_context.clone())
            .hidpi_scale_factor(Scale::new(1.0))
            .delegate(delegate);
        let display_url = url.as_ref().and_then(display_url).unwrap_or_default();
        let title = if display_url.is_empty() {
            "New tab".to_string()
        } else {
            display_url.clone()
        };
        if let Some(url) = url {
            builder = builder.url(url);
        }
        let webview = builder.build();
        let mut tabs = self.tabs.borrow_mut();
        tabs.tabs.push(NativeTab {
            webview,
            title,
            url: display_url,
        });
        let active = tabs.tabs.len() - 1;
        activate_tab(&mut tabs, active, size);
        self.load_complete.set(false);
        self.post_load_repaint_requested.set(false);
        self.force_active_screenshot.set(true);
    }

    fn capture_screenshot(&self, timeout: Duration) -> Result<NativeServoFrame, String> {
        let result = Rc::new(RefCell::new(None));
        let callback_result = result.clone();
        self.webview().take_screenshot(None, move |screenshot| {
            *callback_result.borrow_mut() = Some(screenshot);
        });

        let start = Instant::now();
        while start.elapsed() < timeout {
            self.servo.spin_event_loop();
            self.request_post_load_repaint();
            if self.new_frame_ready.replace(false) {
                self.paint_and_present();
            }
            if let Some(result) = result.borrow_mut().take() {
                let image =
                    result.map_err(|error| format!("Servo screenshot failed: {error:?}"))?;
                return Ok(NativeServoFrame {
                    width: image.width(),
                    height: image.height(),
                    rgba: image.into_raw(),
                });
            }
            std::thread::sleep(Duration::from_millis(8));
        }

        Err("Servo screenshot timed out".to_string())
    }

    fn capture_framebuffer(&self) -> Result<NativeServoFrame, String> {
        self.rendering_context
            .make_current()
            .map_err(|error| format!("failed to make rendering context current: {error:?}"))?;
        let size = self.rendering_context.size();
        let rect = DeviceIntRect::from_origin_and_size(
            DeviceIntPoint::new(0, 0),
            DeviceIntSize::new(size.width as i32, size.height as i32),
        );
        let image = self
            .rendering_context
            .read_to_image(rect)
            .ok_or_else(|| "Servo rendering context did not return an image".to_string())?;

        Ok(NativeServoFrame {
            width: image.width(),
            height: image.height(),
            rgba: image.into_raw(),
        })
    }

    fn paint_and_present(&self) {
        if let Err(error) = self.rendering_context.make_current() {
            eprintln!("failed to make Servo rendering context current: {error:?}");
            return;
        }
        self.webview().paint();
        self.rendering_context.present();
    }

    pub fn resize(&self, width: u32, height: u32) {
        let size = PhysicalSize::new(width.max(1), height.max(1));
        self.rendering_context.resize(size);
        self.webview().resize(size);
        self.paint_and_present();
    }

    pub fn mouse_move(&self, x: u32, y: u32) {
        self.webview()
            .notify_input_event(InputEvent::MouseMove(MouseMoveEvent::new(device_point(
                x, y,
            ))));
        self.spin_after_input();
    }

    pub fn mouse_button(&self, action: NativeMouseButtonAction, button: NativeMouseButton, x: u32, y: u32) {
        if action == NativeMouseButtonAction::Down {
            self.webview().focus();
        }
        self.webview()
            .notify_input_event(InputEvent::MouseButton(MouseButtonEvent::new(
                match action {
                    NativeMouseButtonAction::Down => MouseButtonAction::Down,
                    NativeMouseButtonAction::Up => MouseButtonAction::Up,
                },
                match button {
                    NativeMouseButton::Left => MouseButton::Left,
                    NativeMouseButton::Middle => MouseButton::Middle,
                    NativeMouseButton::Right => MouseButton::Right,
                    NativeMouseButton::Other => MouseButton::Other(0),
                },
                device_point(x, y),
            )));
        self.spin_after_input();
    }

    pub fn wheel(&self, dx: i32, dy: i32, x: u32, y: u32) {
        self.webview()
            .notify_input_event(InputEvent::Wheel(WheelEvent::new(
                WheelDelta {
                    x: dx as f64,
                    y: dy as f64,
                    z: 0.0,
                    mode: WheelMode::DeltaPixel,
                },
                device_point(x, y),
            )));
        self.spin_after_input();
    }

    pub fn insert_text(&self, text: &str) {
        if text.is_empty() {
            return;
        }

        self.webview()
            .notify_input_event(InputEvent::Keyboard(KeyboardEvent::from_state_and_key(
                KeyState::Down,
                Key::Named(NamedKey::Process),
            )));
        self.webview()
            .notify_input_event(InputEvent::Ime(ImeEvent::Composition(CompositionEvent {
                state: CompositionState::End,
                data: text.to_string(),
            })));
        self.webview()
            .notify_input_event(InputEvent::Keyboard(KeyboardEvent::from_state_and_key(
                KeyState::Up,
                Key::Named(NamedKey::Process),
            )));
        self.spin_after_input();
    }

    pub fn key_press(&self, key: NativeKey) {
        let key = match key {
            NativeKey::Up => Key::Named(NamedKey::ArrowUp),
            NativeKey::Down => Key::Named(NamedKey::ArrowDown),
            NativeKey::Left => Key::Named(NamedKey::ArrowLeft),
            NativeKey::Right => Key::Named(NamedKey::ArrowRight),
            NativeKey::PageUp => Key::Named(NamedKey::PageUp),
            NativeKey::PageDown => Key::Named(NamedKey::PageDown),
            NativeKey::Enter => Key::Named(NamedKey::Enter),
            NativeKey::Backspace => Key::Named(NamedKey::Backspace),
        };
        self.webview()
            .notify_input_event(InputEvent::Keyboard(KeyboardEvent::from_state_and_key(
                KeyState::Down,
                key.clone(),
            )));
        self.webview()
            .notify_input_event(InputEvent::Keyboard(KeyboardEvent::from_state_and_key(
                KeyState::Up,
                key,
            )));
        self.spin_after_input();
    }

    fn spin_after_input(&self) {
        self.servo.spin_event_loop();
        self.request_post_load_repaint();
    }

    fn wait_for_active_frame(&self, timeout: Duration) -> bool {
        let start = Instant::now();
        loop {
            self.servo.spin_event_loop();
            if self.new_frame_ready.replace(false) {
                self.paint_and_present();
                return true;
            }
            if start.elapsed() >= timeout {
                self.paint_and_present();
                return false;
            }
            std::thread::sleep(Duration::from_millis(8));
        }
    }

    fn request_active_repaint(&self) {
        self.webview().evaluate_javascript(
            "requestAnimationFrame(() => { \
                const root = document.documentElement; \
                const value = String(Date.now()); \
                root.style.setProperty('--lector-active-tab-repaint-token', value); \
            });",
            |_| {},
        );
    }

    fn request_post_load_repaint(&self) {
        if !self.load_complete.get() || self.post_load_repaint_requested.replace(true) {
            return;
        }

        self.new_frame_ready.set(false);
        self.webview().evaluate_javascript(
            "requestAnimationFrame(() => { \
                const root = document.documentElement; \
                const value = String(Date.now()); \
                root.style.setProperty('--lector-repaint-token', value); \
                root.style.outline = '1px solid transparent'; \
            });",
            |_| {},
        );
    }

    fn webview(&self) -> WebView {
        let tabs = self.tabs.borrow();
        tabs.tabs
            .get(tabs.active)
            .expect("native Servo adapter has no active WebView")
            .webview
            .clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeMouseButton {
    Left,
    Middle,
    Right,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeMouseButtonAction {
    Down,
    Up,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeKey {
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Enter,
    Backspace,
}

fn device_point(x: u32, y: u32) -> servo::WebViewPoint {
    DevicePoint::new(x as f32, y as f32).into()
}

fn normalize_url(input: &str) -> Result<Url, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("URL is empty".to_string());
    }
    if input == "about:blank" {
        return Url::parse(input).map_err(|error| format!("invalid URL: {error}"));
    }
    Url::parse(input)
        .or_else(|_| Url::parse(&format!("https://{input}")))
        .map_err(|error| format!("invalid URL: {error}"))
}

struct TerminalWebViewDelegate {
    tabs: Rc<RefCell<NativeTabs>>,
    rendering_context: Rc<SoftwareRenderingContext>,
    new_frame_ready: Rc<Cell<bool>>,
    load_complete: Rc<Cell<bool>>,
    force_active_screenshot: Rc<Cell<bool>>,
}

impl servo::WebViewDelegate for TerminalWebViewDelegate {
    fn notify_new_frame_ready(&self, webview: WebView) {
        if !self.is_active_webview(&webview) {
            return;
        }
        self.new_frame_ready.set(true);
        if self.rendering_context.make_current().is_ok() {
            webview.paint();
            self.rendering_context.present();
        }
    }

    fn notify_url_changed(&self, webview: WebView, url: Url) {
        if let Some(mut tab) = self.tab_for_webview(&webview) {
            tab.url = display_url(&url).unwrap_or_default();
        }
    }

    fn notify_page_title_changed(&self, webview: WebView, title: Option<String>) {
        if let Some(mut tab) = self.tab_for_webview(&webview) {
            if let Some(title) = title.filter(|title| !title.trim().is_empty()) {
                tab.title = title;
            }
        }
    }

    fn notify_load_status_changed(&self, _: WebView, status: LoadStatus) {
        if status == LoadStatus::Complete {
            self.load_complete.set(true);
        }
    }

    fn request_create_new(&self, parent_webview: WebView, request: CreateNewWebViewRequest) {
        let size = self.rendering_context.size();
        let webview = request
            .builder(self.rendering_context.clone())
            .hidpi_scale_factor(Scale::new(1.0))
            .delegate(parent_webview.delegate())
            .build();
        let title = tab_title(&webview).unwrap_or_else(|| "New tab".to_string());
        let url = webview
            .url()
            .and_then(|url| display_url(&url))
            .unwrap_or_default();
        let mut tabs = self.tabs.borrow_mut();
        tabs.tabs.push(NativeTab {
            webview,
            title,
            url,
        });
        let active = tabs.tabs.len() - 1;
        activate_tab(&mut tabs, active, size);
        self.load_complete.set(false);
        self.force_active_screenshot.set(true);
    }
}

impl TerminalWebViewDelegate {
    fn is_active_webview(&self, webview: &WebView) -> bool {
        let tabs = self.tabs.borrow();
        tabs.tabs
            .get(tabs.active)
            .map(|tab| tab.webview.id() == webview.id())
            .unwrap_or(false)
    }

    fn tab_for_webview(&self, webview: &WebView) -> Option<std::cell::RefMut<'_, NativeTab>> {
        let tabs = self.tabs.borrow_mut();
        let index = tabs.tabs.iter().position(|tab| tab.webview.id() == webview.id())?;
        Some(std::cell::RefMut::map(tabs, |tabs| &mut tabs.tabs[index]))
    }
}

#[derive(Clone)]
struct NoopWaker;

impl EventLoopWaker for NoopWaker {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(self.clone())
    }

    fn wake(&self) {}
}

fn install_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

fn activate_tab(tabs: &mut NativeTabs, index: usize, size: PhysicalSize<u32>) {
    tabs.active = index;
    for (tab_index, tab) in tabs.tabs.iter().enumerate() {
        if tab_index == index {
            tab.webview.show();
            tab.webview.focus();
            tab.webview.resize(size);
        } else {
            tab.webview.hide();
            tab.webview.blur();
        }
    }
}

fn refresh_tab_metadata(tab: &mut NativeTab) {
    if let Some(url) = tab.webview.url().and_then(|url| display_url(&url)) {
        tab.url = url;
    }
    if let Some(title) = tab_title(&tab.webview) {
        tab.title = title;
    }
}

fn tab_title(webview: &WebView) -> Option<String> {
    webview
        .page_title()
        .filter(|title| !title.trim().is_empty())
        .or_else(|| webview.url().and_then(|url| display_url(&url)))
}

fn display_url(url: &Url) -> Option<String> {
    if url.as_str() == "about:blank" {
        None
    } else {
        Some(url.to_string())
    }
}

pub fn render_once(config: NativeServoConfig) -> Result<NativeServoFrame, String> {
    let timeout = config.settle_timeout;
    let adapter = NativeServoAdapter::new(config)?;
    adapter.spin_until_frame(timeout);
    adapter.capture()
}
