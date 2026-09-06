// Who reaps the engine when the app dies badly.
//
// Windows has no process group and no "kill my children when I go": a force kill of
// ulpf-app.exe (Task Manager's End task, a crash, Stop-Process -Force) skips the shell's
// exit handler, so ulpf.exe keeps running, keeps the store's SQLite lock, and the next
// launch is refused by its own writer. A job object carrying
// JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE moves that guarantee into the kernel: the handle is
// held for the app's whole life, and when the last handle to the job closes -- which the
// OS does for every handle of a dying process, however it died -- every process still in
// the job is terminated. The clean-quit path (Child::kill on ExitRequested) is unchanged
// and still the normal way out; this is the net under it.
//
// macOS has no equivalent, and no way to want one: Unix does not kill a child when its
// parent dies, and a SIGKILLed parent runs no code to kill it either -- measured, `kill -9`
// of the app leaves `ulpf serve` running and holding the store. The clean-quit path
// (Child::kill on ExitRequested) is what stops it there; a force quit leaves it until the
// next launch, which meets the held store, finds the holder by its command line
// (`holder.rs`) and offers to stop it and start again.

/// Puts the just-spawned engine in this app's kill-on-job-close job. `Err` says why not;
/// the caller logs it and carries on, because a missing safety net is no reason to refuse
/// to start. No-op off Windows.
#[cfg(not(windows))]
pub(crate) fn adopt(_pid: u32) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
pub(crate) fn adopt(pid: u32) -> Result<(), String> {
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

    // The job handle for this app's whole life. Closing it is what kills the engine, so it
    // is never closed; a raw HANDLE is not Send, hence the usize. 0 records a failed
    // create, so the next start reports it instead of retrying a broken handle.
    static JOB: OnceLock<usize> = OnceLock::new();
    let job = *JOB.get_or_init(|| unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return 0;
        }
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let size = std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
        if SetInformationJobObject(job, JobObjectExtendedLimitInformation, (&raw const info).cast(), size) == 0 {
            CloseHandle(job);
            return 0;
        }
        job as usize
    });
    if job == 0 {
        return Err(format!("no job object: {}", std::io::Error::last_os_error()));
    }
    // A pid is reusable and a handle is not, so this is opened per child, right after the
    // spawn. PROCESS_SET_QUOTA is what the assignment needs, PROCESS_TERMINATE what the
    // job needs to kill it later.
    let child = unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid) };
    if child.is_null() {
        return Err(format!("OpenProcess({pid}): {}", std::io::Error::last_os_error()));
    }
    let ok = unsafe { AssignProcessToJobObject(job as HANDLE, child) } != 0;
    let why = std::io::Error::last_os_error();
    unsafe { CloseHandle(child) };
    if ok {
        Ok(())
    } else {
        Err(format!("AssignProcessToJobObject(pid {pid}): {why}"))
    }
}
