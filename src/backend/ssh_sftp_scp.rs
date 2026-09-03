use std::fs::File;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use ssh2::Session;

use super::{CHUNK, Command, Event, RemoteEntry};
use crate::backend::{Cancel, cancelled, classify};
use crate::connection::{Auth, ConnectionProfile, Protocol};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const SHELL_IDLE: Duration = Duration::from_millis(20);

const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_RETRY: Duration = Duration::from_millis(5);

const PTY_COLS: u32 = 120;
const PTY_ROWS: u32 = 34;

pub fn run(
    profile: ConnectionProfile,
    cmd_rx: Receiver<Command>,
    evt_tx: Sender<Event>,
    cancel: Cancel,
) {
    let session = match connect(&profile, &cmd_rx, &evt_tx) {
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
        if let Err(e) = handle(&profile, &session, sftp.as_ref(), &cmd, &evt_tx, &cancel) {
            evt_tx.send(classify(&cmd, e)).ok();
        }
        if stop {
            break;
        }
    }

    session.disconnect(None, "bye", None).ok();
    evt_tx.send(Event::Disconnected).ok();
}

fn connect(
    profile: &ConnectionProfile,
    cmd_rx: &Receiver<Command>,
    evt_tx: &Sender<Event>,
) -> anyhow::Result<Session> {
    let address = format!("{}:{}", profile.host, profile.port);
    let socket_addr = std::net::ToSocketAddrs::to_socket_addrs(&address)?
        .next()
        .ok_or_else(|| anyhow::anyhow!("could not resolve {address}"))?;

    let tcp = TcpStream::connect_timeout(&socket_addr, CONNECT_TIMEOUT)?;
    tcp.set_nodelay(true).ok();

    let mut session = Session::new()?;
    session.set_tcp_stream(tcp);
    session.handshake()?;

    verify_host_key(&session, profile, cmd_rx, evt_tx)?;

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
    cancel: &Cancel,
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
        } if profile.protocol == Protocol::Sftp => {
            let sftp = need_sftp()?;
            let stat = sftp.stat(Path::new(remote_path))?;
            if stat.is_dir() {
                download_dir(sftp, remote_path, local_path, evt_tx, cancel)?;
            } else {
                let mut local = File::create(local_path)?;
                let mut remote = sftp.open(Path::new(remote_path))?;
                let total = remote.stat()?.size.unwrap_or(0);
                pump(&mut remote, &mut local, total, remote_path, evt_tx, cancel)?;
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
        } if profile.protocol == Protocol::Sftp => {
            let sftp = need_sftp()?;
            if local_path.is_dir() {
                upload_dir(sftp, local_path, remote_path, evt_tx, cancel)?;
            } else {
                let mut local = File::open(local_path)?;
                let total = local.metadata()?.len();
                let mut remote = sftp.create(Path::new(remote_path))?;
                pump(&mut local, &mut remote, total, remote_path, evt_tx, cancel)?;
            }
            evt_tx
                .send(Event::TransferDone {
                    label: remote_path.clone(),
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
                    pump(&mut remote, &mut local, total, remote_path, evt_tx, cancel)?;
                }
                Protocol::Scp => {
                    let (mut remote, stat) = session.scp_recv(Path::new(remote_path))?;
                    pump(
                        &mut remote,
                        &mut local,
                        stat.size(),
                        remote_path,
                        evt_tx,
                        cancel,
                    )?;
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
                    pump(&mut local, &mut remote, total, remote_path, evt_tx, cancel)?;
                }
                Protocol::Scp => {
                    let mut remote =
                        session.scp_send(Path::new(remote_path), 0o644, total, None)?;
                    pump(&mut local, &mut remote, total, remote_path, evt_tx, cancel)?;
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

        Command::TrustHostKey => {}
        Command::Disconnect => {}
    }
    Ok(())
}

fn upload_dir(
    sftp: &ssh2::Sftp,
    local_dir: &std::path::Path,
    remote_dir: &str,
    evt_tx: &Sender<Event>,
    cancel: &Cancel,
) -> anyhow::Result<()> {
    sftp.mkdir(Path::new(remote_dir), 0o755).ok();

    for entry in std::fs::read_dir(local_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue; // skip name that cannot be represented remotely
        };
        let remote_child = format!("{}/{name}", remote_dir.trim_end_matches('/'));
        let local_child = entry.path();

        if entry.file_type()?.is_dir() {
            upload_dir(sftp, &local_child, &remote_child, evt_tx, cancel)?;
        } else {
            let mut local = File::open(&local_child)?;
            let total = local.metadata()?.len();
            let mut remote = sftp.create(Path::new(&remote_child))?;
            pump(
                &mut local,
                &mut remote,
                total,
                &remote_child,
                evt_tx,
                cancel,
            )?;
        }
    }

    Ok(())
}

fn download_dir(
    sftp: &ssh2::Sftp,
    remote_dir: &str,
    local_dir: &std::path::Path,
    evt_tx: &Sender<Event>,
    cancel: &Cancel,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(local_dir)?;

    for (path, stat) in sftp.readdir(Path::new(remote_dir))? {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name == "." || name == ".." {
            continue;
        }

        let remote_child = format!("{}/{name}", remote_dir.trim_end_matches('/'));
        let local_child = local_dir.join(name);

        if stat.is_dir() {
            download_dir(sftp, &remote_child, &local_child, evt_tx, cancel)?;
        } else {
            let mut local = File::create(&local_child)?;
            let mut remote = sftp.open(Path::new(&remote_child))?;
            let total = stat.size.unwrap_or(0);
            pump(
                &mut remote,
                &mut local,
                total,
                &remote_child,
                evt_tx,
                cancel,
            )?;
        }
    }

    Ok(())
}

fn pump<R: Read, W: Write>(
    src: &mut R,
    dst: &mut W,
    total: u64,
    label: &str,
    evt_tx: &Sender<Event>,
    cancel: &Cancel,
) -> anyhow::Result<()> {
    let mut buf = vec![0u8; CHUNK];
    let mut transferred = 0u64;

    loop {
        if cancelled(cancel) {
            anyhow::bail!("cancelled");
        }
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

fn verify_host_key(
    session: &Session,
    profile: &ConnectionProfile,
    cmd_rx: &Receiver<Command>,
    evt_tx: &Sender<Event>,
) -> anyhow::Result<()> {
    let (key, key_type) = session
        .host_key()
        .ok_or_else(|| anyhow::anyhow!("server presented no host key"))?;

    let mut known = session.known_hosts()?;
    let path = known_host_path()
        .ok_or_else(|| anyhow::anyhow!("cannot locate a home directory for known_hosts"))?;
    if path.exists() {
        known.read_file(&path, ssh2::KnownHostFileKind::OpenSSH)?;
    }

    match known.check_port(&profile.host, profile.port, key) {
        ssh2::CheckResult::Match => Ok(()),

        ssh2::CheckResult::Mismatch => anyhow::bail!(
            "HOST KEY CHANGED FOR {}:{}. The server is not the one previously trusted! \
            If you did not just rebuild it, someone may be intercepting the connection! \
            Remove the old entry from ~/.ssh/known_hosts only if you are certain!",
            profile.host,
            profile.port
        ),

        ssh2::CheckResult::Failure => anyhow::bail!("could not check the host key"),

        ssh2::CheckResult::NotFound => {
            evt_tx
                .send(Event::HostKeyUnknown {
                    host: format!("{}:{}", profile.host, profile.port),
                    fingerprint: fingerprint(session),
                    key_type: describe_key_type(key_type),
                })
                .ok();

            match cmd_rx.recv() {
                Ok(Command::TrustHostKey) => {}
                _ => anyhow::bail!("host key was not trusted"),
            }

            let entry = if profile.port == 22 {
                profile.host.clone()
            } else {
                format!("[{}]:{}", profile.host, profile.port)
            };
            known.add(&entry, key, "added by rwinSCP", key_format(key_type))?;

            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            known.write_file(&path, ssh2::KnownHostFileKind::OpenSSH)?;
            Ok(())
        }
    }
}

fn fingerprint(session: &Session) -> String {
    match session.host_key_hash(ssh2::HashType::Sha256) {
        Some(hash) => format!("SHA256:{}", base64_no_pad(hash)),
        None => "unavailable".to_owned(),
    }
}

fn describe_key_type(key_type: ssh2::HostKeyType) -> String {
    match key_type {
        ssh2::HostKeyType::Rsa => "ssh-rsa",
        ssh2::HostKeyType::Dss => "ssh-dss",
        ssh2::HostKeyType::Ecdsa256 => "ecdsa-sha2-nistp256",
        ssh2::HostKeyType::Ecdsa384 => "ecdsa-sha2-nistp384",
        ssh2::HostKeyType::Ecdsa521 => "ecdsa-sha2-nistp521",
        ssh2::HostKeyType::Ed25519 => "ssh-ed25519",
        ssh2::HostKeyType::Unknown => "unknown",
    }
    .to_owned()
}

fn key_format(key_type: ssh2::HostKeyType) -> ssh2::KnownHostKeyFormat {
    match key_type {
        ssh2::HostKeyType::Rsa => ssh2::KnownHostKeyFormat::SshRsa,
        ssh2::HostKeyType::Dss => ssh2::KnownHostKeyFormat::SshDss,
        ssh2::HostKeyType::Ecdsa256 => ssh2::KnownHostKeyFormat::Ecdsa256,
        ssh2::HostKeyType::Ecdsa384 => ssh2::KnownHostKeyFormat::Ecdsa384,
        ssh2::HostKeyType::Ecdsa521 => ssh2::KnownHostKeyFormat::Ecdsa521,
        ssh2::HostKeyType::Ed25519 => ssh2::KnownHostKeyFormat::Ed25519,
        ssh2::HostKeyType::Unknown => ssh2::KnownHostKeyFormat::Unknown,
    }
}

fn base64_no_pad(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        let indices = [n >> 18 & 63, n >> 12 & 63, n >> 6 & 63, n & 63];
        for index in indices.iter().take(chunk.len() + 1) {
            out.push(ALPHABET[*index as usize] as char);
        }
    }
    out
}

fn known_host_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|home| home.join(".ssh").join("known_hosts"))
}

