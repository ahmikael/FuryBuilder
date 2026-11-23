//! Process-tree containment for the isolated Minecraft server.
//!
//! The official modern `server.jar` is a bundler: it extracts libraries and starts a second
//! Java process. Killing only the immediate child would leave the real server running.
//!
//! On Windows the whole tree is placed in a Job Object with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` and an optional process-memory cap. When the last
//! job handle closes - including an abnormal FuryBuilder exit - Windows terminates every
//! process in the job. On Unix the child starts in its own process group and `SIGKILL` is
//! sent to the whole group.

use std::process::Child;

#[cfg(windows)]
mod platform {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    use std::process::{Child, Command, Stdio};
    use std::ptr;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
    };

    pub struct JobHandle(HANDLE);

    // A Job Object handle is process-wide and may be used from any thread.
    unsafe impl Send for JobHandle {}
    unsafe impl Sync for JobHandle {}

    impl Drop for JobHandle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    pub fn create(memory_limit: Option<u64>) -> Option<JobHandle> {
        unsafe {
            let handle = CreateJobObjectW(ptr::null(), ptr::null());
            if handle.is_null() {
                return None;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if let Some(limit) = memory_limit {
                info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_PROCESS_MEMORY;
                info.ProcessMemoryLimit = limit.min(usize::MAX as u64) as usize;
            }
            let configured = SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if configured == 0 {
                CloseHandle(handle);
                return None;
            }
            Some(JobHandle(handle))
        }
    }

    pub fn assign(job: &JobHandle, child: &Child) -> bool {
        unsafe { AssignProcessToJobObject(job.0, child.as_raw_handle() as HANDLE) != 0 }
    }

    pub fn terminate_job(job: &JobHandle) {
        unsafe {
            TerminateJobObject(job.0, 1);
        }
    }

    pub fn taskkill(child: &Child) {
        let mut command = Command::new("taskkill");
        command
            .arg("/PID")
            .arg(child.id().to_string())
            .arg("/T")
            .arg("/F")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let _ = command.status();
    }
}

/// Owns the OS-level containment for one spawned server tree.
pub(crate) struct ProcessTree {
    #[cfg(windows)]
    job: Option<platform::JobHandle>,
    #[cfg(unix)]
    pgid: Option<i32>,
    guarded: bool,
}

impl ProcessTree {
    pub fn prepare(memory_limit_bytes: Option<u64>) -> Self {
        #[cfg(windows)]
        {
            let job = platform::create(memory_limit_bytes);
            Self {
                guarded: job.is_some(),
                job,
            }
        }
        #[cfg(unix)]
        {
            let _ = memory_limit_bytes;
            Self {
                pgid: None,
                guarded: false,
            }
        }
        #[cfg(not(any(windows, unix)))]
        {
            let _ = memory_limit_bytes;
            Self { guarded: false }
        }
    }

    /// Associates the just-spawned child with the containment. Called immediately after
    /// `spawn`, before the bundler can start its real server process.
    pub fn attach(&mut self, child: &Child) {
        #[cfg(windows)]
        {
            if let Some(job) = self.job.as_ref() {
                if platform::assign(job, child) {
                    self.guarded = true;
                    return;
                }
                self.job = None;
            }
            self.guarded = false;
        }
        #[cfg(unix)]
        {
            self.pgid = Some(child.id() as i32);
            self.guarded = true;
        }
        #[cfg(not(any(windows, unix)))]
        {
            let _ = child;
            self.guarded = false;
        }
    }

    pub fn guarded(&self) -> bool {
        self.guarded
    }

    /// Terminates the whole tree. Safe to call after the immediate child already exited,
    /// because a grandchild may still be alive in the same job/process group.
    pub fn terminate(&self, child: &mut Child) {
        #[cfg(windows)]
        {
            if let Some(job) = self.job.as_ref() {
                platform::terminate_job(job);
            } else {
                platform::taskkill(child);
            }
        }
        #[cfg(unix)]
        {
            if let Some(pgid) = self.pgid {
                unsafe {
                    libc::kill(-pgid, libc::SIGKILL);
                }
            }
        }
        let _ = child.kill();
    }
}
