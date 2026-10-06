#![allow(unsafe_code, reason = "Win32 job object calls")]

#[cfg(windows)]
pub fn adopt(pid: u32) {
    use std::sync::OnceLock;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::AssignProcessToJobObject;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

    static JOB: OnceLock<usize> = OnceLock::new();
    let job = *JOB.get_or_init(create);
    if job == 0 {
        return;
    }
    // SAFETY: `job` is a job handle this process created and never closes,
    // and the process handle is closed right after it is assigned.
    unsafe {
        if let Ok(process) = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid) {
            let _ = AssignProcessToJobObject(HANDLE(job as *mut _), process);
            let _ = CloseHandle(process);
        }
    }
}

#[cfg(windows)]
fn create() -> usize {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::JobObjects::{
        CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    // SAFETY: the limit struct outlives the call that copies it, and a job
    // that cannot be set up is closed before reporting none.
    unsafe {
        let Ok(job) = CreateJobObjectW(None, None) else {
            return 0;
        };
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let set = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            std::ptr::from_ref(&info).cast(),
            u32::try_from(std::mem::size_of_val(&info)).unwrap_or(0),
        );
        if set.is_err() {
            let _ = CloseHandle(job);
            return 0;
        }
        job.0 as usize
    }
}

#[cfg(not(windows))]
pub fn adopt(_: u32) {}
