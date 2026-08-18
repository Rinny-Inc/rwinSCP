pub mod ftp;
pub mod s3;
pub mod ssh_sftp_scp;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};

use crate::connection::ConnectionProfile;

#[derive(Debug, Clone)]
pub struct RemoteEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<String>,
}

#[derive(Debug)]
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
    Exec {
        command: String,
    },
    Disconnect,
}

#[derive(Debug)]
pub enum Event {
    Connected,
    ConnectFailed(String),
    Listing {
        path: String,
        entries: Vec<RemoteEntry>,
    },
    Progress {
        transferred: u64,
        total: u64,
        label: String,
    },
    TransferDone {
        label: String,
    },
    ExecOutput(String),
    Error(String),
    Disconnected,
}

pub struct WorkerHandle {
    pub tx: Sender<Command>,
    pub rx: Receiver<Event>,
    pub thread: std::thread::JoinHandle<()>,
}

pub fn spawn(profile: ConnectionProfile) -> WorkerHandle {
    use crate::connection::Protocol;

    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<Command>();
    let (evt_tx, evt_rx) = std::sync::mpsc::channel::<Event>();

    let thread = match profile.protocol {
        Protocol::Ssh | Protocol::Sftp | Protocol::Scp => {
            std::thread::spawn(move || ssh_sftp_scp::run(profile, cmd_rx, evt_tx))
        }
        Protocol::Ftp => std::thread::spawn(move || ftp::run(profile, cmd_rx, evt_tx)),
        Protocol::S3 => std::thread::spawn(move || s3::run(profile, cmd_rx, evt_tx)),
    };

    WorkerHandle {
        tx: cmd_tx,
        rx: evt_rx,
        thread,
    }
}
