use netconsd_module::in6_addr;
use netconsd_module::MsgBuf;
use netconsd_module::NcrxMsg;
use once_cell::sync::OnceCell;
use std::collections::HashSet;
use std::ptr;
use std::time::Duration;
use std::time::Instant;

#[derive(Clone, Debug, Default)]
struct ThreadStats {
    pub msg_count: u64,
    pub extended_count: u64,
    pub legacy_count: u64,
    pub hosts: HashSet<[u8; 16]>,
}

pub struct Stats {
    pub msg_count: u64,
    pub extended_count: u64,
    pub legacy_count: u64,
    pub hosts_count: u64,
    pub uptime: Duration,
}

#[derive(Debug)]
pub struct StatsHolder {
    thread_stats: [ThreadStats; MAX_THREADS],
    nr_workers: usize,
    start_time: Instant,
}

impl StatsHolder {
    fn new(nr_workers: usize) -> StatsHolder {
        if nr_workers > MAX_THREADS {
            panic!(
                "{} is the max worker count supported, got {} instead.",
                MAX_THREADS, nr_workers
            );
        }

        StatsHolder {
            thread_stats: [(); MAX_THREADS].map(|_| ThreadStats::default()),
            nr_workers,
            start_time: Instant::now(),
        }
    }

    pub fn on_new_message(
        &mut self,
        thread: usize,
        src: *const in6_addr,
        buf: *const MsgBuf,
        msg: *const NcrxMsg,
    ) {
        let mut stats = &mut self.thread_stats[thread];
        stats.msg_count += 1;
        if buf != ptr::null() {
            stats.legacy_count += 1;
        }
        if msg != ptr::null() {
            stats.extended_count += 1;
        }
        if let Some(src) = unsafe { src.as_ref() } {
            stats.hosts.insert(src.s6_addr);
        }
    }

    pub fn compute_stats(&self) -> Stats {
        let mut stats = Stats {
            msg_count: 0,
            extended_count: 0,
            legacy_count: 0,
            hosts_count: 0,
            uptime: self.start_time.elapsed(),
        };
        for stat in self.thread_stats.iter().take(self.nr_workers) {
            stats.msg_count += stat.msg_count;
            stats.extended_count += stat.extended_count;
            stats.legacy_count += stat.legacy_count;
            stats.hosts_count += stat.hosts.len() as u64;
        }

        stats
    }
}

pub const MAX_THREADS: usize = 256;
static mut STATS_HOLDER: OnceCell<StatsHolder> = OnceCell::new();

pub fn get_stats_holder() -> &'static mut StatsHolder {
    unsafe {
        STATS_HOLDER
            .get_mut()
            .expect("Could not get StatsHolder instance.")
    }
}

pub fn init_stats_holder(nr_workers: usize) {
    unsafe {
        STATS_HOLDER
            .set(StatsHolder::new(nr_workers))
            .expect("Could not initialize StatsHolder.");
    }
}
