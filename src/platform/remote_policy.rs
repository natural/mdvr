use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    time::Duration,
};

use crate::contracts::MAX_RESOURCE_BYTES;

pub const DEFAULT_REMOTE_MAX_URL_BYTES: usize = 2 * 1024;
pub const DEFAULT_REMOTE_MAX_REDIRECTS: usize = 5;
pub const DEFAULT_REMOTE_MAX_RESPONSE_BYTES: u64 = MAX_RESOURCE_BYTES as u64;
pub const DEFAULT_REMOTE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemoteLimits {
    pub max_url_bytes: usize,
    pub max_redirects: usize,
    pub max_response_bytes: u64,
    pub timeout: Duration,
}

impl Default for RemoteLimits {
    fn default() -> Self {
        Self {
            max_url_bytes: DEFAULT_REMOTE_MAX_URL_BYTES,
            max_redirects: DEFAULT_REMOTE_MAX_REDIRECTS,
            max_response_bytes: DEFAULT_REMOTE_MAX_RESPONSE_BYTES,
            timeout: DEFAULT_REMOTE_TIMEOUT,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteRequest {
    pub url: String,
    pub max_response_bytes: u64,
    pub timeout: Duration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RemotePolicyError {
    InvalidLimit(&'static str),
    EmptyUrl,
    UrlTooLong { bytes: usize, max: usize },
    MalformedUrl,
    UnsupportedScheme,
    CredentialsNotAllowed,
    FragmentNotAllowed,
    BlockedAddress(IpAddr),
    RedirectLimitExceeded { followed: usize, max: usize },
    ResponseTooLarge { bytes: u64, max: u64 },
}

/// Pure policy/data layer for future remote image fetching.
///
/// This type never resolves names, opens sockets, follows redirects, or reads
/// response bytes. Callers must run `authorize_redirect` for every target
/// before following it.
pub struct RemotePolicy {
    limits: RemoteLimits,
}

impl RemotePolicy {
    pub fn new(limits: RemoteLimits) -> Result<Self, RemotePolicyError> {
        if limits.max_url_bytes == 0 {
            return Err(RemotePolicyError::InvalidLimit("max_url_bytes"));
        }
        if limits.max_response_bytes == 0 {
            return Err(RemotePolicyError::InvalidLimit("max_response_bytes"));
        }
        if limits.timeout.is_zero() {
            return Err(RemotePolicyError::InvalidLimit("timeout"));
        }
        Ok(Self { limits })
    }

    pub fn limits(&self) -> RemoteLimits {
        self.limits
    }

    pub fn authorize(&self, url: &str) -> Result<RemoteRequest, RemotePolicyError> {
        validate_url(url, self.limits.max_url_bytes)?;
        Ok(self.request(url))
    }

    /// Validate redirect target before redirect count is consumed or followed.
    pub fn authorize_redirect(
        &self,
        followed: usize,
        target: &str,
    ) -> Result<RemoteRequest, RemotePolicyError> {
        let request = self.authorize(target)?;
        if followed >= self.limits.max_redirects {
            return Err(RemotePolicyError::RedirectLimitExceeded {
                followed,
                max: self.limits.max_redirects,
            });
        }
        Ok(request)
    }

    pub fn validate_destination(&self, address: IpAddr) -> Result<(), RemotePolicyError> {
        if is_blocked_address(address) {
            Err(RemotePolicyError::BlockedAddress(address))
        } else {
            Ok(())
        }
    }

    pub fn validate_response_size(&self, bytes: u64) -> Result<(), RemotePolicyError> {
        if bytes > self.limits.max_response_bytes {
            Err(RemotePolicyError::ResponseTooLarge {
                bytes,
                max: self.limits.max_response_bytes,
            })
        } else {
            Ok(())
        }
    }

    fn request(&self, url: &str) -> RemoteRequest {
        RemoteRequest {
            url: url.to_owned(),
            max_response_bytes: self.limits.max_response_bytes,
            timeout: self.limits.timeout,
        }
    }
}

fn validate_url(url: &str, max_bytes: usize) -> Result<(), RemotePolicyError> {
    if url.is_empty() {
        return Err(RemotePolicyError::EmptyUrl);
    }
    if url.len() > max_bytes {
        return Err(RemotePolicyError::UrlTooLong {
            bytes: url.len(),
            max: max_bytes,
        });
    }
    if url
        .bytes()
        .any(|byte| byte <= b' ' || byte == b'\x7f' || byte == b'\\')
    {
        return Err(RemotePolicyError::MalformedUrl);
    }
    validate_percent_escapes(url)?;

    let Some(scheme_end) = url.find("://") else {
        return Err(RemotePolicyError::UnsupportedScheme);
    };
    let scheme = &url[..scheme_end];
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return Err(RemotePolicyError::UnsupportedScheme);
    }
    let remainder = &url[scheme_end + 3..];
    if remainder.is_empty() {
        return Err(RemotePolicyError::MalformedUrl);
    }
    if remainder.contains('#') {
        return Err(RemotePolicyError::FragmentNotAllowed);
    }

    let authority_end = remainder
        .find('/')
        .into_iter()
        .chain(remainder.find('?'))
        .min()
        .unwrap_or(remainder.len());
    let authority = &remainder[..authority_end];
    if authority.is_empty() || authority.contains('@') {
        return if authority.contains('@') {
            Err(RemotePolicyError::CredentialsNotAllowed)
        } else {
            Err(RemotePolicyError::MalformedUrl)
        };
    }

    let host = parse_authority(authority)?;
    if let Ok(address) = host.parse::<IpAddr>() {
        if is_blocked_address(address) {
            return Err(RemotePolicyError::BlockedAddress(address));
        }
    } else {
        validate_hostname(host)?;
    }
    Ok(())
}

fn parse_authority(authority: &str) -> Result<&str, RemotePolicyError> {
    if authority.starts_with('[') {
        let Some(close) = authority.find(']') else {
            return Err(RemotePolicyError::MalformedUrl);
        };
        let host = &authority[1..close];
        if host.parse::<Ipv6Addr>().is_err() {
            return Err(RemotePolicyError::MalformedUrl);
        }
        let suffix = &authority[close + 1..];
        if !suffix.is_empty() {
            let Some(port) = suffix.strip_prefix(':') else {
                return Err(RemotePolicyError::MalformedUrl);
            };
            validate_port(port)?;
        }
        Ok(host)
    } else {
        if authority.matches(':').count() > 1 {
            return Err(RemotePolicyError::MalformedUrl);
        }
        let (host, port) = authority
            .split_once(':')
            .map_or((authority, None), |(host, port)| (host, Some(port)));
        if host.is_empty() {
            return Err(RemotePolicyError::MalformedUrl);
        }
        if let Some(port) = port {
            validate_port(port)?;
        }
        Ok(host)
    }
}

fn validate_port(port: &str) -> Result<(), RemotePolicyError> {
    if port.is_empty()
        || !port.bytes().all(|byte| byte.is_ascii_digit())
        || port.parse::<u16>().is_err()
    {
        Err(RemotePolicyError::MalformedUrl)
    } else {
        Ok(())
    }
}

fn validate_hostname(host: &str) -> Result<(), RemotePolicyError> {
    if host.is_empty() || host.len() > 253 || host.ends_with('.') {
        return Err(RemotePolicyError::MalformedUrl);
    }
    for label in host.split('.') {
        if label.is_empty()
            || label.starts_with('-')
            || label.ends_with('-')
            || !label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(RemotePolicyError::MalformedUrl);
        }
    }
    Ok(())
}

fn validate_percent_escapes(url: &str) -> Result<(), RemotePolicyError> {
    let bytes = url.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'%'
            && (index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit())
        {
            return Err(RemotePolicyError::MalformedUrl);
        }
    }
    Ok(())
}

fn is_blocked_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_blocked_ipv4(address),
        IpAddr::V6(address) => {
            is_blocked_ipv6(address) || address.to_ipv4_mapped().is_some_and(is_blocked_ipv4)
        }
    }
}

