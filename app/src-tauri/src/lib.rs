// The ULPF desktop shell. The engine, its server and its UI are the `ulpf` sidecar,
// unchanged; this crate starts it against an app-owned data directory, shows the page it
// serves in the window and stops it on quit. Nothing here parses a log.

mod download;
mod holder;
mod ingest;
mod intensity;
mod job;
mod menu;
mod title;

use std::fs;
use std::io::Write;
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use tauri::webview::WebviewWindowBuilder;
use tauri::{AppHandle, DragDropEvent, Manager, RunEvent, WindowEvent};
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;

const START_TIMEOUT: Duration = Duration::from_secs(120);

/// The engine's own words when a second writer meets a store another process holds
/// (`store <dir> is in use by another process`, crates/ulpf-store/src/store.rs). Matched on
/// the middle of the sentence so neither path in it has to be reconstructed here.
const STORE_IN_USE: &str = "is in use by another process";

/// The engine beside the app: the bundler strips the target triple from
/// `binaries/ulpf-<triple>[.exe]` when it copies it next to the executable.
#[cfg(windows)]
const SIDECAR: &str = "ulpf.exe";
#[cfg(not(windows))]
const SIDECAR: &str = "ulpf";

/// The bundled splash page. Tauri serves `frontendDist` from its own scheme on macOS and
/// Linux and from a localhost host on Windows (WebView2 has no custom-scheme origin).
#[cfg(windows)]
const SPLASH: &str = "http://tauri.localhost/index.html";
#[cfg(not(windows))]
const SPLASH: &str = "tauri://localhost/index.html";

/// The scheme the injected interceptor hands a file link to the shell on. A navigation is
/// the one channel a remote origin has: Tauri's ACL gives it no command, and the webview's
/// own hooks for an anchor download and a `target="_blank"` navigation are never called on
/// macOS -- measured 06 Sep 07:09 IST with an `eprintln` in each, a click on either of the
/// UI's two file links reached neither `on_download` nor `on_new_window`, which is why both
/// buttons did nothing in the app while working in a browser.
const SAVE_SCHEME: &str = "ulpf-save:";

