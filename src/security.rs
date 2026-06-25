use std::net::{IpAddr, ToSocketAddrs};
use url::Url;

use crate::error::{AppError, Result};

/// Domains that are permitted as scrape / download targets.
const ALLOWED_DOMAINS: &[&str] = &["doe.gov.ph", "www.doe.gov.ph", "prod-cms.doe.gov.ph"];

/// Returns `true` for loopback, private, link-local, multicast, or reserved IPs.
fn is_internal_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || v4.is_multicast()
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_multicast()
                || v6.is_unspecified()
        }
    }
}

/// Validates that:
/// 1. `url` uses the HTTPS scheme.
/// 2. Its hostname is in `ALLOWED_DOMAINS`.
/// 3. The hostname resolves to a public (non-internal) IP (SSRF prevention).
///
/// Uses synchronous DNS via `std::net::ToSocketAddrs` to avoid an extra async
/// boundary — callers should use `tokio::task::spawn_blocking` if needed.
pub fn validate_url(url_str: &str) -> Result<()> {
    if url_str.is_empty() {
        return Err(AppError::SecurityBlocked("Empty URL".to_string()));
    }

    let parsed = Url::parse(url_str)
        .map_err(|e| AppError::SecurityBlocked(format!("Invalid URL: {e}")))?;

    // 1. HTTPS only
    if parsed.scheme() != "https" {
        return Err(AppError::SecurityBlocked(format!(
            "Scheme '{}' is not allowed (must be https)",
            parsed.scheme()
        )));
    }

    // 2. Allowlist domain check
    let host = parsed
        .host_str()
        .ok_or_else(|| AppError::SecurityBlocked("No hostname in URL".to_string()))?
        .to_lowercase();

    if !ALLOWED_DOMAINS.contains(&host.as_str()) {
        return Err(AppError::SecurityBlocked(format!(
            "Domain '{host}' is not in the allowed list"
        )));
    }

    // 3. SSRF — resolve DNS and reject internal IPs
    let addr_str = format!("{host}:443");
    let addrs = addr_str
        .to_socket_addrs()
        .map_err(|e| AppError::SecurityBlocked(format!("DNS resolution failed for {host}: {e}")))?;

    for addr in addrs {
        if is_internal_ip(addr.ip()) {
            return Err(AppError::SecurityBlocked(format!(
                "Host '{host}' resolved to internal IP {}",
                addr.ip()
            )));
        }
    }

    Ok(())
}


