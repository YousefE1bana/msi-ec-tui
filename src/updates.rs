//! Opt-in release discovery. No hardware, installers, or shell execution.
use semver::Version;
use serde::Deserialize;
use std::io::Read;
use std::time::Duration;

/// Official stable-release endpoint. No machine data is sent.
pub const ENDPOINT: &str = "https://api.github.com/repos/YousefE1bana/msi-ec-tui/releases/latest";
/// Human-readable download page (never automatically installed).
pub const RELEASES: &str = "https://github.com/YousefE1bana/msi-ec-tui/releases/latest";
const MAX_BODY: u64 = 65_536;

/// Last explicit check, independent of hardware support and result notices.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UpdateStatus {
    /// No request has been made.
    #[default]
    NotChecked,
    /// One bounded request is in progress.
    Checking,
    /// Installed version equals the latest stable release.
    UpToDate(String),
    /// A newer stable release exists.
    Available(String),
    /// Installed development version is ahead of the latest stable release.
    CurrentNewer(String),
    /// Offline, timeout, rate limiting, or malformed release metadata.
    Failed(String),
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
}

/// Parse a stable release and compare semantic versions, never strings.
pub fn compare_release(current: &str, body: &[u8]) -> Result<UpdateStatus, String> {
    let release: Release =
        serde_json::from_slice(body).map_err(|_| "Invalid release metadata".to_owned())?;
    let latest = Version::parse(
        release
            .tag_name
            .strip_prefix('v')
            .unwrap_or(&release.tag_name),
    )
    .map_err(|_| "Invalid release version".to_owned())?;
    if release.draft || release.prerelease || !latest.pre.is_empty() {
        return Err("Endpoint did not return a stable release".to_owned());
    }
    let installed = Version::parse(current).map_err(|_| "Invalid installed version".to_owned())?;
    let version = latest.to_string();
    Ok(match installed.cmp_precedence(&latest) {
        std::cmp::Ordering::Less => UpdateStatus::Available(version),
        std::cmp::Ordering::Equal => UpdateStatus::UpToDate(version),
        std::cmp::Ordering::Greater => UpdateStatus::CurrentNewer(version),
    })
}

/// Explicit blocking check, with a five-second total timeout and 64 KiB limit.
/// TLS verification stays enabled. Redirects are refused; the endpoint is fixed.
pub fn check(current: &str) -> Result<UpdateStatus, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(5)))
        .max_redirects(0)
        .build()
        .into();
    let mut response = agent
        .get(ENDPOINT)
        .header("User-Agent", concat!("MEC/", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|_| "Check failed: network, timeout, or GitHub rate limit".to_owned())?;
    let mut body = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(MAX_BODY + 1)
        .read_to_end(&mut body)
        .map_err(|_| "Could not read release metadata".to_owned())?;
    if body.len() as u64 > MAX_BODY {
        return Err("Release metadata exceeds size limit".to_owned());
    }
    compare_release(current, &body)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn metadata(tag: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"tag_name":tag,"draft":false,"prerelease":false}))
            .unwrap()
    }
    #[test]
    fn semantic_order_and_optional_v_prefix() {
        assert_eq!(
            compare_release("1.9.0", &metadata("v1.10.0")),
            Ok(UpdateStatus::Available("1.10.0".into()))
        );
        assert_eq!(
            compare_release("1.10.0", &metadata("1.10.0")),
            Ok(UpdateStatus::UpToDate("1.10.0".into()))
        );
        assert_eq!(
            compare_release("1.11.0", &metadata("v1.10.0")),
            Ok(UpdateStatus::CurrentNewer("1.10.0".into()))
        );
    }
    #[test]
    fn rejects_unstable_missing_and_malformed_data() {
        for data in [
            br#"{"tag_name":"v1.2.0","draft":true,"prerelease":false}"#.as_slice(),
            br#"{"tag_name":"v1.2.0","draft":false,"prerelease":true}"#,
            br#"{"tag_name":"v1.2.0-rc.1","draft":false,"prerelease":false}"#,
            b"{}",
            b"garbage",
        ] {
            assert!(compare_release("1.0.1", data).is_err());
        }
        assert!(compare_release("1.0.1", &metadata("../../invalid")).is_err());
        assert!(compare_release("invalid", &metadata("1.2.0")).is_err());
    }
    #[test]
    fn initial_status_makes_no_network_request() {
        assert_eq!(UpdateStatus::default(), UpdateStatus::NotChecked);
    }
    #[test]
    fn build_metadata_does_not_change_release_precedence() {
        assert_eq!(
            compare_release("1.0.1+development", &metadata("v1.0.1")),
            Ok(UpdateStatus::UpToDate("1.0.1".into()))
        );
    }
}
