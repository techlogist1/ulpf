// Reset: the shell removes the engine's data, the engine never does.
//
// The raw store is append-only by contract -- its API is append and read, with no update
// and no delete (D42), and every record is a link in a chain a stranger re-verifies (D56).
// An engine that could empty its own store would be an engine that can rewrite history. So
// a reset is a shell action on the data directory: stop the engine, remove files, start it
// again through the ordinary launch path.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use crate::{holder, is_toml, navigate, splash_with, start, stop, toast, Engine, Retry, EXIT_WAIT};

/// Everything the chosen reset removes, given the data directory as it stands.
///
/// Keeping the parsers: the events go and the definitions stay. `store/` and every file the
/// engine writes beside `out.jsonl` (the pivot index, `out.vN.jsonl`, `out.vN.meta.json`),
/// plus `watch/`, `pending/` and `staging/` -- those three whole rather than entry by entry,
/// because the ordinary start recreates them empty. `parsers/` and `mappings/` are never in
/// this list. Neither is `engine.log`, which is where a deletion that fails is written, nor
/// `server.url`, which the stop removes.
///
/// Otherwise: the data directory itself, and the ordinary start re-seeds `parsers/` and
/// `mappings/` from the bundle with the generated definitions excluded (D94).
pub(crate) fn reset_paths(data: &Path, keep_parsers: bool) -> Vec<PathBuf> {
    if !keep_parsers {
        return vec![data.to_path_buf()];
    }
    let mut paths: Vec<PathBuf> = ["store", "watch", "pending", "staging"].iter().map(|d| data.join(d)).filter(|p| p.exists()).collect();
    // By prefix, because the version number in out.vN.jsonl is not known here.
    let mut outputs: Vec<PathBuf> = fs::read_dir(data)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("out.")))
        .collect();
    outputs.sort();
    paths.append(&mut outputs);
    paths
}

/// The menu item. Nothing is removed here: the choice page names the directory and says
/// what each button does, and the buttons are what act.
pub(crate) fn ask(app: &AppHandle) {
    let data = app.state::<Engine>().data.lock().unwrap().clone();
    splash_with(
        app,
        &format!(
            "ULPF keeps its data in {}.\n\
             Reset events removes the store, the output files, and anything waiting in watch/ and pending/. Your parsers and mappings stay.\n\
             Reset to first launch removes that whole directory: the parsers come back from the ones ULPF ships with.",
            data.display()
        ),
        false,
        Some(Retry::Reset),
    );
}

/// `stop`, then wait until nothing holds the store, so the deletion cannot race the writer.
/// The child this shell spawned is waited on by pid, the one question that costs nothing
/// and answers exactly when the lock is released. Only with no child of ours is the holder
/// looked for by its command line (`holder::find`, a process listing per poll). A store
/// still held after `EXIT_WAIT` is not a reason to refuse: the deletion reports what it
/// could not remove, and the start reports the holder.
fn stop_and_wait(app: &AppHandle) {
    let store = app.state::<Engine>().data.lock().unwrap().join("store");
    match stop(app) {
        Some(pid) => {
            holder::wait_exit(pid, EXIT_WAIT);
        }
        None => {
            let deadline = Instant::now() + EXIT_WAIT;
            while Instant::now() < deadline && holder::find(&store).is_some() {
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

/// One of the two reset buttons. Stops the engine, removes what the choice says, and
/// re-enters `start` exactly as the app does at launch, so the UI comes back with the
/// parsers it kept (or the bundled ones) and no events. A file that will not go is a line
/// in `engine.log` and a notice; the app comes back up either way.
#[tauri::command]
pub(crate) fn reset(app: AppHandle, keep: bool) {
    thread::spawn(move || {
        let data = app.state::<Engine>().data.lock().unwrap().clone();
        stop_and_wait(&app);
        let mut failed = Vec::new();
        for p in reset_paths(&data, keep) {
            let removed = if p.is_dir() { fs::remove_dir_all(&p) } else { fs::remove_file(&p) };
            if let Err(e) = removed {
                failed.push(format!("shell: reset could not remove {}: {e}", p.display()));
            }
        }
        start(&app, data.clone(), "Restarting");
        // After the start, because every start truncates engine.log.
        if !failed.is_empty() {
            let mut log = fs::OpenOptions::new().create(true).append(true).open(data.join("engine.log")).ok();
            for line in &failed {
                eprintln!("[shell] {line}");
                if let Some(f) = log.as_mut() {
                    let _ = writeln!(f, "{line}");
                }
            }
        }
        let n = parsers(&data);
        let mut text = if keep { format!("Reset: events removed, {n} parsers kept") } else { format!("Reset to first launch: {n} parsers") };
        if !failed.is_empty() {
            text.push_str(&format!(". {} item(s) could not be removed; see engine.log", failed.len()));
        }
        toast(&app, &text);
    });
}

/// The third button. Back to where the window was: the served UI if the engine is up, else
/// the page that was showing when Reset was chosen. Nothing was touched.
#[tauri::command]
pub(crate) fn reset_cancel(app: AppHandle) {
    let engine = app.state::<Engine>();
    let back = engine.url.lock().unwrap().clone().or_else(|| engine.splash.lock().unwrap().clone());
    if let Some(back) = back {
        navigate(&app, &back);
    }
}

fn parsers(data: &Path) -> usize {
    fs::read_dir(data.join("parsers")).into_iter().flatten().flatten().filter(|e| is_toml(&e.path())).count()
}

#[cfg(test)]
mod tests {
    use std::fs;

    /// A data directory with every entry the shell and the engine make: what each choice
    /// removes, and what it must not.
    #[test]
    fn each_choice_removes_exactly_its_own_paths() {
        let data = std::env::temp_dir().join(format!("ulpf-app-reset-{}", std::process::id()));
        let _ = fs::remove_dir_all(&data);
        for d in ["store", "watch", "pending", "staging", "parsers", "mappings"] {
            fs::create_dir_all(data.join(d)).unwrap();
        }
        for f in ["out.jsonl", "out.jsonl.pivot", "out.v1.jsonl", "out.v1.meta.json", "server.url", "engine.log"] {
            fs::write(data.join(f), b"").unwrap();
        }
        fs::write(data.join("parsers/cisco_asa.toml"), b"").unwrap();

        let mut kept = super::reset_paths(&data, true);
        kept.sort();
        let mut want: Vec<_> = ["out.jsonl", "out.jsonl.pivot", "out.v1.jsonl", "out.v1.meta.json", "pending", "staging", "store", "watch"]
            .iter()
            .map(|p| data.join(p))
            .collect();
        want.sort();
        assert_eq!(kept, want);
        // The two the choice is named after, and the two the shell needs after it.
        for keep in ["parsers", "mappings", "engine.log", "server.url"] {
            assert!(!kept.contains(&data.join(keep)), "{keep} is in the keep-parsers set");
        }

        assert_eq!(super::reset_paths(&data, false), vec![data.clone()]);

        // An entry that is not there is not named: a second reset removes nothing twice.
        fs::remove_dir_all(data.join("staging")).unwrap();
        assert!(!super::reset_paths(&data, true).contains(&data.join("staging")));

        assert_eq!(super::parsers(&data), 1);
        fs::remove_dir_all(&data).unwrap();
    }
}