fn is_blocked_ipv4(address: Ipv4Addr) -> bool {
    let [a, b, c, _] = address.octets();
    address.is_private()
        || address.is_loopback()
        || address.is_link_local()
        || address.is_unspecified()
        || address.is_broadcast()
        || (a == 100 && (64..=127).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 0 && c == 2)
        || (a == 198 && (b == 18 || b == 19))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || a >= 224
}

fn is_blocked_ipv6(address: Ipv6Addr) -> bool {
    let segments = address.segments();
    let first = segments[0];
    address.is_loopback()
        || address.is_unspecified()
        || (first & 0xfe00) == 0xfc00
        || (first & 0xffc0) == 0xfe80
        || (first & 0xff00) == 0xff00
        || (segments[0] == 0x0064 && segments[1] == 0xff9b)
        || (segments[0] == 0x2001 && segments[1] == 0)
        || (segments[0] == 0x2001 && segments[1] == 0x0db8)
        || segments[0] == 0x2002
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> RemotePolicy {
        RemotePolicy::new(RemoteLimits::default()).unwrap()
    }

    #[test]
    fn allows_public_http_and_https_and_returns_finite_request_data() {
        let policy = policy();
        let request = policy
            .authorize("https://example.com/assets/image.png")
            .unwrap();
        assert_eq!(request.url, "https://example.com/assets/image.png");
        assert_eq!(
            request.max_response_bytes,
            DEFAULT_REMOTE_MAX_RESPONSE_BYTES
        );
        assert_eq!(request.timeout, DEFAULT_REMOTE_TIMEOUT);
        assert!(policy.authorize("http://example.com").is_ok());
        assert!(
            policy
                .validate_destination("93.184.216.34".parse().unwrap())
                .is_ok()
        );
        assert!(matches!(
            policy.validate_destination("127.0.0.1".parse().unwrap()),
            Err(RemotePolicyError::BlockedAddress(_))
        ));
    }

    #[test]
    fn rejects_credentials() {
        let policy = policy();
        for url in [
            "https://user@example.com/image.png",
            "https://user:password@example.com/image.png",
        ] {
            assert_eq!(
                policy.authorize(url),
                Err(RemotePolicyError::CredentialsNotAllowed)
            );
        }
    }

    #[test]
    fn rejects_unsupported_schemes() {
        let policy = policy();
        for url in [
            "file:///tmp/image.png",
            "ftp://example.com/image.png",
            "javascript:alert(1)",
        ] {
            assert_eq!(
                policy.authorize(url),
                Err(RemotePolicyError::UnsupportedScheme)
            );
        }
    }

    #[test]
    fn rejects_non_public_ipv4_literals() {
        let policy = policy();
        for host in [
            "10.0.0.1",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.1.1",
            "127.0.0.1",
            "100.64.0.1",
            "192.0.2.1",
            "198.18.0.1",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "240.0.0.1",
        ] {
            assert!(matches!(
                policy.authorize(&format!("http://{host}/image.png")),
                Err(RemotePolicyError::BlockedAddress(IpAddr::V4(_)))
            ));
        }
    }

    #[test]
    fn rejects_non_public_ipv6_literals() {
        let policy = policy();
        for host in [
            "::1",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "ff02::1",
            "64:ff9b::127.0.0.1",
            "2001::1",
            "2001:db8::1",
            "2002::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(matches!(
                policy.authorize(&format!("http://[{host}]/image.png")),
                Err(RemotePolicyError::BlockedAddress(IpAddr::V6(_)))
            ));
        }
    }

    #[test]
    fn rejects_malformed_urls_and_fragments() {
        let policy = policy();
        for url in [
            "",
            "https://",
            "https:///image.png",
            "https://example.com:bad/image.png",
            "https://example.com:70000/image.png",
            "http://2001:db8::1/image.png",
            "https://example.com/%zz",
        ] {
            assert_eq!(
                policy.authorize(url),
                if url.is_empty() {
                    Err(RemotePolicyError::EmptyUrl)
                } else if !url.contains("://") {
                    Err(RemotePolicyError::UnsupportedScheme)
                } else {
                    Err(RemotePolicyError::MalformedUrl)
                }
            );
        }
        assert_eq!(
            policy.authorize("https://example.com/image.png#fragment"),
            Err(RemotePolicyError::FragmentNotAllowed)
        );
    }

    #[test]
    fn enforces_redirect_limit_and_validates_target_first() {
        let policy = policy();
        for followed in 0..DEFAULT_REMOTE_MAX_REDIRECTS {
            assert!(
                policy
                    .authorize_redirect(followed, "https://example.com/next")
                    .is_ok()
            );
        }
        assert_eq!(
            policy.authorize_redirect(DEFAULT_REMOTE_MAX_REDIRECTS, "https://example.com/next"),
            Err(RemotePolicyError::RedirectLimitExceeded {
                followed: DEFAULT_REMOTE_MAX_REDIRECTS,
                max: DEFAULT_REMOTE_MAX_REDIRECTS,
            })
        );
        assert_eq!(
            policy.authorize_redirect(DEFAULT_REMOTE_MAX_REDIRECTS, "http://127.0.0.1/next"),
            Err(RemotePolicyError::BlockedAddress(IpAddr::V4(
                Ipv4Addr::new(127, 0, 0, 1,)
            )))
        );
    }

    #[test]
    fn enforces_response_and_timeout_limits() {
        let policy = policy();
        assert!(
            policy
                .validate_response_size(DEFAULT_REMOTE_MAX_RESPONSE_BYTES)
                .is_ok()
        );
        assert_eq!(
            policy.validate_response_size(DEFAULT_REMOTE_MAX_RESPONSE_BYTES + 1),
            Err(RemotePolicyError::ResponseTooLarge {
                bytes: DEFAULT_REMOTE_MAX_RESPONSE_BYTES + 1,
                max: DEFAULT_REMOTE_MAX_RESPONSE_BYTES,
            })
        );
        assert!(policy.limits().timeout > Duration::ZERO);
        assert!(
            RemotePolicy::new(RemoteLimits {
                timeout: Duration::ZERO,
                ..RemoteLimits::default()
            })
            .is_err()
        );
        assert!(
            RemotePolicy::new(RemoteLimits {
                max_response_bytes: 0,
                ..RemoteLimits::default()
            })
            .is_err()
        );
    }
}
