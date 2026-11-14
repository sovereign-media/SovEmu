//! Messages on the `input` DataChannel. All integers are little-endian.
//!
//! ## Client → worker: [`InputPacket`]
//!
//! | Offset | Size | Field |
//! |---|---|---|
//! | 0 | 1 | message type, `0x01` |
//! | 1 | 1 | version, [`crate::INPUT_VERSION`] |
//! | 2 | 4 | `seq`: increments with every packet sent |
//! | 6 | 4 | `client_time_ms`: the client's clock, echoed back in the ack |
//! | 10 | 1 | `n`: number of ports that follow, 1 to [`MAX_PORTS`] |
//! | 11 | 2·n | buttons for ports 0..n, one bit per libretro `RETRO_DEVICE_ID_JOYPAD_*` id |
//!
//! Clients send a packet whenever the state changes and at least every 100 ms regardless.
//! The worker ignores any packet whose `seq` is not newer than the last it applied (comparison
//! wraps), since the channel is unordered.
//!
//! ## Client → worker: [`Hotkeys`]
//!
//! | Offset | Size | Field |
//! |---|---|---|
//! | 0 | 1 | message type, `0x02` |
//! | 1 | 1 | version |
//! | 2 | 1 | flags: bit 0 fast-forward, bit 1 rewind (held while set) |
//!
//! Sent whenever the flags change, and with every input heartbeat while any is set; the worker
//! clears them if none arrives for half a second, so a lost release cannot leave a game spinning.
//!
//! ## Client → worker: [`AnalogPacket`]
//!
//! | Offset | Size | Field |
//! |---|---|---|
//! | 0 | 1 | message type, `0x03` |
//! | 1 | 1 | version |
//! | 2 | 4 | `seq`: its own counter, independent of the button packets' |
//! | 6 | 1 | `n`: number of ports that follow, 1 to [`MAX_PORTS`] |
//! | 7 | 12·n | per port, six `i16`: left stick x, y; right stick x, y; L2, R2 |
//!
//! Sticks run from −32768 to 32767, x to the right and y **down** (libretro's convention);
//! triggers from 0 (released) to 32767. Sent like button packets: on change and at least every
//! 100 ms while any value is off-centre. Added after version 1 without bumping it: clients that
//! never send it leave the sticks centred, and workers that predate it ignore the type.
//!
//! ## Worker → client: [`InputAck`]
//!
//! | Offset | Size | Field |
//! |---|---|---|
//! | 0 | 1 | message type, `0x81` |
//! | 1 | 1 | version |
//! | 2 | 4 | `seq` of the packet acknowledged |
//! | 6 | 4 | `client_time_ms`, echoed |
//! | 10 | 4 | `frame`: the emulated frame that first saw this input |
//!
//! Sent after the frame that consumed the input has run, so `now - client_time_ms` on the
//! client is the input round trip up to emulation: network both ways plus frame alignment.

use std::fmt;

use crate::INPUT_VERSION;

/// The most controller ports a packet can carry.
pub const MAX_PORTS: usize = 4;

const TYPE_INPUT: u8 = 0x01;
const TYPE_HOTKEYS: u8 = 0x02;
const TYPE_ANALOG: u8 = 0x03;
const ANALOG_HEADER: usize = 7;
const ANALOG_PORT: usize = 12;
const TYPE_ACK: u8 = 0x81;
const INPUT_HEADER: usize = 11;
const ACK_LEN: usize = 14;

