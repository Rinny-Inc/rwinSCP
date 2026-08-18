use std::fs::File;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::mpsc::{Receiver, Sender};

use ssh2::Session;

use super::{Command, Event, RemoteEntry};
use crate::connection::{Auth, ConnectionProfile, Protocol};

const CHUNK: usize = 64 * 1024;

pub fn run(profile: ConnectionProfile, cmd_rx: Receiver<Command>, evt_tx: Sender<Event>) {
    let session = match connect(&profile) {
        Ok(s) => s,
        Err(e) => {
            let _ = evt_tx.send(Event::ConnectFailed(e.to_string()));
            return;
        }
    };
    evt_tx.send(Event::Connected).ok();

    let sftp = if matches!(profile.protocol, Protocol::Sftp | Protocol::Scp) {
        session.sftp().ok()
    } else {
        None
    };

    while let Ok(cmd) = cmd_rx.recv() {
        let result = handle(&profile, &session, sftp.as_ref(), &cmd, &evt_tx);
        if let Err(e) = result {
            evt_tx.send(Event::Error(e.to_string())).ok();
        }
        if matches!(cmd, Command::Disconnect) {
            break;
        }
    }
    evt_tx.send(Event::Disconnected).ok();
}

fn connect(profile: &ConnectionProfile) -> anyhow::Result<Session> {
    let tcp = TcpStream::connect((profile.host.as_str(), profile.port))?;
    let mut session = Session::new()?;
    session.set_tcp_stream(tcp);
    session.handshake()?;

    match &profile.auth {
        Auth::Password(pw) => {
            session.userauth_password(&profile.username, pw)?;
        }
        Auth::KeyFile { path, passphrase } => {
            let pass = if passphrase.is_empty() {
                None
            } else {
                Some(passphrase.as_str())
            };
            session.userauth_pubkey_file(&profile.username, None, Path::new(path), pass)?;
        }
        Auth::S3Keys { .. } => anyhow::bail!("S3 credentials are not valid for SSH"),
    }

    if !session.authenticated() {
        anyhow::bail!("authentication failed");
    }
    Ok(session)
}

fn handle(
    profile: &ConnectionProfile,
    session: &Session,
    sftp: Option<&ssh2::Sftp>,
    cmd: &Command,
    evt_tx: &Sender<Event>,
) -> anyhow::Result<()> {
    match cmd {
        Command::List { path } => {
            let sftp = sftp.ok_or_else(|| anyhow::anyhow!("directory listing needs SFTP"))?;
            let entries = sftp
                .readdir(Path::new(path))?
                .into_iter()
                .filter_map(|(p, stat)| {
                    let name = p.file_name()?.to_string_lossy().to_string();
                    if name == "." || name == ".." {
                        return None;
                    }
                    Some(RemoteEntry {
                        name,
                        is_dir: stat.is_dir(),
                        size: stat.size.unwrap_or(0),
                        modified: stat.mtime.map(|t| t.to_string()),
                    })
                })
                .collect();
            evt_tx
                .send(Event::Listing {
                    path: path.clone(),
                    entries,
                })
                .ok();
        }

        Command::Download {
            remote_path,
            local_path,
        } => {
            let mut local = File::create(local_path)?;
            match profile.protocol {
                Protocol::Sftp => {
                    let sftp = sftp.ok_or_else(|| anyhow::anyhow!("no sftp channel"))?;
                    let mut remote = sftp.open(Path::new(remote_path))?;
                    let total = remote.stat()?.size.unwrap_or(0);
                    copy_with_progress(&mut remote, &mut local, total, remote_path, evt_tx)?;
                }
                Protocol::Scp => {
                    let (mut remote, stat) = session.scp_recv(Path::new(remote_path))?;
                    let total = stat.size();
                    copy_with_progress(&mut remote, &mut local, total, remote_path, evt_tx)?;
                }
                _ => anyhow::bail!("download not supported for this protocol"),
            }
            evt_tx
                .send(Event::TransferDone {
                    label: remote_path.clone(),
                })
                .ok();
        }

        Command::Upload {
            local_path,
            remote_path,
        } => {
            let mut local = File::open(local_path)?;
            let total = local.metadata()?.len();
            match profile.protocol {
                Protocol::Sftp => {
                    let sftp = sftp.ok_or_else(|| anyhow::anyhow!("no sftp channel"))?;
                    let mut remote = sftp.create(Path::new(remote_path))?;
                    copy_with_progress(&mut local, &mut remote, total, remote_path, evt_tx)?;
                }
                Protocol::Scp => {
                    let mode = 0o644;
                    let mut remote = session.scp_send(Path::new(remote_path), mode, total, None)?;
                    copy_with_progress(&mut local, &mut remote, total, remote_path, evt_tx)?;
                }
                _ => anyhow::bail!("upload not supported for this protocol"),
            }
            evt_tx
                .send(Event::TransferDone {
                    label: remote_path.clone(),
                })
                .ok();
        }

        Command::Mkdir { path } => {
            let sftp = sftp.ok_or_else(|| anyhow::anyhow!("no sftp channel"))?;
            sftp.mkdir(Path::new(path), 0o755)?;
        }

        Command::Delete { path, is_dir } => {
            let sftp = sftp.ok_or_else(|| anyhow::anyhow!("no sftp channel"))?;
            if *is_dir {
                sftp.rmdir(Path::new(path))?;
            } else {
                sftp.unlink(Path::new(path))?;
            }
        }

        Command::Rename { from, to } => {
            let sftp = sftp.ok_or_else(|| anyhow::anyhow!("no sftp channel"))?;
            sftp.rename(Path::new(from), Path::new(to), None)?;
        }

        Command::Exec { command } => {
            let mut channel = session.channel_session()?;
            channel.exec(command)?;
            let mut output = String::new();
            channel.read_to_string(&mut output)?;
            channel.wait_close()?;
            evt_tx.send(Event::ExecOutput(output)).ok();
        }

        Command::Disconnect => {}
    }
    Ok(())
}

fn copy_with_progress<R: Read, W: Write>(
    src: &mut R,
    dst: &mut W,
    total: u64,
    label: &str,
    evt_tx: &Sender<Event>,
) -> anyhow::Result<()> {
    let mut buf = [0u8; CHUNK];
    let mut transferred: u64 = 0;
    loop {
        let n = src.read(&mut buf)?;
        if n == 0 {
            break;
        }
        dst.write_all(&buf[..n])?;
        transferred += n as u64;
        evt_tx
            .send(Event::Progress {
                transferred,
                total,
                label: label.to_string(),
            })
            .ok();
    }
    dst.flush()?;
    Ok(())
}
