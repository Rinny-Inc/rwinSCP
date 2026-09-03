pub mod ftp;
pub mod s3;
pub mod ssh_sftp_scp;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};

use crate::connection::{ConnectionProfile, Protocol};

#[derive(Debug, Clone)]
pub struct RemoteEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<String>,
}

/// UI -> worker.
#[derive(Debug)]
#[allow(
    dead_code,
    reason = "Rename and Exec are implemented by every backend but not yet wired to a view"
)]
pub enum Command {
    List {
        path: String,
    },
    Download {
        remote_path: String,
        local_path: PathBuf,
    },
    Upload {
        local_path: PathBuf,
        remote_path: String,
    },
    Mkdir {
        path: String,
    },
    Delete {
        path: String,
        is_dir: bool,
    },
    Rename {
        from: String,
        to: String,
    },
    /// SSH only: run a shell command, return combined output.
    Exec {
        command: String,
    },
    /// SSH only: raw bytes typed into interactive shell
    ShellInput(String),
    TrustHostKey,
    Disconnect,
}

/// Worker -> UI.
#[derive(Debug)]
pub enum Event {
    Connected,
    ConnectFailed(String),
    HostKeyUnknown {
        host: String,
        fingerprint: String,
        key_type: String,
    },
    Listing {
        path: String,
        entries: Vec<RemoteEntry>,
    },
    Progress {
        transferred: u64,
        total: u64,
        label: String,
    },
    TransferCancelled {
        label: String,
    },
    TransferDone {
        label: String,
    },
    ExecOutput(String),
    ShellOutput(String),
    Error(String),
    Disconnected,
}

pub struct WorkerHandle {
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    tx: Sender<Command>,
    rx: Receiver<Event>,
    _thread: std::thread::JoinHandle<()>,
}

impl WorkerHandle {
    /// Asks the worker to abandon whatever transfer it is running
    pub fn cancel(&self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Fire-and-forget send
    pub fn send(&self, cmd: Command) {
        self.tx.send(cmd).ok();
    }

    /// Non-blocking drain of pending events
    pub fn try_recv(&self) -> Result<Event, std::sync::mpsc::TryRecvError> {
        self.rx.try_recv()
    }
}

/// Spawns the worker for a profile's protocol
pub fn spawn(profile: ConnectionProfile) -> WorkerHandle {
    let (tx, cmd_rx) = std::sync::mpsc::channel::<Command>();
    let (evt_tx, rx) = std::sync::mpsc::channel::<Event>();
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    let thread = match profile.protocol {
        Protocol::Ssh | Protocol::Sftp | Protocol::Scp => {
            let cancel = cancel.clone();
            std::thread::spawn(move || ssh_sftp_scp::run(profile, cmd_rx, evt_tx, cancel))
        }
        Protocol::Ftp => {
            let cancel = cancel.clone();
            std::thread::spawn(move || ftp::run(profile, cmd_rx, evt_tx, cancel))
        }
        Protocol::S3 => {
            let cancel = cancel.clone();
            std::thread::spawn(move || s3::run(profile, cmd_rx, evt_tx, cancel))
        }
    };

    WorkerHandle {
        cancel,
        tx,
        rx,
        _thread: thread,
    }
}

/// Shared chunk size for streamed transfers
pub(crate) const CHUNK: usize = 64 * 1024;
pub(crate) type Cancel = std::sync::Arc<std::sync::atomic::AtomicBool>;
pub(crate) fn cancelled(flag: &Cancel) -> bool {
    flag.load(std::sync::atomic::Ordering::Relaxed)
}
pub(crate) fn classify(cmd: &Command, error: anyhow::Error) -> Event {
    let message = error.to_string();
    let label = match cmd {
        Command::Download { remote_path, .. } => remote_path.clone(),
        Command::Upload { remote_path, .. } => remote_path.clone(),
        _ => String::new(),
    };

    if message == "cancelled" {
        Event::TransferCancelled { label }
    } else {
        Event::Error(message)
    }
}
