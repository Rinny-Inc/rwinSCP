use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::sync::mpsc::{Receiver, Sender};

use suppaftp::FtpStream;

use super::{Command, Event, RemoteEntry};
use crate::connection::{Auth, ConnectionProfile};

pub fn run(profile: ConnectionProfile, cmd_rx: Receiver<Command>, evt_tx: Sender<Event>) {
    let mut ftp = match connect(&profile) {
        Ok(f) => f,
        Err(e) => {
            evt_tx.send(Event::ConnectFailed(e.to_string())).ok();
            return;
        }
    };
    evt_tx.send(Event::Connected).ok();

    while let Ok(cmd) = cmd_rx.recv() {
        if let Err(e) = handle(&mut ftp, &cmd, &evt_tx) {
            evt_tx.send(Event::Error(e.to_string())).ok();
        }
        if matches!(cmd, Command::Disconnect) {
            break;
        }
    }
    ftp.quit().ok();
    evt_tx.send(Event::Disconnected).ok();
}

fn connect(profile: &ConnectionProfile) -> anyhow::Result<FtpStream> {
    let mut ftp = FtpStream::connect((profile.host.as_str(), profile.port))?;
    let password = match &profile.auth {
        Auth::Password(pw) => pw.clone(),
        _ => anyhow::bail!("FTP requires a username/password"),
    };
    ftp.login(&profile.username, &password)?;
    ftp.transfer_type(suppaftp::types::FileType::Binary)?;
    if !profile.remote_start_dir.is_empty() {
        ftp.cwd(&profile.remote_start_dir).ok();
    }
    Ok(ftp)
}

fn handle(ftp: &mut FtpStream, cmd: &Command, evt_tx: &Sender<Event>) -> anyhow::Result<()> {
    match cmd {
        Command::List { path } => {
            ftp.cwd(path)?;
            let lines = ftp.list(None)?;
            let entries = lines.iter().filter_map(|l| parse_list_line(l)).collect();
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
            let mut out = BufWriter::new(File::create(local_path)?);
            ftp.retr(remote_path, |reader| {
                std::io::copy(reader, &mut out).map_err(suppaftp::FtpError::ConnectionError)
            })?;
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
            let mut input = BufReader::new(File::open(local_path)?);
            ftp.put_file(remote_path, &mut input)?;
            evt_tx
                .send(Event::TransferDone {
                    label: remote_path.clone(),
                })
                .ok();
        }

        Command::Mkdir { path } => {
            ftp.mkdir(path)?;
        }

        Command::Delete { path, is_dir } => {
            if *is_dir {
                ftp.rmdir(path)?;
            } else {
                ftp.rm(path)?;
            }
        }

        Command::Rename { from, to } => {
            ftp.rename(from, to)?;
        }

        Command::Exec { .. } => {
            anyhow::bail!("FTP has no remote shell; use SSH for that");
        }

        Command::Disconnect => {}
    }
    Ok(())
}

fn parse_list_line(line: &str) -> Option<RemoteEntry> {
    let parts: Vec<&str> = line
        .splitn(9, char::is_whitespace)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.len() < 9 {
        return None;
    }
    let is_dir = parts[0].starts_with('d');
    let size = parts[4].parse::<u64>().unwrap_or(0);
    let modified = format!("{} {} {}", parts[5], parts[6], parts[7]);
    let name = parts[8].to_string();
    Some(RemoteEntry {
        name,
        is_dir,
        size,
        modified: Some(modified),
    })
}
