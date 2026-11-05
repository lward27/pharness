//! The Campus session egress contract.
//!
//! Lab slots are default-deny. A session whose effective policy approves
//! egress reaches the internet only through the admin-owned Campus egress
//! proxy: the slot NetworkPolicy lets the session Pod open TCP connections to
//! the proxy (and the broker) and nothing else, the session has no DNS, and
//! the proxy
//!
//! - accepts only `CONNECT <approved host>:443`, for a host that is both in
//!   [`CAMPUS_EGRESS_ROUTES`] and in the session Pod's
//!   [`CAMPUS_EGRESS_HOSTS_ANNOTATION`];
//! - resolves the host itself and refuses the tunnel when any answer is not a
//!   public unicast address ([`is_public_egress_address`]), then connects to
//!   an address it checked, so a rebinding answer is never dialled;
//! - terminates TLS with a certificate from the Campus egress CA, checks every
//!   HTTP request with [`check_campus_egress_request`], and re-originates it
//!   over verified TLS to the same host. Redirects are returned to the client
//!   unchanged; following one opens a new tunnel that is checked again.
//!
//! Everything here is pure so that the proxy, the provider and their tests
//! share one definition.

use std::collections::BTreeSet;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

/// The only destination port.
pub const CAMPUS_EGRESS_PORT: u16 = 443;
/// Pod label that admits a session to the egress proxy in the slot
/// NetworkPolicy. Only the provider sets it, and only for sessions whose
/// effective policy approves egress.
pub const CAMPUS_EGRESS_LABEL: &str = "pharness.dev/campus-egress";
pub const CAMPUS_EGRESS_LABEL_VALUE: &str = "proxy";
/// Pod annotation with the session's approved hosts, comma separated and
/// sorted. The proxy reads it from the Pod that owns the connection's source
/// address; admission refuses hosts outside [`CAMPUS_EGRESS_ROUTES`].
pub const CAMPUS_EGRESS_HOSTS_ANNOTATION: &str = "pharness.dev/campus-egress-hosts";
/// Largest request body the proxy forwards (git-upload-pack negotiation).
pub const CAMPUS_EGRESS_MAX_REQUEST_BODY_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TARGET_BYTES: usize = 8 * 1024;

/// What a route is for, which fixes the methods it accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CampusEgressRouteKind {
    /// Package indexes and archives: `GET` and `HEAD`.
    Package,
    /// Git smart-HTTP clone and fetch: `GET` and `HEAD`, plus `POST` to
    /// `/<owner>/<repo>/git-upload-pack`. `git-receive-pack` (push) is
    /// refused in every form.
    GitRead,
    /// Archive and raw-file retrieval: `GET` and `HEAD`.
    Retrieval,
}

/// The admin-approved M1 routes. Nothing else is reachable from a session.
pub const CAMPUS_EGRESS_ROUTES: &[(&str, CampusEgressRouteKind)] = &[
    ("pypi.org", CampusEgressRouteKind::Package),
    ("files.pythonhosted.org", CampusEgressRouteKind::Package),
    ("registry.npmjs.org", CampusEgressRouteKind::Package),
    ("github.com", CampusEgressRouteKind::GitRead),
    ("codeload.github.com", CampusEgressRouteKind::Retrieval),
    (
        "raw.githubusercontent.com",
        CampusEgressRouteKind::Retrieval,
    ),
];

pub fn campus_egress_route(host: &str) -> Option<CampusEgressRouteKind> {
    CAMPUS_EGRESS_ROUTES
        .iter()
        .find(|(approved, _)| *approved == host)
        .map(|(_, kind)| *kind)
}

/// Why the proxy refused a tunnel or a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CampusEgressDenial {
    #[error("only CONNECT to an approved host on port 443 is accepted")]
    Connect,
    #[error("the host is not approved for this session")]
    Host,
    #[error("no running Campus session owns the connection")]
    UnknownSession,
    #[error("the host resolves to a non-public address or not at all")]
    Address,
    #[error("the request target is malformed")]
    Target,
    #[error("the request names a different host than its tunnel")]
    HostMismatch,
    #[error("the method is not allowed on this route")]
    Method,
    #[error("git push (receive-pack) is never allowed")]
    GitPush,
    #[error("protocol upgrades are not allowed")]
    Upgrade,
    #[error("the request body is too large")]
    BodyTooLarge,
}

impl CampusEgressDenial {
    pub fn code(self) -> &'static str {
        match self {
            Self::Connect => "connect",
            Self::Host => "host",
            Self::UnknownSession => "unknown_session",
            Self::Address => "address",
            Self::Target => "target",
            Self::HostMismatch => "host_mismatch",
            Self::Method => "method",
            Self::GitPush => "git_push",
            Self::Upgrade => "upgrade",
            Self::BodyTooLarge => "body_too_large",
        }
    }
}

