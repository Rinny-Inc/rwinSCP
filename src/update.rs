use std::{
    env,
    sync::mpsc::{Receiver, Sender},
};

const LATEST_RELEASE_API: &str = "https://api.github.com/repos/Rinny-Inc/rwinSCP/releases/latest";

const USER_AGENT: &str = concat!("rwinSCP/", env!("CARGO_PKG_VERSION"));

const ASSET_SUFFIX: Option<&str> = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
    Some("macos-arm64.dmg")
} else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
    Some("macos-x64.dmg")
} else if cfg!(target_os = "linux") {
    Some("linux-x64.AppImage")
} else if cfg!(target_os = "windows") {
    Some("windows-x64-setup.exe")
} else {
    None
};

#[derive(Debug, Clone)]
pub struct Available {
    pub version: String,
    pub url: String,
    pub asset: Option<Asset>,
}

#[derive(Debug, Clone)]
pub struct Asset {
    pub name: String,
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

    if !is_newer(tag, env!("CARGO_PKG_VERSION")) {
        return;
    }
    let url = json
        .get("html_url")
        .and_then(|u| u.as_str())
        .unwrap_or("https://github.com/Rinny-Inc/rwinSCP/releases")
        .to_owned();

    tx.send(Available {
        version: tag.trim_start_matches('v').to_owned(),
        url,
        asset: platform_asset(&json),
    })
    .ok();
}

fn platform_asset(release: &serde_json::Value) -> Option<Asset> {
    let suffix = ASSET_SUFFIX?;

    release.get("assets")?.as_array()?.iter().find_map(|asset| {
        let name = asset.get("name")?.as_str()?;
        if !name.ends_with(suffix) {
            return None;
        }
        Some(Asset {
            name: name.to_owned(),
            url: asset.get("browser_download_url")?.as_str()?.to_owned(),
        })
    })
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
    use super::{is_newer, platform_asset};

    #[test]
    fn picks_the_asset_for_this_platform() {
        let release = serde_json::json!({
            "assets": [
                { "name": "rwinSCP-macos-arm64.dmg",
                  "browser_download_url": "https://example.invalid/arm64.dmg" },
                { "name": "rwinSCP-macos-x64.dmg",
                  "browser_download_url": "https://example.invalid/x64.dmg" },
                { "name": "rwinSCP-0.9.7-linux-x64.AppImage",
                  "browser_download_url": "https://example.invalid/app.AppImage" },
                { "name": "rwinSCP-0.9.7-linux-x64.deb",
                  "browser_download_url": "https://example.invalid/pkg.deb" },
                { "name": "rwinSCP-0.9.7-windows-x64-setup.exe",
                  "browser_download_url": "https://example.invalid/setup.exe" }
            ]
        });

        let asset = platform_asset(&release).expect("this platform has an asset");
        let expected = super::ASSET_SUFFIX.expect("this platform builds an asset");
        assert!(
            asset.name.ends_with(expected),
            "picked {} for a platform expecting {expected}",
            asset.name
        );
    }

    #[test]
    fn missing_asset_is_not_invented() {
        let release = serde_json::json!({
            "assets": [
                { "name": "source.tar.gz",
                  "browser_download_url": "https://example.invalid/source.tar.gz" }
            ]
        });
        assert!(platform_asset(&release).is_none());
    }

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
