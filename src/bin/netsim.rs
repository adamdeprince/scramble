//! `netsim`: runs a program on a pseudo-terminal and relays it to the real
//! terminal through a simulated network link: a one-way delay in each
//! direction (so the round trip is twice that) and a bandwidth limit on
//! the program's output. A way to see how the game copes with a slow SSH
//! connection, reproducibly and without a second machine.
//!
//! ```text
//! netsim --delay-ms 40 --kbps 2000 -- silkie-scramble --no-tempfile
//! ```
//!
//! Pass `--no-tempfile` to the game: the program and the terminal still
//! share a filesystem here, which a remote program would not.

use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::{Duration, Instant};

struct Args {
    delay: Duration,
    kbps: Option<f64>,
    command: Vec<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut delay = Duration::from_millis(40);
    let mut kbps = None;
    let mut it = std::env::args().skip(1);
    let mut command = Vec::new();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--delay-ms" => {
                let v: u64 = it.next().and_then(|v| v.parse().ok()).ok_or("--delay-ms needs milliseconds")?;
                delay = Duration::from_millis(v);
            }
            "--kbps" => {
                let v: f64 = it.next().and_then(|v| v.parse().ok()).ok_or("--kbps needs kilobits per second")?;
                kbps = Some(v);
            }
            "--" => {
                command.extend(it.by_ref());
            }
            "-h" | "--help" => return Err(String::new()),
            other => command.push(other.to_string()),
        }
    }
    if command.is_empty() {
        return Err("no command given".into());
    }
    Ok(Args { delay, kbps, command })
}

fn winsize(fd: i32) -> libc::winsize {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    // SAFETY: TIOCGWINSZ writes a winsize into the struct we pass.
    unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) };
    ws
}

/// Bytes a link holds in flight before the sender has to wait, like the
/// buffers of a TCP connection: without it a slow link would swallow
/// everything at once and the program would never feel backpressure.
const IN_FLIGHT: usize = 256 * 1024;

/// A one-directional link: bytes arrive, wait out the delay, then leave no
/// faster than the bandwidth allows.
fn link(mut from: impl Read + Send + 'static, mut to: impl Write + Send + 'static, delay: Duration, kbps: Option<f64>) {
    let (tx, rx) = mpsc::channel::<(Instant, Vec<u8>)>();
    let budget = Arc::new((Mutex::new(0usize), Condvar::new()));
    let reader_budget = budget.clone();
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            match from.read(&mut buf) {
                Ok(0) | Err(_) => return,
                Ok(n) => {
                    let (lock, cv) = &*reader_budget;
                    let mut held = lock.lock().expect("budget lock");
                    while *held > 0 && *held + n > IN_FLIGHT {
                        held = cv.wait(held).expect("budget lock");
                    }
                    *held += n;
                    drop(held);
                    if tx.send((Instant::now() + delay, buf[..n].to_vec())).is_err() {
                        return;
                    }
                }
            }
        }
    });
    std::thread::spawn(move || {
        let mut free_at = Instant::now();
        for (due, bytes) in rx {
            let now = Instant::now();
            if due > now {
                std::thread::sleep(due - now);
            }
            if let Some(k) = kbps {
                // Serialization: the link is busy for len * 8 / rate.
                let start = free_at.max(Instant::now());
                let busy = Duration::from_secs_f64(bytes.len() as f64 * 8.0 / (k * 1000.0));
                free_at = start + busy;
                let now = Instant::now();
                if free_at > now {
                    std::thread::sleep(free_at - now);
                }
            }
            if to.write_all(&bytes).and_then(|_| to.flush()).is_err() {
                return;
            }
            let (lock, cv) = &*budget;
            *lock.lock().expect("budget lock") -= bytes.len();
            cv.notify_all();
        }
    });
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("netsim: {e}");
            }
            eprintln!("usage: netsim [--delay-ms N] [--kbps N] -- command [args...]");
            std::process::exit(2);
        }
    };
    let mut master: i32 = -1;
    let mut slave: i32 = -1;
    let mut ws = winsize(0);
    // SAFETY: openpty fills in two descriptors.
    let r = unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), &mut ws) };
    if r != 0 {
        eprintln!("netsim: openpty failed: {}", std::io::Error::last_os_error());
        std::process::exit(1);
    }
    let slave_fd = slave;
    let mut cmd = Command::new(&args.command[0]);
    cmd.args(&args.command[1..]);
    // SAFETY: the descriptors are valid; each Stdio takes its own dup.
    unsafe {
        cmd.stdin(Stdio::from_raw_fd(libc::dup(slave_fd)))
            .stdout(Stdio::from_raw_fd(libc::dup(slave_fd)))
            .stderr(Stdio::from_raw_fd(libc::dup(slave_fd)))
            .pre_exec(move || {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("netsim: cannot start {}: {e}", args.command[0]);
            std::process::exit(1);
        }
    };
    // SAFETY: the slave belongs to the child now.
    unsafe { libc::close(slave_fd) };
    // SAFETY: master is a valid descriptor we own.
    let master_out = unsafe { OwnedFd::from_raw_fd(master) };
    let master_in = master_out.try_clone().expect("dup master");
    let master_raw = master_out.as_raw_fd();
    if crossterm::terminal::enable_raw_mode().is_err() {
        eprintln!("netsim: stdin is not a terminal");
    }
    link(std::io::stdin(), std::fs::File::from(master_in), args.delay, None);
    link(std::fs::File::from(master_out), std::io::stdout(), args.delay, args.kbps);
    // Pass window size changes on: the kernel then signals the program.
    if let Ok(mut signals) = signal_hook::iterator::Signals::new([libc::SIGWINCH]) {
        std::thread::spawn(move || {
            for _ in signals.forever() {
                let ws = winsize(0);
                // SAFETY: TIOCSWINSZ reads the struct.
                unsafe { libc::ioctl(master_raw, libc::TIOCSWINSZ, &ws) };
            }
        });
    }
    let status = child.wait();
    // Let the last output drain through the delayed link.
    std::thread::sleep(args.delay + Duration::from_millis(200));
    let _ = crossterm::terminal::disable_raw_mode();
    match status {
        Ok(s) => std::process::exit(s.code().unwrap_or(1)),
        Err(_) => std::process::exit(1),
    }
}
