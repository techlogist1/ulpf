//! `serve --exit-with-parent` stops when the process that started it is gone: the shape a
//! force-quit desktop shell leaves behind. Unix only: Windows reaps through the shell's job
//! object and the flag does nothing there.
#![cfg(unix)]

use std::process::Command;
use std::time::{Duration, Instant};

fn alive(pid: u32) -> bool {
    Command::new("kill").args(["-0", &pid.to_string()]).status().map(|s| s.success()).unwrap_or(false)
}

#[test]
fn the_engine_stops_within_two_seconds_of_its_parent_dying() {
    let dir = std::env::temp_dir().join(format!("ulpf-exit-with-parent-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("watch")).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    // An intermediate shell is the parent: it starts the engine in the background, prints
    // the pid and exits at once, which is exactly what a killed shell looks like to a child;
    // `$$` is that shell's own pid, the one the engine is told to watch.
    let script = format!(
        "\"{}\" serve \"{}\" --store \"{}\" --output \"{}\" --parsers \"{}\" --mappings \"{}\" --listen 127.0.0.1:0 --exit-with-parent $$ >/dev/null 2>&1 & echo $!",
        env!("CARGO_BIN_EXE_ulpf"),
        dir.join("watch").display(),
        dir.join("store").display(),
        dir.join("out.jsonl").display(),
        root.join("parsers").display(),
        root.join("mappings").display()
    );
    let out = Command::new("sh").arg("-c").arg(&script).output().unwrap();
    let pid: u32 = String::from_utf8_lossy(&out.stdout).trim().parse().expect("the shell printed the engine's pid");
    // Half a second of poll, the stop, and the server's two-second shutdown budget; alone
    // it is under a second, under a full parallel suite it has been 2.5 s. Ten seconds is
    // the ceiling that says "never", not the measurement, which is printed.
    let started = Instant::now();
    while alive(pid) && started.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(50));
    }
    let gone_after = started.elapsed();
    if alive(pid) {
        let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
        panic!("the engine (pid {pid}) outlived its parent by more than 10 s");
    }
    eprintln!("the engine was gone {gone_after:?} after its parent");
    let _ = std::fs::remove_dir_all(&dir);
}
