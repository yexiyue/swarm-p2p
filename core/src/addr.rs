//! Multiaddr 地址分类谓词。
//!
//! 全仓共享的单一实现——地址「可拨性/可路由范围」的判定曾散落在
//! event loop、infra、presence 三处手写，谓词位运算漂移过一次
//! （IPv6 link-local 漏判），故收口于此。

use libp2p::{Multiaddr, multiaddr::Protocol};

/// 含 loopback 地址（127.0.0.0/8、::1）
pub fn is_loopback(addr: &Multiaddr) -> bool {
    addr.iter().any(|p| match p {
        Protocol::Ip4(ip) => ip.is_loopback(),
        Protocol::Ip6(ip) => ip.is_loopback(),
        _ => false,
    })
}

/// 含 loopback 或 unspecified 地址（对任何对端都不可拨）
pub fn is_loopback_or_unspecified(addr: &Multiaddr) -> bool {
    addr.iter().any(|p| match p {
        Protocol::Ip4(ip) => ip.is_loopback() || ip.is_unspecified(),
        Protocol::Ip6(ip) => ip.is_loopback() || ip.is_unspecified(),
        _ => false,
    })
}

/// 可路由于局域网的私网地址（IPv4 私网段 / IPv6 ULA fc00::/7），
/// 排除 loopback/link-local/unspecified
pub fn is_private_lan(addr: &Multiaddr) -> bool {
    addr.iter().any(|p| match p {
        Protocol::Ip4(ip) => {
            ip.is_private() && !ip.is_loopback() && !ip.is_link_local() && !ip.is_unspecified()
        }
        Protocol::Ip6(ip) => is_v6_ula(&ip) && !ip.is_loopback() && !ip.is_unspecified(),
        _ => false,
    })
}

/// 公网可路由地址（含 DNS 名）：排除 loopback/unspecified/私网/ULA/link-local
pub fn is_public_routable(addr: &Multiaddr) -> bool {
    addr.iter().any(|p| match p {
        Protocol::Ip4(ip) => {
            !ip.is_private() && !ip.is_loopback() && !ip.is_link_local() && !ip.is_unspecified()
        }
        Protocol::Ip6(ip) => {
            !ip.is_loopback() && !ip.is_unspecified() && !is_v6_ula(&ip) && !is_v6_link_local(&ip)
        }
        Protocol::Dns(_) | Protocol::Dns4(_) | Protocol::Dns6(_) => true,
        _ => false,
    })
}

/// p2p-circuit 跳数（0=直连地址，1=一跳中继，>1 libp2p 硬拒）
pub fn circuit_hops(addr: &Multiaddr) -> usize {
    addr.iter()
        .filter(|p| matches!(p, Protocol::P2pCircuit))
        .count()
}

fn is_v6_ula(ip: &std::net::Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xfe00) == 0xfc00
}

fn is_v6_link_local(ip: &std::net::Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xffc0) == 0xfe80
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(s: &str) -> Multiaddr {
        s.parse().unwrap()
    }

    #[test]
    fn classification_matrix() {
        // loopback / unspecified
        assert!(is_loopback(&addr("/ip4/127.0.0.1/tcp/1")));
        assert!(is_loopback_or_unspecified(&addr("/ip4/0.0.0.0/tcp/1")));
        assert!(!is_loopback_or_unspecified(&addr("/ip4/192.168.1.2/tcp/1")));

        // 私网 LAN
        assert!(is_private_lan(&addr("/ip4/192.168.1.2/tcp/1")));
        assert!(is_private_lan(&addr("/ip6/fd00::1/tcp/1")));
        assert!(!is_private_lan(&addr("/ip4/127.0.0.1/tcp/1")));
        assert!(!is_private_lan(&addr("/ip4/8.8.8.8/tcp/1")));

        // 公网
        assert!(is_public_routable(&addr("/ip4/203.0.113.7/tcp/1")));
        assert!(is_public_routable(&addr("/dns4/relay.example.com/tcp/1")));
        assert!(!is_public_routable(&addr("/ip4/192.168.1.2/tcp/1")));
        assert!(
            !is_public_routable(&addr("/ip6/fe80::1/tcp/1")),
            "IPv6 link-local 不是公网"
        );
        assert!(!is_public_routable(&addr("/ip6/fd00::1/tcp/1")));

        // circuit 跳数
        assert_eq!(circuit_hops(&addr("/ip4/1.2.3.4/tcp/1")), 0);
        assert_eq!(
            circuit_hops(&addr(
                "/ip4/1.2.3.4/tcp/1/p2p/12D3KooWEyoppNCUx8Yx66oV9fJnriXwCcXwDDUA2kj6vnc6iDEp/p2p-circuit"
            )),
            1
        );
    }
}
