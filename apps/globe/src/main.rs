//! Silver Skies Pro's 3D map: a window with MapLibre GL in the system web
//! view (WebView2, WKWebView or WebKitGTK), started by Pro.
//!
//! Pro writes one JSON message per line to stdin:
//! - `{"type":"scene","lat":..,"lon":..,"name":".."}` flies there;
//! - `{"type":"radar",...}` replaces the radar volume (served to the page as
//!   `radar.json`, then announced);
//! - `{"type":"title","text":".."}`.
//!
//! The page's log lines come back on stdout as `log <text>`. The window
//! closes when stdin does (Pro quit).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::borrow::Cow;
use std::io::BufRead;
use std::sync::{Arc, Mutex};

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::http::{header, Request, Response, StatusCode};
use wry::WebViewBuilder;

const PAGE: &str = include_str!("globe.html");

enum Msg {
    /// A line for the page.
    Script(String),
    Title(String),
    /// The page has loaded: send what was waiting.
    PageReady,
    Quit,
}

/// The page's address: custom protocols are `http://<scheme>.<host>` on
/// Windows.
fn start_url() -> &'static str {
    if cfg!(windows) {
        "http://ssglobe.localhost/"
    } else {
        "ssglobe://localhost/"
    }
}

fn respond(status: StatusCode, mime: &str, body: Vec<u8>) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, "no-store")
        .body(Cow::<'static, [u8]>::Owned(body))
        .unwrap_or_else(|_| Response::new(Cow::Borrowed(&[][..])))
}

fn main() {
    let event_loop = EventLoopBuilder::<Msg>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let ipc_proxy = event_loop.create_proxy();
    let radar: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(b"null".to_vec()));

    // Pro's messages.
    {
        let radar = radar.clone();
        std::thread::spawn(move || {
            let stdin = std::io::stdin();
            for line in stdin.lock().lines() {
                let Ok(line) = line else { break };
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
                    continue;
                };
                let msg = match v.get("type").and_then(|t| t.as_str()) {
                    Some("radar") => {
                        if let Ok(mut r) = radar.lock() {
                            *r = line.into_bytes();
                        }
                        Msg::Script("window.ss && window.ss({type:'radar'})".into())
                    }
                    Some("title") => Msg::Title(
                        v.get("text")
                            .and_then(|t| t.as_str())
                            .unwrap_or("3D Globe")
                            .to_string(),
                    ),
                    Some(_) => Msg::Script(format!("window.ss && window.ss({line})")),
                    None => continue,
                };
                if proxy.send_event(msg).is_err() {
                    return;
                }
            }
            let _ = proxy.send_event(Msg::Quit);
        });
    }

    let window = match WindowBuilder::new()
        .with_title("3D Map · Silver Skies Pro")
        .with_inner_size(LogicalSize::new(1280.0, 820.0))
        .build(&event_loop)
    {
        Ok(w) => w,
        Err(e) => {
            println!("error window: {e}");
            return;
        }
    };

    let builder = WebViewBuilder::new()
        .with_custom_protocol(
            "ssglobe".into(),
            move |_id, req: Request<Vec<u8>>| match req.uri().path() {
                "/" | "/index.html" => {
                    respond(StatusCode::OK, "text/html", PAGE.as_bytes().to_vec())
                }
                "/radar.json" => {
                    let body = radar.lock().map(|r| r.clone()).unwrap_or_default();
                    respond(StatusCode::OK, "application/json", body)
                }
                _ => respond(StatusCode::NOT_FOUND, "text/plain", b"not found".to_vec()),
            },
        )
        .with_ipc_handler(move |req: Request<String>| {
            let body = req.body();
            if body == "ready" {
                let _ = ipc_proxy.send_event(Msg::PageReady);
            }
            // One line each, for Pro's log.
            println!("log {}", body.replace('\n', " "));
        })
        .with_devtools(cfg!(debug_assertions))
        .with_url(start_url());

    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
    ))]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder.build_gtk(window.default_vbox().expect("tao window has a vbox"))
    };
    #[cfg(not(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
    )))]
    let webview = builder.build(&window);

    let webview = match webview {
        Ok(w) => w,
        Err(e) => {
            println!("error web view: {e}");
            return;
        }
    };
    println!("ready");

    // Messages that arrive before the page can take them.
    let mut waiting: Option<Vec<String>> = Some(Vec::new());
    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(Msg::Script(js)) => match &mut waiting {
                Some(w) => w.push(js),
                None => {
                    let _ = webview.evaluate_script(&js);
                }
            },
            Event::UserEvent(Msg::PageReady) => {
                for js in waiting.take().unwrap_or_default() {
                    let _ = webview.evaluate_script(&js);
                }
            }
            Event::UserEvent(Msg::Title(t)) => window.set_title(&t),
            Event::UserEvent(Msg::Quit)
            | Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *flow = ControlFlow::Exit,
            _ => {}
        }
    });
}
