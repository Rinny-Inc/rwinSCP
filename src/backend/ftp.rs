use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::sync::mpsc::{Receiver, Sender};

use suppaftp::FtpStream;
use suppaftp::types::FileType;

use super::{Cancel, Command, Event, RemoteEntry};
use crate::backend::{cancelled, classify};
use crate::connection::{Auth, ConnectionProfile};

pub fn run(
    profile: ConnectionProfile,
    cmd_rx: Receiver<Command>,
    evt_tx: Sender<Event>,
    cancel: Cancel,
) {
    let mut ftp = match connect(&profile) {
        Ok(ftp) => ftp,
        Err(e) => {
            evt_tx.send(Event::ConnectFailed(e.to_string())).ok();
            return;
        }
    };
    evt_tx.send(Event::Connected).ok();

    while let Ok(cmd) = cmd_rx.recv() {
        let stop = matches!(cmd, Command::Disconnect);
        cancel.store(false, std::sync::atomic::Ordering::Relaxed);
        if let Err(e) = handle(&mut ftp, &cmd, &evt_tx, &cancel) {
            evt_tx.send(classify(&cmd, e)).ok();
        }
        if stop {
            break;
        }
    }

    ftp.quit().ok();
    evt_tx.send(Event::Disconnected).ok();
}

fn connect(profile: &ConnectionProfile) -> anyhow::Result<FtpStream> {
    let Auth::Password(password) = &profile.auth else {
        anyhow::bail!("FTP requires username/password authentication");
    };

    let mut ftp = FtpStream::connect((profile.host.as_str(), profile.port))?;
    ftp.login(&profile.username, password)?;
    ftp.transfer_type(FileType::Binary)?;

    if !profile.remote_start_dir.is_empty() {
        ftp.cwd(&profile.remote_start_dir).ok();
    }
    Ok(ftp)
}

fn handle(
    ftp: &mut FtpStream,
    cmd: &Command,
    evt_tx: &Sender<Event>,
    cancel: &Cancel,
) -> anyhow::Result<()> {
    match cmd {
        Command::List { path } => {
            ftp.cwd(path)?;
            let entries = ftp
                .list(None)?
                .iter()
                .filter_map(|line| parse_list_line(line))
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
            if ftp.size(remote_path).is_err() && ftp.cwd(remote_path).is_ok() {
                download_dir(ftp, remote_path, local_path, evt_tx, cancel)?;
                evt_tx
                    .send(Event::TransferDone {
                        label: remote_path.clone(),
                    })
                    .ok();
                return Ok(());
            }
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
        } if local_path.is_dir() => {
            upload_dir(ftp, local_path, remote_path, evt_tx, cancel)?;
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

        Command::Mkdir { path } => ftp.mkdir(path)?,

        Command::Delete { path, is_dir } => {
            if *is_dir {
                ftp.rmdir(path)?;
            } else {
                ftp.rm(path)?;
            }
        }

        Command::Rename { from, to } => ftp.rename(from, to)?,

        Command::ShellInput(_) => anyhow::bail!("FTP has no remote shell"),

        Command::Exec { .. } => anyhow::bail!("FTP has no remote shell"),

        Command::TrustHostKey => {}
        Command::Disconnect => {}
    }
    Ok(())
}

/// `drwxr-xr-x  2 user group 4096 Jan  1 00:00 some name with spaces`
fn parse_list_line(line: &str) -> Option<RemoteEntry> {
    let mut fields = line.split_whitespace();
    let permissions = fields.next()?;
    let _links = fields.next()?;
    let _owner = fields.next()?;
    let _group = fields.next()?;
    let size = fields.next()?;
    let month = fields.next()?;
    let day = fields.next()?;
    let time_or_year = fields.next()?;

    let name_offset = line.find(time_or_year).map(|i| i + time_or_year.len())?;
    let name = line[name_offset..].trim_start();
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }

    Some(RemoteEntry {
        name: name.to_owned(),
        is_dir: permissions.starts_with('d'),
        size: size.parse().unwrap_or(0),
        modified: Some(format!("{month} {day} {time_or_year}")),
    })
}