fn format_unix_time(secs: u64) -> String {
    chrono::DateTime::from_timestamp(secs as i64, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_reference_vectors() {
        assert_eq!(base64_no_pad(b""), "");
        assert_eq!(base64_no_pad(b"f"), "Zg");
        assert_eq!(base64_no_pad(b"fo"), "Zm8");
        assert_eq!(base64_no_pad(b"foo"), "Zm9v");
        assert_eq!(base64_no_pad(b"foob"), "Zm9vYg");
        assert_eq!(base64_no_pad(b"fooba"), "Zm9vYmE");
        assert_eq!(base64_no_pad(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_covers_the_whole_alphabet() {
        let bytes: Vec<u8> = (0u8..=255).collect();
        let encoded = base64_no_pad(&bytes);
        assert!(encoded.contains('+') && encoded.contains('/'));
        assert_eq!(encoded.len(), 342);
    }

    fn profile(host: &str) -> ConnectionProfile {
        let mut p = ConnectionProfile::new(Protocol::Sftp);
        p.host = host.to_owned();
        p
    }

    #[test]
    fn bare_host_takes_the_protocol_default_port() {
        let mut p = profile("example.com");
        p.normalize_endpoint();
        assert_eq!(p.host, "example.com");
        assert_eq!(p.port, 22);
    }

    #[test]
    fn inline_port_is_split_off() {
        let mut p = profile("example.com:2222");
        p.normalize_endpoint();
        assert_eq!(p.host, "example.com");
        assert_eq!(p.port, 2222);
    }

    #[test]
    fn bracketed_ipv6_keeps_its_colons() {
        let mut p = profile("[::1]:2222");
        p.normalize_endpoint();
        assert_eq!(p.host, "[::1]");
        assert_eq!(p.port, 2222);
    }

    #[test]
    fn bare_ipv6_is_left_alone() {
        let mut p = profile("fe80::1");
        p.normalize_endpoint();
        assert_eq!(p.host, "fe80::1");
        assert_eq!(p.port, 22);
    }

    #[test]
    fn a_nonsense_port_falls_back_to_the_default() {
        let mut p = profile("example.com:notaport");
        p.normalize_endpoint();
        assert_eq!(p.port, 22);
    }

    #[test]
    fn switching_protocol_moves_the_default_port() {
        let mut p = profile("example.com");
        p.set_protocol(Protocol::Ftp);
        assert_eq!(p.port, 21);
    }
}
