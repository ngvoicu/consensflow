//! Local Network guidance from a failed harness's own error or screen, never a probe.
//!
//! Node's `EHOSTUNREACH` can also mean an absent host. A private address can
//! be routed over a VPN, rather than on a local interface. The hint therefore
//! says *if* the address is local and does not claim that macOS denied access.

use std::net::IpAddr;

/// Adds only what the failure itself can support. Other platforms do not have
/// the macOS setting, and an unrelated network error earns no hint.
pub(crate) fn append_guidance(failed: &mut String, words: &str, from_screen: bool) {
    if !cfg!(target_os = "macos") {
        return;
    }
    match evidence(words) {
        Some(Evidence::Denied) if from_screen => failed.push_str(
            "; its screen showed the macOS local network denial code; turn ConsensFlow on in System Settings > Privacy & Security > Local Network",
        ),
        Some(Evidence::Denied) => failed.push_str(
            "; the harness reported that macOS denied local network access; turn ConsensFlow on in System Settings > Privacy & Security > Local Network",
        ),
        Some(Evidence::Possible) => failed.push_str(
            "; If that address is on your local network, check that ConsensFlow is on in System Settings > Privacy & Security > Local Network; the error does not identify the cause",
        ),
        None => {}
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Evidence {
    Denied,
    Possible,
}

/// Pi puts the underlying Node connection error on the first line of its
/// `errorMessage`. Only an exact error-shaped first line is used, not an
/// occurrence elsewhere in the text. The fully qualified Network framework
/// reason is an explicit denial; the socket error is only a possible one.
fn evidence(words: &str) -> Option<Evidence> {
    let headline = words.lines().next()?.trim();
    if headline == "NWPath.UnsatisfiedReason.localNetworkDenied" {
        return Some(Evidence::Denied);
    }
    let cause = headline
        .strip_prefix("Connection error.: ")
        .unwrap_or(headline);
    let mut parts = cause.split_ascii_whitespace();
    let operation = parts.next()?;
    let error = parts.next()?;
    let destination = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    match (operation, error) {
        ("connect", "EHOSTUNREACH") if private_endpoint(destination) => Some(Evidence::Possible),
        ("getaddrinfo", "ENOTFOUND" | "EAI_AGAIN") if local_name(destination) => {
            Some(Evidence::Possible)
        }
        _ => None,
    }
}

/// Node writes `address:port`; IPv6 can include brackets and an interface zone.
/// Private IPv4 and unique- or link-local IPv6 can also be routed, so they are
/// evidence for a conditional hint, not proof of a local network.
fn private_endpoint(endpoint: &str) -> bool {
    let Some((host, port)) = endpoint.rsplit_once(':') else {
        return false;
    };
    if port.parse::<u16>().is_err() {
        return false;
    }
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let host = host.split('%').next().unwrap_or(host);
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => ip.is_private() || ip.is_link_local(),
        Ok(IpAddr::V6(ip)) => ip.is_unique_local() || ip.is_unicast_link_local(),
        Err(_) => false,
    }
}

/// A `.local` lookup is local by Apple's definition, including a final dot.
fn local_name(name: &str) -> bool {
    let name = name.strip_suffix('.').unwrap_or(name);
    name.len() > ".local".len()
        && name.is_ascii()
        && name.to_ascii_lowercase().ends_with(".local")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::{evidence, Evidence};

    #[test]
    fn only_the_connection_error_can_give_the_conditional_hint() {
        for words in [
            "connect EHOSTUNREACH 10.2.3.4:443",
            "Connection error.: connect EHOSTUNREACH 172.16.2.3:9000",
            "connect EHOSTUNREACH 169.254.10.1:9",
            "connect EHOSTUNREACH [fd00::1]:443",
            "connect EHOSTUNREACH fe80::1%en0:443",
            "getaddrinfo ENOTFOUND server.local",
            "Connection error.: getaddrinfo EAI_AGAIN SERVER.LOCAL.",
        ] {
            assert_eq!(evidence(words), Some(Evidence::Possible), "{words}");
        }
        for words in [
            "connect EHOSTUNREACH 127.0.0.1:443",
            "connect EHOSTUNREACH 172.32.0.1:443",
            "connect EHOSTUNREACH 192.168.1.1:not-a-port",
            "connect ECONNREFUSED 192.168.1.1:443",
            "connect EHOSTUNREACH 8.8.8.8:443",
            "getaddrinfo ENOTFOUND notlocal.example",
            "service says connect EHOSTUNREACH 10.2.3.4:443",
            "error\nconnect EHOSTUNREACH 10.2.3.4:443",
            "localNetworkDenied was not reported",
        ] {
            assert_eq!(evidence(words), None, "{words}");
        }
        assert_eq!(
            evidence("NWPath.UnsatisfiedReason.localNetworkDenied"),
            Some(Evidence::Denied)
        );
    }
}
