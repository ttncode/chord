//! Tray "Check for updates": newest GitHub release → download its installer → run it silently.

/// Parsed from the GitHub "latest release" JSON: version and installer URL, if it has one.
#[derive(Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub installer: String,
}

pub fn parse_latest(json: &str) -> Option<Release> {
    let tag = regex::Regex::new(r#""tag_name"\s*:\s*"v(\d+\.\d+\.\d+)""#).unwrap();
    let version = tag.captures(json)?[1].to_string();
    // Only this repo's own release download, so a tampered response can't point the installer elsewhere.
    let pattern = format!(
        r#""browser_download_url"\s*:\s*"(https://github\.com/ttncode/chord/releases/download/v{0}/chord-setup-{0}\.exe)""#,
        regex::escape(&version)
    );
    let installer = regex::Regex::new(&pattern).unwrap().captures(json)?[1].to_string();
    Some(Release { version, installer })
}

/// True when `latest` (e.g. "0.3.0") is a higher version than `current`.
pub fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |v: &str| v.split('.').map(|n| n.parse::<u32>().ok()).collect::<Option<Vec<_>>>();
    matches!((parse(latest), parse(current)), (Some(l), Some(c)) if l > c)
}

#[cfg(windows)]
pub fn check_in_background() {
    std::thread::spawn(check);
}

#[cfg(windows)]
fn check() {
    use crate::win::message;
    let current = env!("CARGO_PKG_VERSION");
    let Some(json) = curl(&["-H", "User-Agent: chord", "https://api.github.com/repos/ttncode/chord/releases/latest"]) else {
        return message("Could not reach GitHub to check for updates. Check the internet connection and try again.");
    };
    let Some(release) = parse_latest(&json) else {
        return message("Could not read the latest chord release from GitHub. Try again later.");
    };
    if !is_newer(&release.version, current) {
        return message(&format!("chord {current} is the latest version."));
    }
    if !ask(&format!("chord {} is available (you have {current}).\n\nDownload and install it now? chord restarts by itself.", release.version)) {
        return;
    }
    let path = std::env::temp_dir().join(format!("chord-setup-{}.exe", release.version));
    let path_text = path.to_string_lossy().into_owned();
    if curl(&["-o", &path_text, &release.installer]).is_none() {
        return message("Could not download the update. Try again later.");
    }
    // The installer closes this chord before copying files, then starts the new one.
    if let Err(e) = std::process::Command::new(&path).args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART"]).spawn() {
        message(&format!("Could not start the update installer: {e}"));
    }
}

/// curl.exe ships with Windows 10 1803+; stdout on success.
#[cfg(windows)]
fn curl(args: &[&str]) -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new("curl.exe")
        .args(["-sfL", "--max-time", "120"])
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(windows)]
fn ask(text: &str) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{IDYES, MB_ICONQUESTION, MB_YESNO, MessageBoxW};
    use windows::core::{HSTRING, w};
    unsafe { MessageBoxW(None, &HSTRING::from(text), w!("chord"), MB_YESNO | MB_ICONQUESTION) == IDYES }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{"url":"x","tag_name": "v0.3.0","name":"chord v0.3.0","assets":[
        {"name":"SHA256SUMS.txt","browser_download_url":"https://github.com/ttncode/chord/releases/download/v0.3.0/SHA256SUMS.txt"},
        {"name":"chord-setup-0.3.0.exe","browser_download_url":"https://github.com/ttncode/chord/releases/download/v0.3.0/chord-setup-0.3.0.exe"}]}"#;

    #[test]
    fn reads_version_and_installer() {
        assert_eq!(
            parse_latest(JSON),
            Some(Release {
                version: "0.3.0".into(),
                installer: "https://github.com/ttncode/chord/releases/download/v0.3.0/chord-setup-0.3.0.exe".into(),
            })
        );
    }

    #[test]
    fn rejects_installer_from_anywhere_but_this_repo() {
        let evil = JSON.replace("github.com/ttncode/chord/releases/download/v0.3.0/chord-setup", "evil.example/chord-setup");
        assert_eq!(parse_latest(&evil), None);
    }

    #[test]
    fn no_installer_or_garbage_is_none() {
        assert_eq!(parse_latest(r#"{"tag_name":"v0.3.0","assets":[]}"#), None);
        assert_eq!(parse_latest(r#"{"message":"API rate limit exceeded"}"#), None);
        assert_eq!(parse_latest(""), None);
    }

    #[test]
    fn compares_versions_numerically() {
        assert!(is_newer("0.3.0", "0.2.1"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0.0", "0.99.0"));
        assert!(!is_newer("0.2.1", "0.2.1"));
        assert!(!is_newer("0.2.0", "0.2.1"));
        assert!(!is_newer("garbage", "0.2.1"));
    }
}
