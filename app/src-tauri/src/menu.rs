// The window menu and every action behind it.

use std::path::PathBuf;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, FilePath};

use crate::ingest::ingest_paths;

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
        "quit" => app.exit(0),
        _ => {}
    }
}

fn to_paths(picked: Vec<FilePath>) -> Vec<PathBuf> {
    picked.into_iter().filter_map(|p| p.into_path().ok()).collect()
}
