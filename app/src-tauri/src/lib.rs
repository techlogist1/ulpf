// The ULPF desktop shell. The engine, its server and its UI are the `ulpf` sidecar,
// unchanged; this crate starts it against an app-owned data directory, shows the page it
// serves in the window and stops it on quit. Nothing here parses a log.

mod ingest;
mod intensity;
mod menu;
mod title;

use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, DragDropEvent, Manager, RunEvent, WindowEvent};
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;

const START_TIMEOUT: Duration = Duration::from_secs(120);

/// The bundled splash page. Tauri serves `frontendDist` from its own scheme on macOS and
/// Linux and from a localhost host on Windows (WebView2 has no custom-scheme origin).
#[cfg(windows)]
const SPLASH: &str = "http://tauri.localhost/index.html";
#[cfg(not(windows))]
const SPLASH: &str = "tauri://localhost/index.html";

/// The one shared object: the data directory the engine runs against, the child, the
/// server URL once it answered, and why the child died if it did.
pub(crate) struct Engine {
    pub(crate) data: Mutex<PathBuf>,
    child: Mutex<Option<CommandChild>>,
    /// Bumped by every start and stop, so a poll or a Terminated event from an earlier
    /// child cannot touch the current one.
    generation: AtomicU64,
    pub(crate) url: Mutex<Option<String>>,
    down: Mutex<Option<String>>,
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let data = configured_data_dir(&handle);
            app.manage(Engine {
                data: Mutex::new(data.clone()),
                child: Mutex::new(None),
                generation: AtomicU64::new(0),
                url: Mutex::new(None),
                down: Mutex::new(None),
            });
            menu::install(&handle)?;
            let h = handle.clone();
            thread::spawn(move || start(&h, data, "Starting"));
            thread::spawn(move || title::title_loop(&handle));
            Ok(())
        })
        // Window menu and tray menu clicks both land here.
        .on_menu_event(|app, event| menu::action(app, event.id().as_ref()))
        .on_window_event(|window, event| match event {
            // A drop anywhere on the window; Tauri owns the drop when `dragDropEnabled`
            // is set, on macOS and on Windows alike, so the served page never sees it.
            WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) => ingest::ingest_paths(window.app_handle(), paths),
            // Closing the window hides it; the engine keeps ingesting and the tray brings
            // the window back. Quit (menu, tray, Cmd+Q) is what stops the engine.
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
            }
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("building the ULPF shell")
        .run(|app, event| match event {
            // Both fire on the way out (Cmd+Q, app.exit). Killing the engine outright
            // (std's Child::kill: SIGKILL on macOS, TerminateProcess on Windows) is safe:
            // the raw store is append-only, its SQLite lock dies with the process, and the
            // next start completes the interrupted output from the store before it ingests
            // anything new (D59, kill recovery).
            RunEvent::ExitRequested { .. } | RunEvent::Exit => stop(app),
            // macOS: a click on the dock icon while the window is hidden.
            #[cfg(target_os = "macos")]
            RunEvent::Reopen { .. } => menu::show(app),
            _ => {}
        });
}

// ---- data directory -------------------------------------------------------------------

/// The override the user chose, kept as one line in the app's config directory:
/// macOS `~/Library/Application Support/dev.ulpf.desktop/data_dir`, Windows
/// `%APPDATA%\dev.ulpf.desktop\data_dir` (both from Tauri's path resolver).
pub(crate) fn config_file(app: &AppHandle) -> PathBuf {
    app.path().app_config_dir().expect("app config dir").join("data_dir")
}

/// The chosen directory, else Tauri's app data directory: macOS
/// `~/Library/Application Support/dev.ulpf.desktop`, Windows `%APPDATA%\dev.ulpf.desktop`.
fn configured_data_dir(app: &AppHandle) -> PathBuf {
    fs::read_to_string(config_file(app))
        .ok()
        .map(|s| PathBuf::from(s.trim()))
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| app.path().app_data_dir().expect("app data dir"))
}

/// The app-owned layout: watch/, pending/, parsers/ and mappings/ seeded from the bundled
/// `parsers/*.toml` and `mappings/*.toml` when they hold no definition yet. The engine
/// creates store/ itself. Resources live in `Contents/Resources` on macOS and beside the
/// executable on Windows; `resource_dir` knows which.
fn prepare(app: &AppHandle, data: &Path) -> std::io::Result<()> {
    for d in ["watch", "pending", "parsers", "mappings"] {
        fs::create_dir_all(data.join(d))?;
    }
    let resources = app.path().resource_dir().map_err(|e| std::io::Error::other(e.to_string()))?;
    for d in ["parsers", "mappings"] {
        let dst = data.join(d);
        if fs::read_dir(&dst)?.flatten().any(|e| is_toml(&e.path())) {
            continue;
        }
        for entry in fs::read_dir(resources.join(d))?.flatten() {
            if is_toml(&entry.path()) {
                fs::copy(entry.path(), dst.join(entry.file_name()))?;
            }
        }
    }
    Ok(())
}