/// Injected into every page the window loads, the served UI included (an initialization
/// script runs at document start on any origin). A click on a link the page means as a file
/// -- `download` or `target="_blank"` -- becomes a navigation to `SAVE_SCHEME<url>`, which
/// the navigation handler cancels and `download::save` fulfils. Capturing, so the page's own
/// click handlers still run; nothing else on the page is touched.
const INTERCEPT: &str = "document.addEventListener('click', function (e) {\
     var a = e.target && e.target.closest && e.target.closest('a[href]');\
     if (!a || (!a.hasAttribute('download') && a.target !== '_blank')) return;\
     e.preventDefault();\
     location.href = 'ulpf-save:' + a.href;\
   }, true);";

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
    /// The pid the splash page's one button stops: the writer holding this store.
    holder: Mutex<Option<u32>>,
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        // The one command, for the splash page's one button.
        .invoke_handler(tauri::generate_handler![stop_holder])
        .setup(|app| {
            let handle = app.handle().clone();
            window(&handle)?;
            let data = configured_data_dir(&handle);
            app.manage(Engine {
                data: Mutex::new(data.clone()),
                child: Mutex::new(None),
                generation: AtomicU64::new(0),
                url: Mutex::new(None),
                down: Mutex::new(None),
                holder: Mutex::new(None),
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
/// executable on Windows; `resource_dir` knows which. A generated parser is never seeded:
/// see `is_generated`.
fn prepare(app: &AppHandle, data: &Path, notes: &mut Vec<String>) -> std::io::Result<()> {
    for d in ["watch", "pending", "parsers", "mappings"] {
        fs::create_dir_all(data.join(d))?;
    }
    let resources = app.path().resource_dir().map_err(|e| std::io::Error::other(e.to_string()))?;
    for d in ["parsers", "mappings"] {
        let dst = data.join(d);
        if fs::read_dir(&dst)?.flatten().any(|e| is_toml(&e.path())) {
            continue;
        }
        let mut skipped = Vec::new();
        for entry in fs::read_dir(resources.join(d))?.flatten() {
            let src = entry.path();
            if !is_toml(&src) {
                continue;
            }
            if is_generated(&src) {
                skipped.push(entry.file_name().to_string_lossy().into_owned());
                continue;
            }
            fs::copy(&src, dst.join(entry.file_name()))?;
        }
        if !skipped.is_empty() {
            notes.push(format!("shell: {} generated definition(s) not copied into {}: {}", skipped.len(), dst.display(), skipped.join(", ")));
        }
    }
    Ok(())
}

fn is_toml(p: &Path) -> bool {
    p.extension().is_some_and(|x| x == "toml")
}

/// A parser the inference engine wrote, `origin = "inferred"` and priority -1. It is never
/// copied into the data directory: a bundle built after someone approved a proposal would
/// otherwise arrive already knowing the unseen format, and the app could raise no proposal
/// of its own. The same test the engine uses in `ulpf demo --reset` (crates/ulpf/src/demo.rs),
/// on the text and not the parsed TOML so a file this shell cannot parse is still caught.
fn is_generated(p: &Path) -> bool {
    fs::read_to_string(p).is_ok_and(|t| t.lines().any(|l| l.trim_start().starts_with("origin") && l.contains("inferred")))
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

    // Lines the shell wants in engine.log; written once the file exists, below.
    let mut notes: Vec<String> = Vec::new();
    if let Err(e) = prepare(app, &data, &mut notes) {
        return fail(app, generation, "start failed", &format!("Cannot prepare {}: {e}", data.display()));
    }
    // Bind port 0, read the kernel's pick, release it, hand it to the engine. The engine
    // reports the address it bound in its own stderr line. ULPF_APP_PORT pins a port
    // instead, which is how the port-in-use path is exercised.
    let pinned = std::env::var("ULPF_APP_PORT").ok().and_then(|v| v.parse::<u16>().ok());
    let port = match TcpListener::bind(("127.0.0.1", pinned.unwrap_or(0))).and_then(|l| l.local_addr()) {
        Ok(a) => a.port(),
        Err(e) => {
            let asked = pinned.map_or_else(|| "a free port on 127.0.0.1".to_string(), |p| format!("port {p} on 127.0.0.1"));
            return fail(
                app,
                generation,
                "port in use",
                &format!("ULPF could not take {asked}: {e}.\nQuit whatever is listening there, or start ULPF with ULPF_APP_PORT unset and it will pick a free port."),
            );
        }
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
        Err(e) => {
            return fail(
                app,
                generation,
                "engine missing",
                &format!("ULPF could not start its engine: {e}.\nThe engine ships beside the app as {SIDECAR}; reinstalling ULPF replaces it."),
            )
        }
    };
    // The kernel's net under the clean-quit path: whatever happens to this process, the
    // engine goes with it (job.rs; no-op off Windows). A failure here is not fatal, so it
    // is a line in the log and nothing else.
    if let Err(e) = job::adopt(child.pid()) {
        notes.push(format!("shell: the engine is not in a kill-on-close job ({e}); a force kill of the window can leave it running"));
    }
    *engine.child.lock().unwrap() = Some(child);

    let h = app.clone();
    let store = data.join("store");
    // This run's engine output, so a failure has a file to name. Truncated per start: the
    // engine's own store and output are the durable record, this is the last words.
    let log = data.join("engine.log");
    let mut log_file = fs::File::create(&log).ok();
    // What the shell did before the engine printed anything, in the same file, so the one
    // log a user is told to read holds both halves.
    for note in &notes {
        eprintln!("[shell] {note}");
        if let Some(f) = log_file.as_mut() {
            let _ = writeln!(f, "{note}");
        }
    }
    tauri::async_runtime::spawn(async move {
        let mut last = String::new();
        while let Some(event) = rx.recv().await {
            match event {
                CommandEvent::Stderr(line) | CommandEvent::Stdout(line) => {
                    let text = String::from_utf8_lossy(&line);
                    let text = text.trim_end();
                    eprintln!("[ulpf] {text}");
                    if let Some(f) = log_file.as_mut() {
                        let _ = writeln!(f, "{text}");
                    }
                    if !text.is_empty() {
                        last = text.to_string();
                    }
                }
                CommandEvent::Terminated(t) => {
                    let why = match (t.code, t.signal) {
                        (Some(c), _) => format!("exit {c}"),
                        (None, Some(s)) => format!("signal {s}"),
                        _ => "exit unknown".to_string(),
                    };
                    // One writer at a time: the engine refuses a store another process
                    // holds and says so. That is not a broken install, it is a process to
                    // stop, so it gets its own sentence and a button rather than the
                    // generic "the engine stopped" with the refusal buried in it.
                    if last.contains(STORE_IN_USE) {
                        locked(&h, generation, &store, &log);
                        continue;
                    }
                    let said = if last.is_empty() { "It printed nothing.".to_string() } else { format!("Its last words: {last}") };
                    fail(&h, generation, &why, &format!("The engine stopped ({why}). {said}\nThe whole of its output is in {}", log.display()));
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

/// The recovery button on the splash page: which flag the fragment carries, which is also
/// which label the page puts on it. Every failure gets one, because every failure the shell
/// can show is one a fresh start might survive -- a port that has been freed, a store whose
/// holder has gone, an engine that died once.
#[derive(Copy, Clone)]
pub(crate) enum Retry {
    /// Start the engine again. Nothing to stop first.
    StartAgain,
    /// Stop the process holding the store, then start again.
    StopHolder,
}

impl Retry {
    fn flag(self) -> &'static str {
        match self {
            Retry::StartAgain => "+",
            Retry::StopHolder => "*",
        }
    }
}

/// Records why the engine is not serving, if this is still the current engine, and shows
/// the reason and a Start again button on the splash page.
fn fail(app: &AppHandle, generation: u64, why: &str, message: &str) {
    down(app, generation, why, message, Retry::StartAgain);
}

/// The store is held by another writer. Names the process, offers to stop it, and names
/// the log; the button is what `stop_holder` acts on. If the holder cannot be found (it
/// exited between the refusal and the look, or `ps` said nothing) the sentence says so and
/// there is nothing to press but the retry.
fn locked(app: &AppHandle, generation: u64, store: &Path, log: &Path) {
    let holder = holder::find(store);
    if let Some(pid) = holder {
        *app.state::<Engine>().holder.lock().unwrap() = Some(pid);
    }
    let offer = match holder {
        Some(pid) => format!("is held by ulpf (pid {pid}). Stop it and start again?"),
        None => "is held by another writer that is no longer running. Start again?".to_string(),
    };
    down(
        app,
        generation,
        "store in use",
        &format!("The engine's store at {} {offer}\nThe whole of its output is in {}", store.display(), log.display()),
        if holder.is_some() { Retry::StopHolder } else { Retry::StartAgain },
    );
}

fn down(app: &AppHandle, generation: u64, why: &str, message: &str, retry: Retry) {
    let engine = app.state::<Engine>();
    if engine.generation.load(Relaxed) != generation {
        return;
    }
    *engine.down.lock().unwrap() = Some(why.to_string());
    *engine.url.lock().unwrap() = None;
    let _ = fs::remove_file(engine.data.lock().unwrap().join("server.url"));
    set_title(app, &format!("ULPF · engine down ({why})"));
    splash_with(app, message, true, Some(retry));
}

/// The splash page's one button, whichever label it is carrying: stop the process holding
/// the store if there is one, then start again through the ordinary start path. Reachable
/// from the bundled splash page only -- the served UI is a remote origin, and Tauri's ACL
/// gives a remote origin no command at all.
///
/// A kill that fails is a notice, not a refusal: the holder may have exited on its own
/// between the refusal and this click, and `kill` on a pid that is gone is an error the
/// retry must survive. The start's own outcome is the answer either way -- it serves, or it
/// meets the holder again and says so.
#[tauri::command]
fn stop_holder(app: AppHandle) -> Result<(), String> {
    let engine = app.state::<Engine>();
    let holder = engine.holder.lock().unwrap().take();
    let data = engine.data.lock().unwrap().clone();
    if let Some(pid) = holder {
        if let Err(e) = holder::kill(pid) {
            toast(&app, &format!("pid {pid} did not stop ({e}); starting anyway"));
        }
        // The OS drops the SQLite lock when the process goes, but not before it has gone.
        thread::sleep(Duration::from_millis(400));
    }
    let app = app.clone();
    // Stop first: on the "no answer within two minutes" path the child may still be alive,
    // and a second engine on one store would only be refused by the first.
    thread::spawn(move || {
        stop(&app);
        start(&app, data, "Starting");
    });
    Ok(())
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

/// The one window, built from its own `tauri.conf.json` entry (`"create": false` there, so
/// this is the only place it is created; every value stays in the config). Creating it here
/// is what lets the interceptor and the navigation handler be attached, which is how the
/// served UI's file links come to save a file in the app (`download.rs`).
fn window(app: &AppHandle) -> tauri::Result<()> {
    let config = app.config().app.windows.first().expect("the main window in tauri.conf.json").clone();
    let h = app.clone();
    WebviewWindowBuilder::from_config(app, &config)?
        .initialization_script(INTERCEPT)
        .on_navigation(move |url| match url.as_str().strip_prefix(SAVE_SCHEME) {
            Some(target) => {
                if let Ok(target) = target.parse() {
                    download::save(&h, &target);
                }
                false
            }
            None => true,
        })
        .build()?;
    Ok(())
}

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
    splash_with(app, text, error, None);
}

/// A `Retry` flag after the `!` asks the page for its recovery button (`stop_holder`) under
/// that flag's label.
fn splash_with(app: &AppHandle, text: &str, error: bool, retry: Option<Retry>) {
    let flags = format!("{}{}", if error { "!" } else { "" }, retry.map(Retry::flag).unwrap_or_default());
    navigate(app, &format!("{SPLASH}#{flags}{}", percent_encode(text)));
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

/// One GET over loopback, body on 200: `<base>` is `http://127.0.0.1:<port>`.
pub(crate) fn http_get(base: &str, path: &str) -> Option<String> {
    let addr: SocketAddr = base.trim_start_matches("http://").parse().ok()?;
    String::from_utf8(download::get(addr, path).ok()?.1).ok()
}

#[cfg(test)]
mod tests {
    use std::fs;

    /// The predicate that keeps a generated parser out of the bundle's first-run copy, on
    /// the three shapes that matter: the engine's own output, a hand-written parser with a
    /// slot called `origin`, and a hand-written one whose prose says "inferred".
    #[test]
    fn only_the_engines_own_parsers_read_as_generated() {
        let dir = std::env::temp_dir().join(format!("ulpf-app-generated-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, text: &str| {
            let p = dir.join(name);
            fs::write(&p, text).unwrap();
            p
        };
        let generated = write("mikrotik_inferred.toml", "[parser]\nname = \"mikrotik_inferred\"\norigin = \"inferred\"\npriority = -1\n");
        let named_origin = write("cisco_ios.toml", "[parser]\nname = \"cisco_ios\"\npatterns = [\n  '{origin:word}: {message:rest}',\n]\n");
        let says_inferred = write("prose.toml", "[parser]\nname = \"prose\"\ndescription = \"the fields are not inferred, they are documented\"\n");
        assert!(super::is_generated(&generated));
        assert!(!super::is_generated(&named_origin));
        assert!(!super::is_generated(&says_inferred));
        assert!(!super::is_generated(&dir.join("no-such-file.toml")));
        let _ = fs::remove_dir_all(&dir);

        // And no false positive on what the repo actually ships, read where it lives.
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../parsers");
        let mut seen = 0;
        for e in fs::read_dir(&repo).expect("the repo's parsers/").flatten() {
            if super::is_toml(&e.path()) {
                seen += 1;
                assert!(!super::is_generated(&e.path()), "{} reads as generated", e.path().display());
            }
        }
        assert!(seen >= 12, "only {seen} parsers found in {}", repo.display());
    }

    #[test]
    fn percent_encoding_keeps_unreserved_and_escapes_the_rest() {
        assert_eq!(super::percent_encode("Starting the engine"), "Starting%20the%20engine");
        assert_eq!(super::percent_encode("a-b.c_d~e"), "a-b.c_d~e");
        assert_eq!(super::percent_encode("é#"), "%C3%A9%23");
    }
}
