use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Ssh,
    Sftp,
    Scp,
    Ftp,
    S3,
}

impl Protocol {
    pub const ALL: [Protocol; 5] = [
        Protocol::Ssh,
        Protocol::Sftp,
        Protocol::Scp,
        Protocol::Ftp,
        Protocol::S3,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Protocol::Ssh => "SSH",
            Protocol::Sftp => "SFTP",
            Protocol::Scp => "SCP",
            Protocol::Ftp => "FTP",
            Protocol::S3 => "S3",
        }
    }

    pub fn default_port(self) -> u16 {
        match self {
            Protocol::Ssh | Protocol::Sftp | Protocol::Scp => 22,
            Protocol::Ftp => 21,
            Protocol::S3 => 443,
        }
    }

    pub fn browsable(self) -> bool {
        matches!(
            self,
            Protocol::Sftp | Protocol::Scp | Protocol::Ftp | Protocol::S3
        )
    }

    pub fn is_object_store(self) -> bool {
        matches!(self, Protocol::S3)
    }

    pub fn has_shell(self) -> bool {
        matches!(self, Protocol::Ssh | Protocol::Sftp | Protocol::Scp)
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Auth {
    Password(String),
    KeyFile {
        path: String,
        passphrase: String,
    },
    S3Keys {
        access_key: String,
        secret_key: String,
    },
}

impl Auth {
    pub fn for_protocol(protocol: Protocol) -> Self {
        if protocol.is_object_store() {
            Auth::S3Keys {
                access_key: String::new(),
                secret_key: String::new(),
            }
        } else {
            Auth::Password(String::new())
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConnectionProfile {
    pub name: String,
    pub protocol: Protocol,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: Auth,
    /// S3 only.
    pub bucket: String,
    pub region: String,
    pub remote_start_dir: String,
}

impl ConnectionProfile {
    pub fn new(protocol: Protocol) -> Self {
        Self {
            name: String::new(),
            protocol,
            host: String::new(),
            port: protocol.default_port(),
            username: String::new(),
            auth: Auth::for_protocol(protocol),
            bucket: String::new(),
            region: "us-east-1".into(),
            remote_start_dir: "/".into(),
        }
    }

    pub fn display_name(&self) -> &str {
        if !self.name.trim().is_empty() {
            self.name.trim()
        } else if self.protocol.is_object_store() {
            self.bucket.as_str()
        } else {
            self.host.as_str()
        }
    }

    pub fn endpoint(&self) -> String {
        if self.protocol.is_object_store() {
            format!("{} · {}/{}", self.protocol, self.region, self.bucket)
        } else if self.username.is_empty() {
            format!("{} · {}:{}", self.protocol, self.host, self.port)
        } else {
            format!(
                "{} · {}@{}:{}",
                self.protocol, self.username, self.host, self.port
            )
        }
    }

    pub fn is_connectable(&self) -> bool {
        if self.protocol.is_object_store() {
            !self.bucket.trim().is_empty()
        } else {
            !self.host.trim().is_empty()
        }
    }

    pub fn set_protocol(&mut self, protocol: Protocol) {
        if self.protocol == protocol {
            return;
        }
        self.protocol = protocol;
        self.port = protocol.default_port();
        let compatible = matches!(
            (&self.auth, protocol.is_object_store()),
            (Auth::S3Keys { .. }, true) | (Auth::Password(_) | Auth::KeyFile { .. }, false)
        );
        if !compatible {
            self.auth = Auth::for_protocol(protocol);
        }
    }

    pub fn normalize_endpoint(&mut self) {
        let host = self.host.trim().to_owned();

        let split = if let Some(rest) = host.strip_prefix('[') {
            rest.split_once("]:")
                .map(|(addr, port)| (format!("[{addr}]"), port.to_owned()))
        } else if host.matches(':').count() == 1 {
            host.split_once(':')
                .map(|(addr, port)| (addr.to_owned(), port.to_owned()))
        } else {
            None
        };

        match split {
            Some((addr, port)) if port.parse::<u16>().is_ok_and(|p| p > 0) => {
                self.host = addr;
                self.port = port.parse().expect("checked above");
            }
            _ => {
                self.host = host;
                self.port = self.protocol.default_port();
            }
        }
    }
}

impl Default for ConnectionProfile {
    fn default() -> Self {
        Self::new(Protocol::Sftp)
    }
}
