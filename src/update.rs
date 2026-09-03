use std::sync::mpsc::{Receiver, Sender};

const LATEST_RELEASE_API: &str = "https://api.github.com/repos/Rinny-Inc/rwinSCP/releases/latest";

const USER_AGENT: &str = concat!("rwinSCP/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone)]
pub struct Available {
    pub version: String,
    pub url: String,
}

pub struct UpdateCheck {
    rx: Receiver<Available>,
}

impl UpdateCheck {
    pub fn spawn() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || check(&tx));
        Self { rx }
    }

    pub fn poll(&self) -> Option<Available> {
        self.rx.try_recv().ok()
    }
}

fn check(tx: &Sender<Available>) {
    let Ok(response) = ureq::get(LATEST_RELEASE_API)
        .set("User-Agent", USER_AGENT)
        .set("Accept", "application/vnd.github+json")
        .call()
    else {
        return;
    };

    let Ok(json) = response.into_json::<serde_json::Value>() else {
        return;
    };

    let Some(tag) = json.get("tag_name").and_then(|t| t.as_str()) else {
        return;
    };
    let url = json
        .get("html_url")
        .and_then(|u| u.as_str())
        .unwrap_or("https://github.com/Rinny-Inc/rwinSCP/releases")
        .to_owned();

    if is_newer(tag, env!("CARGO_PKG_VERSION")) {
        tx.send(Available {
            version: tag.trim_start_matches('v').to_owned(),
            url,
        })
        .ok();
    }
}

fn is_newer(candidate: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> {
        v.trim()
            .trim_start_matches(['v', 'V'])
            .split(['-', '+'])
            .next()
            .unwrap_or("")
            .split('.')
            .map(|part| part.parse().unwrap_or(0))
            .collect()
    };

    let (candidate, current) = (parse(candidate), parse(current));
    if candidate.is_empty() {
        return false;
    }

    let len = candidate.len().max(current.len());
    for i in 0..len {
        let a = candidate.get(i).copied().unwrap_or(0);
        let b = current.get(i).copied().unwrap_or(0);
        if a != b {
            return a > b;
        }
    }
    false
}

pub fn open_in_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(all(unix, not(target_os = "macos")))]
    let program = "xdg-open";

    std::process::Command::new(program).arg(url).spawn().ok();
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn detects_a_newer_release() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("0.1.1", "0.1.0"));
        assert!(is_newer("v1.0.0", "0.9.9"));
    }

    #[test]
    fn ignores_same_or_older() {
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("v0.1.0", "0.2.0"));
        assert!(!is_newer("v0.9.9", "1.0.0"));
    }

    #[test]
    fn compares_numerically_not_lexically() {
        assert!(is_newer("v0.10.0", "0.9.0"));
        assert!(!is_newer("v0.9.0", "0.10.0"));
    }

    #[test]
    fn tolerates_odd_tags() {
        assert!(!is_newer("", "0.1.0"));
        assert!(!is_newer("not-a-version", "0.1.0"));
        assert!(is_newer("v0.2.0-rc1", "0.1.0"));
        assert!(!is_newer("v0.1", "0.1.0"));
    }
}