/// Why a message could not be decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    TooShort,
    WrongType(u8),
    UnsupportedVersion(u8),
    BadPortCount(u8),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort => f.write_str("message too short"),
            Self::WrongType(t) => write!(f, "unexpected message type {t:#04x}"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported input protocol version {v}"),
            Self::BadPortCount(n) => write!(f, "invalid port count {n}"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// The full controller state of every port the client drives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputPacket {
    pub seq: u32,
    pub client_time_ms: u32,
    /// Buttons for ports `0..ports.len()`.
    pub ports: Vec<u16>,
}

impl InputPacket {
    pub fn encode(&self) -> Vec<u8> {
        let ports = &self.ports[..self.ports.len().min(MAX_PORTS)];
        let mut out = Vec::with_capacity(INPUT_HEADER + 2 * ports.len());
        out.extend_from_slice(&[TYPE_INPUT, INPUT_VERSION]);
        out.extend_from_slice(&self.seq.to_le_bytes());
        out.extend_from_slice(&self.client_time_ms.to_le_bytes());
        out.push(ports.len() as u8);
        for buttons in ports {
            out.extend_from_slice(&buttons.to_le_bytes());
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let header = check_header(bytes, TYPE_INPUT, INPUT_HEADER)?;
        let n = header[10];
        if n == 0 || n as usize > MAX_PORTS {
            return Err(DecodeError::BadPortCount(n));
        }
        let body =
            bytes.get(INPUT_HEADER..INPUT_HEADER + 2 * n as usize).ok_or(DecodeError::TooShort)?;
        Ok(Self {
            seq: u32_at(bytes, 2),
            client_time_ms: u32_at(bytes, 6),
            ports: body.as_chunks::<2>().0.iter().map(|b| u16::from_le_bytes(*b)).collect(),
        })
    }

    /// Whether this packet is newer than one with sequence number `last` (wrapping).
    pub fn is_newer_than(&self, last: u32) -> bool {
        let diff = self.seq.wrapping_sub(last);
        diff != 0 && diff < u32::MAX / 2
    }
}

/// Emulator hotkeys held on the client.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Hotkeys {
    pub fast_forward: bool,
    pub rewind: bool,
}

impl Hotkeys {
    pub fn encode(&self) -> [u8; 3] {
        [TYPE_HOTKEYS, INPUT_VERSION, u8::from(self.fast_forward) | (u8::from(self.rewind) << 1)]
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        check_header(bytes, TYPE_HOTKEYS, 3)?;
        Ok(Self { fast_forward: bytes[2] & 1 != 0, rewind: bytes[2] & 2 != 0 })
    }
}

/// One port's analog controls.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Analog {
    /// Left stick, x right and y down, −32768 to 32767.
    pub left: (i16, i16),
    pub right: (i16, i16),
    /// Triggers, 0 to 32767.
    pub l2: i16,
    pub r2: i16,
}

/// The analog state of every port the client drives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalogPacket {
    pub seq: u32,
    pub ports: Vec<Analog>,
}

impl AnalogPacket {
    pub fn encode(&self) -> Vec<u8> {
        let ports = &self.ports[..self.ports.len().min(MAX_PORTS)];
        let mut out = Vec::with_capacity(ANALOG_HEADER + ANALOG_PORT * ports.len());
        out.extend_from_slice(&[TYPE_ANALOG, INPUT_VERSION]);
        out.extend_from_slice(&self.seq.to_le_bytes());
        out.push(ports.len() as u8);
        for a in ports {
            for v in [a.left.0, a.left.1, a.right.0, a.right.1, a.l2, a.r2] {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        check_header(bytes, TYPE_ANALOG, ANALOG_HEADER)?;
        let n = bytes[6];
        if n == 0 || n as usize > MAX_PORTS {
            return Err(DecodeError::BadPortCount(n));
        }
        let body = bytes
            .get(ANALOG_HEADER..ANALOG_HEADER + ANALOG_PORT * n as usize)
            .ok_or(DecodeError::TooShort)?;
        let ports = body
            .as_chunks::<ANALOG_PORT>()
            .0
            .iter()
            .map(|p| {
                let v = |i: usize| i16::from_le_bytes([p[2 * i], p[2 * i + 1]]);
                Analog { left: (v(0), v(1)), right: (v(2), v(3)), l2: v(4), r2: v(5) }
            })
            .collect();
        Ok(Self { seq: u32_at(bytes, 2), ports })
    }

    /// Whether this packet is newer than one with sequence number `last` (wrapping).
    pub fn is_newer_than(&self, last: u32) -> bool {
        let diff = self.seq.wrapping_sub(last);
        diff != 0 && diff < u32::MAX / 2
    }
}

/// A message from the client on the input channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientMessage {
    Input(InputPacket),
    Hotkeys(Hotkeys),
    Analog(AnalogPacket),
}

impl ClientMessage {
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        match bytes.first() {
            Some(&TYPE_HOTKEYS) => Hotkeys::decode(bytes).map(Self::Hotkeys),
            Some(&TYPE_ANALOG) => AnalogPacket::decode(bytes).map(Self::Analog),
            _ => InputPacket::decode(bytes).map(Self::Input),
        }
    }
}

/// Acknowledges that the frame `frame` consumed input packet `seq`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputAck {
    pub seq: u32,
    pub client_time_ms: u32,
    pub frame: u32,
}

