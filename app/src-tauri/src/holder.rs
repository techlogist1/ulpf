// Who else has the store.
//
// The engine allows one writer: the catalogue's connection is opened in SQLite's exclusive
// locking mode and holds the file lock until it closes, so a second writer is refused with
// `store <dir> is in use by another process` and exits 2 (crates/ulpf-store/src/store.rs).
// There is no lock file to inspect and no pid to read out of the catalogue -- the
// catalogue is the thing that is locked -- so the holder is found the only way left: a
// running `ulpf` whose command line carries the same --store path.

use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

/// The pid of a running `ulpf` whose command line names `store`, if there is one. The
/// first match: two writers on one store cannot both hold the lock, so there is at most
/// one that matters.
pub(crate) fn find(store: &Path) -> Option<u32> {
    pick(&processes()?, &store.to_string_lossy())
}

/// `pid command line` per line in, the pid of the `ulpf` naming `store` out. Never this
/// process: the shell is not a ulpf, but a data directory under a path that contains one
/// would otherwise be able to name it.
fn pick(lines: &str, store: &str) -> Option<u32> {
    lines
        .lines()
        .filter_map(|line| {
            let (pid, cmd) = line.trim_start().split_once(' ')?;
            Some((pid.parse::<u32>().ok()?, cmd))
        })
        .find(|(pid, cmd)| *pid != std::process::id() && cmd.contains("ulpf") && cmd.contains(store))
        .map(|(pid, _)| pid)
}

/// `pid command line` per line, on both platforms. `ps` is in every POSIX base system;
/// `Get-CimInstance Win32_Process` is the one place Windows keeps a full command line
/// (Get-Process does not carry it).
#[cfg(not(windows))]
fn processes() -> Option<String> {
    let out = Command::new("ps").args(["-axo", "pid=,command="]).output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(windows)]
fn processes() -> Option<String> {
    let out = no_window(Command::new("powershell").args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "Get-CimInstance Win32_Process -Filter \"Name='ulpf.exe'\" | ForEach-Object { \"$($_.ProcessId) $($_.CommandLine)\" }",
    ]))
    .output()
    .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Blocks until `pid` is gone or `timeout` passes; true when it is gone. A killed process
/// releases the store's lock only once the kernel has reaped it, and that is what a reset
/// waits on before it deletes. Asked of the kernel per poll (`kill -0`, `WaitForSingleObject`),
/// never of a shell: the previous wait polled `find`, which on Windows spawns PowerShell
/// for `Get-CimInstance` -- seconds per call on a cold machine, paid ten times a second.
pub(crate) fn wait_exit(pid: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while alive(pid) {
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(50));
    }
    true
}

/// `kill -0` sends nothing; success means the pid exists and is ours to signal.
#[cfg(not(windows))]
fn alive(pid: u32) -> bool {
    Command::new("kill").args(["-0", &pid.to_string()]).output().map(|o| o.status.success()).unwrap_or(false)
}

/// A handle opened for SYNCHRONIZE only, waited on for zero milliseconds: signalled means
/// exited. A pid that cannot be opened is one that is already gone.
#[cfg(windows)]
fn alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    unsafe {
        let h = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if h.is_null() {
            return false;
        }
        let alive = WaitForSingleObject(h, 0) == WAIT_TIMEOUT;
        CloseHandle(h);
        alive
    }
}

/// Force-kills one pid. There is no portable stdlib way to signal a process this one did
/// not spawn, so this is the platform's own tool.
pub(crate) fn kill(pid: u32) -> Result<(), String> {
    #[cfg(not(windows))]
    let mut cmd = Command::new("kill");
    #[cfg(not(windows))]
    cmd.args(["-9", &pid.to_string()]);
    #[cfg(windows)]
    let mut cmd = Command::new("taskkill");
    #[cfg(windows)]
    {
        cmd.args(["/PID", &pid.to_string(), "/F"]);
        no_window(&mut cmd);
    }
    match cmd.output() {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => Err(e.to_string()),
    }
}

/// CREATE_NO_WINDOW: a console child of a GUI app otherwise flashes a black window over it.
#[cfg(windows)]
fn no_window(cmd: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    /// This process is alive and stays so; a child that has exited is gone at once.
    #[test]
    fn wait_exit_answers_for_a_live_and_for_a_gone_process() {
        assert!(!super::wait_exit(std::process::id(), Duration::from_millis(120)));
        #[cfg(windows)]
        let mut child = std::process::Command::new("cmd").args(["/C", "exit"]).spawn().unwrap();
        #[cfg(not(windows))]
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        assert!(super::wait_exit(pid, Duration::from_secs(2)));
    }

    /// The shapes `ps -axo pid=,command=` and `Get-CimInstance` both print: leading
    /// spaces, the pid, one space, the rest of the line. Windows paths included, because
    /// that is the platform the item came from.
    #[test]
    fn picks_the_ulpf_whose_command_line_names_the_store() {
        let lines = "  501 /usr/bin/whatever --store C:\\data\\store\n\
                     \x20 1904 C:\\ULPF\\ulpf.exe serve W --store C:\\data\\store --output o\n\
                     \x20 2000 ulpf serve x --store /other/store\n\
                     header line with no pid\n";
        assert_eq!(super::pick(lines, "C:\\data\\store"), Some(1904));
        assert_eq!(super::pick(lines, "/other/store"), Some(2000));
        assert_eq!(super::pick(lines, "/nothing/holds/this"), None);
        assert_eq!(super::pick("", "/x"), None);
        // Never this process, whatever its command line says.
        let me = format!(" {} ulpf serve --store /mine\n", std::process::id());
        assert_eq!(super::pick(&me, "/mine"), None);
    }
}
