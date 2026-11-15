//! What each system calls its buttons, and simple diagrams of pads, as data every client draws
//! (docs/CONTROLLERS.md, "Systems and diagrams").
//!
//! A system's labels say which of its console's buttons each RetroPad button is, following its
//! core's own layout: on the Mega Drive (Genesis Plus GX) RetroPad Y, B and A are A, B and C. A
//! diagram is a few rounded rectangles for the body and one per button, on a 160×100 canvas;
//! the drawings are SovEmu's own schematics, not any maker's artwork. Systems' diagrams key
//! their buttons by RetroPad button (`B`, `START` …); controllers' by input (`south`, `l1` …).

use crate::controllers::PadButton;
use crate::controllers::PadButton::*;

/// A rounded rectangle; a circle when `r` is half its width and height.
#[derive(Debug, Clone, Copy, PartialEq, async_graphql::SimpleObject)]
pub struct Shape {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub r: f64,
    /// 0 the body, 1 a recess drawn darker (a screen, a stick's well).
    pub tone: i32,
}

/// A button on a diagram.
#[derive(Debug, Clone, PartialEq, async_graphql::SimpleObject)]
pub struct PadKey {
    /// A RetroPad button on a system's pad, an input name (`south` …) on a controller's.
    pub key: String,
    /// What the pad prints on it.
    pub label: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub r: f64,
}

/// A pad, drawn from shapes on a `width`×`height` canvas.
#[derive(Debug, Clone, PartialEq, async_graphql::SimpleObject)]
pub struct PadLayout {
    /// A system id, or a controller family (`xbox`, `playstation`, `nintendo`, `generic`).
    pub id: String,
    pub name: String,
    pub width: f64,
    pub height: f64,
    pub body: Vec<Shape>,
    pub keys: Vec<PadKey>,
}

/// One of a system's buttons: the RetroPad button that drives it and what the console calls it.
#[derive(Debug, Clone, PartialEq, async_graphql::SimpleObject)]
pub struct SystemButton {
    pub button: PadButton,
    pub label: String,
}

/// A system's buttons and its pad's diagram.
#[derive(Debug, Clone, PartialEq, async_graphql::SimpleObject)]
pub struct SystemControls {
    /// The buttons the system's pad has, in the order to list them.
    pub buttons: Vec<SystemButton>,
    pub layout: PadLayout,
}

const DPAD: [(PadButton, &str); 4] = [(Up, "↑"), (Down, "↓"), (Left, "←"), (Right, "→")];

/// The RetroPad buttons a system uses and what it calls them. Direction buttons come first.
fn labels(system: &str) -> Vec<(PadButton, &'static str)> {
    let rest: &[(PadButton, &str)] = match system {
        "nes" => &[(A, "A"), (B, "B"), (Start, "Start"), (Select, "Select")],
        "snes" => &[
            (A, "A"),
            (B, "B"),
            (X, "X"),
            (Y, "Y"),
            (L, "L"),
            (R, "R"),
            (Start, "Start"),
            (Select, "Select"),
        ],
        "gb" | "gbc" => &[(A, "A"), (B, "B"), (Start, "Start"), (Select, "Select")],
        "gba" => &[(A, "A"), (B, "B"), (L, "L"), (R, "R"), (Start, "Start"), (Select, "Select")],
        "megadrive" => &[
            (Y, "A"),
            (B, "B"),
            (A, "C"),
            (L, "X"),
            (X, "Y"),
            (R, "Z"),
            (Start, "Start"),
            (Select, "Mode"),
        ],
        "mastersystem" => &[(B, "1"), (A, "2"), (Start, "Pause")],
        "gamegear" => &[(B, "1"), (A, "2"), (Start, "Start")],
        "psx" | "ps2" => &[
            (B, "✕"),
            (A, "○"),
            (Y, "□"),
            (X, "△"),
            (L, "L1"),
            (R, "R1"),
            (L2, "L2"),
            (R2, "R2"),
            (L3, "L3"),
            (R3, "R3"),
            (Start, "Start"),
            (Select, "Select"),
        ],
        "fbneo" => &[
            (B, "1"),
            (A, "2"),
            (Y, "3"),
            (X, "4"),
            (L, "5"),
            (R, "6"),
            (Start, "Start"),
            (Select, "Coin"),
        ],
        "neogeo" => &[(B, "A"), (A, "B"), (Y, "C"), (X, "D"), (Start, "Start"), (Select, "Coin")],
        "n64" => &[(B, "A"), (Y, "B"), (L2, "Z"), (L, "L"), (R, "R"), (Start, "Start")],
        "dreamcast" => {
            &[(B, "A"), (A, "B"), (Y, "X"), (X, "Y"), (L2, "L"), (R2, "R"), (Start, "Start")]
        }
        "gamecube" => &[
            (B, "A"),
            (Y, "B"),
            (A, "X"),
            (X, "Y"),
            (R, "Z"),
            (L2, "L"),
            (R2, "R"),
            (Start, "Start"),
        ],
        _ => &[
            (B, "B"),
            (A, "A"),
            (Y, "Y"),
            (X, "X"),
            (L, "L"),
            (R, "R"),
            (L2, "L2"),
            (R2, "R2"),
            (L3, "L3"),
            (R3, "R3"),
            (Start, "Start"),
            (Select, "Select"),
        ],
    };
    DPAD.iter().copied().chain(rest.iter().copied()).collect()
}