impl InputAck {
    pub fn encode(&self) -> [u8; ACK_LEN] {
        let mut out = [0u8; ACK_LEN];
        out[0] = TYPE_ACK;
        out[1] = INPUT_VERSION;
        out[2..6].copy_from_slice(&self.seq.to_le_bytes());
        out[6..10].copy_from_slice(&self.client_time_ms.to_le_bytes());
        out[10..14].copy_from_slice(&self.frame.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        check_header(bytes, TYPE_ACK, ACK_LEN)?;
        Ok(Self {
            seq: u32_at(bytes, 2),
            client_time_ms: u32_at(bytes, 6),
            frame: u32_at(bytes, 10),
        })
    }
}

fn check_header(bytes: &[u8], kind: u8, min_len: usize) -> Result<&[u8], DecodeError> {
    if bytes.len() < min_len {
        return Err(DecodeError::TooShort);
    }
    if bytes[0] != kind {
        return Err(DecodeError::WrongType(bytes[0]));
    }
    if bytes[1] != INPUT_VERSION {
        return Err(DecodeError::UnsupportedVersion(bytes[1]));
    }
    Ok(bytes)
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_round_trips() {
        let packet = InputPacket { seq: 7, client_time_ms: 123_456, ports: vec![0x0108, 0] };
        let bytes = packet.encode();
        assert_eq!(bytes.len(), 15);
        assert_eq!(InputPacket::decode(&bytes), Ok(packet));
    }

    #[test]
    fn input_layout_is_stable() {
        // Clients in other languages are tested against these exact bytes.
        let bytes =
            InputPacket { seq: 0x0403_0201, client_time_ms: 0x0807_0605, ports: vec![0x0a09] }
                .encode();
        assert_eq!(bytes, [0x01, 1, 1, 2, 3, 4, 5, 6, 7, 8, 1, 0x09, 0x0a]);
        let ack = InputAck { seq: 1, client_time_ms: 2, frame: 3 }.encode();
        assert_eq!(ack, [0x81, 1, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0]);
    }

    #[test]
    fn hotkeys_round_trip_and_dispatch() {
        let both = Hotkeys { fast_forward: true, rewind: true };
        assert_eq!(both.encode(), [0x02, 1, 0b11]);
        assert_eq!(Hotkeys { fast_forward: true, rewind: false }.encode(), [0x02, 1, 0b01]);
        assert_eq!(
            Hotkeys::decode(&[0x02, 1, 0b10]),
            Ok(Hotkeys { fast_forward: false, rewind: true })
        );
        assert_eq!(ClientMessage::decode(&both.encode()), Ok(ClientMessage::Hotkeys(both)));
        let input = InputPacket { seq: 1, client_time_ms: 2, ports: vec![3] };
        assert_eq!(ClientMessage::decode(&input.encode()), Ok(ClientMessage::Input(input)));
        assert_eq!(Hotkeys::decode(&[0x02, 1]), Err(DecodeError::TooShort));
    }

    #[test]
    fn analog_layout_is_stable_and_round_trips() {
        // Clients in other languages are tested against these exact bytes.
        let packet = AnalogPacket {
            seq: 0x0403_0201,
            ports: vec![Analog { left: (-32768, 32767), right: (1, -2), l2: 0, r2: 0x1234 }],
        };
        let bytes = packet.encode();
        assert_eq!(
            bytes,
            [0x03, 1, 1, 2, 3, 4, 1, 0x00, 0x80, 0xff, 0x7f, 1, 0, 0xfe, 0xff, 0, 0, 0x34, 0x12]
        );
        assert_eq!(AnalogPacket::decode(&bytes), Ok(packet.clone()));
        assert_eq!(ClientMessage::decode(&bytes), Ok(ClientMessage::Analog(packet)));
        assert_eq!(AnalogPacket::decode(&bytes[..10]), Err(DecodeError::TooShort));
        let mut zero = bytes.clone();
        zero[6] = 0;
        assert_eq!(AnalogPacket::decode(&zero), Err(DecodeError::BadPortCount(0)));
    }

    #[test]
    fn ack_round_trips() {
        let ack = InputAck { seq: u32::MAX, client_time_ms: 9, frame: 600 };
        assert_eq!(InputAck::decode(&ack.encode()), Ok(ack));
    }

    #[test]
    fn rejects_malformed_input() {
        assert_eq!(InputPacket::decode(&[0x01, 1]), Err(DecodeError::TooShort));
        let mut bytes = InputPacket { seq: 1, client_time_ms: 0, ports: vec![1] }.encode();
        bytes[0] = 0x02;
        assert_eq!(InputPacket::decode(&bytes), Err(DecodeError::WrongType(0x02)));
        bytes[0] = 0x01;
        bytes[1] = 9;
        assert_eq!(InputPacket::decode(&bytes), Err(DecodeError::UnsupportedVersion(9)));
        bytes[1] = 1;
        bytes[10] = 0;
        assert_eq!(InputPacket::decode(&bytes), Err(DecodeError::BadPortCount(0)));
        bytes[10] = 2; // claims two ports but carries one
        assert_eq!(InputPacket::decode(&bytes), Err(DecodeError::TooShort));
        assert_eq!(InputAck::decode(&bytes), Err(DecodeError::TooShort));
    }

    #[test]
    fn sequence_comparison_wraps() {
        let packet = |seq| InputPacket { seq, client_time_ms: 0, ports: vec![0] };
        assert!(packet(2).is_newer_than(1));
        assert!(!packet(1).is_newer_than(1));
        assert!(!packet(1).is_newer_than(2));
        assert!(packet(0).is_newer_than(u32::MAX));
        assert!(packet(5).is_newer_than(u32::MAX - 5));
    }
}
