use std::fmt;
use std::str::FromStr;

use sovemu_libretro_ffi as ffi;

/// The state of one RetroPad: one bit per button, numbered as libretro's
/// `RETRO_DEVICE_ID_JOYPAD_*` ids, so the value is also what a core reads through
/// `RETRO_DEVICE_ID_JOYPAD_MASK`.
///
/// Clients send the whole state every time rather than press/release events, so a lost
/// input packet is corrected by the next one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct JoypadButtons(pub u16);

macro_rules! buttons {
    ($($name:ident = $id:ident, $text:literal;)*) => {
        impl JoypadButtons {
            $(
                #[doc = concat!("The `", $text, "` button.")]
                pub const $name: Self = Self(1 << ffi::$id);
            )*

            const NAMES: &[(&'static str, Self)] = &[$(($text, Self::$name)),*];
        }
    };
}

buttons! {
    B = RETRO_DEVICE_ID_JOYPAD_B, "b";
    Y = RETRO_DEVICE_ID_JOYPAD_Y, "y";
    SELECT = RETRO_DEVICE_ID_JOYPAD_SELECT, "select";
    START = RETRO_DEVICE_ID_JOYPAD_START, "start";
    UP = RETRO_DEVICE_ID_JOYPAD_UP, "up";
    DOWN = RETRO_DEVICE_ID_JOYPAD_DOWN, "down";
    LEFT = RETRO_DEVICE_ID_JOYPAD_LEFT, "left";
    RIGHT = RETRO_DEVICE_ID_JOYPAD_RIGHT, "right";
    A = RETRO_DEVICE_ID_JOYPAD_A, "a";
    X = RETRO_DEVICE_ID_JOYPAD_X, "x";
    L = RETRO_DEVICE_ID_JOYPAD_L, "l";
    R = RETRO_DEVICE_ID_JOYPAD_R, "r";
    L2 = RETRO_DEVICE_ID_JOYPAD_L2, "l2";
    R2 = RETRO_DEVICE_ID_JOYPAD_R2, "r2";
    L3 = RETRO_DEVICE_ID_JOYPAD_L3, "l3";
    R3 = RETRO_DEVICE_ID_JOYPAD_R3, "r3";
}

impl JoypadButtons {
    /// No buttons held.
    pub const NONE: Self = Self(0);

    /// Whether the button with libretro id `id` is held.
    pub fn is_pressed(self, id: u32) -> bool {
        id < 16 && self.0 & (1 << id) != 0
    }

    /// Whether every button in `other` is held.
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for JoypadButtons {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for JoypadButtons {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// An unknown button name given to [`JoypadButtons::from_str`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseButtonsError(pub String);

impl fmt::Display for ParseButtonsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<_> = JoypadButtons::NAMES.iter().map(|(n, _)| *n).collect();
        write!(f, "unknown button {:?} (expected one of {})", self.0, names.join(", "))
    }
}

impl std::error::Error for ParseButtonsError {}

/// Parses `+`-separated button names, case-insensitively: `"start"`, `"a+b"`, `"Up+Right"`.
/// `"none"` (or an empty string) is no buttons.
impl FromStr for JoypadButtons {
    type Err = ParseButtonsError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut buttons = Self::NONE;
        for name in s.split('+').map(str::trim).filter(|n| !n.is_empty()) {
            let name = name.to_ascii_lowercase();
            if name == "none" {
                continue;
            }
            let (_, button) = Self::NAMES
                .iter()
                .find(|(n, _)| *n == name)
                .ok_or_else(|| ParseButtonsError(name.clone()))?;
            buttons |= *button;
        }
        Ok(buttons)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_match_libretro_ids() {
        assert_eq!(JoypadButtons::B.0, 1);
        assert_eq!(JoypadButtons::START.0, 1 << 3);
        assert_eq!(JoypadButtons::A.0, 1 << 8);
        assert_eq!(JoypadButtons::R3.0, 1 << 15);
    }

    #[test]
    fn parses_combinations() {
        assert_eq!("start".parse(), Ok(JoypadButtons::START));
        assert_eq!("A + b".parse(), Ok(JoypadButtons::A | JoypadButtons::B));
        assert_eq!("none".parse(), Ok(JoypadButtons::NONE));
        assert_eq!("".parse(), Ok(JoypadButtons::NONE));
        assert!("turbo".parse::<JoypadButtons>().is_err());
    }

    #[test]
    fn is_pressed_checks_one_id() {
        let held = JoypadButtons::A | JoypadButtons::UP;
        assert!(held.is_pressed(ffi::RETRO_DEVICE_ID_JOYPAD_A));
        assert!(held.is_pressed(ffi::RETRO_DEVICE_ID_JOYPAD_UP));
        assert!(!held.is_pressed(ffi::RETRO_DEVICE_ID_JOYPAD_B));
        assert!(!held.is_pressed(ffi::RETRO_DEVICE_ID_JOYPAD_MASK));
    }
}

/// One port's analog controls, as libretro's `RETRO_DEVICE_ANALOG` reports them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct AnalogSticks {
    /// Left stick, x right and y down, −32768 to 32767.
    pub left: (i16, i16),
    pub right: (i16, i16),
    /// Analog triggers, 0 to 32767 (cores read them as analog L2 and R2).
    pub l2: i16,
    pub r2: i16,
}

impl AnalogSticks {
    /// Centred sticks, released triggers.
    pub const CENTRED: Self = Self { left: (0, 0), right: (0, 0), l2: 0, r2: 0 };

    /// Answers `input_state(port, RETRO_DEVICE_ANALOG, index, id)`. Analog buttons other than
    /// the triggers read as fully pressed or released from the digital state.
    pub(crate) fn read(self, buttons: JoypadButtons, index: u32, id: u32) -> i16 {
        match (index, id) {
            (ffi::RETRO_DEVICE_INDEX_ANALOG_LEFT, ffi::RETRO_DEVICE_ID_ANALOG_X) => self.left.0,
            (ffi::RETRO_DEVICE_INDEX_ANALOG_LEFT, ffi::RETRO_DEVICE_ID_ANALOG_Y) => self.left.1,
            (ffi::RETRO_DEVICE_INDEX_ANALOG_RIGHT, ffi::RETRO_DEVICE_ID_ANALOG_X) => self.right.0,
            (ffi::RETRO_DEVICE_INDEX_ANALOG_RIGHT, ffi::RETRO_DEVICE_ID_ANALOG_Y) => self.right.1,
            (ffi::RETRO_DEVICE_INDEX_ANALOG_BUTTON, id) => {
                let digital = if buttons.is_pressed(id) { i16::MAX } else { 0 };
                match id {
                    ffi::RETRO_DEVICE_ID_JOYPAD_L2 => self.l2.max(digital),
                    ffi::RETRO_DEVICE_ID_JOYPAD_R2 => self.r2.max(digital),
                    _ => digital,
                }
            }
            _ => 0,
        }
    }
}

#[cfg(test)]
mod analog_tests {
    use super::*;

    #[test]
    fn reads_sticks_triggers_and_analog_buttons() {
        let a = AnalogSticks { left: (100, -200), right: (-300, 400), l2: 1000, r2: 0 };
        let none = JoypadButtons::NONE;
        assert_eq!(
            a.read(none, ffi::RETRO_DEVICE_INDEX_ANALOG_LEFT, ffi::RETRO_DEVICE_ID_ANALOG_X),
            100
        );
        assert_eq!(
            a.read(none, ffi::RETRO_DEVICE_INDEX_ANALOG_LEFT, ffi::RETRO_DEVICE_ID_ANALOG_Y),
            -200
        );
        assert_eq!(
            a.read(none, ffi::RETRO_DEVICE_INDEX_ANALOG_RIGHT, ffi::RETRO_DEVICE_ID_ANALOG_X),
            -300
        );
        assert_eq!(
            a.read(none, ffi::RETRO_DEVICE_INDEX_ANALOG_RIGHT, ffi::RETRO_DEVICE_ID_ANALOG_Y),
            400
        );
        let button = ffi::RETRO_DEVICE_INDEX_ANALOG_BUTTON;
        assert_eq!(a.read(none, button, ffi::RETRO_DEVICE_ID_JOYPAD_L2), 1000);
        assert_eq!(
            a.read(JoypadButtons::R2, button, ffi::RETRO_DEVICE_ID_JOYPAD_R2),
            i16::MAX,
            "digital R2 counts fully"
        );
        assert_eq!(a.read(JoypadButtons::A, button, ffi::RETRO_DEVICE_ID_JOYPAD_A), i16::MAX);
        assert_eq!(a.read(none, button, ffi::RETRO_DEVICE_ID_JOYPAD_A), 0);
    }
}
