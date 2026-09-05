// Files and folders that arrive by a drop on the window or through File > Add: copied
// whole into the watched directory, then the page says what arrived.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;

use tauri::{AppHandle, Manager};

use crate::{toast, Engine};

/// Both entry points land here. Each item is copied into `<data>/staging` on the same
/// volume and then renamed into `<data>/watch` under a unique name, so the engine's poller
/// sees a complete file or nothing, never a half-written one. Folders keep their
/// structure (the engine walks them); only regular files are copied.
pub(crate) fn ingest_paths(app: &AppHandle, paths: &[PathBuf]) {
    let (app, paths) = (app.clone(), paths.to_vec());
    thread::spawn(move || {
        let data = app.state::<Engine>().data.lock().unwrap().clone();
        let (watch, staging) = (data.join("watch"), data.join("staging"));
        if let Err(e) = fs::create_dir_all(&staging) {
            return toast(&app, &format!("Cannot write to {}: {e}", data.display()));
        }
        let mut files = 0usize;
        let mut names = Vec::new();
        let mut problems = Vec::new();
        for src in &paths {
            let Some(name) = src.file_name() else { continue };
            let shown = name.to_string_lossy().into_owned();
            let tmp = staging.join(name);
            let _ = fs::remove_dir_all(&tmp);
            let _ = fs::remove_file(&tmp);
            match copy_tree(src, &tmp) {
                Ok(0) => {
                    let _ = fs::remove_dir_all(&tmp);
                    problems.push(format!("{shown} (no regular files)"));
                }
                Ok(n) => {
                    let dst = unique(&watch, name);
                    match fs::rename(&tmp, &dst) {
                        Ok(()) => {
                            files += n;
                            names.push(dst.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(shown));
                        }
                        Err(e) => problems.push(format!("{shown} ({e})")),
                    }
                }
                Err(e) => problems.push(format!("{shown} ({e})")),
            }
        }
        let mut text = format!("Added {files} file{} to the watch folder", if files == 1 { "" } else { "s" });
        if !names.is_empty() {
            let shown: Vec<&str> = names.iter().take(4).map(String::as_str).collect();
            text.push_str(&format!(": {}{}", shown.join(", "), if names.len() > 4 { ", …" } else { "" }));
        }
        if !problems.is_empty() {
            text.push_str(&format!(". Not copied: {}", problems.join(", ")));
        }
        toast(&app, &text);
    });
}

/// Copies a regular file, or a directory tree of regular files; returns the count.
fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<usize> {
    let kind = fs::symlink_metadata(src)?.file_type();
    if kind.is_file() {
        fs::copy(src, dst)?;
        return Ok(1);
    }
    if !kind.is_dir() {
        return Ok(0); // symlinks, sockets, devices
    }
    fs::create_dir_all(dst)?;
    let mut n = 0;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        n += copy_tree(&entry.path(), &dst.join(entry.file_name()))?;
    }
    Ok(n)
}

/// `name`, else `stem (2).ext`, `stem (3).ext`, ...
fn unique(dir: &Path, name: &OsStr) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let p = Path::new(name);
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (2u32..).map(|i| dir.join(format!("{stem} ({i}){ext}"))).find(|c| !c.exists()).expect("an unused name")
}

#[cfg(test)]
mod tests {
    use std::fs;

    #[test]
    fn unique_names_do_not_collide() {
        let dir = std::env::temp_dir().join(format!("ulpf-app-unique-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let name = std::ffi::OsStr::new("fw.log");
        assert_eq!(super::unique(&dir, name), dir.join("fw.log"));
        fs::write(dir.join("fw.log"), b"").unwrap();
        assert_eq!(super::unique(&dir, name), dir.join("fw (2).log"));
        fs::write(dir.join("fw (2).log"), b"").unwrap();
        assert_eq!(super::unique(&dir, name), dir.join("fw (3).log"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn copy_tree_takes_regular_files_only_and_keeps_structure() {
        let root = std::env::temp_dir().join(format!("ulpf-app-copy-{}", std::process::id()));
        let (src, dst) = (root.join("src"), root.join("dst"));
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("a.log"), b"a").unwrap();
        fs::write(src.join("sub/b.log"), b"b").unwrap();
        assert_eq!(super::copy_tree(&src, &dst).unwrap(), 2);
        assert_eq!(fs::read(dst.join("sub/b.log")).unwrap(), b"b");
        assert_eq!(super::copy_tree(&src.join("a.log"), &root.join("solo.log")).unwrap(), 1);
        fs::remove_dir_all(&root).unwrap();
    }
}
