// The window title as the engine's status line: `ULPF · engine ok · 123,456 events ·
// 2 pending · Balanced · 4 of 8 cores · index on`, once a second from /api/metrics,
// /api/pending and /api/status. While the engine is starting or down, `start` and `fail`
// own the title.

use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::intensity;
use crate::{http_get, set_title, Engine};

pub(crate) fn title_loop(app: &AppHandle) {
    loop {
        thread::sleep(Duration::from_secs(1));
        let url = app.state::<Engine>().url.lock().unwrap().clone();
        let Some(url) = url else { continue };
        let json = |path: &str| http_get(&url, path).and_then(|b| serde_json::from_str::<serde_json::Value>(&b).ok());
        let events = json("/api/metrics").and_then(|v| v["engine"]["emitted"].as_u64());
        let pending = json("/api/pending").and_then(|v| v.as_array().map(Vec::len));
        let status = json("/api/status");
        if let (Some(events), Some(pending)) = (events, pending) {
            let running = (status.as_ref().and_then(|v| v["threads"].as_u64()), status.as_ref().and_then(|v| v["pivot_index"].as_bool()));
            set_title(app, &format!("ULPF · engine ok · {} events · {pending} pending · {}", commas(events), intensity_part(app, running)));
        }
    }
}

/// The setting's name with the numbers the running engine reports, not the ones the
/// setting asks for: while a restart is in flight the two disagree and the title says so
/// rather than quoting a number nothing is using.
fn intensity_part(app: &AppHandle, running: (Option<u64>, Option<bool>)) -> String {
    let (chosen, cores) = (intensity::load(app), intensity::cores());
    match running {
        (Some(threads), Some(index)) if threads as usize == chosen.threads(cores) && index == chosen.pivot() => {
            format!("{} · {threads} of {cores} cores · index {}", chosen.name(), intensity::on_off(index))
        }
        _ => "restarting".to_string(),
    }
}

fn commas(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn commas_groups_thousands() {
        assert_eq!(super::commas(0), "0");
        assert_eq!(super::commas(999), "999");
        assert_eq!(super::commas(1000), "1,000");
        assert_eq!(super::commas(123_456_789), "123,456,789");
    }
}
