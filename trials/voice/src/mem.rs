//! The most memory this process has held, as the operating system counts it.

/// Peak resident memory in bytes: the peak working set on Windows, `VmHWM`
/// on Linux and Android. `None` where neither is available.
pub fn peak_bytes() -> Option<u64> {
    imp::peak_bytes()
}

/// Asks the operating system to run this process ahead of ordinary ones,
/// so that other work on the machine disturbs a measurement less, and not
/// to put the computer to sleep while this process is running (a sleep in
/// the middle of a run ruined one). Both end with the process. Does nothing
/// where it is not Windows.
pub fn run_ahead_of_other_work() {
    imp::run_ahead_of_other_work();
}

#[cfg(windows)]
mod imp {
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(process: isize, counters: *mut Counters, cb: u32) -> i32;
        fn SetPriorityClass(process: isize, class: u32) -> i32;
        fn SetThreadExecutionState(flags: u32) -> u32;
    }

    pub fn run_ahead_of_other_work() {
        const HIGH_PRIORITY_CLASS: u32 = 0x80;
        const ES_CONTINUOUS: u32 = 0x8000_0000;
        const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
        // SAFETY: calls on this process's own handle and thread with documented constants.
        unsafe {
            SetPriorityClass(GetCurrentProcess(), HIGH_PRIORITY_CLASS);
            SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED);
        }
    }

    pub fn peak_bytes() -> Option<u64> {
        let mut counters = Counters::default();
        let size = std::mem::size_of::<Counters>() as u32;
        counters.cb = size;
        // SAFETY: the structure is the one the call documents, and its size is passed.
        let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, size) };
        (ok != 0).then_some(counters.peak_working_set_size as u64)
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn run_ahead_of_other_work() {}

    pub fn peak_bytes() -> Option<u64> {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        let line = status.lines().find(|l| l.starts_with("VmHWM:"))?;
        let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
        Some(kb * 1024)
    }
}
