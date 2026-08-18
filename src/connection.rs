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

    pub fn label(&self) -> &'static str {
        match self {
            Protocol::Ssh => "SSH (shell)",
            Protocol::Sftp => "SFTP",
            Protocol::Scp => "SCP",
            Protocol::Ftp => "FTP",
            Protocol::S3 => "S3",
        }
    }

    pub fn default_port(&self) -> u16 {
        match self {
            Protocol::Ssh | Protocol::Sftp | Protocol::Scp => 22,
            Protocol::Ftp => 21,
            Protocol::S3 => 443,
        }
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Auth {
    Password(String),
    KeyFile {
        path: String,
        passphrase: String,
    },
    /// S3 access key / secret key pair.
    S3Keys {
        access_key: String,
        secret_key: String,
    },
}

#[derive(Debug, Clone)]
pub struct ConnectionProfile {
    pub name: String,
    pub protocol: Protocol,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: Auth,
    /// S3 bucket name / region, unused for other protocols
    pub bucket: String,
    pub region: String,
    pub remote_start_dir: String,
}

impl Default for ConnectionProfile {
    fn default() -> Self {
        Self {
            name: "New connection".into(),
            protocol: Protocol::Sftp,
            host: String::new(),
            port: Protocol::Sftp.default_port(),
            username: String::new(),
            auth: Auth::Password(String::new()),
            bucket: String::new(),
            region: "us-east-1".into(),
            remote_start_dir: "/".into(),
        }
    }
}
