// SPDX-License-Identifier: AGPL-3.0-or-later
//! The outbound URL policy (`docs/specs/m0/05-oidc.md` B2, Q4): which URLs and addresses the
//! server may fetch for an identity provider or the admission webhook.
//!
//! A URL is refused unless its scheme is `https` (or `http` with the insecure switch), it has a
//! host, and it carries no user name or password. Every address the server connects to, whether
//! written in the URL or found by DNS, falls in one class:
//!
//! | Class | Addresses | Allowed |
//! | --- | --- | --- |
//! | public | everything below does not name | always |
//! | loopback | `127.0.0.0/8`, `::1` | only with the insecure switch |
//! | private | `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, `100.64.0.0/10` (shared), `198.18.0.0/15` (benchmarking), `192.0.0.0/24`, `fc00::/7` (unique local), `fec0::/10` (site local), `64:ff9b:1::/48` | only when private networks are allowed (a single-organization instance) |
//! | never | `0.0.0.0/8`, `169.254.0.0/16` and `fe80::/10` (link local, where cloud metadata services listen), the metadata services inside private ranges (`100.100.100.200`, `fd00:ec2::254`), multicast, `240.0.0.0/4` and broadcast, the documentation ranges, `::`, `100::/64`, `2001::/32` (Teredo, which tunnels to an IPv4 address the policy cannot see), other `::/96` | never |
//!
//! The never class is checked before the private one, so a metadata address stays refused
//! where private networks are allowed.
//!
//! An IPv6 address that carries an IPv4 one (`::ffff:0:0/96` mapped, `64:ff9b::/96` NAT64,
//! `2002::/16` 6to4) is classed by the IPv4 address it carries.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use url::{Host, Url};

/// What the server may fetch. `Copy`: every client and check holds its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UrlPolicy {
    allow_insecure: bool,
    allow_private: bool,
}

/// Why a URL or address was refused. Neither the URL nor the address is repeated: a URL can
/// carry a password.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Refused {
    #[error("the URL is not an absolute URL")]
    NotAbsolute,
    #[error("the URL's scheme is not https (http needs the insecure-providers switch)")]
    Scheme,
    #[error("the URL has no host")]
    NoHost,
    #[error("the URL carries a user name or password")]
    Credentials,
    #[error("the host is a loopback address, allowed only with the insecure-providers switch")]
    Loopback,
    #[error("the host is a private network address, which a multi-organization instance refuses")]
    Private,
    #[error(
        "the host is an address the server never connects to (link local, multicast, reserved)"
    )]
    Never,
    #[error("the host name resolves to no address")]
    NoAddress,
}

/// The class of an address (see the module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressClass {
    Public,
    Loopback,
    Private,
    Never,
}

impl UrlPolicy {
    /// `allow_insecure` is the instance's insecure-providers switch: it also allows `http` and
    /// loopback addresses, for tests and the development stack. `allow_private` allows private
    /// network addresses; the caller sets it on a single-organization instance and clears it on
    /// a multi-organization one, where one organization's admin must not reach the operator's
    /// network.
    #[must_use]
    pub const fn new(allow_insecure: bool, allow_private: bool) -> Self {
        Self {
            allow_insecure,
            allow_private,
        }
    }

    /// Whether `http` and loopback addresses are allowed.
    #[must_use]
    pub const fn allows_insecure(self) -> bool {
        self.allow_insecure
    }

    /// Check what can be checked without DNS: the scheme, the credentials and an address
    /// written as the host. A host name is checked when the client resolves it, at every
    /// connection.
    pub fn check_url(self, url: &Url) -> Result<(), Refused> {
        match url.scheme() {
            "https" => {}
            "http" if self.allow_insecure => {}
            _ => return Err(Refused::Scheme),
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(Refused::Credentials);
        }
        match url.host() {
            None => Err(Refused::NoHost),
            Some(Host::Domain(_)) => Ok(()),
            Some(Host::Ipv4(ip)) => self.check_address(IpAddr::V4(ip)),
            Some(Host::Ipv6(ip)) => self.check_address(IpAddr::V6(ip)),
        }
    }

