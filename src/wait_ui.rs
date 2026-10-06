use j_jump::{Error, Result};
use std::{
    fs::File,
    io::{Read, Write},
    os::fd::AsRawFd,
    sync::mpsc::{Receiver, TryRecvError},
    time::{Duration, Instant},
};

const LOADING_AFTER: Duration = Duration::from_millis(150);
const CHOICE_AFTER: Duration = Duration::from_millis(900);
const POLL_SLICE: Duration = Duration::from_millis(40);

struct InputMode {
    fd: libc::c_int,
    original: libc::termios,
    original_flags: libc::c_int,
}

impl InputMode {
    fn one_key(tty: &File) -> Result<Self> {
        let fd = tty.as_raw_fd();
        let mut mode = std::mem::MaybeUninit::<libc::termios>::uninit();
        if unsafe { libc::tcgetattr(fd, mode.as_mut_ptr()) } != 0 {
            return Err(Error(130, "terminal settings unavailable".into()));
        }
        let original = unsafe { mode.assume_init() };
        let original_flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if original_flags < 0 {
            return Err(Error(130, "terminal choice setup failed".into()));
        }
        let mut input = original;
        input.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ISIG);
        input.c_cc[libc::VMIN] = 1;
        input.c_cc[libc::VTIME] = 0;
        if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &input) } != 0 {
            return Err(Error(130, "terminal choice setup failed".into()));
        }
        if unsafe { libc::fcntl(fd, libc::F_SETFL, original_flags | libc::O_NONBLOCK) } != 0 {
            unsafe { libc::tcsetattr(fd, libc::TCSANOW, &original) };
            return Err(Error(130, "terminal choice setup failed".into()));
        }
        Ok(Self {
            fd,
            original,
            original_flags,
        })
    }
}

impl Drop for InputMode {
    fn drop(&mut self) {
        unsafe {
            // A choice typed at the same instant as a provider reply must not become
            // an accidental answer to the following directory picker.
            libc::tcflush(self.fd, libc::TCIFLUSH);
            libc::fcntl(self.fd, libc::F_SETFL, self.original_flags);
            libc::tcsetattr(self.fd, libc::TCSANOW, &self.original);
        }
    }
}

fn line_end(tty: &mut File) -> Result<()> {
    tty.write_all(b"\r\n")
        .and_then(|_| tty.flush())
        .map_err(|_| Error(130, "terminal closed".into()))
}