// --- Drawing helpers --------------------------------------------------------------------------

fn rect(x: f64, y: f64, w: f64, h: f64, r: f64) -> Shape {
    Shape { x, y, w, h, r, tone: 0 }
}

fn recess(x: f64, y: f64, w: f64, h: f64, r: f64) -> Shape {
    Shape { tone: 1, ..rect(x, y, w, h, r) }
}

/// Builds a diagram's keys, labelling each from a lookup by key.
struct Keys<'a> {
    keys: Vec<PadKey>,
    label: &'a dyn Fn(&str) -> Option<String>,
}

impl Keys<'_> {
    #[allow(clippy::too_many_arguments)]
    fn add(&mut self, key: &str, x: f64, y: f64, w: f64, h: f64, r: f64) {
        // A key the system does not use is not drawn as a button.
        if let Some(label) = (self.label)(key) {
            self.keys.push(PadKey { key: key.to_owned(), label, x, y, w, h, r });
        }
    }
    fn circle(&mut self, key: &str, cx: f64, cy: f64, d: f64) {
        self.add(key, cx - d / 2.0, cy - d / 2.0, d, d, d / 2.0);
    }
    /// A cross of four arms around (cx, cy), `s` across.
    fn dpad(&mut self, keys: [&str; 4], cx: f64, cy: f64, s: f64) {
        let a = s / 3.0;
        let [up, down, left, right] = keys;
        self.add(up, cx - a / 2.0, cy - s / 2.0, a, a, 1.0);
        self.add(down, cx - a / 2.0, cy + a / 2.0, a, a, 1.0);
        self.add(left, cx - s / 2.0, cy - a / 2.0, a, a, 1.0);
        self.add(right, cx + a / 2.0, cy - a / 2.0, a, a, 1.0);
    }
    /// Four face buttons in a diamond: north, south, west, east.
    fn diamond(&mut self, keys: [&str; 4], cx: f64, cy: f64, spread: f64, d: f64) {
        let [n, s, w, e] = keys;
        self.circle(n, cx, cy - spread, d);
        self.circle(s, cx, cy + spread, d);
        self.circle(w, cx - spread, cy, d);
        self.circle(e, cx + spread, cy, d);
    }
}

const RETRO_DPAD: [&str; 4] = ["UP", "DOWN", "LEFT", "RIGHT"];
const INPUT_DPAD: [&str; 4] = ["up", "down", "left", "right"];

