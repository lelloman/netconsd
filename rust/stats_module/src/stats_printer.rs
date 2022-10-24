use crate::stats_holder::get_stats_holder;
use crate::stats_holder::Stats;
use once_cell::sync::OnceCell;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;
use termion::clear;
use termion::cursor;

static RUNNING: AtomicBool = AtomicBool::new(false);
static mut STATS_PRINTER_DATA: OnceCell<StatsPrinterData> = OnceCell::new();

#[derive(Debug)]
struct StatsPrinterData {
    pub join_handle: Option<JoinHandle<()>>,
    pub last_msg_count_time: u128,
    pub latest_msg_counts: [u64; 2],
}

fn get_stats_printer_data() -> &'static mut StatsPrinterData {
    unsafe {
        STATS_PRINTER_DATA
            .get_mut()
            .expect("Could not get StatsPrinterData")
    }
}

fn get_current_time_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Could not get current time.")
        .as_millis()
}

fn format_uptime(duration: &Duration) -> String {
    let hours = duration.as_secs() / 3600;
    let minutes = (duration.as_secs() / 60) % 60;
    let seconds = duration.as_secs() % 60;
    format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
}

fn get_messages_per_sec(stats: &Stats) -> u64 {
    let mut printer_data = get_stats_printer_data();
    let now = get_current_time_millis();
    let prev_counts = &mut printer_data.latest_msg_counts;
    if printer_data.last_msg_count_time + 1000 < now {
        prev_counts[1] = prev_counts[0];
        prev_counts[0] = stats.msg_count;
        printer_data.last_msg_count_time += 1000;
    }
    let elapsed = now - printer_data.last_msg_count_time;
    let prev_factor = (1000 - elapsed) as f64 / 1000.0;

    let diff_current = stats.msg_count - prev_counts[0];
    let diff_prev = (prev_counts[0] - prev_counts[1]) as f64 * prev_factor;
    diff_current + diff_prev as u64
}

fn print_stats() {
    let stats = get_stats_holder().compute_stats();
    println!("{}{}", clear::All, cursor::Goto(1, 1));
    println!("Netconsd stats - {}\n", format_uptime(&stats.uptime));
    println!("Tot messages:\t{}", stats.msg_count);
    println!("Extended:\t{}", stats.extended_count);
    println!("Legacy:\t\t{}", stats.legacy_count);
    println!("Hosts:\t\t{}", stats.hosts_count);
    println!("Ingestion:\t{} msg/sec", get_messages_per_sec(&stats));
}

pub fn init_stats_printer() {
    RUNNING.store(true, Ordering::SeqCst);
    let join_handle = thread::spawn(|| {
        while RUNNING.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(300));
            print_stats();
        }
    });
    unsafe {
        STATS_PRINTER_DATA
            .set(StatsPrinterData {
                join_handle: Some(join_handle),
                last_msg_count_time: get_current_time_millis(),
                latest_msg_counts: [0u64; 2],
            })
            .expect("Could not set stats printer data.");
    }
    print!("{}", cursor::Hide);
}

pub fn exit_stats_printer() {
    RUNNING.store(false, Ordering::SeqCst);
    if let Some(join_handle) = get_stats_printer_data().join_handle.take() {
        join_handle.join().expect("Could not join printer.");
    }
    print!("{}", cursor::Show);
    print_stats();
}
