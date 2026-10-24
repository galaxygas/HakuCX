/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! GX Control's CPU and RAM limits, enforced by Windows job objects.
//!
//! - The CPU limiter puts the browser process in a job with a hard CPU rate cap. Servo content
//!   processes it launches inherit the job, so the cap covers the whole browser. Throttling is
//!   safe: nothing fails, it just runs slower.
//! - The RAM "hard limit" uses a second job holding only content processes (multi-process
//!   mode), with a job-wide memory cap. If pages exceed it, a content process fails its
//!   allocation and that page crashes, never the browser UI. In single-process mode there is no
//!   separate content process, so the hard limit is not available and the UI says so.
//! - The soft RAM limit (snoozing background tabs) is done by the browser itself.

use haku_core::settings::GxControl;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

static ENFORCED: AtomicBool = AtomicBool::new(false);

pub fn enforced() -> bool {
    ENFORCED.load(Ordering::Relaxed)
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_CPU_RATE_CONTROL_ENABLE,
        JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP, JOB_OBJECT_LIMIT_JOB_MEMORY,
        JOBOBJECT_CPU_RATE_CONTROL_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectCpuRateControlInformation, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    use windows::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
    };
    use windows::core::PCWSTR;

    struct Jobs {
        browser: Option<HANDLE>,
        content: Option<HANDLE>,
        adopted: Vec<u32>,
        /// Windows rejects "rate control off" on a job that never had it on.
        cpu_cap_set: bool,
    }
    // Job handles are process-wide kernel handles; access is serialized by the mutex.
    unsafe impl Send for Jobs {}

    static JOBS: Mutex<Jobs> = Mutex::new(Jobs {
        browser: None,
        content: None,
        adopted: Vec::new(),
        cpu_cap_set: false,
    });

    fn create_job() -> Option<HANDLE> {
        unsafe { CreateJobObjectW(None, PCWSTR::null()) }.ok()
    }

    pub fn apply(control: &GxControl) {
        let mut jobs = JOBS.lock().expect("jobs lock");
        if jobs.browser.is_none() {
            let Some(job) = create_job() else {
                log::warn!("could not create a job object; GX Control limits are not enforced");
                return;
            };
            // SAFETY: valid job and pseudo-handle for this process.
            if let Err(e) = unsafe { AssignProcessToJobObject(job, GetCurrentProcess()) } {
                log::warn!("could not join the CPU job ({e}); GX Control limits are not enforced");
                let _ = unsafe { CloseHandle(job) };
                return;
            }
            jobs.browser = Some(job);
        }
        let job = jobs.browser.expect("set above");
        if control.cpu_limiter || jobs.cpu_cap_set {
            let mut info = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION::default();
            if control.cpu_limiter {
                info.ControlFlags =
                    JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP;
                // CpuRate is in 1/100 of a percent of all processors.
                info.Anonymous.CpuRate = control.cpu_limit.clamp(1, 100) * 100;
            }
            // SAFETY: `info` is a valid JOBOBJECT_CPU_RATE_CONTROL_INFORMATION of the given size.
            let cpu = unsafe {
                SetInformationJobObject(
                    job,
                    JobObjectCpuRateControlInformation,
                    &info as *const _ as *const core::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
                )
            };
            match cpu {
                Ok(()) => jobs.cpu_cap_set = control.cpu_limiter,
                Err(e) => log::warn!("CPU limit not applied: {e}"),
            }
        }
        ENFORCED.store(true, Ordering::Relaxed);

        // Content-process memory job (multi-process mode only).
        if let Some(content) = jobs.content {
            let mut ext = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            if control.ram_limiter && control.ram_hard_limit {
                ext.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_JOB_MEMORY;
                ext.JobMemoryLimit = (control.ram_limit_mb as usize).saturating_mul(1024 * 1024);
            }
            // SAFETY: `ext` is a valid JOBOBJECT_EXTENDED_LIMIT_INFORMATION.
            let _ = unsafe {
                SetInformationJobObject(
                    content,
                    JobObjectExtendedLimitInformation,
                    &ext as *const _ as *const core::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
        }
    }

    /// Put newly started content processes into the content job.
    pub fn adopt_content_processes(pids: &[u32], control: &GxControl) {
        let mut jobs = JOBS.lock().expect("jobs lock");
        let new: Vec<u32> = pids
            .iter()
            .copied()
            .filter(|p| !jobs.adopted.contains(p))
            .collect();
        if new.is_empty() {
            return;
        }
        if jobs.content.is_none() {
            jobs.content = create_job();
        }
        let Some(content) = jobs.content else { return };
        for pid in new {
            // SAFETY: OpenProcess returns an owned handle we close after assignment.
            if let Ok(handle) =
                unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid) }
            {
                if unsafe { AssignProcessToJobObject(content, handle) }.is_ok() {
                    jobs.adopted.push(pid);
                }
                let _ = unsafe { CloseHandle(handle) };
            }
        }
        drop(jobs);
        apply(control);
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub fn apply(_control: &GxControl) {
        ENFORCED.store(false, Ordering::Relaxed);
    }
    pub fn adopt_content_processes(_pids: &[u32], _control: &GxControl) {}
}

pub fn apply(control: &GxControl) {
    imp::apply(control);
}

pub fn adopt_content_processes(pids: &[u32], control: &GxControl) {
    imp::adopt_content_processes(pids, control);
}
