//! Scripted input for headless runs: which buttons player 1 holds on which frames.

use std::ops::RangeInclusive;
use std::str::FromStr;

use sovemu_retro_host::JoypadButtons;

/// Buttons held over an inclusive range of frames, parsed from `120-130:start` or `300:a+b`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputHold {
    pub frames: RangeInclusive<u64>,
    pub buttons: JoypadButtons,
}

impl FromStr for InputHold {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (frames, buttons) =
            s.split_once(':').ok_or_else(|| format!("expected FRAMES:BUTTONS, got {s:?}"))?;
        let frame =
            |f: &str| f.trim().parse::<u64>().map_err(|_| format!("{f:?} is not a frame number"));
        let frames = match frames.split_once('-') {
            Some((start, end)) => frame(start)?..=frame(end)?,
            None => {
                let f = frame(frames)?;
                f..=f
            }
        };
        if frames.is_empty() {
            return Err(format!("frame range {s:?} is empty"));
        }
        let buttons = buttons.parse::<JoypadButtons>().map_err(|e| e.to_string())?;
        Ok(Self { frames, buttons })
    }
}

/// The buttons held on `frame`: the union of every hold covering it.
pub fn buttons_at(holds: &[InputHold], frame: u64) -> JoypadButtons {
    holds
        .iter()
        .filter(|h| h.frames.contains(&frame))
        .fold(JoypadButtons::NONE, |acc, h| acc | h.buttons)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ranges_and_single_frames() {
        assert_eq!(
            "120-130:start".parse(),
            Ok(InputHold { frames: 120..=130, buttons: JoypadButtons::START })
        );
        assert_eq!(
            "300:a+b".parse(),
            Ok(InputHold { frames: 300..=300, buttons: JoypadButtons::A | JoypadButtons::B })
        );
        assert!("130-120:start".parse::<InputHold>().is_err());
        assert!("start".parse::<InputHold>().is_err());
        assert!("x:start".parse::<InputHold>().is_err());
        assert!("10:turbo".parse::<InputHold>().is_err());
    }

    #[test]
    fn overlapping_holds_combine() {
        let holds: Vec<InputHold> =
            ["10-20:right", "15:a"].iter().map(|s| s.parse().unwrap()).collect();
        assert_eq!(buttons_at(&holds, 9), JoypadButtons::NONE);
        assert_eq!(buttons_at(&holds, 10), JoypadButtons::RIGHT);
        assert_eq!(buttons_at(&holds, 15), JoypadButtons::RIGHT | JoypadButtons::A);
        assert_eq!(buttons_at(&holds, 20), JoypadButtons::RIGHT);
        assert_eq!(buttons_at(&holds, 21), JoypadButtons::NONE);
    }
}
