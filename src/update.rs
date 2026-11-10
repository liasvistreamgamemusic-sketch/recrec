//! Update check against GitHub Releases, and installer download with checksum verification.
//!
//! The asset names (`recrec-<version>-setup.exe`, `SHA256SUMS.txt`) are a contract with
//! `.github/workflows/release.yml`.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

const REPOSITORY_URL: &str = env!("CARGO_PKG_REPOSITORY");
const GITHUB_URL: &str = "https://github.com/";
const GITHUB_API_REPOS_URL: &str = "https://api.github.com/repos/";
const USER_AGENT: &str = concat!("recrec/", env!("CARGO_PKG_VERSION"));
const CHECKSUMS_FILE: &str = "SHA256SUMS.txt";
const PARTIAL_EXTENSION: &str = "part";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(10);
const CHECK_TIMEOUT: Duration = Duration::from_secs(10);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// Release metadata and the checksum list are a few kB; anything bigger is not ours.
const MAX_METADATA_BYTES: u64 = 1024 * 1024;
const MAX_INSTALLER_BYTES: u64 = 200 * 1024 * 1024;
const COPY_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("通信に失敗しました: {0}")]
    Http(#[from] Box<ureq::Error>),
    #[error("ファイルを書き込めません: {0}")]
    Io(#[from] io::Error),
    #[error("リリース情報を解釈できません: {0}")]
    Json(#[from] serde_json::Error),
    #[error("バージョンとして読めないタグです: {0}")]
    BadTag(String),
    #[error("想定外の URL です")]
    UntrustedUrl,
    #[error("このリリースにはインストーラーがありません")]
    NoInstaller,
    #[error("ダウンロードが大きすぎます")]
    TooLarge,
    #[error("チェックサムの一覧にインストーラーが載っていません")]
    ChecksumMissing,
    #[error("チェックサムが一致しません (ダウンロードが壊れています)")]
    ChecksumMismatch,
}

impl From<ureq::Error> for UpdateError {
    fn from(e: ureq::Error) -> Self {
        Self::Http(Box::new(e))
    }
}

/// Where to get the installer of a release and the list to verify it against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installer {
    pub url: String,
    pub checksums_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    /// The release page, for a browser.
    pub page_url: String,
    /// `None` when the release lacks the installer or its checksum list.
    pub installer: Option<Installer>,
}

#[derive(Deserialize)]
struct RawRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    assets: Vec<RawAsset>,
}

#[derive(Deserialize)]
struct RawAsset {
    name: String,
    browser_download_url: String,
}

pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("Cargo guarantees a SemVer package version")
}

fn releases_api_url() -> Option<String> {
    let slug = REPOSITORY_URL.strip_prefix(GITHUB_URL)?;
    Some(format!("{GITHUB_API_REPOS_URL}{slug}/releases/latest"))
}

pub fn installer_file_name(version: &Version) -> String {
    format!("recrec-{version}-setup.exe")
}

/// Only URLs under this repository's releases are ever opened or downloaded.
fn is_release_url(url: &str) -> bool {
    url.strip_prefix(REPOSITORY_URL)
        .is_some_and(|rest| rest.starts_with("/releases/"))
}

/// Reads the GitHub "latest release" response.
pub fn parse_release(json: &[u8]) -> Result<Release, UpdateError> {
    let raw: RawRelease = serde_json::from_slice(json)?;
    let tag = raw.tag_name.strip_prefix('v').unwrap_or(&raw.tag_name);
    let version = Version::parse(tag).map_err(|_| UpdateError::BadTag(raw.tag_name.clone()))?;
    if !is_release_url(&raw.html_url) {
        return Err(UpdateError::UntrustedUrl);
    }
    let installer_name = installer_file_name(&version);
    let asset_url = |name: &str| {
        raw.assets
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.browser_download_url.clone())
    };
    let installer = match (asset_url(&installer_name), asset_url(CHECKSUMS_FILE)) {
        (Some(url), Some(checksums_url)) => {
            if !is_release_url(&url) || !is_release_url(&checksums_url) {
                return Err(UpdateError::UntrustedUrl);
            }
            Some(Installer { url, checksums_url })
        }
        _ => None,
    };
    Ok(Release {
        version,
        page_url: raw.html_url,
        installer,
    })
}

/// Whether the user should be told about `release`.
pub fn is_newer(release: &Release, current: &Version, skipped: Option<&str>) -> bool {
    release.version > *current && skipped != Some(release.version.to_string().as_str())
}

/// Finds the SHA-256 of `file_name` in `sha256sum`-style text, lowercased.
pub fn parse_checksum(sums: &str, file_name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        // `sha256sum` marks binary mode with a leading `*`.
        let name = parts.next()?.trim_start_matches('*');
        (name == file_name).then(|| hash.to_ascii_lowercase())
    })
}

fn agent(total_timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(true)
        .user_agent(USER_AGENT)
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_recv_response(Some(RESPONSE_TIMEOUT))
        .timeout_global(Some(total_timeout))
        .build()
        .new_agent()
}

