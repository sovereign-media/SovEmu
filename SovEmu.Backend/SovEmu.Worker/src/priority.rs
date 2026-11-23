//! The stream's packets are marked for priority on the way (`--udp-dscp`, sovereign-media/SovEmu#189).
//!
//! A DSCP value in each packet's IP header says how it would like to be queued. A Wi-Fi access
//! point maps it to one of its four queues (WMM): 34 (AF41, "interactive video", what WebRTC
//! uses for a game or a call's picture) goes to the video queue, which is sent ahead of ordinary
//! traffic, so a download elsewhere in the house delays the game less. Routers and switches in
//! between may honour it, ignore it or clear it; it never costs anything. 0 leaves packets
//! unmarked.

use std::net::UdpSocket;

/// The highest DSCP value (six bits).
const MAX_DSCP: u8 = 63;

/// The IP header's traffic-class byte for a DSCP value: the value in its top six bits.
pub(crate) fn traffic_class(dscp: u8) -> u32 {
    u32::from(dscp.min(MAX_DSCP)) << 2
}

/// Marks everything `socket` sends with `dscp` (0: unmarked, nothing is changed). A socket that
/// cannot be marked still streams: the failure is logged and the stream goes unmarked.
pub(crate) fn mark(socket: &UdpSocket, dscp: u8) {
    if dscp == 0 {
        return;
    }
    let class = traffic_class(dscp);
    let socket2 = socket2::SockRef::from(socket);
    let v6 = socket.local_addr().is_ok_and(|addr| addr.is_ipv6());
    let marked = if v6 { mark_v6(&socket2, class) } else { socket2.set_tos_v4(class) };
    match marked {
        Ok(()) => tracing::debug!(dscp, "stream packets marked for priority"),
        Err(e) => tracing::warn!(dscp, error = %e, "stream packets cannot be marked for priority"),
    }
}

/// Sets an IPv6 socket's traffic class.
#[cfg(target_os = "linux")]
fn mark_v6(socket: &socket2::SockRef<'_>, class: u32) -> std::io::Result<()> {
    socket.set_tclass_v6(class)
}

/// Sets an IPv6 socket's traffic class: only where the server runs (Linux). Elsewhere (a Mac,
/// for development) an IPv6 stream goes unmarked.
#[cfg(not(target_os = "linux"))]
fn mark_v6(_socket: &socket2::SockRef<'_>, _class: u32) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "IPv6 traffic class is set on Linux only",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dscp_value_sits_in_the_top_six_bits_of_the_traffic_class() {
        assert_eq!(traffic_class(34), 0x88, "AF41");
        assert_eq!(traffic_class(46), 0xB8, "EF");
        assert_eq!(traffic_class(0), 0);
        assert_eq!(traffic_class(200), 63 << 2, "no more than six bits");
    }

    #[test]
    fn a_socket_is_marked_and_still_sends() {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        mark(&socket, 34);
        let tos = socket2::SockRef::from(&socket).tos_v4().unwrap();
        assert_eq!(tos & 0xFC, 0x88, "the DSCP bits are set");
        let to = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket.send_to(b"frame", to.local_addr().unwrap()).unwrap();
        let mut buf = [0; 8];
        assert_eq!(to.recv_from(&mut buf).unwrap().0, 5);
        // Zero leaves a socket as it was.
        let plain = UdpSocket::bind("127.0.0.1:0").unwrap();
        mark(&plain, 0);
        assert_eq!(socket2::SockRef::from(&plain).tos_v4().unwrap() & 0xFC, 0);
    }
}
