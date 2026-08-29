use std::fs::File;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use ssh2::Session;

use super::{CHUNK, Command, Event, RemoteEntry};
use crate::connection::{Auth, ConnectionProfile, Protocol};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const SHELL_IDLE: Duration = Duration::from_millis(20);

const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_RETRY: Duration = Duration::from_millis(5);

const PTY_COLS: u32 = 120;
const PTY_ROWS: u32 = 34;

pub fn run(profile: ConnectionProfile, cmd_rx: Receiver<Command>, evt_tx: Sender<Event>) {
    let session = match connect(&profile) {
        Ok(session) => session,
        Err(e) => {
            evt_tx.send(Event::ConnectFailed(e.to_string())).ok();
            return;
        }
    };
    evt_tx.send(Event::Connected).ok();

    if profile.protocol == Protocol::Ssh {
        if let Err(e) = run_shell(&session, cmd_rx, &evt_tx) {
            evt_tx.send(Event::Error(e.to_string())).ok();
        }
        session.set_blocking(true);
        session.disconnect(None, "bye", None).ok();
        evt_tx.send(Event::Disconnected).ok();
        return;
    }

    let sftp = matches!(profile.protocol, Protocol::Sftp | Protocol::Scp)
        .then(|| session.sftp().ok())
        .flatten();

    while let Ok(cmd) = cmd_rx.recv() {
        let stop = matches!(cmd, Command::Disconnect);
        if let Err(e) = handle(&profile, &session, sftp.as_ref(), &cmd, &evt_tx) {
            evt_tx.send(Event::Error(e.to_string())).ok();
        }
        if stop {
            break;
        }
    }

    session.disconnect(None, "bye", None).ok();
    evt_tx.send(Event::Disconnected).ok();
}

fn connect(profile: &ConnectionProfile) -> anyhow::Result<Session> {
    let address = format!("{}:{}", profile.host, profile.port);
    let socket_addr = std::net::ToSocketAddrs::to_socket_addrs(&address)?
        .next()
        .ok_or_else(|| anyhow::anyhow!("could not resolve {address}"))?;

    let tcp = TcpStream::connect_timeout(&socket_addr, CONNECT_TIMEOUT)?;
    tcp.set_nodelay(true).ok();

    let mut session = Session::new()?;
    session.set_tcp_stream(tcp);
    session.handshake()?;

    match &profile.auth {
        Auth::Password(password) => {
            session.userauth_password(&profile.username, password)?;
        }
        Auth::KeyFile { path, passphrase } => {
            let passphrase = (!passphrase.is_empty()).then_some(passphrase.as_str());
            session.userauth_pubkey_file(&profile.username, None, Path::new(path), passphrase)?;
        }
        Auth::S3Keys { .. } => anyhow::bail!("S3 credentials cannot authenticate an SSH session"),
    }

    if !session.authenticated() {
        anyhow::bail!("authentication failed");
    }
    Ok(session)
}