fn key_name(b: PadButton) -> &'static str {
    match b {
        B => "B",
        Y => "Y",
        Select => "SELECT",
        Start => "START",
        Up => "UP",
        Down => "DOWN",
        Left => "LEFT",
        Right => "RIGHT",
        A => "A",
        X => "X",
        L => "L",
        R => "R",
        L2 => "L2",
        R2 => "R2",
        L3 => "L3",
        R3 => "R3",
    }
}

/// A controller-shaped body with grips: the modern pads, and the systems drawn after them.
fn gamepad_body() -> Vec<Shape> {
    vec![
        rect(12.0, 30.0, 136.0, 42.0, 20.0),
        rect(14.0, 50.0, 34.0, 44.0, 17.0),
        rect(112.0, 50.0, 34.0, 44.0, 17.0),
    ]
}

/// A system's pad. Keys are RetroPad buttons, labelled with the system's names.
fn system_layout(id: &str, name: &str) -> PadLayout {
    let names = labels(id);
    let label =
        |key: &str| names.iter().find(|(b, _)| key_name(*b) == key).map(|(_, l)| (*l).to_owned());
    let mut k = Keys { keys: Vec::new(), label: &label };
    let body = match id {
        "nes" => {
            k.dpad(RETRO_DPAD, 38.0, 55.0, 24.0);
            k.add("SELECT", 64.0, 60.0, 12.0, 5.0, 2.5);
            k.add("START", 82.0, 60.0, 12.0, 5.0, 2.5);
            k.circle("B", 112.0, 60.0, 13.0);
            k.circle("A", 130.0, 60.0, 13.0);
            vec![rect(10.0, 32.0, 140.0, 46.0, 4.0), recess(56.0, 52.0, 46.0, 18.0, 2.0)]
        }
        "snes" => {
            k.add("L", 16.0, 21.0, 38.0, 8.0, 4.0);
            k.add("R", 106.0, 21.0, 38.0, 8.0, 4.0);
            k.dpad(RETRO_DPAD, 40.0, 55.0, 24.0);
            k.add("SELECT", 66.0, 58.0, 11.0, 5.0, 2.5);
            k.add("START", 83.0, 58.0, 11.0, 5.0, 2.5);
            k.diamond(["X", "B", "Y", "A"], 120.0, 55.0, 11.0, 10.0);
            vec![rect(10.0, 27.0, 140.0, 56.0, 28.0)]
        }
        "gb" | "gbc" => {
            k.dpad(RETRO_DPAD, 64.0, 66.0, 18.0);
            k.circle("B", 92.0, 70.0, 10.0);
            k.circle("A", 105.0, 64.0, 10.0);
            k.add("SELECT", 70.0, 83.0, 9.0, 4.0, 2.0);
            k.add("START", 83.0, 83.0, 9.0, 4.0, 2.0);
            vec![rect(46.0, 4.0, 68.0, 92.0, 7.0), recess(52.0, 10.0, 56.0, 42.0, 3.0)]
        }
        "gba" => {
            k.add("L", 14.0, 20.0, 30.0, 8.0, 4.0);
            k.add("R", 116.0, 20.0, 30.0, 8.0, 4.0);
            k.dpad(RETRO_DPAD, 28.0, 46.0, 18.0);
            k.circle("SELECT", 28.0, 64.0, 5.0);
            k.circle("START", 28.0, 72.0, 5.0);
            k.circle("B", 124.0, 56.0, 10.0);
            k.circle("A", 137.0, 47.0, 10.0);
            vec![rect(8.0, 25.0, 144.0, 56.0, 22.0), recess(46.0, 31.0, 68.0, 44.0, 2.0)]
        }
        "megadrive" => {
            k.add("SELECT", 122.0, 23.0, 18.0, 6.0, 3.0);
            k.dpad(RETRO_DPAD, 38.0, 55.0, 24.0);
            k.add("START", 72.0, 46.0, 14.0, 5.0, 2.5);
            k.circle("L", 98.0, 47.0, 8.0);
            k.circle("X", 112.0, 43.0, 8.0);
            k.circle("R", 126.0, 39.0, 8.0);
            k.circle("Y", 100.0, 62.0, 11.0);
            k.circle("B", 114.0, 58.0, 11.0);
            k.circle("A", 128.0, 54.0, 11.0);
            vec![rect(10.0, 28.0, 140.0, 52.0, 24.0)]
        }
        "mastersystem" => {
            k.dpad(RETRO_DPAD, 40.0, 56.0, 26.0);
            k.add("START", 72.0, 38.0, 16.0, 6.0, 2.0);
            k.circle("B", 108.0, 58.0, 13.0);
            k.circle("A", 128.0, 58.0, 13.0);
            vec![rect(10.0, 32.0, 140.0, 46.0, 4.0)]
        }
        "gamegear" => {
            k.dpad(RETRO_DPAD, 27.0, 55.0, 20.0);
            k.circle("START", 130.0, 36.0, 6.0);
            k.circle("B", 124.0, 61.0, 11.0);
            k.circle("A", 138.0, 52.0, 11.0);
            vec![rect(4.0, 24.0, 152.0, 60.0, 26.0), recess(48.0, 31.0, 64.0, 46.0, 3.0)]
        }
        "psx" | "ps2" => {
            k.add("L2", 20.0, 14.0, 30.0, 6.0, 3.0);
            k.add("L", 20.0, 22.0, 30.0, 6.0, 3.0);
            k.add("R2", 110.0, 14.0, 30.0, 6.0, 3.0);
            k.add("R", 110.0, 22.0, 30.0, 6.0, 3.0);
            k.dpad(RETRO_DPAD, 34.0, 46.0, 22.0);
            k.add("SELECT", 64.0, 46.0, 9.0, 4.0, 2.0);
            k.add("START", 87.0, 46.0, 9.0, 4.0, 2.0);
            k.diamond(["X", "B", "Y", "A"], 126.0, 46.0, 10.0, 9.0);
            k.circle("L3", 62.0, 66.0, 14.0);
            k.circle("R3", 98.0, 66.0, 14.0);
            gamepad_body()
        }
        "fbneo" | "neogeo" => {
            k.add("SELECT", 58.0, 30.0, 14.0, 5.0, 2.5);
            k.add("START", 78.0, 30.0, 14.0, 5.0, 2.5);
            k.dpad(RETRO_DPAD, 34.0, 58.0, 30.0);
            if id == "neogeo" {
                k.circle("B", 86.0, 66.0, 12.0);
                k.circle("A", 101.0, 59.0, 12.0);
                k.circle("Y", 117.0, 55.0, 12.0);
                k.circle("X", 133.0, 54.0, 12.0);
            } else {
                k.circle("Y", 92.0, 48.0, 12.0);
                k.circle("X", 108.0, 45.0, 12.0);
                k.circle("L", 124.0, 45.0, 12.0);
                k.circle("B", 92.0, 66.0, 12.0);
                k.circle("A", 108.0, 63.0, 12.0);
                k.circle("R", 124.0, 63.0, 12.0);
            }
            vec![rect(4.0, 24.0, 152.0, 62.0, 6.0), recess(24.0, 48.0, 20.0, 20.0, 10.0)]
        }
        "n64" => {
            k.add("L", 14.0, 23.0, 30.0, 6.0, 3.0);
            k.add("R", 116.0, 23.0, 30.0, 6.0, 3.0);
            k.dpad(RETRO_DPAD, 30.0, 46.0, 18.0);
            k.circle("START", 80.0, 42.0, 8.0);
            k.add("L2", 72.0, 88.0, 16.0, 6.0, 2.0);
            k.circle("Y", 104.0, 46.0, 10.0);
            k.circle("B", 114.0, 55.0, 10.0);
            vec![
                rect(10.0, 29.0, 140.0, 34.0, 15.0),
                rect(16.0, 48.0, 28.0, 44.0, 14.0),
                rect(66.0, 52.0, 28.0, 34.0, 14.0),
                rect(116.0, 48.0, 28.0, 44.0, 14.0),
                recess(73.0, 58.0, 14.0, 14.0, 7.0),
                recess(122.0, 34.0, 20.0, 20.0, 10.0),
            ]
        }
        "dreamcast" => {
            k.add("L2", 30.0, 18.0, 30.0, 6.0, 3.0);
            k.add("R2", 100.0, 18.0, 30.0, 6.0, 3.0);
            k.dpad(RETRO_DPAD, 46.0, 68.0, 20.0);
            k.circle("START", 80.0, 72.0, 7.0);
            k.diamond(["X", "B", "Y", "A"], 116.0, 52.0, 10.0, 9.0);
            vec![rect(18.0, 25.0, 124.0, 62.0, 28.0), recess(38.0, 34.0, 16.0, 16.0, 8.0)]
        }
        "gamecube" => {
            k.add("L2", 18.0, 22.0, 30.0, 6.0, 3.0);
            k.add("R2", 112.0, 16.0, 30.0, 6.0, 3.0);
            k.add("R", 112.0, 24.0, 24.0, 5.0, 2.5);
            k.dpad(RETRO_DPAD, 58.0, 66.0, 14.0);
            k.circle("START", 80.0, 48.0, 6.0);
            k.circle("B", 118.0, 52.0, 15.0);
            k.circle("Y", 104.0, 61.0, 8.0);
            k.circle("A", 133.0, 45.0, 9.0);
            k.circle("X", 118.0, 37.0, 9.0);
            let mut body = gamepad_body();
            body.push(recess(34.0, 38.0, 16.0, 16.0, 8.0));
            body
        }
        _ => {
            k.dpad(RETRO_DPAD, 34.0, 46.0, 22.0);
            k.diamond(["X", "B", "Y", "A"], 126.0, 46.0, 10.0, 9.0);
            k.add("SELECT", 64.0, 46.0, 9.0, 4.0, 2.0);
            k.add("START", 87.0, 46.0, 9.0, 4.0, 2.0);
            k.add("L", 20.0, 22.0, 30.0, 6.0, 3.0);
            k.add("R", 110.0, 22.0, 30.0, 6.0, 3.0);
            gamepad_body()
        }
    };
    PadLayout {
        id: id.to_owned(),
        name: name.to_owned(),
        width: 160.0,
        height: 100.0,
        body,
        keys: k.keys,
    }
}

