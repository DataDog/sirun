use nix::libc::{getrusage, timeval, RUSAGE_CHILDREN};
use std::mem::MaybeUninit;
use std::ops::Sub;

#[derive(Clone, Copy)]
pub(crate) struct Rusage {
    pub(crate) user_time: f64,
    pub(crate) system_time: f64,
    pub(crate) max_res_size: f64,
}

fn μs_from_timeval(tv: timeval) -> f64 {
    let seconds = tv.tv_sec;
    let μs = tv.tv_usec as i64;
    let val = seconds * 1000000 + μs;
    val as f64
}

impl Rusage {
    pub fn new() -> Rusage {
        let data = unsafe {
            let mut data = MaybeUninit::zeroed().assume_init();
            if getrusage(RUSAGE_CHILDREN, &mut data) == -1 {
                panic!("getrusage is not working correctly");
            }
            data
        };

        Rusage {
            user_time: μs_from_timeval(data.ru_utime) as f64,
            system_time: μs_from_timeval(data.ru_stime) as f64,
            max_res_size: data.ru_maxrss as f64,
        }
    }
}

impl Sub for Rusage {
    type Output = Self;

    fn sub(self, other: Self) -> Self::Output {
        Self {
            user_time: self.user_time - other.user_time,
            system_time: self.system_time - other.system_time,
            max_res_size: self.max_res_size - other.max_res_size,
        }
    }
}

/// Returns the CPU time (user_µs, system_µs) of a running process by reading
/// `/proc/{pid}/stat`. Returns `None` if the file cannot be read or parsed.
#[cfg(target_os = "linux")]
pub(crate) fn read_child_cpu_us(pid: u32) -> Option<(f64, f64)> {
    let contents = std::fs::read_to_string(format!("/proc/{}/stat", pid)).ok()?;
    // The second field (process name) is wrapped in parentheses and may contain
    // spaces. Find the last ')' to reliably skip past it.
    let after_name = contents.rfind(')')? + 1;
    let fields: Vec<&str> = contents[after_name..].split_whitespace().collect();
    // After ')': state(0) ppid(1) pgrp(2) session(3) tty_nr(4) tpgid(5)
    // flags(6) minflt(7) cminflt(8) majflt(9) cmajflt(10) utime(11) stime(12)
    let utime_jiffies: u64 = fields.get(11)?.parse().ok()?;
    let stime_jiffies: u64 = fields.get(12)?.parse().ok()?;
    let ticks_raw = unsafe { nix::libc::sysconf(nix::libc::_SC_CLK_TCK) };
    if ticks_raw <= 0 {
        return None;
    }
    let ticks = ticks_raw as f64;
    Some((
        utime_jiffies as f64 * 1_000_000.0 / ticks,
        stime_jiffies as f64 * 1_000_000.0 / ticks,
    ))
}

/// Returns the CPU time (user_µs, system_µs) of a running process via
/// `proc_pidinfo(PROC_PIDTASKINFO)`. Returns `None` on failure.
#[cfg(target_os = "macos")]
pub(crate) fn read_child_cpu_us(pid: u32) -> Option<(f64, f64)> {
    const PROC_PIDTASKINFO: nix::libc::c_int = 4;

    #[repr(C)]
    struct ProcTaskInfo {
        pti_virtual_size: u64,
        pti_resident_size: u64,
        pti_total_user: u64,   // nanoseconds — XNU bsd/kern/proc_info.c fill_taskprocinfo()
        pti_total_system: u64, // nanoseconds — XNU bsd/kern/proc_info.c fill_taskprocinfo()
        pti_threads_user: u64,
        pti_threads_system: u64,
        pti_policy: i32,
        pti_faults: i32,
        pti_pageins: i32,
        pti_cow_faults: i32,
        pti_messages_sent: i32,
        pti_messages_received: i32,
        pti_syscalls_mach: i32,
        pti_syscalls_unix: i32,
        pti_csw: i32,
        pti_threadnum: i32,
        pti_numrunning: i32,
        pti_priority: i32,
    }

    extern "C" {
        fn proc_pidinfo(
            pid: nix::libc::c_int,
            flavor: nix::libc::c_int,
            arg: u64,
            buffer: *mut nix::libc::c_void,
            buffersize: nix::libc::c_int,
        ) -> nix::libc::c_int;
    }

    let mut info = std::mem::MaybeUninit::<ProcTaskInfo>::zeroed();
    let ret = unsafe {
        proc_pidinfo(
            pid as nix::libc::c_int,
            PROC_PIDTASKINFO,
            0,
            info.as_mut_ptr() as *mut nix::libc::c_void,
            std::mem::size_of::<ProcTaskInfo>() as nix::libc::c_int,
        )
    };
    if ret < std::mem::size_of::<ProcTaskInfo>() as nix::libc::c_int {
        return None;
    }
    let info = unsafe { info.assume_init() };
    // Convert nanoseconds to microseconds.
    Some((
        info.pti_total_user as f64 / 1_000.0,
        info.pti_total_system as f64 / 1_000.0,
    ))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(crate) fn read_child_cpu_us(_pid: u32) -> Option<(f64, f64)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_child_cpu_us_returns_some_for_current_process() {
        let pid = std::process::id();
        let result = read_child_cpu_us(pid);
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        assert!(result.is_some(), "expected CPU reading for pid {}", pid);
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let _ = result;
    }

    #[test]
    fn read_child_cpu_us_values_are_non_negative() {
        let pid = std::process::id();
        if let Some((utime, stime)) = read_child_cpu_us(pid) {
            assert!(utime >= 0.0);
            assert!(stime >= 0.0);
        }
    }
}