fn is_toml(p: &Path) -> bool {
    p.extension().is_some_and(|x| x == "toml")
}

// ---- engine lifecycle -----------------------------------------------------------------

/// Starts the sidecar against `data` on a free localhost port, waits for `/api/status`,
/// records the URL in `<data>/server.url` and points the window at it. Runs on its own
/// thread; every failure lands on the splash page and in the title. `verb` is what the
/// window says it is doing: "Starting" at launch, "Restarting" after a setting changed.
pub(crate) fn start(app: &AppHandle, data: PathBuf, verb: &'static str) {
    let engine = app.state::<Engine>();
    let generation = engine.generation.fetch_add(1, Relaxed) + 1;
    *engine.data.lock().unwrap() = data.clone();
    *engine.url.lock().unwrap() = None;
    *engine.down.lock().unwrap() = None;
    let _ = fs::remove_file(data.join("server.url"));
    let (chosen, cores) = (intensity::load(app), intensity::cores());
    set_title(app, "ULPF · starting engine…");
    splash(app, &format!("{verb} the engine at {}: {} of {cores} cores, entity index {}", chosen.name(), chosen.threads(cores), intensity::on_off(chosen.pivot())), false);

    if let Err(e) = prepare(app, &data) {
        return fail(app, generation, "start failed", &format!("Cannot prepare {}: {e}", data.display()));
    }
    // Bind port 0, read the kernel's pick, release it, hand it to the engine. The engine
    // reports the address it bound in its own stderr line.
    let port = match TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()) {
        Ok(a) => a.port(),
        Err(e) => return fail(app, generation, "start failed", &format!("No free localhost port: {e}")),
    };
    let url = format!("http://127.0.0.1:{port}");
    let arg = |p: &str| data.join(p).to_string_lossy().into_owned();
    // Every path is absolute: the engine's --parsers and --mappings default to paths
    // relative to its working directory, which an app has no useful one of.
    let mut args = vec![
        "serve".to_string(),
        arg("watch"),
        "--store".into(),
        arg("store"),
        "--output".into(),
        arg("out.jsonl"),
        "--pending".into(),
        arg("pending"),
        "--parsers".into(),
        arg("parsers"),
        "--mappings".into(),
        arg("mappings"),
        "--listen".into(),
        format!("127.0.0.1:{port}"),
    ];
    // The intensity setting, applied where the engine takes it: both the worker count and
    // the entity index are fixed when the process starts, which is why changing the setting
    // comes back through here rather than reaching into a running engine.
    args.extend(chosen.args(cores));
    // The bundler copies `binaries/ulpf-<triple>[.exe]` beside the app executable with the
    // triple stripped; `sidecar("ulpf")` resolves that name on both platforms and, on
    // Windows, spawns without a console window.
    let spawned = app
        .shell()
        .sidecar("ulpf")
        .map_err(|e| e.to_string())
        .and_then(|c| c.args(&args).spawn().map_err(|e| e.to_string()));
    let (mut rx, child) = match spawned {
        Ok(x) => x,
        Err(e) => return fail(app, generation, "start failed", &format!("Cannot start the engine: {e}")),
    };
    *engine.child.lock().unwrap() = Some(child);

    let h = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                CommandEvent::Stderr(line) | CommandEvent::Stdout(line) => eprintln!("[ulpf] {}", String::from_utf8_lossy(&line).trim_end()),
                CommandEvent::Terminated(t) => {
                    let why = match (t.code, t.signal) {
                        (Some(c), _) => format!("exit {c}"),
                        (None, Some(s)) => format!("signal {s}"),
                        _ => "exit unknown".to_string(),
                    };
                    fail(&h, generation, &why, &format!("The engine stopped ({why}). Restart the app, or check {} for what it left behind.", h.state::<Engine>().data.lock().unwrap().display()));
                }
                _ => {}
            }
        }
    });

    let started = Instant::now();
    while started.elapsed() < START_TIMEOUT {
        if engine.generation.load(Relaxed) != generation || engine.down.lock().unwrap().is_some() {
            return;
        }
        if http_get(&url, "/api/status").is_some() {
            let _ = fs::write(data.join("server.url"), &url);
            *engine.url.lock().unwrap() = Some(url.clone());
            navigate(app, &url);
            intensity::ready_notice(app, chosen, cores);
            return;
        }
        thread::sleep(Duration::from_millis(200));
    }
    fail(app, generation, "no answer", "The engine did not answer within two minutes.");
}

