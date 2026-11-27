//! The default gateway: the router NAT-PMP asks. Read from Linux's routing table, where the
//! server runs; elsewhere (a developer's Mac) there is none, and only UPnP is tried.

use std::net::Ipv4Addr;

/// The IPv4 default route's gateway, with the lowest metric.
pub(crate) fn default_gateway() -> Option<Ipv4Addr> {
    if cfg!(target_os = "linux") {
        parse_routes(&std::fs::read_to_string("/proc/net/route").ok()?)
    } else {
        None
    }
}

/// `/proc/net/route`'s default gateway: a route to 0.0.0.0/0 that is up and goes through a
/// gateway. Addresses there are hexadecimal, in the machine's (little-endian) byte order.
fn parse_routes(table: &str) -> Option<Ipv4Addr> {
    const UP: u32 = 0x1;
    const GATEWAY: u32 = 0x2;
    table
        .lines()
        .skip(1)
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            let hex = |i: usize| f.get(i).and_then(|v| u32::from_str_radix(v, 16).ok());
            let (destination, gateway, flags, metric, mask) =
                (hex(1)?, hex(2)?, hex(3)?, f.get(6)?, hex(7)?);
            let default = destination == 0 && mask == 0 && flags & (UP | GATEWAY) == UP | GATEWAY;
            (default && gateway != 0).then(|| {
                (metric.parse::<u32>().unwrap_or(u32::MAX), Ipv4Addr::from(gateway.to_le_bytes()))
            })
        })
        .min_by_key(|(metric, _)| *metric)
        .map(|(_, gateway)| gateway)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_route_with_the_lowest_metric_wins() {
        let table = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n\
                     wlan0\t00000000\t0A00A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0\n\
                     eth0\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0\n\
                     eth0\t0001A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0\n";
        assert_eq!(parse_routes(table), Some(Ipv4Addr::new(192, 168, 1, 1)));
    }

    #[test]
    fn no_default_route_is_no_gateway() {
        let table = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n\
                     eth0\t0001A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0\n";
        assert_eq!(parse_routes(table), None);
        assert_eq!(parse_routes(""), None);
    }
}