/// Asks GitHub for the latest published release. Blocks; call from a worker thread.
pub fn fetch_latest() -> Result<Release, UpdateError> {
    let url = releases_api_url().ok_or(UpdateError::UntrustedUrl)?;
    let mut response = agent(CHECK_TIMEOUT)
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .call()?;
    let json = response
        .body_mut()
        .with_config()
        .limit(MAX_METADATA_BYTES)
        .read_to_vec()?;
    parse_release(&json)
}

/// Downloads the installer of `release` into `dir` and verifies it against the release's
/// checksum list. Blocks; call from a worker thread. `on_progress` gets (received, total).
pub fn download_installer(
    release: &Release,
    dir: &Path,
    on_progress: impl FnMut(u64, Option<u64>),
) -> Result<PathBuf, UpdateError> {
    let installer = release.installer.as_ref().ok_or(UpdateError::NoInstaller)?;
    // The name is built from the parsed version, never taken from the network.
    let file_name = installer_file_name(&release.version);
    let agent = agent(DOWNLOAD_TIMEOUT);
    let sums = agent
        .get(&installer.checksums_url)
        .call()?
        .body_mut()
        .with_config()
        .limit(MAX_METADATA_BYTES)
        .read_to_string()?;
    let expected = parse_checksum(&sums, &file_name).ok_or(UpdateError::ChecksumMissing)?;

    let mut response = agent.get(&installer.url).call()?;
    let total = response.body().content_length();
    fs::create_dir_all(dir)?;
    let dest = dir.join(file_name);
    save_verified(
        response.body_mut().as_reader(),
        &dest,
        &expected,
        MAX_INSTALLER_BYTES,
        total,
        on_progress,
    )?;
    Ok(dest)
}

/// Streams `source` to `dest` through a `.part` file, which only becomes `dest` when the
/// SHA-256 matches. On any failure nothing is left behind.
fn save_verified(
    source: impl Read,
    dest: &Path,
    expected_sha256: &str,
    limit: u64,
    total: Option<u64>,
    on_progress: impl FnMut(u64, Option<u64>),
) -> Result<(), UpdateError> {
    let partial = dest.with_extension(PARTIAL_EXTENSION);
    let result = write_and_hash(source, &partial, limit, total, on_progress).and_then(|actual| {
        if actual != expected_sha256 {
            return Err(UpdateError::ChecksumMismatch);
        }
        Ok(fs::rename(&partial, dest)?)
    });
    if result.is_err()
        && let Err(e) = fs::remove_file(&partial)
        && e.kind() != io::ErrorKind::NotFound
    {
        log::warn!("could not remove partial download: {e}");
    }
    result
}

