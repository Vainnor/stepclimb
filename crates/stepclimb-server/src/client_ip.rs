// SPDX-License-Identifier: AGPL-3.0-or-later
//! The address a request came from, for the rate limits (docs/specs/m0/08-sessions-and-csrf.md
//! B22, Q7).
//!
//! [`TrustedProxies::client_ip`] starts from the TCP peer's address. Only when that peer is a
//! trusted proxy (`STEPCLIMB_TRUSTED_PROXIES`, none unless configured) does it read
//! `X-Forwarded-For`, and of that header only the right-most address that is not itself a
//! trusted proxy. Each proxy appends the address it received the request from, so the entries
//! left of the first untrusted one were written by a party the server does not trust and are
//! never read. No other header (`Forwarded`, `X-Real-IP`, ...) is read.
//!
//! A trusted range must hold proxies only, never clients: a client inside it could name any
//! address it likes. Ranges wider than /8 (IPv4) or /32 (IPv6) are refused, so `0.0.0.0/0` or
//! `::/0` cannot trust the whole internet by mistake.

use std::fmt;
use std::net::IpAddr;
use std::sync::Arc;

use axum::http::HeaderMap;

/// The header a trusted proxy appends the client's address to.
pub const X_FORWARDED_FOR: &str = "x-forwarded-for";

/// The proxies whose `X-Forwarded-For` the server believes: exact addresses and CIDR ranges.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrustedProxies(Arc<[Network]>);

/// One address (`10.201.73.2`) or range (`10.201.73.0/24`, `fd00::/32`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Network {
    base: IpAddr,
    prefix: u8,
}

/// Why an entry is not an address or range. The configuration error adds the setting's name and
/// the entry's position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TrustedProxyError {
    #[error("is not an IP address or CIDR range")]
    NotAnAddress,
    #[error("has a prefix longer than the address (at most 32 for IPv4, 128 for IPv6)")]
    PrefixTooLong,
    #[error(
        "is wider than /8 (IPv4) or /32 (IPv6); a trusted range must hold proxies only, never \
         clients"
    )]
    PrefixTooShort,
    #[error("has bits set after its prefix; write the range's first address, such as 10.0.0.0/8")]
    HostBitsSet,
    #[error("is an IPv4-mapped IPv6 address; write the IPv4 address instead")]
    Ipv4Mapped,
}

impl TrustedProxies {
    /// No trusted proxy: every request's client is its TCP peer.
    pub fn none() -> Self {
        Self::default()
    }

    /// Parse a comma-separated list. Blanks around an entry are ignored, an empty entry is not.
    /// An error carries the 1-based position of the entry.
    pub fn parse(value: &str) -> Result<Self, (usize, TrustedProxyError)> {
        value
            .split(',')
            .enumerate()
            .map(|(index, entry)| Network::parse(entry.trim()).map_err(|error| (index + 1, error)))
            .collect::<Result<Vec<_>, _>>()
            .map(|networks| Self(networks.into()))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn contains(&self, address: IpAddr) -> bool {
        self.0.iter().any(|network| network.contains(address))
    }

    /// The client of a request whose TCP peer is `peer`. An untrusted peer is the client. From
    /// a trusted peer, the right-most `X-Forwarded-For` entry that is not a trusted proxy is the
    /// client; when that entry is not an address, or the header is absent or names only trusted
    /// proxies, the nearest proxy is (the last trusted address walked), so a malformed header
    /// can never move the key to an address the attacker chose.
    pub fn client_ip(&self, peer: IpAddr, headers: &HeaderMap) -> IpAddr {
        let mut nearest = peer.to_canonical();
        if !self.contains(nearest) {
            return nearest;
        }
        for entry in forwarded_for(headers).into_iter().rev() {
            let Some(address) = parse_address(entry) else {
                return nearest;
            };
            if !self.contains(address) {
                return address;
            }
            nearest = address;
        }
        nearest
    }
}

impl fmt::Display for TrustedProxies {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, network) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str(",")?;
            }
            write!(f, "{network}")?;
        }
        Ok(())
    }
}