/// A response is consumed only while this invocation still wants it. Dropping the receiver
/// after a local choice cannot make a late provider result into a directory selection.
#[cfg(test)]
pub fn wait_for_jev<T>(
    tty: File,
    rx: Receiver<Result<T>>,
    deadline: Instant,
) -> Result<Option<Result<T>>> {
    wait_for_provider(tty, rx, deadline, "Jev")
}
pub fn wait_for_provider<T>(
    mut tty: File,
    rx: Receiver<Result<T>>,
    deadline: Instant,
    provider: &str,
) -> Result<Option<Result<T>>> {
    let started = Instant::now();
    let mut loading = false;
    let mut choice = false;
    let mut continued = false;
    let _input_mode = InputMode::one_key(&tty)?;
    loop {
        let now = Instant::now();
        if now >= deadline {
            if choice {
                write!(tty, "\r\n{provider} deadline reached. No directory selected; use jjump --offline query --interactive.\r\n")
                    .map_err(|_| Error(130, "terminal closed".into()))?;
            }
            return Err(Error(
                5,
                format!("{provider} deadline reached; use jjump --offline query --interactive")
                    .into(),
            ));
        }
        match rx.try_recv() {
            Ok(result) => {
                if choice {
                    line_end(&mut tty)?;
                }
                return Ok(Some(result));
            }
            Err(TryRecvError::Disconnected) => {
                if choice {
                    line_end(&mut tty)?;
                }
                return Ok(Some(Err(Error(
                    5,
                    format!("{provider} worker stopped; browse locally: jjump --offline query --interactive").into(),
                ))));
            }
            Err(TryRecvError::Empty) => {}
        }
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(started);
        if !loading && elapsed >= LOADING_AFTER {
            write!(tty, "Checking with {provider}...\r\n")
                .and_then(|_| tty.flush())
                .map_err(|_| Error(130, "terminal closed".into()))?;
            loading = true;
        }
        if loading && !choice && elapsed >= CHOICE_AFTER {
            write!(
                tty,
                "{provider}'s taking the scenic route. Use local picks now? [Enter=yes / W=wait] "
            )
            .and_then(|_| tty.flush())
            .map_err(|_| Error(130, "terminal closed".into()))?;
            choice = true;
        }
        {
            let mut byte = [0];
            match tty.read(&mut byte) {
                Ok(0) => {
                    // macOS may return zero for a nonblocking /dev/tty with no key.
                    let mut mode = std::mem::MaybeUninit::<libc::termios>::uninit();
                    if unsafe { libc::tcgetattr(tty.as_raw_fd(), mode.as_mut_ptr()) } != 0 {
                        return Err(Error(130, "terminal choice cancelled".into()));
                    }
                }
                Ok(_) => match byte[0] {
                    b'\r' | b'\n' if choice => {
                        tty.write_all(b"\r\nUsing local picks.\r\n")
                            .map_err(|_| Error(130, "terminal closed".into()))?;
                        return Ok(None);
                    }
                    b'w' | b'W' if choice && !continued => {
                        tty.write_all(b"\r\nOkay, a little longer... [Enter=local] ")
                            .and_then(|_| tty.flush())
                            .map_err(|_| Error(130, "terminal closed".into()))?;
                        continued = true;
                    }
                    3 | 4 | 27 => return Err(Error(130, "selection cancelled".into())),
                    _ => {}
                },
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(Error(130, "terminal choice cancelled".into())),
            }
            let wait = deadline
                .saturating_duration_since(Instant::now())
                .min(POLL_SLICE);
            if !wait.is_zero() {
                std::thread::sleep(wait);
            }
        }
        if !choice {
            let next = if !loading {
                started + LOADING_AFTER
            } else {
                started + CHOICE_AFTER
            };
            let wait = next
                .min(deadline)
                .saturating_duration_since(Instant::now())
                .min(POLL_SLICE);
            if !wait.is_zero() {
                std::thread::sleep(wait);
            }
        }
        // The one-key guard restores the tty on every exit, including early cancellation.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::fd::FromRawFd, sync::mpsc, thread};

    fn pty_pair() -> (File, File) {
        let (mut master, mut slave) = (0, 0);
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            },
            0
        );
        unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) }
    }

    fn until(master: &mut File, needle: &[u8]) -> Vec<u8> {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut out = Vec::new();
        while Instant::now() < deadline {
            let mut p = libc::pollfd {
                fd: master.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            if unsafe { libc::poll(&mut p, 1, 30) } > 0 {
                let mut b = [0; 512];
                let n = master.read(&mut b).unwrap();
                out.extend_from_slice(&b[..n]);
                if out.windows(needle.len()).any(|w| w == needle) {
                    return out;
                }
            }
        }
        panic!(
            "terminal text did not appear: {}",
            String::from_utf8_lossy(&out)
        );
    }

    #[test]
    fn fast_result_does_not_flash() {
        let (master, slave) = pty_pair();
        let (tx, rx) = mpsc::channel();
        tx.send(Ok::<_, Error>(42)).unwrap();
        assert_eq!(
            wait_for_jev(slave, rx, Instant::now() + Duration::from_secs(1))
                .unwrap()
                .unwrap()
                .unwrap(),
            42
        );
        let mut p = libc::pollfd {
            fd: master.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let _ = unsafe { libc::poll(&mut p, 1, 20) };
        if p.revents & libc::POLLIN != 0 {
            let mut b = [0; 128];
            let n = (&master).read(&mut b).unwrap();
            assert!(n == 0, "unexpected fast-path output: {:?}", &b[..n]);
        }
    }

    #[test]
    fn already_expired_deadline_rejects_even_a_queued_result() {
        let (_master, slave) = pty_pair();
        let (tx, rx) = mpsc::channel();
        tx.send(Ok::<_, Error>(42)).unwrap();
        assert_eq!(wait_for_jev(slave, rx, Instant::now()).unwrap_err().0, 5);
    }

    #[test]
    fn enter_uses_local_and_drops_late_result() {
        let (mut master, slave) = pty_pair();
        let (tx, rx) = mpsc::channel::<Result<i32>>();
        let started = Instant::now();
        let task = thread::spawn(move || {
            wait_for_jev(slave, rx, Instant::now() + Duration::from_secs(10))
        });
        let shown = until(&mut master, b"[Enter=yes / W=wait]");
        assert!(
            shown
                .windows(b"Checking with Jev...".len())
                .any(|w| w == b"Checking with Jev...")
        );
        master.write_all(b"\r").unwrap();
        assert!(task.join().unwrap().unwrap().is_none());
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(tx.send(Ok(42)).is_err());
    }

    #[test]
    fn w_waits_for_result_and_escape_cancels() {
        let (mut master, slave) = pty_pair();
        let (tx, rx) = mpsc::channel::<Result<i32>>();
        let task =
            thread::spawn(move || wait_for_jev(slave, rx, Instant::now() + Duration::from_secs(2)));
        until(&mut master, b"[Enter=yes / W=wait]");
        master.write_all(b"W").unwrap();
        until(&mut master, b"a little longer");
        tx.send(Ok(7)).unwrap();
        assert_eq!(task.join().unwrap().unwrap().unwrap().unwrap(), 7);

        let (mut master, slave) = pty_pair();
        let (_tx, rx) = mpsc::channel::<Result<i32>>();
        let task =
            thread::spawn(move || wait_for_jev(slave, rx, Instant::now() + Duration::from_secs(2)));
        until(&mut master, b"[Enter=yes / W=wait]");
        master.write_all(b"\x1b").unwrap();
        assert_eq!(task.join().unwrap().unwrap_err().0, 130);
    }

    #[test]
    fn w_without_result_reaches_deadline() {
        let (mut master, slave) = pty_pair();
        let (_tx, rx) = mpsc::channel::<Result<i32>>();
        let task = thread::spawn(move || {
            wait_for_jev(slave, rx, Instant::now() + Duration::from_millis(1350))
        });
        until(&mut master, b"[Enter=yes / W=wait]");
        master.write_all(b"W").unwrap();
        until(&mut master, b"a little longer");
        until(&mut master, b"Jev deadline reached.");
        assert_eq!(task.join().unwrap().unwrap_err().0, 5);
    }
}