/// A system's buttons and pad.
pub fn system_controls(system: &crate::systems::System) -> SystemControls {
    SystemControls {
        buttons: labels(system.id)
            .into_iter()
            .map(|(button, label)| SystemButton { button, label: label.to_owned() })
            .collect(),
        layout: system_layout(system.id, system.name),
    }
}

/// The controller families' labels, as docs/CONTROLLERS.md gives them.
fn family_label(family: &str, input: &str) -> Option<&'static str> {
    let xbox = |i: &str| -> Option<&'static str> {
        Some(match i {
            "south" => "A",
            "east" => "B",
            "west" => "X",
            "north" => "Y",
            "l1" => "LB",
            "r1" => "RB",
            "l2" => "LT",
            "r2" => "RT",
            "select" => "View",
            "start" => "Menu",
            "l3" => "LS",
            "r3" => "RS",
            "up" => "↑",
            "down" => "↓",
            "left" => "←",
            "right" => "→",
            _ => return None,
        })
    };
    match (family, input) {
        ("playstation", "south") => Some("✕"),
        ("playstation", "east") => Some("○"),
        ("playstation", "west") => Some("□"),
        ("playstation", "north") => Some("△"),
        ("playstation", "l1") => Some("L1"),
        ("playstation", "r1") => Some("R1"),
        ("playstation", "l2") => Some("L2"),
        ("playstation", "r2") => Some("R2"),
        ("playstation", "select") => Some("Create"),
        ("playstation", "start") => Some("Options"),
        ("playstation", "l3") => Some("L3"),
        ("playstation", "r3") => Some("R3"),
        ("nintendo", "south") => Some("B"),
        ("nintendo", "east") => Some("A"),
        ("nintendo", "west") => Some("Y"),
        ("nintendo", "north") => Some("X"),
        ("nintendo", "l1") => Some("L"),
        ("nintendo", "r1") => Some("R"),
        ("nintendo", "l2") => Some("ZL"),
        ("nintendo", "r2") => Some("ZR"),
        ("nintendo", "select") => Some("−"),
        ("nintendo", "start") => Some("+"),
        _ => xbox(input),
    }
}

