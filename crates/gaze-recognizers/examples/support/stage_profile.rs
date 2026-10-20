use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;
use std::sync::OnceLock;
use std::time::Instant;

use serde::Serialize;

pub enum Stage {
    StartupModelLoad,
    Detection,
    Restore,
}

impl Stage {
    fn name(self) -> &'static str {
        match self {
            Self::StartupModelLoad => "startup_model_load",
            Self::Detection => "detection",
            Self::Restore => "restore",
        }
    }
}

#[derive(Default, Serialize)]
struct Total {
    wall_seconds: f64,
    cpu_seconds: f64,
    samples: u64,
}

thread_local! {
    static TOTALS: RefCell<BTreeMap<&'static str, Total>> = RefCell::new(
        ["startup_model_load", "detection", "restore"].into_iter()
            .map(|name| (name, Total::default())).collect()
    );
}

fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("GAZE_BENCH_PROFILE").as_deref() == Ok("1"))
}

pub struct Clock {
    wall: Instant,
    cpu: f64,
}

pub fn start() -> io::Result<Option<Clock>> {
    if enabled() {
        Ok(Some(Clock {
            wall: Instant::now(),
            cpu: process_cpu_seconds()?,
        }))
    } else {
        Ok(None)
    }
}

pub fn finish(stage: Stage, clock: Option<Clock>) -> io::Result<()> {
    if let Some(clock) = clock {
        let wall_seconds = clock.wall.elapsed().as_secs_f64();
        let cpu_seconds = (process_cpu_seconds()? - clock.cpu).max(0.0);
        TOTALS.with(|totals| {
            let mut totals = totals.borrow_mut();
            let total = totals.get_mut(stage.name()).expect("closed stage set");
            total.wall_seconds += wall_seconds;
            total.cpu_seconds += cpu_seconds;
            total.samples += 1;
        });
    }
    Ok(())
}

pub fn emit() -> Result<(), Box<dyn std::error::Error>> {
    if enabled() {
        let profile = TOTALS.with(|totals| serde_json::to_string(&*totals.borrow()))?;
        eprintln!("GAZE_BENCH_PROFILE {profile}");
    }
    Ok(())
}

#[cfg(unix)]
fn process_cpu_seconds() -> io::Result<f64> {
    let mut stamp = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: stamp is a valid writable timespec and this clock needs no external state.
    let result = unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut stamp) };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(stamp.tv_sec as f64 + stamp.tv_nsec as f64 / 1_000_000_000.0)
}

#[cfg(not(unix))]
fn process_cpu_seconds() -> io::Result<f64> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "native stage profiling requires POSIX",
    ))
}
