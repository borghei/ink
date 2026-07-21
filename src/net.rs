//! Hardened HTTP fetching.
//!
//! Two blocking clients, both with timeouts and bounded redirects:
//! - `user_client`: for URLs the user typed on the command line. Private
//!   hosts are allowed (the user asked for them).
//! - `content_client`: for URLs found inside documents (remote images).
//!   Every redirect hop re-validates the target host, so a public URL cannot
//!   bounce the request to localhost or the cloud metadata endpoint.
//!
//! All fetches enforce a hard response-size cap via streamed reads — a
//! Content-Length header is checked first, but never trusted as the only
//! guard.

use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use std::io::Read;
use std::net::{IpAddr, ToSocketAddrs};
use std::sync::OnceLock;
use std::time::Duration;

/// Cap for documents fetched from a user-supplied URL.
pub const DOC_FETCH_CAP: u64 = 10 * 1024 * 1024;
/// Cap for images referenced inside documents.
pub const IMAGE_FETCH_CAP: u64 = 20 * 1024 * 1024;

const USER_AGENT: &str = concat!("ink-md/", env!("CARGO_PKG_VERSION"));

fn user_client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::limited(5))
            .user_agent(USER_AGENT)
            .build()
            .expect("http client")
    })
}

fn content_client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        let policy = reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() > 5 {
                return attempt.error("too many redirects");
            }
            match attempt.url().host_str() {
                Some(host) if !is_private_host(host) => attempt.follow(),
                _ => attempt.error("redirect to private or unresolvable host"),
            }
        });
        Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .redirect(policy)
            .user_agent(USER_AGENT)
            .build()
            .expect("http client")
    })
}

/// True if `host` is (or resolves to) a loopback, private, link-local, or
/// otherwise non-public address. Used to keep document-driven fetches from
/// probing the local machine or network (SSRF).
pub fn is_private_host(host: &str) -> bool {
    if let Ok(ip) = host.trim_matches(['[', ']']).parse::<IpAddr>() {
        return is_private_ip(&ip);
    }
    match (host, 0u16).to_socket_addrs() {
        Ok(addrs) => {
            let mut any = false;
            for addr in addrs {
                any = true;
                if is_private_ip(&addr.ip()) {
                    return true;
                }
            }
            !any // unresolvable → treat as private/blocked
        }
        Err(_) => true,
    }
}

fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                // Carrier-grade NAT 100.64.0.0/10
                || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xc0) == 64)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                // Unique-local fc00::/7
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                // Link-local fe80::/10
                || (v6.segments()[0] & 0xffc0) == 0xfe80
                // IPv4-mapped: check the embedded v4
                || v6
                    .to_ipv4_mapped()
                    .map(|v4| is_private_ip(&IpAddr::V4(v4)))
                    .unwrap_or(false)
        }
    }
}

/// Fetch a text document from a user-supplied URL (timeouts + size cap; no
/// private-host restriction — the user explicitly asked for this URL).
pub fn fetch_text(url: &str, cap: u64) -> Result<String> {
    let resp = user_client()
        .get(url)
        .send()
        .with_context(|| format!("cannot fetch '{url}'"))?
        .error_for_status()
        .with_context(|| format!("cannot fetch '{url}'"))?;
    let bytes = read_capped(resp, cap, url)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Fetch bytes for a URL found inside a document (remote image). Refuses
/// private hosts outright and re-validates on every redirect hop.
pub fn fetch_untrusted_bytes(url: &str, cap: u64) -> Result<Vec<u8>> {
    let parsed: reqwest::Url = url.parse().with_context(|| format!("bad url '{url}'"))?;
    match parsed.host_str() {
        Some(host) if !is_private_host(host) => {}
        _ => bail!("refusing to fetch from private or unresolvable host"),
    }
    let resp = content_client()
        .get(parsed)
        .send()
        .with_context(|| format!("cannot fetch '{url}'"))?
        .error_for_status()
        .with_context(|| format!("cannot fetch '{url}'"))?;
    read_capped(resp, cap, url)
}

fn read_capped(resp: reqwest::blocking::Response, cap: u64, url: &str) -> Result<Vec<u8>> {
    if let Some(len) = resp.content_length() {
        if len > cap {
            bail!("'{url}' is too large ({len} bytes; limit {cap})");
        }
    }
    let mut buf = Vec::new();
    resp.take(cap + 1)
        .read_to_end(&mut buf)
        .with_context(|| format!("error reading '{url}'"))?;
    if buf.len() as u64 > cap {
        bail!("'{url}' is too large (limit {cap} bytes)");
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_ips_detected() {
        for host in [
            "127.0.0.1",
            "10.0.0.5",
            "172.16.1.1",
            "192.168.1.1",
            "169.254.169.254",
            "0.0.0.0",
            "100.64.0.1",
            "[::1]",
            "[fc00::1]",
            "[fe80::1]",
            "[::ffff:127.0.0.1]",
        ] {
            assert!(is_private_host(host), "{host} should be private");
        }
    }

    #[test]
    fn public_ips_pass() {
        for host in ["1.1.1.1", "93.184.216.34", "[2606:4700::1111]"] {
            assert!(!is_private_host(host), "{host} should be public");
        }
    }
}