/// Parse the authority of a `CONNECT` request. Only `<approved host>:443`
/// passes, so IP literals, other ports, user info and trailing dots are all
/// refused here.
pub fn parse_campus_connect_target(authority: &str) -> Result<&'static str, CampusEgressDenial> {
    let (host, port) = authority
        .rsplit_once(':')
        .ok_or(CampusEgressDenial::Connect)?;
    if port != "443" {
        return Err(CampusEgressDenial::Connect);
    }
    let host = host.to_ascii_lowercase();
    CAMPUS_EGRESS_ROUTES
        .iter()
        .find(|(approved, _)| *approved == host)
        .map(|(approved, _)| *approved)
        .ok_or(CampusEgressDenial::Host)
}

/// The annotation value for a set of approved hosts.
pub fn format_campus_egress_hosts<'a>(hosts: impl IntoIterator<Item = &'a str>) -> String {
    hosts
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(",")
}

/// Parse [`CAMPUS_EGRESS_HOSTS_ANNOTATION`]. Any host outside the routes, an
/// empty entry or a repeat refuses the whole value.
pub fn parse_campus_egress_hosts(
    value: &str,
) -> Result<BTreeSet<&'static str>, CampusEgressDenial> {
    let mut hosts = BTreeSet::new();
    for entry in value.split(',') {
        let host = CAMPUS_EGRESS_ROUTES
            .iter()
            .find(|(approved, _)| *approved == entry)
            .map(|(approved, _)| *approved)
            .ok_or(CampusEgressDenial::Host)?;
        if !hosts.insert(host) {
            return Err(CampusEgressDenial::Host);
        }
    }
    Ok(hosts)
}

/// One HTTP request inside an approved tunnel.
#[derive(Debug, Clone, Copy)]
pub struct CampusEgressRequest<'a> {
    pub tunnel_host: &'a str,
    pub method: &'a str,
    /// The `Host` header, or the request URI's authority.
    pub host: Option<&'a str>,
    /// The origin-form request target, for example `/simple/requests/`.
    pub target: &'a str,
    pub content_type: Option<&'a str>,
    /// An `Upgrade` header or a `Connection: upgrade` token was present.
    pub upgrade: bool,
    pub content_length: Option<u64>,
}

impl fmt::Display for CampusEgressRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = self.target.split('?').next().unwrap_or_default();
        let path = path.get(..path.len().min(160)).unwrap_or_default();
        write!(formatter, "{} {}{}", self.method, self.tunnel_host, path)
    }
}

/// Decide one request. The rules depend only on the route kind of the
/// tunnel's host:
///
/// | Route | Methods |
/// | --- | --- |
/// | package, retrieval | `GET`, `HEAD` |
/// | git read (`github.com`) | `GET`, `HEAD`; `POST /<owner>/<repo>/git-upload-pack` with `application/x-git-upload-pack-request` |
///
/// Every route refuses `git-receive-pack` anywhere in the decoded target,
/// protocol upgrades, absolute-form targets and a `Host` other than the
/// tunnel's.
pub fn check_campus_egress_request(
    request: &CampusEgressRequest<'_>,
) -> Result<(), CampusEgressDenial> {
    use CampusEgressDenial as D;
    let kind = campus_egress_route(request.tunnel_host).ok_or(D::Host)?;
    if request.upgrade {
        return Err(D::Upgrade);
    }
    let target = request.target;
    if !target.starts_with('/')
        || target.starts_with("//")
        || target.len() > MAX_TARGET_BYTES
        || target.contains('#')
        || !target.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(D::Target);
    }
    let host = request.host.ok_or(D::HostMismatch)?.to_ascii_lowercase();
    let host = host.strip_suffix(":443").unwrap_or(&host);
    if host != request.tunnel_host {
        return Err(D::HostMismatch);
    }
    let decoded = percent_decode_lossy(target).to_ascii_lowercase();
    if decoded.contains("receive-pack") {
        return Err(D::GitPush);
    }
    if request
        .content_length
        .is_some_and(|length| length > CAMPUS_EGRESS_MAX_REQUEST_BODY_BYTES)
    {
        return Err(D::BodyTooLarge);
    }
    match request.method {
        "GET" | "HEAD" => Ok(()),
        "POST" if kind == CampusEgressRouteKind::GitRead => {
            let content_type = request
                .content_type
                .map(|value| value.trim().to_ascii_lowercase());
            if is_upload_pack_path(target)
                && content_type.as_deref() == Some("application/x-git-upload-pack-request")
            {
                Ok(())
            } else {
                Err(D::Method)
            }
        }
        _ => Err(D::Method),
    }
}

/// `/<owner>/<repo>/git-upload-pack` with plain segments and no query.
fn is_upload_pack_path(target: &str) -> bool {
    let segments = target.split('/').skip(1).collect::<Vec<_>>();
    let [owner, repository, "git-upload-pack"] = segments.as_slice() else {
        return false;
    };
    [owner, repository].iter().all(|segment| {
        !segment.is_empty()
            && **segment != "."
            && **segment != ".."
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    })
}