    /// Check one address the client is about to connect to.
    pub fn check_address(self, ip: IpAddr) -> Result<(), Refused> {
        match classify(ip) {
            AddressClass::Public => Ok(()),
            AddressClass::Loopback if self.allow_insecure => Ok(()),
            AddressClass::Loopback => Err(Refused::Loopback),
            AddressClass::Private if self.allow_private => Ok(()),
            AddressClass::Private => Err(Refused::Private),
            AddressClass::Never => Err(Refused::Never),
        }
    }
}

/// The class of `ip` (the table in the module docs).
#[must_use]
pub fn classify(ip: IpAddr) -> AddressClass {
    match ip {
        IpAddr::V4(v4) => classify_v4(v4),
        IpAddr::V6(v6) => classify_v6(v6),
    }
}

fn in_v4(ip: Ipv4Addr, network: [u8; 4], prefix: u32) -> bool {
    let mask = u32::MAX.checked_shl(32 - prefix).unwrap_or(0);
    u32::from(ip) & mask == u32::from(Ipv4Addr::from(network)) & mask
}

const V4_NEVER: &[([u8; 4], u32)] = &[
    ([0, 0, 0, 0], 8),
    ([169, 254, 0, 0], 16),
    ([100, 100, 100, 200], 32),
    ([224, 0, 0, 0], 4),
    ([240, 0, 0, 0], 4),
    ([192, 0, 2, 0], 24),
    ([198, 51, 100, 0], 24),
    ([203, 0, 113, 0], 24),
];

const V4_PRIVATE: &[([u8; 4], u32)] = &[
    ([10, 0, 0, 0], 8),
    ([172, 16, 0, 0], 12),
    ([192, 168, 0, 0], 16),
    ([100, 64, 0, 0], 10),
    ([198, 18, 0, 0], 15),
    ([192, 0, 0, 0], 24),
];

fn classify_v4(ip: Ipv4Addr) -> AddressClass {
    if in_v4(ip, [127, 0, 0, 0], 8) {
        AddressClass::Loopback
    } else if V4_NEVER.iter().any(|&(net, len)| in_v4(ip, net, len)) {
        AddressClass::Never
    } else if V4_PRIVATE.iter().any(|&(net, len)| in_v4(ip, net, len)) {
        AddressClass::Private
    } else {
        AddressClass::Public
    }
}

fn in_v6(ip: Ipv6Addr, network: Ipv6Addr, prefix: u32) -> bool {
    let mask = u128::MAX.checked_shl(128 - prefix).unwrap_or(0);
    u128::from(ip) & mask == u128::from(network) & mask
}

const fn v6(segments: [u16; 8]) -> Ipv6Addr {
    let [a, b, c, d, e, f, g, h] = segments;
    Ipv6Addr::new(a, b, c, d, e, f, g, h)
}

const V6_NEVER: &[(Ipv6Addr, u32)] = &[
    (v6([0xfe80, 0, 0, 0, 0, 0, 0, 0]), 10),
    (v6([0xff00, 0, 0, 0, 0, 0, 0, 0]), 8),
    (v6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 0]), 32),
    (v6([0x100, 0, 0, 0, 0, 0, 0, 0]), 64),
    (v6([0x2001, 0, 0, 0, 0, 0, 0, 0]), 32),
    (v6([0xfd00, 0xec2, 0, 0, 0, 0, 0, 0x254]), 128),
    (v6([0, 0, 0, 0, 0, 0, 0, 0]), 96),
];

const V6_PRIVATE: &[(Ipv6Addr, u32)] = &[
    (v6([0xfc00, 0, 0, 0, 0, 0, 0, 0]), 7),
    (v6([0xfec0, 0, 0, 0, 0, 0, 0, 0]), 10),
    (v6([0x64, 0xff9b, 1, 0, 0, 0, 0, 0]), 48),
];

