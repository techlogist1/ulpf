// The window menu and every action behind it.

use std::fs;
use std::path::PathBuf;
use std::thread;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
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

    Ok(())
}

/// Every menu action by id. The native pickers feed the same `ingest_paths` a
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
        "quit" => app.exit(0),
        _ => {}
    }
}

fn to_paths(picked: Vec<FilePath>) -> Vec<PathBuf> {
    picked.into_iter().filter_map(|p| p.into_path().ok()).collect()
}