fn run_shell(
    session: &Session,
    cmd_rx: Receiver<Command>,
    evt_tx: &Sender<Event>,
) -> anyhow::Result<()> {
    let mut channel = session.channel_session()?;
    channel.request_pty("xterm-256color", None, Some((PTY_COLS, PTY_ROWS, 0, 0)))?;
    channel.shell()?;

    session.set_blocking(false);

    let mut buf = [0u8; 8192];
    let result = loop {
        let mut stop = false;
        let mut fatal = None;
        loop {
            match cmd_rx.try_recv() {
                Ok(Command::ShellInput(data)) => {
                    let mut pending = data.as_bytes();
                    let deadline = Instant::now() + WRITE_TIMEOUT;
                    while !pending.is_empty() {
                        match channel.write(pending) {
                            Ok(0) => break,
                            Ok(n) => pending = &pending[n..],
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                if Instant::now() > deadline {
                                    break;
                                }
                                std::thread::sleep(WRITE_RETRY);
                            }
                            Err(e) => {
                                fatal = Some(anyhow::Error::from(e));
                                break;
                            }
                        }
                    }
                }
                Ok(Command::Disconnect) | Err(TryRecvError::Disconnected) => {
                    stop = true;
                    break;
                }
                Ok(_) => {
                    evt_tx
                        .send(Event::Error(
                            "That operation needs SFTP; this is a shell session".to_owned(),
                        ))
                        .ok();
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        if let Some(e) = fatal {
            break Err(e);
        }
        if stop {
            break Ok(());
        }

        let mut idle = true;
        match channel.read(&mut buf) {
            Ok(0) => {
                if channel.eof() {
                    break Ok(());
                }
            }
            Ok(n) => {
                idle = false;
                evt_tx
                    .send(Event::ShellOutput(
                        String::from_utf8_lossy(&buf[..n]).into_owned(),
                    ))
                    .ok();
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => break Err(anyhow::Error::from(e)),
        }

        if idle {
            std::thread::sleep(SHELL_IDLE);
        }
    };

    session.set_blocking(true);
    channel.close().ok();
    channel.wait_close().ok();
    result
}

fn handle(
    profile: &ConnectionProfile,
    session: &Session,
    sftp: Option<&ssh2::Sftp>,
    cmd: &Command,
    evt_tx: &Sender<Event>,
) -> anyhow::Result<()> {
    let need_sftp = || sftp.ok_or_else(|| anyhow::anyhow!("this operation requires SFTP"));

    match cmd {
        Command::List { path } => {
            let entries = need_sftp()?
                .readdir(Path::new(path))?
                .into_iter()
                .filter_map(|(entry_path, stat)| {
                    let name = entry_path.file_name()?.to_string_lossy().into_owned();
                    (name != "." && name != "..").then(|| RemoteEntry {
                        name,
                        is_dir: stat.is_dir(),
                        size: stat.size.unwrap_or(0),
                        modified: stat.mtime.map(format_unix_time),
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
                    let mut remote = need_sftp()?.open(Path::new(remote_path))?;
                    let total = remote.stat()?.size.unwrap_or(0);
                    pump(&mut remote, &mut local, total, remote_path, evt_tx)?;
                }
                Protocol::Scp => {
                    let (mut remote, stat) = session.scp_recv(Path::new(remote_path))?;
                    pump(&mut remote, &mut local, stat.size(), remote_path, evt_tx)?;
                }
                _ => anyhow::bail!("{} cannot transfer files", profile.protocol),
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
                    let mut remote = need_sftp()?.create(Path::new(remote_path))?;
                    pump(&mut local, &mut remote, total, remote_path, evt_tx)?;
                }
                Protocol::Scp => {
                    let mut remote =
                        session.scp_send(Path::new(remote_path), 0o644, total, None)?;
                    pump(&mut local, &mut remote, total, remote_path, evt_tx)?;
                    remote.send_eof()?;
                    remote.wait_eof()?;
                    remote.close()?;
                    remote.wait_close()?;
                }
                _ => anyhow::bail!("{} cannot transfer files", profile.protocol),
            }
            evt_tx
                .send(Event::TransferDone {
                    label: remote_path.clone(),
                })
                .ok();
        }

        Command::Mkdir { path } => need_sftp()?.mkdir(Path::new(path), 0o755)?,

        Command::Delete { path, is_dir } => {
            let sftp = need_sftp()?;
            if *is_dir {
                sftp.rmdir(Path::new(path))?;
            } else {
                sftp.unlink(Path::new(path))?;
            }
        }

        Command::Rename { from, to } => {
            need_sftp()?.rename(Path::new(from), Path::new(to), None)?;
        }

        Command::Exec { command } => {
            let mut channel = session.channel_session()?;
            channel.exec(command)?;
            let mut output = String::new();
            channel.read_to_string(&mut output)?;
            let mut stderr = String::new();
            channel.stderr().read_to_string(&mut stderr).ok();
            if !stderr.is_empty() {
                output.push_str(&stderr);
            }
            channel.wait_close()?;
            evt_tx.send(Event::ExecOutput(output)).ok();
        }

        Command::ShellInput(_) => {
            anyhow::bail!("the interactive shell is only available over SSH")
        }

        Command::Disconnect => {}
    }
    Ok(())
}

fn pump<R: Read, W: Write>(
    src: &mut R,
    dst: &mut W,
    total: u64,
    label: &str,
    evt_tx: &Sender<Event>,
) -> anyhow::Result<()> {
    let mut buf = vec![0u8; CHUNK];
    let mut transferred = 0u64;

    loop {
        let read = src.read(&mut buf)?;
        if read == 0 {
            break;
        }
        dst.write_all(&buf[..read])?;
        transferred += read as u64;
        evt_tx
            .send(Event::Progress {
                transferred,
                total,
                label: label.to_owned(),
            })
            .ok();
    }

    dst.flush()?;
    Ok(())
}

fn format_unix_time(secs: u64) -> String {
    chrono::DateTime::from_timestamp(secs as i64, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}