const NAT64: Ipv6Addr = v6([0x64, 0xff9b, 0, 0, 0, 0, 0, 0]);
const SIX_TO_FOUR: Ipv6Addr = v6([0x2002, 0, 0, 0, 0, 0, 0, 0]);

fn classify_v6(ip: Ipv6Addr) -> AddressClass {
    if let Some(v4) = embedded_v4(ip) {
        return classify_v4(v4);
    }
    if ip == Ipv6Addr::LOCALHOST {
        AddressClass::Loopback
    } else if V6_NEVER.iter().any(|&(net, len)| in_v6(ip, net, len)) {
        AddressClass::Never
    } else if V6_PRIVATE.iter().any(|&(net, len)| in_v6(ip, net, len)) {
        AddressClass::Private
    } else {
        AddressClass::Public
    }
}

/// The IPv4 address an IPv4-mapped, NAT64 or 6to4 address carries.
fn embedded_v4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    let bits = u128::from(ip);
    if let Some(v4) = ip.to_ipv4_mapped() {
        Some(v4)
    } else if in_v6(ip, NAT64, 96) {
        // The low 32 bits of the address, by the mask.
        Some(Ipv4Addr::from((bits & 0xffff_ffff) as u32))
    } else if in_v6(ip, SIX_TO_FOUR, 16) {
        // Bits 16 to 47, by the shift and mask.
        Some(Ipv4Addr::from(((bits >> 80) & 0xffff_ffff) as u32))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class(text: &str) -> AddressClass {
        classify(text.parse().expect("an address"))
    }

    #[test]
    fn addresses_fall_in_the_documented_classes() {
        for public in ["93.184.215.14", "1.1.1.1", "2606:4700::1111", "172.32.0.1"] {
            assert_eq!(class(public), AddressClass::Public, "{public}");
        }
        for loopback in ["127.0.0.1", "127.255.255.254", "::1", "::ffff:127.0.0.1"] {
            assert_eq!(class(loopback), AddressClass::Loopback, "{loopback}");
        }
        for private in [
            "10.1.2.3",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "100.64.0.1",
            "198.18.0.1",
            "192.0.0.8",
            "fd00::1",
            "fec0::1",
            "64:ff9b:1::1",
            "::ffff:10.0.0.1",
        ] {
            assert_eq!(class(private), AddressClass::Private, "{private}");
        }
        for never in [
            "0.0.0.0",
            "169.254.169.254",
            "224.0.0.1",
            "255.255.255.255",
            "240.0.0.1",
            "192.0.2.1",
            "::",
            "fe80::1",
            "ff02::1",
            "2001:db8::1",
            "100::1",
            "::7f00:1",
        ] {
            assert_eq!(class(never), AddressClass::Never, "{never}");
        }
    }

    #[test]
    fn metadata_addresses_inside_private_ranges_are_never_allowed() {
        let everything = UrlPolicy::new(true, true);
        for metadata in ["100.100.100.200", "fd00:ec2::254"] {
            assert_eq!(class(metadata), AddressClass::Never, "{metadata}");
            let ip: IpAddr = metadata.parse().expect("an address");
            assert_eq!(
                everything.check_address(ip),
                Err(Refused::Never),
                "{metadata}"
            );
        }
        // Their neighbors stay private.
        assert_eq!(class("100.100.100.201"), AddressClass::Private);
        assert_eq!(class("fd00:ec2::253"), AddressClass::Private);
    }

    #[test]
    fn teredo_is_never_allowed() {
        for teredo in ["2001::1", "2001:0:4136:e378:8000:63bf:3fff:fdd2"] {
            assert_eq!(class(teredo), AddressClass::Never, "{teredo}");
        }
        assert_eq!(class("2001:4860::8888"), AddressClass::Public);
    }

    #[test]
    fn an_ipv6_address_carrying_ipv4_is_classed_by_it() {
        assert_eq!(class("64:ff9b::7f00:1"), AddressClass::Loopback);
        assert_eq!(class("64:ff9b::a9fe:a9fe"), AddressClass::Never);
        assert_eq!(class("64:ff9b::808:808"), AddressClass::Public);
        assert_eq!(class("2002:c0a8:0101::1"), AddressClass::Private);
        assert_eq!(class("2002:7f00:1::"), AddressClass::Loopback);
        assert_eq!(class("::ffff:169.254.169.254"), AddressClass::Never);
    }

    #[test]
    fn the_policy_allows_each_class_only_under_its_switch() {
        let strict = UrlPolicy::new(false, false);
        let single_org = UrlPolicy::new(false, true);
        let insecure = UrlPolicy::new(true, false);
        let loopback: IpAddr = "127.0.0.1".parse().expect("an address");
        let private: IpAddr = "10.0.0.1".parse().expect("an address");
        let link_local: IpAddr = "169.254.169.254".parse().expect("an address");
        let public: IpAddr = "8.8.8.8".parse().expect("an address");

        assert_eq!(strict.check_address(public), Ok(()));
        assert_eq!(strict.check_address(loopback), Err(Refused::Loopback));
        assert_eq!(strict.check_address(private), Err(Refused::Private));
        assert_eq!(single_org.check_address(private), Ok(()));
        assert_eq!(single_org.check_address(loopback), Err(Refused::Loopback));
        assert_eq!(insecure.check_address(loopback), Ok(()));
        assert_eq!(insecure.check_address(private), Err(Refused::Private));
        for policy in [strict, single_org, insecure, UrlPolicy::new(true, true)] {
            assert_eq!(policy.check_address(link_local), Err(Refused::Never));
        }
    }

    fn check(policy: UrlPolicy, url: &str) -> Result<(), Refused> {
        policy.check_url(&Url::parse(url).expect("a URL"))
    }

    #[test]
    fn a_url_needs_https_unless_the_insecure_switch_is_on() {
        let strict = UrlPolicy::new(false, true);
        let insecure = UrlPolicy::new(true, true);
        assert_eq!(check(strict, "https://idp.example.test/realms/a"), Ok(()));
        assert_eq!(
            check(strict, "http://idp.example.test/"),
            Err(Refused::Scheme)
        );
        assert_eq!(check(insecure, "http://idp.example.test/"), Ok(()));
        for other in ["ftp://idp.example.test/", "file:///etc/passwd", "data:,x"] {
            assert_eq!(check(insecure, other), Err(Refused::Scheme), "{other}");
        }
    }

    #[test]
    fn a_url_with_credentials_is_refused() {
        let policy = UrlPolicy::new(true, true);
        assert_eq!(
            check(policy, "https://user:secret@idp.example.test/"),
            Err(Refused::Credentials)
        );
        assert_eq!(
            check(policy, "https://user@idp.example.test/"),
            Err(Refused::Credentials)
        );
    }

    #[test]
    fn an_address_written_as_the_host_is_checked_in_every_spelling() {
        let strict = UrlPolicy::new(false, false);
        for url in [
            "https://127.0.0.1/",
            "https://127.1/",
            "https://0x7f000001/",
            "https://2130706433/",
            "https://[::1]/",
            "https://[::ffff:127.0.0.1]/",
        ] {
            assert_eq!(check(strict, url), Err(Refused::Loopback), "{url}");
        }
        assert_eq!(check(strict, "https://10.0.0.1/"), Err(Refused::Private));
        assert_eq!(
            check(strict, "https://169.254.169.254/latest/meta-data/"),
            Err(Refused::Never)
        );
        assert_eq!(check(strict, "https://[fe80::1]/"), Err(Refused::Never));
        assert_eq!(check(strict, "https://93.184.215.14/"), Ok(()));
        // A name is checked when it is resolved, not here.
        assert_eq!(check(strict, "https://localhost/"), Ok(()));
    }
}