fn write_and_hash(
    mut source: impl Read,
    path: &Path,
    limit: u64,
    total: Option<u64>,
    mut on_progress: impl FnMut(u64, Option<u64>),
) -> Result<String, UpdateError> {
    let mut file = File::create(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; COPY_BUFFER_BYTES];
    let mut received = 0u64;
    loop {
        let n = source.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        received += n as u64;
        if received > limit {
            return Err(UpdateError::TooLarge);
        }
        hasher.update(&buffer[..n]);
        file.write_all(&buffer[..n])?;
        on_progress(received, total);
    }
    file.sync_all()?;
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Deletes leftovers of earlier updates. A missing directory is fine.
pub fn clean_update_dir(dir: &Path) -> io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let path = entry?.path();
        if path.is_file() {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELLO_SHA256: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

    fn release_json(tag: &str, assets: &[(&str, &str)]) -> Vec<u8> {
        let assets: Vec<_> = assets
            .iter()
            .map(|(name, url)| serde_json::json!({ "name": name, "browser_download_url": url }))
            .collect();
        serde_json::to_vec(&serde_json::json!({
            "tag_name": tag,
            "html_url": format!("{REPOSITORY_URL}/releases/tag/{tag}"),
            "prerelease": false,
            "assets": assets,
        }))
        .unwrap()
    }

    fn download_url(tag: &str, name: &str) -> String {
        format!("{REPOSITORY_URL}/releases/download/{tag}/{name}")
    }

    fn release(version: &str) -> Release {
        Release {
            version: Version::parse(version).unwrap(),
            page_url: format!("{REPOSITORY_URL}/releases/tag/v{version}"),
            installer: None,
        }
    }

    #[test]
    fn api_url_is_derived_from_the_repository() {
        let url = releases_api_url().unwrap();
        assert!(url.starts_with("https://api.github.com/repos/"));
        assert!(url.ends_with("/recrec/releases/latest"));
    }

    #[test]
    fn release_with_both_assets_offers_the_installer() {
        let setup = download_url("v0.2.0", "recrec-0.2.0-setup.exe");
        let sums = download_url("v0.2.0", "SHA256SUMS.txt");
        let zip = download_url("v0.2.0", "recrec-0.2.0-windows-x64.zip");
        let json = release_json(
            "v0.2.0",
            &[
                ("recrec-0.2.0-windows-x64.zip", &zip),
                ("recrec-0.2.0-setup.exe", &setup),
                ("SHA256SUMS.txt", &sums),
            ],
        );
        let parsed = parse_release(&json).unwrap();
        assert_eq!(parsed.version, Version::new(0, 2, 0));
        assert_eq!(
            parsed.installer,
            Some(Installer {
                url: setup,
                checksums_url: sums
            })
        );
    }

    #[test]
    fn release_missing_an_asset_has_no_installer() {
        let setup = download_url("v0.2.0", "recrec-0.2.0-setup.exe");
        let json = release_json("v0.2.0", &[("recrec-0.2.0-setup.exe", &setup)]);
        assert_eq!(parse_release(&json).unwrap().installer, None);
        // An installer of another version does not count either.
        let sums = download_url("v0.2.0", "SHA256SUMS.txt");
        let json = release_json(
            "v0.3.0",
            &[
                ("recrec-0.2.0-setup.exe", &setup),
                ("SHA256SUMS.txt", &sums),
            ],
        );
        assert_eq!(parse_release(&json).unwrap().installer, None);
    }

    #[test]
    fn assets_outside_the_repository_are_rejected() {
        for url in [
            "https://example.com/recrec-0.2.0-setup.exe",
            "http://github.com/liasvistreamgamemusic-sketch/recrec/releases/download/v0.2.0/recrec-0.2.0-setup.exe",
            "https://github.com/someone-else/recrec/releases/download/v0.2.0/recrec-0.2.0-setup.exe",
        ] {
            let sums = download_url("v0.2.0", "SHA256SUMS.txt");
            let json = release_json(
                "v0.2.0",
                &[("recrec-0.2.0-setup.exe", url), ("SHA256SUMS.txt", &sums)],
            );
            assert!(
                matches!(parse_release(&json), Err(UpdateError::UntrustedUrl)),
                "{url}"
            );
        }
    }

    #[test]
    fn release_page_outside_the_repository_is_rejected() {
        let json =
            br#"{"tag_name": "v0.2.0", "html_url": "https://example.com/releases/tag/v0.2.0"}"#;
        assert!(matches!(
            parse_release(json),
            Err(UpdateError::UntrustedUrl)
        ));
    }

    #[test]
    fn unreadable_tags_and_broken_json_are_errors() {
        let json = release_json("nightly", &[]);
        assert!(matches!(parse_release(&json), Err(UpdateError::BadTag(t)) if t == "nightly"));
        assert!(matches!(
            parse_release(b"{ not json"),
            Err(UpdateError::Json(_))
        ));
    }

    #[test]
    fn only_strictly_newer_unskipped_releases_are_announced() {
        let current = Version::new(0, 2, 0);
        assert!(is_newer(&release("0.2.1"), &current, None));
        assert!(is_newer(&release("1.0.0"), &current, None));
        assert!(!is_newer(&release("0.2.0"), &current, None));
        assert!(!is_newer(&release("0.1.9"), &current, None));
        assert!(!is_newer(&release("0.2.1"), &current, Some("0.2.1")));
        // Skipping one release does not hide the next.
        assert!(is_newer(&release("0.2.2"), &current, Some("0.2.1")));
    }

    #[test]
    fn checksum_lines_are_matched_by_file_name() {
        let sums = format!(
            "{}  recrec-0.2.0-windows-x64.zip\r\n{}  recrec-0.2.0-setup.exe\r\n",
            "a".repeat(64),
            HELLO_SHA256.to_ascii_uppercase()
        );
        assert_eq!(
            parse_checksum(&sums, "recrec-0.2.0-setup.exe").as_deref(),
            Some(HELLO_SHA256)
        );
        assert_eq!(parse_checksum(&sums, "recrec-0.3.0-setup.exe"), None);
        let binary_mode = format!("{HELLO_SHA256} *recrec-0.2.0-setup.exe\n");
        assert_eq!(
            parse_checksum(&binary_mode, "recrec-0.2.0-setup.exe").as_deref(),
            Some(HELLO_SHA256)
        );
    }

    #[test]
    fn matching_download_is_kept_and_progress_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("setup.exe");
        let mut seen = Vec::new();
        save_verified(
            &b"hello"[..],
            &dest,
            HELLO_SHA256,
            1024,
            Some(5),
            |received, total| seen.push((received, total)),
        )
        .unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"hello");
        assert_eq!(seen.last(), Some(&(5, Some(5))));
        assert!(!dest.with_extension(PARTIAL_EXTENSION).exists());
    }

    #[test]
    fn mismatching_download_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("setup.exe");
        let result = save_verified(&b"hellp"[..], &dest, HELLO_SHA256, 1024, None, |_, _| {});
        assert!(matches!(result, Err(UpdateError::ChecksumMismatch)));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn oversized_download_is_aborted_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("setup.exe");
        let result = save_verified(&b"hello"[..], &dest, HELLO_SHA256, 4, None, |_, _| {});
        assert!(matches!(result, Err(UpdateError::TooLarge)));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn cleaning_removes_old_downloads_and_tolerates_a_missing_dir() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("recrec-0.2.0-setup.exe"), b"x").unwrap();
        fs::write(dir.path().join("recrec-0.2.0-setup.part"), b"x").unwrap();
        clean_update_dir(dir.path()).unwrap();
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        clean_update_dir(&dir.path().join("missing")).unwrap();
    }
}