impl Network {
    fn parse(entry: &str) -> Result<Self, TrustedProxyError> {
        let (address, prefix) = match entry.split_once('/') {
            Some((address, prefix)) => (address, Some(prefix)),
            None => (entry, None),
        };
        let base: IpAddr = address
            .parse()
            .map_err(|_| TrustedProxyError::NotAnAddress)?;
        if let IpAddr::V6(v6) = base
            && v6.to_ipv4_mapped().is_some()
        {
            return Err(TrustedProxyError::Ipv4Mapped);
        }
        let bits = address_bits(base);
        let prefix = match prefix {
            None => bits,
            // `u8::from_str` takes a leading `+`; a prefix is digits only.
            Some(p) if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) => {
                return Err(TrustedProxyError::NotAnAddress);
            }
            Some(p) => p.parse().map_err(|_| TrustedProxyError::PrefixTooLong)?,
        };
        if prefix > bits {
            return Err(TrustedProxyError::PrefixTooLong);
        }
        if prefix < min_prefix(base) {
            return Err(TrustedProxyError::PrefixTooShort);
        }
        let network = Self { base, prefix };
        if network.masked(base) != base {
            return Err(TrustedProxyError::HostBitsSet);
        }
        Ok(network)
    }

    fn contains(&self, address: IpAddr) -> bool {
        match (self.base, address) {
            (IpAddr::V4(_), IpAddr::V4(_)) | (IpAddr::V6(_), IpAddr::V6(_)) => {
                self.masked(address) == self.base
            }
            _ => false,
        }
    }

    /// `address` with every bit after this network's prefix cleared.
    fn masked(&self, address: IpAddr) -> IpAddr {
        match address {
            IpAddr::V4(v4) => {
                let mask = u32::MAX
                    .checked_shl(32 - u32::from(self.prefix))
                    .unwrap_or(0);
                IpAddr::V4((u32::from(v4) & mask).into())
            }
            IpAddr::V6(v6) => {
                let mask = u128::MAX
                    .checked_shl(128 - u32::from(self.prefix))
                    .unwrap_or(0);
                IpAddr::V6((u128::from(v6) & mask).into())
            }
        }
    }
}

impl fmt::Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.prefix == address_bits(self.base) {
            write!(f, "{}", self.base)
        } else {
            write!(f, "{}/{}", self.base, self.prefix)
        }
    }
}

/// The widest range that may be trusted: /8 for IPv4, /32 for IPv6.
fn min_prefix(address: IpAddr) -> u8 {
    match address {
        IpAddr::V4(_) => 8,
        IpAddr::V6(_) => 32,
    }
}

fn address_bits(address: IpAddr) -> u8 {
    match address {
        IpAddr::V4(_) => 32,
        IpAddr::V6(_) => 128,
    }
}

/// Every `X-Forwarded-For` entry, left to right. Several header lines are one list in the order
/// they arrived (RFC 9110 section 5.3).
fn forwarded_for(headers: &HeaderMap) -> Vec<&[u8]> {
    headers
        .get_all(X_FORWARDED_FOR)
        .iter()
        .flat_map(|value| value.as_bytes().split(|&byte| byte == b','))
        .collect()
}