/// Decode `%XX` escapes repeatedly until nothing changes, so double-encoded
/// names are caught too. Invalid escapes are kept as they are.
fn percent_decode_lossy(value: &str) -> String {
    let mut current = value.to_string();
    for _ in 0..4 {
        let bytes = current.as_bytes();
        let mut decoded = Vec::with_capacity(bytes.len());
        let mut index = 0;
        while index < bytes.len() {
            let hex = |byte: u8| (byte as char).to_digit(16);
            if bytes[index] == b'%' && index + 2 < bytes.len() {
                if let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) {
                    decoded.push((high * 16 + low) as u8);
                    index += 3;
                    continue;
                }
            }
            decoded.push(bytes[index]);
            index += 1;
        }
        let next = String::from_utf8_lossy(&decoded).into_owned();
        if next == current {
            break;
        }
        current = next;
    }
    current
}

/// Whether the proxy may dial this address: public unicast only. Private,
/// shared (CGNAT), loopback, link-local (including the 169.254.169.254
/// metadata service), cluster, documentation, benchmarking, multicast and
/// reserved IPv4 ranges are refused, and for IPv6 everything outside global
/// unicast `2000::/3`, plus the documentation, 6to4 and IETF special ranges
/// inside it. IPv4-mapped, NAT64 and unique-local (`fc00::/7`, so `fd00::/8`)
/// addresses fall outside `2000::/3` and are refused.
pub fn is_public_egress_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_public_v4(address),
        IpAddr::V6(address) => is_public_v6(address),
    }
}

fn in_v4(address: Ipv4Addr, network: [u8; 4], prefix: u32) -> bool {
    let mask = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    (u32::from(address) & mask) == (u32::from(Ipv4Addr::from(network)) & mask)
}

fn is_public_v4(address: Ipv4Addr) -> bool {
    const REFUSED: &[([u8; 4], u32)] = &[
        ([0, 0, 0, 0], 8),
        ([10, 0, 0, 0], 8),
        ([100, 64, 0, 0], 10),
        ([127, 0, 0, 0], 8),
        ([169, 254, 0, 0], 16),
        ([172, 16, 0, 0], 12),
        ([192, 0, 0, 0], 24),
        ([192, 0, 2, 0], 24),
        ([192, 31, 196, 0], 24),
        ([192, 52, 193, 0], 24),
        ([192, 88, 99, 0], 24),
        ([192, 168, 0, 0], 16),
        ([192, 175, 48, 0], 24),
        ([198, 18, 0, 0], 15),
        ([198, 51, 100, 0], 24),
        ([203, 0, 113, 0], 24),
        ([224, 0, 0, 0], 4),
        ([240, 0, 0, 0], 4),
    ];
    !REFUSED
        .iter()
        .any(|(network, prefix)| in_v4(address, *network, *prefix))
}

fn in_v6(address: Ipv6Addr, network: Ipv6Addr, prefix: u32) -> bool {
    let mask = if prefix == 0 {
        0
    } else {
        u128::MAX << (128 - prefix)
    };
    (u128::from(address) & mask) == (u128::from(network) & mask)
}

fn is_public_v6(address: Ipv6Addr) -> bool {
    const REFUSED: &[(Ipv6Addr, u32)] = &[
        // IETF protocol assignments, including Teredo (2001::/32).
        (Ipv6Addr::new(0x2001, 0, 0, 0, 0, 0, 0, 0), 23),
        (Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0), 32),
        // 6to4 embeds an arbitrary IPv4 address.
        (Ipv6Addr::new(0x2002, 0, 0, 0, 0, 0, 0, 0), 16),
        (Ipv6Addr::new(0x3fff, 0, 0, 0, 0, 0, 0, 0), 20),
    ];
    in_v6(address, Ipv6Addr::new(0x2000, 0, 0, 0, 0, 0, 0, 0), 3)
        && !REFUSED
            .iter()
            .any(|(network, prefix)| in_v6(address, *network, *prefix))
}

/// The addresses the proxy may dial for a resolved host, IPv4 first. Any
/// non-public answer refuses the whole set: a name that resolves to an
/// internal address is treated as hostile, not filtered.
pub fn select_campus_egress_addresses(
    resolved: impl IntoIterator<Item = SocketAddr>,
) -> Result<Vec<SocketAddr>, CampusEgressDenial> {
    let mut addresses = resolved.into_iter().collect::<Vec<_>>();
    if addresses.is_empty()
        || addresses.iter().any(|address| {
            !is_public_egress_address(address.ip()) || address.port() != CAMPUS_EGRESS_PORT
        })
    {
        return Err(CampusEgressDenial::Address);
    }
    addresses.sort_by_key(|address| (address.is_ipv6(), *address));
    addresses.dedup();
    Ok(addresses)
}

#[cfg(test)]
mod tests;
