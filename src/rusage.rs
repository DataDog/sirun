use nix::libc::{getrusage, timeval, RUSAGE_CHILDREN};
use std::mem::MaybeUninit;
use std::ops::Sub;

#[derive(Clone, Copy)]
pub(crate) struct Rusage {
    pub(crate) user_time: f64,
    pub(crate) system_time: f64,
    pub(crate) max_res_size: f64,
}

fn ms_from_timeval(tv: timeval) -> f64 {
    let seconds = tv.tv_sec;
    let ms = tv.tv_usec as i64;
    let val = seconds * 1000000 + ms;
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
            user_time: ms_from_timeval(data.ru_utime) as f64,
            system_time: ms_from_timeval(data.ru_stime) as f64,
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