/// One entry as a bare address, IPv4-mapped IPv6 as IPv4. An entry with a port, brackets or
/// anything else is not an address.
fn parse_address(entry: &[u8]) -> Option<IpAddr> {
    let text = std::str::from_utf8(entry).ok()?.trim_matches([' ', '\t']);
    text.parse::<IpAddr>()
        .ok()
        .map(|address| address.to_canonical())
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderName, HeaderValue};

    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    fn headers(pairs: &[(&str, &[u8])]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.append(
                HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_bytes(value).unwrap(),
            );
        }
        headers
    }

    fn xff(value: &str) -> HeaderMap {
        headers(&[("x-forwarded-for", value.as_bytes())])
    }

    fn caddy() -> TrustedProxies {
        TrustedProxies::parse("10.201.73.2").unwrap()
    }

    #[test]
    fn entries_parse_as_addresses_and_ranges() {
        let trusted = TrustedProxies::parse(" 10.201.73.2 ,10.0.0.0/8,fd00:1::/32, ::1").unwrap();
        assert_eq!(
            trusted.to_string(),
            "10.201.73.2,10.0.0.0/8,fd00:1::/32,::1"
        );
        for inside in ["10.201.73.2", "10.255.0.1", "fd00:1::5", "::1"] {
            assert!(trusted.contains(ip(inside)), "{inside}");
        }
        for outside in ["192.0.2.1", "11.0.0.1", "fe80::1", "::2", "0.0.0.0"] {
            assert!(!trusted.contains(ip(outside)), "{outside}");
        }
        let widest = TrustedProxies::parse("10.0.0.0/8,2001:db8::/32").unwrap();
        assert!(widest.contains(ip("10.1.2.3")));
        assert!(widest.contains(ip("2001:db8:ffff::1")));
        assert!(!widest.contains(ip("11.0.0.1")));
        assert!(
            !TrustedProxies::parse("10.0.0.0/8")
                .unwrap()
                .contains(ip("::a00:1"))
        );
    }

    #[test]
    fn bad_entries_are_refused_by_position() {
        for (value, expected) in [
            ("10.0.0.1,", (2, TrustedProxyError::NotAnAddress)),
            ("caddy", (1, TrustedProxyError::NotAnAddress)),
            ("10.0.0.1:443", (1, TrustedProxyError::NotAnAddress)),
            ("10.0.0.0/", (1, TrustedProxyError::NotAnAddress)),
            ("10.0.0.0/+8", (1, TrustedProxyError::NotAnAddress)),
            ("10.0.0.0/33", (1, TrustedProxyError::PrefixTooLong)),
            ("10.0.0.0/999", (1, TrustedProxyError::PrefixTooLong)),
            ("::/129", (1, TrustedProxyError::PrefixTooLong)),
            ("0.0.0.0/0", (1, TrustedProxyError::PrefixTooShort)),
            ("10.0.0.0/7", (1, TrustedProxyError::PrefixTooShort)),
            ("10.0.0.1, ::/0", (2, TrustedProxyError::PrefixTooShort)),
            ("2001:db8::/31", (1, TrustedProxyError::PrefixTooShort)),
            ("10.0.0.1/8", (1, TrustedProxyError::HostBitsSet)),
            ("10.0.0.1, fd00::1/32", (2, TrustedProxyError::HostBitsSet)),
            ("::ffff:10.0.0.1", (1, TrustedProxyError::Ipv4Mapped)),
        ] {
            assert_eq!(TrustedProxies::parse(value), Err(expected), "{value}");
        }
    }

    #[test]
    fn an_untrusted_peer_is_the_client_whatever_it_forwards() {
        let peer = ip("198.51.100.7");
        assert_eq!(caddy().client_ip(peer, &xff("203.0.113.9")), peer);
        assert_eq!(
            TrustedProxies::none().client_ip(peer, &xff("203.0.113.9")),
            peer
        );
    }

    #[test]
    fn behind_a_trusted_peer_the_forwarded_address_is_the_client() {
        assert_eq!(
            caddy().client_ip(ip("10.201.73.2"), &xff("203.0.113.9")),
            ip("203.0.113.9")
        );
    }

    #[test]
    fn the_right_most_untrusted_entry_wins_over_spoofed_ones_to_its_left() {
        let trusted = TrustedProxies::parse("10.201.73.2,10.0.0.0/24").unwrap();
        let forwarded = xff("1.1.1.1, 203.0.113.9, 10.0.0.5");
        assert_eq!(
            trusted.client_ip(ip("10.201.73.2"), &forwarded),
            ip("203.0.113.9")
        );
    }

    #[test]
    fn several_header_lines_are_one_list_in_order() {
        let forwarded = headers(&[
            ("x-forwarded-for", b"1.1.1.1"),
            ("x-forwarded-for", b"203.0.113.9"),
        ]);
        assert_eq!(
            caddy().client_ip(ip("10.201.73.2"), &forwarded),
            ip("203.0.113.9")
        );
    }

    #[test]
    fn a_malformed_or_missing_entry_leaves_the_nearest_proxy_as_the_client() {
        let trusted = TrustedProxies::parse("10.201.73.2,10.0.0.5").unwrap();
        let peer = ip("10.201.73.2");
        for value in [
            "203.0.113.9:1234",
            "1.1.1.1, unknown",
            "1.1.1.1,",
            "",
            "[2001:db8::1]",
        ] {
            assert_eq!(trusted.client_ip(peer, &xff(value)), peer, "{value:?}");
        }
        assert_eq!(trusted.client_ip(peer, &HeaderMap::new()), peer);
        assert_eq!(
            trusted.client_ip(peer, &headers(&[("x-forwarded-for", b"1.1.1.1, \xff")])),
            peer
        );
        // Through two trusted hops, a bad entry stops at the second hop.
        assert_eq!(
            trusted.client_ip(peer, &xff("1.1.1.1, nonsense, 10.0.0.5")),
            ip("10.0.0.5")
        );
        // A chain of trusted proxies only: the farthest one is all that is known.
        assert_eq!(trusted.client_ip(peer, &xff("10.0.0.5")), ip("10.0.0.5"));
    }

    #[test]
    fn only_x_forwarded_for_is_read() {
        let other = headers(&[
            ("forwarded", b"for=203.0.113.9"),
            ("x-real-ip", b"203.0.113.9"),
            ("true-client-ip", b"203.0.113.9"),
            ("cf-connecting-ip", b"203.0.113.9"),
        ]);
        let peer = ip("10.201.73.2");
        assert_eq!(caddy().client_ip(peer, &other), peer);
    }

    #[test]
    fn ipv4_mapped_addresses_are_compared_as_ipv4() {
        let peer = ip("::ffff:10.201.73.2");
        assert_eq!(
            caddy().client_ip(peer, &xff("::ffff:203.0.113.9")),
            ip("203.0.113.9")
        );
        let untrusted = ip("::ffff:198.51.100.7");
        assert_eq!(
            caddy().client_ip(untrusted, &HeaderMap::new()),
            ip("198.51.100.7")
        );
    }

    #[test]
    fn ipv6_clients_and_proxies_work_like_ipv4() {
        let trusted = TrustedProxies::parse("fd00::/32").unwrap();
        assert_eq!(
            trusted.client_ip(ip("fd00::2"), &xff("2001:db8::1, fd00::3")),
            ip("2001:db8::1")
        );
    }
}
