// The one performance setting: three named choices, each label carrying this machine's own
// numbers. The engine fixes its worker count when it starts (D40, D60) and the entity index
// is a start-time switch (D66), so a change restarts the sidecar instead of pretending to
// take effect live.

use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::{start, stop, toast, Engine};

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum Intensity {
    Low,
    Balanced,
    Max,
}

use Intensity::{Balanced, Low, Max};

impl Intensity {
    pub(crate) const ALL: [Intensity; 3] = [Low, Balanced, Max];

    /// The word shown, and the word kept in the settings file.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Low => "Low",
            Balanced => "Balanced",
            Max => "Max",
        }
    }

    /// The menu item id.
    pub(crate) fn id(self) -> &'static str {
        match self {
            Low => "intensity_low",
            Balanced => "intensity_balanced",
            Max => "intensity_max",
        }
    }

    pub(crate) fn from_id(id: &str) -> Option<Self> {
        Intensity::ALL.into_iter().find(|i| i.id() == id)
    }

    /// Worker threads on a machine with `cores` of them. Max is the engine's own default.
    pub(crate) fn threads(self, cores: usize) -> usize {
        match self {
            Low if cores < 4 => 1,
            Low => 2,
            Balanced => (cores / 2).max(1),
            Max => cores.saturating_sub(1).max(1),
        }
    }

    /// The entity index. Only Low gives up the pivot: the index costs an order of magnitude
    /// on bulk throughput (D66) and is `serve`'s own default otherwise.
    pub(crate) fn pivot(self) -> bool {
        self != Low
    }

    /// `Low · 2 of 8 cores · entity index off`: the menu item and the splash line.
    pub(crate) fn label(self, cores: usize) -> String {
        format!("{} · {} of {cores} cores · entity index {}", self.name(), self.threads(cores), on_off(self.pivot()))
    }

    /// What the choice means to the engine, appended to the serve arguments.
    pub(crate) fn args(self, cores: usize) -> [String; 4] {
        ["-j".into(), self.threads(cores).to_string(), "--pivot".into(), on_off(self.pivot()).into()]
    }
}

pub(crate) fn on_off(b: bool) -> &'static str {
    if b {
        "on"
    } else {
        "off"
    }
}

pub(crate) fn cores() -> usize {
    thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
}

/// One word beside the `data_dir` override, in the same directory: macOS
/// `~/Library/Application Support/dev.ulpf.desktop/intensity`, Windows
/// `%APPDATA%\dev.ulpf.desktop\intensity`.
fn file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("intensity"))
}

/// The kept choice. A missing, unreadable or unrecognised file means Balanced, which is
/// also what a fresh install gets.
pub(crate) fn load(app: &AppHandle) -> Intensity {
    file(app)
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| Intensity::ALL.into_iter().find(|i| i.name().eq_ignore_ascii_case(s.trim())))
        .unwrap_or(Balanced)
}

/// Keeps the choice, re-marks the menu, and restarts the engine against the same data
/// directory on a fresh free port. The stop is Quit's stop (`Child::kill`: SIGKILL on
/// macOS, TerminateProcess on Windows), which the engine's kill recovery makes safe (D59),
/// and the generation counter keeps the dying child's exit off the new one.
pub(crate) fn choose(app: &AppHandle, chosen: Intensity) {
    let Some(path) = file(app) else {
        return toast(app, "Cannot find the app's settings directory.");
    };
    if let Err(e) = fs::create_dir_all(path.parent().unwrap_or(&path)).and_then(|()| fs::write(&path, chosen.name())) {
        return toast(app, &format!("Cannot remember that setting: {e}"));
    }
    check_marks(app, chosen);
    let cores = cores();
    toast(app, &format!("Restarting the engine at {}: {} of {cores} cores, entity index {}", chosen.name(), chosen.threads(cores), on_off(chosen.pivot())));
    let data = app.state::<Engine>().data.lock().unwrap().clone();
    let app = app.clone();
    thread::spawn(move || {
        // The notice goes on the page that is still up, because a restart on this machine
        // finishes faster than the splash it navigates to can paint. Long enough to read.
        thread::sleep(Duration::from_millis(900));
        stop(&app);
        start(&app, data, "Restarting");
    });
}

/// The check mark follows the choice. A native check item toggles itself when it is
/// clicked, so all three are set here, not only the new one.
fn check_marks(app: &AppHandle, chosen: Intensity) {
    let Some(file) = app.menu().and_then(|m| m.get("file")).and_then(|k| k.as_submenu().cloned()) else { return };
    let Some(sub) = file.get("intensity").and_then(|k| k.as_submenu().cloned()) else { return };
    for i in Intensity::ALL {
        if let Some(item) = sub.get(i.id()).and_then(|k| k.as_check_menuitem().cloned()) {
            let _ = item.set_checked(i == chosen);
        }
    }
}

/// Says the engine is back, once the page the notice is injected into has had time to load
/// (the window has just been navigated to the new port).
pub(crate) fn ready_notice(app: &AppHandle, chosen: Intensity, cores: usize) {
    thread::sleep(Duration::from_millis(700));
    toast(app, &format!("Engine ready at {}", chosen.label(cores)));
}

#[cfg(test)]
mod tests {
    use super::Intensity;
    use super::Intensity::{Balanced, Low, Max};

    #[test]
    fn threads_and_index_per_choice() {
        // The demo machine: 8 cores.
        assert_eq!((Low.threads(8), Balanced.threads(8), Max.threads(8)), (2, 4, 7));
        // Fewer than four cores: Low takes one, and nothing ever asks for zero.
        assert_eq!((Low.threads(2), Balanced.threads(2), Max.threads(2)), (1, 1, 1));
        assert_eq!(Low.threads(1), 1);
        assert_eq!((Low.pivot(), Balanced.pivot(), Max.pivot()), (false, true, true));
    }

    #[test]
    fn labels_and_args_carry_the_numbers() {
        assert_eq!(Low.label(8), "Low · 2 of 8 cores · entity index off");
        assert_eq!(Max.label(8), "Max · 7 of 8 cores · entity index on");
        assert_eq!(Balanced.args(8), ["-j", "4", "--pivot", "on"]);
        assert_eq!(Low.args(8), ["-j", "2", "--pivot", "off"]);
    }

    #[test]
    fn ids_round_trip() {
        for i in Intensity::ALL {
            assert_eq!(Intensity::from_id(i.id()), Some(i));
        }
        assert_eq!(Intensity::from_id("intensity_none"), None);
    }
}
