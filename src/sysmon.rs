//! The machine's resources, read once a second by a thread of their own while the panel shows: CPU,
//! GPU, memory, and each disk (space, and what is read and written).
//!
//! The GPU: on Windows the system's performance counters (the ones the task manager shows), on Linux
//! what the driver tells in sysfs (AMD, Intel); unknown elsewhere.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sysinfo::{Disks, System};

/// How often the thread reads.
const TICK: Duration = Duration::from_secs(1);
/// Readings kept for the curves.
pub const HISTORY: usize = 60;

#[derive(Clone, Default)]
pub struct Snapshot {
    /// Percents, the latest last.
    pub cpu: VecDeque<f32>,
    pub gpu: VecDeque<f32>,
    /// Whether the GPU's load could be read at all.
    pub gpu_known: bool,
    pub ram_used: u64,
    pub ram_total: u64,
    pub disks: Vec<DiskInfo>,
}

#[derive(Clone)]
pub struct DiskInfo {
    /// Where it is mounted ("C:\", "/home"...).
    pub mount: String,
    pub used: u64,
    pub total: u64,
    /// Bytes a second.
    pub read: u64,
    pub written: u64,
}

/// The reading thread: it stops when this is dropped.
pub struct Monitor {
    snapshot: Arc<Mutex<Snapshot>>,
    stop: Arc<AtomicBool>,
}

impl Monitor {
    pub fn start(ctx: &egui::Context) -> Self {
        let snapshot = Arc::new(Mutex::new(Snapshot::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (shared, stopped, ctx) = (snapshot.clone(), stop.clone(), ctx.clone());
        let _ = std::thread::Builder::new().name("sysmon".into()).spawn(move || run(&shared, &stopped, &ctx));
        Self { snapshot, stop }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.snapshot.lock().unwrap().clone()
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn run(shared: &Mutex<Snapshot>, stop: &AtomicBool, ctx: &egui::Context) {
    let mut system = System::new();
    let mut disks = Disks::new_with_refreshed_list();
    let mut gpu = gpu::Reader::new();
    system.refresh_cpu_usage();
    let mut last = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(TICK);
        let elapsed = last.elapsed().as_secs_f64().max(0.001);
        last = Instant::now();
        system.refresh_cpu_usage();
        system.refresh_memory();
        // Drives plugged in or out show up.
        disks.refresh(true);
        let load = gpu.read();
        let mut list: Vec<DiskInfo> = disks
            .list()
            .iter()
            .filter(|d| d.total_space() > 0)
            .map(|d| {
                let usage = d.usage();
                DiskInfo {
                    mount: d.mount_point().display().to_string(),
                    used: d.total_space().saturating_sub(d.available_space()),
                    total: d.total_space(),
                    read: (usage.read_bytes as f64 / elapsed) as u64,
                    written: (usage.written_bytes as f64 / elapsed) as u64,
                }
            })
            .collect();
        list.sort_by(|a, b| a.mount.cmp(&b.mount));
        list.dedup_by(|a, b| a.mount == b.mount);
        {
            let mut s = shared.lock().unwrap();
            push(&mut s.cpu, system.global_cpu_usage());
            s.gpu_known = load.is_some();
            push(&mut s.gpu, load.unwrap_or(0.0));
            s.ram_used = system.used_memory();
            s.ram_total = system.total_memory();
            s.disks = list;
        }
        ctx.request_repaint();
    }
}

fn push(history: &mut VecDeque<f32>, value: f32) {
    if history.len() == HISTORY {
        history.pop_front();
    }
    history.push_back(value.clamp(0.0, 100.0));
}

#[cfg(windows)]
mod gpu {
    use std::collections::HashMap;

    use windows_sys::Win32::System::Performance::{PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA};

    /// The load of the GPU's engines, by process; the busiest engine is the GPU's load, as in the task
    /// manager.
    pub struct Reader {
        query: PDH_HQUERY,
        counter: PDH_HCOUNTER,
    }

    // The handles are only used from the thread that owns the reader.
    unsafe impl Send for Reader {}

    impl Reader {
        pub fn new() -> Self {
            let mut query = std::ptr::null_mut();
            let mut counter = std::ptr::null_mut();
            let path: Vec<u16> = "\\GPU Engine(*)\\Utilization Percentage\0".encode_utf16().collect();
            unsafe {
                if PdhOpenQueryW(std::ptr::null(), 0, &mut query) != 0 {
                    return Self { query: std::ptr::null_mut(), counter };
                }
                if PdhAddEnglishCounterW(query, path.as_ptr(), 0, &mut counter) != 0 {
                    counter = std::ptr::null_mut();
                }
                // A rate needs two readings: the first one here.
                PdhCollectQueryData(query);
            }
            Self { query, counter }
        }

        pub fn read(&mut self) -> Option<f32> {
            if self.query.is_null() || self.counter.is_null() {
                return None;
            }
            unsafe {
                if PdhCollectQueryData(self.query) != 0 {
                    return None;
                }
                let (mut size, mut count) = (0u32, 0u32);
                if PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut size, &mut count, std::ptr::null_mut()) != PDH_MORE_DATA {
                    return None;
                }
                // Items, then the names they point to, in one buffer.
                let item = std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
                let mut buffer = vec![PDH_FMT_COUNTERVALUE_ITEM_W::default(); (size as usize).div_ceil(item)];
                if PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut size, &mut count, buffer.as_mut_ptr()) != 0 {
                    return None;
                }
                // "pid_1234_luid_0x0_0x1_phys_0_eng_3_engtype_3D": the processes of each engine summed.
                let mut engines: HashMap<String, f64> = HashMap::new();
                for it in &buffer[..count as usize] {
                    if it.FmtValue.CStatus != 0 || it.szName.is_null() {
                        continue;
                    }
                    let len = (0..).take_while(|&k| *it.szName.add(k) != 0).count();
                    let name = String::from_utf16_lossy(std::slice::from_raw_parts(it.szName, len));
                    let engine = name.find("luid_").map_or(name.as_str(), |k| &name[k..]).to_owned();
                    *engines.entry(engine).or_default() += it.FmtValue.Anonymous.doubleValue;
                }
                Some(engines.values().copied().fold(0.0, f64::max) as f32)
            }
        }
    }

    impl Drop for Reader {
        fn drop(&mut self) {
            if !self.query.is_null() {
                unsafe { PdhCloseQuery(self.query) };
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod gpu {
    use std::path::PathBuf;

    /// The busiest card that tells its load (`gpu_busy_percent`: AMD, some Intel).
    pub struct Reader {
        files: Vec<PathBuf>,
    }

    impl Reader {
        pub fn new() -> Self {
            let files = std::fs::read_dir("/sys/class/drm")
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path().join("device/gpu_busy_percent"))
                .filter(|p| p.exists())
                .collect();
            Self { files }
        }

        pub fn read(&mut self) -> Option<f32> {
            self.files.iter().filter_map(|p| std::fs::read_to_string(p).ok()?.trim().parse::<f32>().ok()).reduce(f32::max)
        }
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod gpu {
    pub struct Reader;

    impl Reader {
        pub fn new() -> Self {
            Self
        }

        pub fn read(&mut self) -> Option<f32> {
            None
        }
    }
}
