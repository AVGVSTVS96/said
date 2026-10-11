use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, bounded};
use said_core::Result;

const MAX_REPLY: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlCommand {
    Hotkey,
    HotkeyInteractive,
    HotkeyInsertLast,
    Toggle,
    Interactive,
    InsertLast,
    Cancel,
    Status,
    Start,
    Stop,
    HistorySearch { query: String },
}

impl ControlCommand {
    pub fn parse(command: &str) -> Result<Self> {
        match command {
            "hotkey" => Ok(Self::Hotkey),
            "hotkey interactive" => Ok(Self::HotkeyInteractive),
            "hotkey insert-last" => Ok(Self::HotkeyInsertLast),
            "toggle" => Ok(Self::Toggle),
            "interactive" => Ok(Self::Interactive),
            "insert-last" => Ok(Self::InsertLast),
            "cancel" => Ok(Self::Cancel),
            "status" => Ok(Self::Status),
            "start" => Ok(Self::Start),
            "stop" => Ok(Self::Stop),
            "history" => Ok(Self::HistorySearch { query: String::new() }),
            value if value.starts_with("history ") => Ok(Self::HistorySearch { query: value[8..].into() }),
            _ => Err(
                "expected hotkey [interactive|insert-last], toggle, start, stop, interactive, insert-last, cancel, status or history <FTS query>"
                    .into(),
            ),
        }
    }
}

pub struct ControlRequest {
    pub command: ControlCommand,
    pub reply: Sender<std::result::Result<String, String>>,
}

pub struct ControlSocket {
    path: PathBuf,
    stop: UnixStream,
    thread: Option<JoinHandle<()>>,
    _lock: File,
    requests: Receiver<ControlRequest>,
}

pub fn socket_path(runtime: &Path, storage_key: &str) -> Result<PathBuf> {
    if storage_key.is_empty()
        || !storage_key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
    {
        return Err("storage key must be a filename component".into());
    }
    Ok(runtime.join(format!("{storage_key}.sock")))
}

impl ControlSocket {
    pub fn bind(runtime: &Path, storage_key: &str) -> Result<Self> {
        let metadata = fs::metadata(runtime)?;
        // The runtime directory, not a process-wide umask, closes bind-to-chmod races.
        if !metadata.is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o077 != 0
        {
            return Err(
                "XDG_RUNTIME_DIR must be an owner-only directory owned by this user".into(),
            );
        }
        let path = socket_path(runtime, storage_key)?;
        let lock_path = path.with_extension("sock.lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(lock_path)?;
        if lock.metadata()?.uid() != unsafe { libc::geteuid() } {
            return Err("control lock belongs to a different user".into());
        }
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err("said control socket is already running".into());
        }
        match fs::symlink_metadata(&path) {
            Ok(metadata)
                if metadata.file_type().is_socket()
                    && metadata.uid() == unsafe { libc::geteuid() } =>
            {
                fs::remove_file(&path)?
            }
            Ok(_) => return Err("refusing to replace a non-socket control path".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let listener = UnixListener::bind(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        let (stop, stopped) = UnixStream::pair()?;
        let (sender, requests) = bounded(32);
        let thread = thread::spawn(move || {
            let mut descriptors = [
                libc::pollfd {
                    fd: listener.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: stopped.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            loop {
                if unsafe { libc::poll(descriptors.as_mut_ptr(), 2, -1) } < 0 {
                    if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                        continue;
                    }
                    break;
                }
                if descriptors[1].revents != 0 {
                    break;
                }
                if descriptors[0].revents & libc::POLLIN != 0 {
                    let Ok((mut stream, _)) = listener.accept() else {
                        break;
                    };
                    let response = serve(&mut stream, &sender);
                    let message = match response {
                        Ok(message) if message.len() <= MAX_REPLY - 4 => format!("ok {message}\n"),
                        Ok(_) => {
                            "error control reply exceeds 16 MiB; narrow the history query\n".into()
                        }
                        Err(error) => format!("error {error}\n"),
                    };
                    let _ = stream.write_all(message.as_bytes());
                }
            }
        });
        Ok(Self {
            path,
            stop,
            thread: Some(thread),
            _lock: lock,
            requests,
        })
    }

    pub fn requests(&self) -> Receiver<ControlRequest> {
        self.requests.clone()
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ControlSocket {
    fn drop(&mut self) {
        let _ = self.stop.write_all(&[1]);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = fs::remove_file(&self.path);
    }
}

fn serve(stream: &mut UnixStream, sender: &Sender<ControlRequest>) -> Result<String> {
    stream.set_read_timeout(Some(Duration::from_secs(1)))?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of_val(&credentials) as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    } != 0
        || credentials.uid != unsafe { libc::geteuid() }
    {
        return Err("control peer must be the same user".into());
    }
    let mut line = String::new();
    BufReader::new(stream).take(128).read_line(&mut line)?;
    let command = ControlCommand::parse(
        line.strip_suffix('\n')
            .ok_or("unterminated control command")?,
    )?;
    let (reply, response) = bounded(1);
    sender.try_send(ControlRequest { command, reply })?;
    response
        .recv_timeout(Duration::from_secs(3))?
        .map_err(Into::into)
}

pub fn run_cli(runtime: &Path, storage_key: &str, arguments: &[String]) -> Result<String> {
    let command = arguments.join(" ");
    if command.len() > 126 || command.contains(['\n', '\r', '\0']) {
        return Err("control command must be one line of at most 126 bytes".into());
    }
    ControlCommand::parse(&command)?;
    let mut stream = UnixStream::connect(socket_path(runtime, storage_key)?)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    writeln!(stream, "{command}")?;
    let mut response = String::new();
    BufReader::new(stream)
        .take(MAX_REPLY as u64 + 1)
        .read_line(&mut response)?;
    if response.len() > MAX_REPLY || !response.ends_with('\n') {
        return Err("control reply is too large or truncated".into());
    }
    match response.strip_prefix("ok ") {
        Some(response) => Ok(response.trim_end_matches('\n').into()),
        None => Err(response.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_permissions_exclusivity_reply_and_cleanup() {
        let runtime = std::env::temp_dir().join(format!("said-control-{}", std::process::id()));
        fs::create_dir(&runtime).unwrap();
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).unwrap();
        let server = ControlSocket::bind(&runtime, "test").unwrap();
        assert_eq!(fs::metadata(server.path()).unwrap().mode() & 0o777, 0o600);
        assert!(ControlSocket::bind(&runtime, "test").is_err());
        let receiver = server.requests();
        let worker = thread::spawn(move || {
            let request = receiver.recv().unwrap();
            assert_eq!(request.command, ControlCommand::Toggle);
            request.reply.send(Ok("accepted".into())).unwrap();
        });
        assert_eq!(
            run_cli(&runtime, "test", &["toggle".into()]).unwrap(),
            "accepted"
        );
        worker.join().unwrap();
        assert!(run_cli(&runtime, "test", &["paste".into()]).is_err());
        drop(server);
        assert!(!runtime.join("test.sock").exists());
        fs::remove_dir_all(runtime).unwrap();
    }
}
