// The window menu, the tray, and every action behind them.

use std::fs;
use std::path::PathBuf;
use std::thread;

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, FilePath};
use tauri_plugin_opener::OpenerExt;

use crate::ingest::ingest_paths;
use crate::{config_file, start, stop, toast, Engine};

pub(crate) fn install(app: &AppHandle) -> tauri::Result<()> {
    let item = |id: &str, text: &str, accel: Option<&str>| MenuItem::with_id(app, id, text, true, accel);
    // macOS puts the first submenu in the application menu slot (About and Quit live
    // there); Windows shows it as a plain "ULPF" menu in the window's menu bar.
    let app_menu = Submenu::with_items(
        app,
        "ULPF",
        true,
        &[&PredefinedMenuItem::about(app, None, None)?, &PredefinedMenuItem::separator(app)?, &item("quit", "Quit ULPF", Some("CmdOrCtrl+Q"))?],
    )?;
    let file = Submenu::with_items(
        app,
        "File",
        true,
        &[
            &item("add_files", "Add files…", Some("CmdOrCtrl+O"))?,
            &item("add_folder", "Add folder…", Some("CmdOrCtrl+Shift+O"))?,
            &PredefinedMenuItem::separator(app)?,
            &item("open_output", "Open output folder", Some("CmdOrCtrl+Shift+E"))?,
            &item("open_browser", "Open in browser", Some("CmdOrCtrl+Shift+B"))?,
            &PredefinedMenuItem::separator(app)?,
            &item("choose_data", "Choose data directory…", None)?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    app.set_menu(Menu::with_items(app, &[&app_menu, &file, &edit])?)?;

    // The tray: the window can be closed and the engine keeps running; this is the way
    // back, and Quit here is the same Quit as the menu's.
    let tray_menu = Menu::with_items(
        app,
        &[
            &item("show", "Show ULPF", None)?,
            &item("open_output", "Open output folder", None)?,
            &item("open_browser", "Open in browser", None)?,
            &PredefinedMenuItem::separator(app)?,
            &item("quit", "Quit ULPF", None)?,
        ],
    )?;
    let tray = TrayIconBuilder::with_id("ulpf").menu(&tray_menu).show_menu_on_left_click(true).tooltip("ULPF");
    // macOS: a template image (alpha only) in the menu bar, so the glyph follows the bar's
    // light or dark style. Windows: the coloured app icon in the notification area.
    #[cfg(target_os = "macos")]
    let tray = tray.icon(glyph()).icon_as_template(true);
    #[cfg(not(target_os = "macos"))]
    let tray = tray.icon(app.default_window_icon().map(|i| Image::new_owned(i.rgba().to_vec(), i.width(), i.height())).unwrap_or_else(glyph));
    tray.build(app)?;
    Ok(())
}

/// The ULPF mark, three bars, as a 44 px RGBA image drawn here rather than shipped as a
/// file: the shape is the splash page's `.mark`.
fn glyph() -> Image<'static> {
    const N: usize = 44;
    let mut px = vec![0u8; N * N * 4];
    for (y0, w) in [(10, 28), (20, 17), (30, 23)] {
        for y in y0..y0 + 5 {
            for x in 8..8 + w {
                let i = (y * N + x) * 4;
                px[i..i + 4].copy_from_slice(&[0, 0, 0, 255]);
            }
        }
    }
    Image::new_owned(px, N as u32, N as u32)
}

/// Every menu and tray action by id. The native pickers feed the same `ingest_paths` a
/// drop does.
pub(crate) fn action(app: &AppHandle, id: &str) {
    match id {
        "add_files" => {
            let h = app.clone();
            app.dialog().file().set_title("Add log files to ULPF").pick_files(move |picked| ingest_paths(&h, &to_paths(picked.unwrap_or_default())));
        }
        "add_folder" => {
            let h = app.clone();
            app.dialog().file().set_title("Add a folder of logs to ULPF").pick_folder(move |picked| ingest_paths(&h, &to_paths(picked.into_iter().collect())));
        }
        "choose_data" => {
            let h = app.clone();
            app.dialog().file().set_title("Choose where ULPF keeps its data").pick_folder(move |picked| {
                let Some(dir) = picked.and_then(|p| p.into_path().ok()) else { return };
                if let Err(e) = fs::create_dir_all(config_file(&h).parent().unwrap_or(&dir)).and_then(|()| fs::write(config_file(&h), dir.to_string_lossy().as_bytes())) {
                    return toast(&h, &format!("Cannot remember that directory: {e}"));
                }
                // The engine restarts against the new directory; the old one is left as it
                // is (its store, output and pending proposals stay where they were).
                thread::spawn(move || {
                    stop(&h);
                    start(&h, dir);
                });
            });
        }
        "open_output" => {
            let data = app.state::<Engine>().data.lock().unwrap().clone();
            let out = data.join("out.jsonl");
            // Finder or Explorer with out.jsonl selected; the directory alone before the
            // engine has written the first line.
            let opened = if out.exists() { app.opener().reveal_item_in_dir(&out) } else { app.opener().open_path(data.to_string_lossy(), None::<&str>) };
            if let Err(e) = opened {
                toast(app, &format!("Cannot open {}: {e}", data.display()));
            }
        }
        "open_browser" => {
            let url = app.state::<Engine>().url.lock().unwrap().clone();
            match url {
                Some(url) => {
                    if let Err(e) = app.opener().open_url(&url, None::<&str>) {
                        toast(app, &format!("Cannot open {url}: {e}"));
                    }
                }
                None => toast(app, "The engine is not serving yet."),
            }
        }
        "show" => show(app),
        "quit" => app.exit(0),
        _ => {}
    }
}

pub(crate) fn show(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn to_paths(picked: Vec<FilePath>) -> Vec<PathBuf> {
    picked.into_iter().filter_map(|p| p.into_path().ok()).collect()
}
