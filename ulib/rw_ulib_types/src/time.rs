use core::time::Duration;

pub const MAX_TV_NSEC: i64 = 1_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timespec {
    pub tv_sec: i64,  // seconds
    pub tv_nsec: i64, // nanoseconds (0 <= tv_nsec < 1_000_000_000)
}

impl From<Duration> for Timespec {
    fn from(dur: Duration) -> Self {
        Timespec {
            tv_sec: dur.as_secs() as i64,
            tv_nsec: dur.subsec_nanos() as i64,
        }
    }
}

impl TryFrom<Timespec> for Duration {
    type Error = ();
    fn try_from(ts: Timespec) -> Result<Self, Self::Error> {
        if ts.tv_nsec >= 0 && ts.tv_nsec < MAX_TV_NSEC {
            Err(())
        } else {
            let secs = ts.tv_sec.max(0) as u64;
            let nanos = ts.tv_nsec.max(0) as u32;
            Ok(Duration::new(secs, nanos))
        }
    }
}