/// The player's controllers, by family: `xbox`, `playstation`, `nintendo` and `generic`. Keys
/// are inputs (`south`, `l1` …).
pub fn controller_layouts() -> Vec<PadLayout> {
    [
        ("xbox", "Xbox"),
        ("playstation", "PlayStation"),
        ("nintendo", "Nintendo"),
        ("generic", "Controller"),
    ]
    .into_iter()
    .map(|(family, name)| {
        let label = |key: &str| family_label(family, key).map(str::to_owned);
        let mut k = Keys { keys: Vec::new(), label: &label };
        k.add("l2", 20.0, 14.0, 30.0, 6.0, 3.0);
        k.add("l1", 20.0, 22.0, 30.0, 6.0, 3.0);
        k.add("r2", 110.0, 14.0, 30.0, 6.0, 3.0);
        k.add("r1", 110.0, 22.0, 30.0, 6.0, 3.0);
        k.diamond(["north", "south", "west", "east"], 126.0, 46.0, 10.0, 9.0);
        k.add("select", 64.0, 44.0, 9.0, 5.0, 2.5);
        k.add("start", 87.0, 44.0, 9.0, 5.0, 2.5);
        if family == "playstation" {
            // The PlayStation's D-pad sits up top, its sticks low in the middle.
            k.dpad(INPUT_DPAD, 34.0, 46.0, 22.0);
            k.circle("l3", 62.0, 66.0, 14.0);
            k.circle("r3", 98.0, 66.0, 14.0);
        } else {
            // Offset sticks: the left one up top, the D-pad below it.
            k.circle("l3", 34.0, 46.0, 15.0);
            k.dpad(INPUT_DPAD, 58.0, 66.0, 18.0);
            k.circle("r3", 100.0, 66.0, 14.0);
        }
        PadLayout {
            id: family.to_owned(),
            name: name.to_owned(),
            width: 160.0,
            height: 100.0,
            body: gamepad_body(),
            keys: k.keys,
        }
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_system_labels_its_buttons_and_draws_each_of_them() {
        for system in crate::systems::SYSTEMS {
            let controls = system_controls(system);
            let keys: Vec<&str> = controls.layout.keys.iter().map(|k| k.key.as_str()).collect();
            for b in &controls.buttons {
                assert!(keys.contains(&key_name(b.button)), "{}: {:?} is not drawn", system.id, b);
            }
            assert_eq!(
                keys.len(),
                controls.buttons.len(),
                "{}: a drawn key has no label",
                system.id
            );
            for k in &controls.layout.keys {
                assert!(
                    k.x >= 0.0 && k.y >= 0.0 && k.x + k.w <= 160.0 && k.y + k.h <= 100.0,
                    "{}: {k:?}",
                    system.id
                );
            }
        }
    }

    #[test]
    fn systems_name_buttons_as_their_cores_do() {
        let md = labels("megadrive");
        assert!(md.contains(&(Y, "A")) && md.contains(&(B, "B")) && md.contains(&(A, "C")));
        assert!(labels("psx").contains(&(B, "✕")));
        assert_eq!(labels("nes").len(), 8, "four directions, A, B, Start, Select");
    }

    #[test]
    fn controllers_draw_every_input() {
        for layout in controller_layouts() {
            assert_eq!(layout.keys.len(), 16, "{}", layout.id);
        }
        let ps = &controller_layouts()[1];
        assert_eq!(ps.keys.iter().find(|k| k.key == "south").unwrap().label, "✕");
    }
}
