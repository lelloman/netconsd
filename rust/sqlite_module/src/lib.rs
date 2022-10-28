use netconsd_module::c_int;
use netconsd_module::format_in6_addr_ptr;
use netconsd_module::in6_addr;
use netconsd_module::str_from_c_void;
use netconsd_module::MsgBuf;
use netconsd_module::NcrxMsg;
use once_cell::sync::OnceCell;
use rusqlite::Connection;
use rusqlite::Statement;
use rusqlite::ToSql;
use std::os::raw::c_void;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc::channel;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;

static SQLITE_WRITER: OnceCell<Mutex<SqliteWriter>> = OnceCell::new();
static mut SENDERS: OnceCell<[Sender<Row>; 256]> = OnceCell::new();

const MAX_THREADS: usize = 256;
const DB_NAME: &str = "netcons.db";
const TABLE_NAME: &str = "messages";
const BATCH_SIZE: usize = 1000;
const RECEIVE_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_WRITE_INTERVAL: Duration = Duration::from_secs(10);

struct SqliteWriter {
    join_handle: Option<JoinHandle<()>>,
    running: Arc<AtomicBool>,
    receiver: Option<Receiver<Row>>,
}

#[derive(Debug)]
struct Row {
    text: String,
    src: String,
    seq: Option<u64>,
    facility: Option<u8>,
    level: Option<u8>,
    oos: Option<u8>,
    seq_reset: Option<u8>,
}

impl Row {
    fn new_legacy(src: *const in6_addr, msg: &MsgBuf) -> Row {
        Row {
            text: format!("{}", str_from_c_void(msg.iovec.iov_base)),
            src: format_in6_addr_ptr(src),
            seq: None,
            facility: None,
            level: None,
            oos: None,
            seq_reset: None,
        }
    }
    fn new_extended(src: *const in6_addr, msg: &NcrxMsg) -> Row {
        Row {
            text: format!("{}", str_from_c_void(msg.text as *const c_void)),
            src: format_in6_addr_ptr(src),
            seq: Some(msg.seq),
            facility: Some(msg.facility),
            level: Some(msg.level),
            oos: Some(if msg.get_oos() { 1 } else { 0 }),
            seq_reset: Some(if msg.get_seq_reset() { 1 } else { 0 }),
        }
    }
}

fn get_sqlite_writer() -> MutexGuard<'static, SqliteWriter> {
    SQLITE_WRITER.get().expect("").lock().expect("")
}

fn get_sender_for_thread(thread: usize) -> &'static Sender<Row> {
    unsafe { &SENDERS.get().expect("Could not get static senders")[thread as usize] }
}

fn initialize_db() -> Connection {
    let connection = Connection::open(DB_NAME).expect("Could not open sqlite db.");
    connection
        .execute(
            &format!(
                "CREATE TABLE IF NOT EXISTS {} (text TEXT, src TEXT, seq INTEGER, facility INTEGER, level INTEGER, oos INTEGER, seq_reset, INTEGER)",
                TABLE_NAME
            ),
            (),
        )
        .expect("Could not create messages table.");
    connection
}

fn make_insert_statement(connection: &Connection, batch_size: usize) -> Statement {
    let mut values = " (?, ?, ?, ?, ?, ?, ?),".repeat(batch_size - 1);
    values += " (?, ?, ?, ?, ?, ?, ?)";
    connection
        .prepare(&format!(
            "INSERT INTO {} (text, src, seq, facility, level, oos, seq_reset) VALUES {}",
            TABLE_NAME, values
        ))
        .expect("Could not prepare insert statement")
}

fn flush_insert_queue(queue: &mut Vec<Row>, statement: &mut Statement) {
    let mut params: Vec<&dyn ToSql> = Vec::with_capacity(queue.len() * 7);
    for row in queue.iter() {
        params.push(&row.text as &dyn ToSql);
        params.push(&row.src as &dyn ToSql);
        params.push(&row.seq as &dyn ToSql);
        params.push(&row.facility as &dyn ToSql);
        params.push(&row.level as &dyn ToSql);
        params.push(&row.oos as &dyn ToSql);
        params.push(&row.seq_reset as &dyn ToSql);
    }
    statement.execute(&*params).unwrap();
    queue.clear();
}

impl SqliteWriter {
    fn new(nr_workers: usize) -> SqliteWriter {
        if nr_workers > MAX_THREADS {
            panic!(
                "{} is the max worker count supported, got {} instead.",
                MAX_THREADS, nr_workers
            );
        }

        let (sender, receiver) = channel::<Row>();
        unsafe {
            SENDERS
                .set([(); MAX_THREADS].map(|_| sender.clone()))
                .expect("Could not set static senders.");
        }
        SqliteWriter {
            join_handle: None,
            running: Arc::new(AtomicBool::new(false)),
            receiver: Some(receiver),
        }
    }

    fn start(&mut self) {
        if self.running.fetch_or(true, Ordering::SeqCst) {
            panic!("Already running!");
        }
        let running = self.running.clone();
        let receiver = self.receiver.take().expect("Could not take receiver.");

        self.join_handle = Some(thread::spawn(move || {
            let connection = initialize_db();
            let mut insert_stmt = make_insert_statement(&connection, BATCH_SIZE);
            let mut queue: Vec<Row> = Vec::with_capacity(BATCH_SIZE);
            let mut last_write = Instant::now();
            while running.load(Ordering::SeqCst) {
                match receiver.recv_timeout(RECEIVE_TIMEOUT) {
                    Ok(row) => queue.push(row),
                    Err(_) => {}
                }

                if queue.len() == BATCH_SIZE {
                    flush_insert_queue(&mut queue, &mut insert_stmt);
                    last_write = Instant::now();
                } else if !queue.is_empty() && last_write.elapsed() >= MAX_WRITE_INTERVAL {
                    let mut stmt = make_insert_statement(&connection, queue.len());
                    flush_insert_queue(&mut queue, &mut stmt);
                    last_write = Instant::now();
                }
            }
            if !queue.is_empty() {
                let mut statement = make_insert_statement(&connection, queue.len());
                flush_insert_queue(&mut queue, &mut statement);
            }
        }));
    }

    fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.join_handle.take() {
            handle.join().expect("Could not join writer thread.");
        }
    }
}

pub fn netconsd_output_init(nr_workers: c_int) -> c_int {
    println!("Rust sqlite module init ({} workers)", nr_workers);
    if let Err(_) = SQLITE_WRITER.set(Mutex::new(SqliteWriter::new(nr_workers as usize))) {
        panic!("Could not set static sqlite writer.");
    }

    get_sqlite_writer().start();
    0
}

pub fn netconsd_output_handler(
    t: c_int,
    src: *const in6_addr,
    buf: *const MsgBuf,
    msg: *const NcrxMsg,
) -> i32 {
    let row = if let Some(buf) = unsafe { buf.as_ref() } {
        Row::new_legacy(src, buf)
    } else if let Some(msg) = unsafe { msg.as_ref() } {
        Row::new_extended(src, msg)
    } else {
        panic!("Both legacy and extended messages were NULL.")
    };
    get_sender_for_thread(t as usize)
        .send(row)
        .expect("Failed to send Row to writer thread.");
    0
}

pub fn netconsd_output_exit() {
    get_sqlite_writer().stop();
    println!("Rust sqlite module unloaded.");
}