/// Records why the engine is not serving, if this is still the current engine, and shows
/// the reason on the splash page.
fn fail(app: &AppHandle, generation: u64, why: &str, message: &str) {
    let engine = app.state::<Engine>();
    if engine.generation.load(Relaxed) != generation {
        return;
    }
    *engine.down.lock().unwrap() = Some(why.to_string());
    *engine.url.lock().unwrap() = None;
    let _ = fs::remove_file(engine.data.lock().unwrap().join("server.url"));
    set_title(app, &format!("ULPF · engine down ({why})"));
    splash(app, message, true);
}

pub(crate) fn stop(app: &AppHandle) {
    let Some(engine) = app.try_state::<Engine>() else { return };
    engine.generation.fetch_add(1, Relaxed);
    if let Some(child) = engine.child.lock().unwrap().take() {
        let _ = child.kill();
    }
    let _ = fs::remove_file(engine.data.lock().unwrap().join("server.url"));
    *engine.url.lock().unwrap() = None;
}

// ---- window ---------------------------------------------------------------------------

pub(crate) fn set_title(app: &AppHandle, title: &str) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_title(title);
    }
}

fn navigate(app: &AppHandle, url: &str) {
    if let (Some(w), Ok(url)) = (app.get_webview_window("main"), url.parse::<tauri::Url>()) {
        let _ = w.navigate(url);
    }
}

/// Shows the splash page with `text` as its status line (`!` marks an error). The text
/// travels in the URL fragment, so it is there when the page loads and a fragment change
/// on a page already showing updates it in place without a reload.
fn splash(app: &AppHandle, text: &str, error: bool) {
    navigate(app, &format!("{SPLASH}#{}{}", if error { "!" } else { "" }, percent_encode(text)));
}

/// A short-lived notice at the bottom of whatever page the window shows (the splash or the
/// served UI): one element the shell injects, replaced by the next notice, gone after six
/// seconds. The served UI is not restyled and does not know the shell exists.
pub(crate) fn toast(app: &AppHandle, text: &str) {
    let Some(w) = app.get_webview_window("main") else { return };
    let Ok(text) = serde_json::to_string(text) else { return };
    let js = format!(
        "(function(){{var t=document.getElementById('ulpf-shell-toast');\
         if(!t){{t=document.createElement('div');t.id='ulpf-shell-toast';\
         t.style.cssText='position:fixed;left:50%;bottom:28px;transform:translateX(-50%);max-width:72vw;\
         padding:10px 16px;border-radius:8px;background:#20242b;color:#e8ebef;\
         font:13px/1.45 -apple-system,\"Segoe UI\",system-ui,sans-serif;box-shadow:0 6px 24px rgba(0,0,0,.45);\
         border:1px solid #333a44;z-index:2147483647;pointer-events:none;transition:opacity .3s;white-space:pre-wrap';\
         document.body.appendChild(t);}}\
         t.textContent={text};t.style.opacity='1';clearTimeout(t._h);\
         t._h=setTimeout(function(){{t.style.opacity='0';}},6000);}})();"
    );
    let _ = w.eval(&js);
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ---- helpers --------------------------------------------------------------------------

/// One GET over loopback, body on 200. The server answers JSON with a Content-Length
/// and honours Connection: close, which is all this needs.
// ponytail: hand-rolled HTTP/1.1; switch to ureq if the server ever chunks a response.
pub(crate) fn http_get(base: &str, path: &str) -> Option<String> {
    let addr: SocketAddr = base.trim_start_matches("http://").parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(500)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    write!(s, "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n").ok()?;
    let mut buf = String::new();
    s.read_to_string(&mut buf).ok()?;
    let (head, body) = buf.split_once("\r\n\r\n")?;
    head.starts_with("HTTP/1.1 200").then(|| body.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn percent_encoding_keeps_unreserved_and_escapes_the_rest() {
        assert_eq!(super::percent_encode("Starting the engine"), "Starting%20the%20engine");
        assert_eq!(super::percent_encode("a-b.c_d~e"), "a-b.c_d~e");
        assert_eq!(super::percent_encode("é#"), "%C3%A9%23");
    }
}