fn upload_dir(
    ftp: &mut FtpStream,
    local_dir: &std::path::Path,
    remote_dir: &str,
    evt_tx: &Sender<Event>,
    cancel: &Cancel,
) -> anyhow::Result<()> {
    if cancelled(cancel) {
        anyhow::bail!("cancelled");
    }
    ftp.mkdir(remote_dir).ok();

    for entry in std::fs::read_dir(local_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let remote_child = format!("{}/{name}", remote_dir.trim_end_matches('/'));

        if entry.file_type()?.is_dir() {
            upload_dir(ftp, &entry.path(), &remote_child, evt_tx, cancel)?;
        } else {
            if cancelled(cancel) {
                anyhow::bail!("cancelled");
            }
            let mut input = BufReader::new(File::open(entry.path())?);
            ftp.put_file(&remote_child, &mut input)?;
            let moved = std::fs::metadata(entry.path())
                .map(|m| m.len())
                .unwrap_or(0);
            evt_tx
                .send(Event::Progress {
                    transferred: moved,
                    total: moved,
                    label: remote_child.clone(),
                })
                .ok();
        }
    }
    Ok(())
}

fn download_dir(
    ftp: &mut FtpStream,
    remote_dir: &str,
    local_dir: &std::path::Path,
    evt_tx: &Sender<Event>,
    cancel: &Cancel,
) -> anyhow::Result<()> {
    if cancelled(cancel) {
        anyhow::bail!("cancelled");
    }
    std::fs::create_dir_all(local_dir)?;

    ftp.cwd(remote_dir)?;
    let entries: Vec<RemoteEntry> = ftp
        .list(None)?
        .iter()
        .filter_map(|line| parse_list_line(line))
        .collect();

    for entry in entries {
        let remote_child = format!("{}/{}", remote_dir.trim_end_matches('/'), entry.name);
        let local_child = local_dir.join(&entry.name);

        if entry.is_dir {
            download_dir(ftp, &remote_child, &local_child, evt_tx, cancel)?;
            ftp.cwd(remote_dir)?;
        } else {
            if cancelled(cancel) {
                anyhow::bail!("cancelled");
            }
            let mut out = BufWriter::new(File::create(&local_child)?);
            ftp.retr(&remote_child, |reader| {
                std::io::copy(reader, &mut out).map_err(suppaftp::FtpError::ConnectionError)
            })?;
            evt_tx
                .send(Event::Progress {
                    transferred: entry.size,
                    total: entry.size,
                    label: remote_child.clone(),
                })
                .ok();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_unix_list_line() {
        let entry = parse_list_line("-rw-r--r-- 1 user group 4096 Jan  1 00:00 readme.txt")
            .expect("should parse");
        assert_eq!(entry.name, "readme.txt");
        assert_eq!(entry.size, 4096);
        assert!(!entry.is_dir);
    }

    #[test]
    fn directories_are_recognised() {
        let entry = parse_list_line("drwxr-xr-x 2 user group 4096 Jan  1 00:00 docs")
            .expect("should parse");
        assert!(entry.is_dir);
    }

    #[test]
    fn names_with_spaces_survive() {
        let entry =
            parse_list_line("-rw-r--r-- 1 user group 12 Jan  1 00:00 my holiday photos.zip")
                .expect("should parse");
        assert_eq!(entry.name, "my holiday photos.zip");
    }

    #[test]
    fn unparseable_lines_are_skipped_not_guessed() {
        assert!(parse_list_line("total 42").is_none());
        assert!(parse_list_line("").is_none());
        assert!(parse_list_line("garbage").is_none());
    }

    #[test]
    fn dot_entries_are_dropped() {
        assert!(parse_list_line("drwxr-xr-x 2 u g 4096 Jan  1 00:00 .").is_none());
        assert!(parse_list_line("drwxr-xr-x 2 u g 4096 Jan  1 00:00 ..").is_none());
    }
}
