//! BIOS files: which each system uses, where they are, and whether they are there.
//!
//! They live on the server under `SovEmu__Paths__Bios` (`<data>/bios`), in any layout: one folder
//! per system as SovEmu recommends (`bios/psx/scph5501.bin`, `bios/dreamcast/dc_boot.bin`),
//! Batocera's (`bios/scph5501.bin`, `bios/dc/dc_boot.bin`), or a read-only share as it is. The
//! whole folder is indexed, and each file a core reads is chosen by ([`choose`]):
//!
//! 1. a known dump's MD5, anywhere (when that MD5 is this file's alone in the system: the
//!    GameCube's USA and JAP IPLs share theirs, so those also need their name);
//! 2. its name (any letter case), preferring the system's own folder, then a folder named like
//!    the system (`dc`, `ps2`), then the shallowest;
//! 3. for cores that also look beside the games (FBNeo), its exact name in a ROM folder.
//!
//! Nothing is renamed or moved: each session gets a folder of links, each file under the name
//! and in the place its core expects ([`link_session`]), which the worker shows the core
//! (`SovEmu.Worker/src/sysdir.rs`).
//!
//! The names, sizes and MD5s are those of libretro's `System.dat` (libretro-database
//! b2e5eaa256eb30c6301ed42a8eab3ba2afd74b8a), limited to the files SovEmu's cores read. Each
//! group says how much a system depends on it: [`Importance::Required`] (the core cannot start
//! a game without it), `Recommended` (the core has a built-in replacement that plays most
//! games) or `Optional` (a boot logo, an add-on).
//!
//! A file found by its name whose checksum none of the known dumps has is *unverified*:
//! possibly a bad dump, possibly a revision the list lacks. It is used all the same.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime};

use md5::{Digest, Md5};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Importance {
    Required,
    Recommended,
    Optional,
}

/// A file a core reads.
#[derive(Debug)]
pub struct Known {
    /// Its path in the system's folder.
    pub name: &'static str,
    /// Other names the core reads it under.
    pub alt: &'static [&'static str],
    pub label: &'static str,
    pub size: u64,
    /// Good dumps (empty: any content is fine).
    pub md5: &'static [&'static str],
}

#[derive(Debug)]
pub enum Kind {
    /// Any one of these files is enough.
    AnyOf(&'static [Known]),
    /// Any number of dumps under any name, as PCSX2 lists every BIOS in its folder:
    /// files of `size` bytes, known by MD5 (`(md5, name)`).
    Dumps { size: u64, known: &'static [(&'static str, &'static str)] },
}

#[derive(Debug)]
pub struct Group {
    pub label: &'static str,
    pub importance: Importance,
    /// What it does for the system, and what happens without it.
    pub help: &'static str,
    pub kind: Kind,
}

#[derive(Debug)]
pub struct Spec {
    pub system: &'static str,
    /// Where the core looks for the system folder's files, in its system directory.
    pub core_path: &'static str,
    /// The core also finds them beside the games (FBNeo looks in the ROM's folder first).
    pub beside_games: bool,
    pub groups: &'static [Group],
}

const fn one(
    name: &'static str,
    label: &'static str,
    size: u64,
    md5: &'static [&'static str],
) -> Known {
    Known { name, alt: &[], label, size, md5 }
}

const PSX: u64 = 512 * 1024;
const GC_NTSC: &[&str] = &[
    "fc924a7c879b661abc37cec4f018fdf3",
    "019e39822a9ca3029124f74dd4d55ac4",
    "b17148254a5799684c7d783206504926",
];
const GC_PAL: &[&str] = &[
    "0cdda509e2da83c85bfe423dd87346cc",
    "339848a0b7c2124cf155276c1e79cbd0",
    "db92574caab77a7ec99d4605fd6f2450",
];
const NEOGEO: Known =
    one("neogeo.zip", "Neo Geo BIOS set", 1_859_335, &["00dad01abdbf8ea9e79ad2fe11bdb182"]);

pub(crate) const SPECS: &[Spec] = &[
    Spec {
        system: "nes",
        core_path: "",
        beside_games: false,
        groups: &[Group {
            label: "Famicom Disk System",
            importance: Importance::Optional,
            help: "Needed only for Famicom Disk System games (.fds).",
            kind: Kind::AnyOf(&[one(
                "disksys.rom",
                "Disk System BIOS",
                8192,
                &["ca30b50f880eb660a320674ed365ef7a"],
            )]),
        }],
    },
    Spec {
        system: "gb",
        core_path: "",
        beside_games: false,
        groups: &[Group {
            label: "Boot ROM",
            importance: Importance::Optional,
            help: "Shows the Nintendo logo at start-up. Games play the same without it.",
            kind: Kind::AnyOf(&[one(
                "gb_bios.bin",
                "Game Boy boot ROM",
                256,
                &["32fbbd84168d3482956eb3c5051637f5"],
            )]),
        }],
    },
    Spec {
        system: "gbc",
        core_path: "",
        beside_games: false,
        groups: &[Group {
            label: "Boot ROM",
            importance: Importance::Optional,
            help: "Shows the start-up logo and colourises original Game Boy games as the console did.",
            kind: Kind::AnyOf(&[one(
                "gbc_bios.bin",
                "Game Boy Color boot ROM",
                2304,
                &["dbfce9db9deaa2567f6a84fde55f9680"],
            )]),
        }],
    },
    Spec {
        system: "gba",
        core_path: "",
        beside_games: false,
        groups: &[Group {
            label: "BIOS",
            importance: Importance::Recommended,
            help: "mGBA's built-in BIOS plays almost every game; the real one fixes the few that \
                   rely on its exact behaviour, and shows the start-up logo.",
            kind: Kind::AnyOf(&[one(
                "gba_bios.bin",
                "Game Boy Advance BIOS",
                16384,
                &["a860e8c0b6d573d191e4ec7db1b1e4f6"],
            )]),
        }],
    },
    Spec {
        system: "megadrive",
        core_path: "",
        beside_games: false,
        groups: &[Group {
            label: "TMSS BIOS",
            importance: Importance::Optional,
            help: "Shows the \"produced by or under license from Sega\" screen. Games play the same without it.",
            kind: Kind::AnyOf(&[one(
                "bios_MD.bin",
                "Mega Drive TMSS BIOS",
                2048,
                &["d3293ebaaa7f4eb2a6766b68a0fb4609"],
            )]),
        }],
    },
    Spec {
        system: "mastersystem",
        core_path: "",
        beside_games: false,
        groups: &[
            Group {
                label: "BIOS (Europe)",
                importance: Importance::Optional,
                help: "The console's start-up screen. Games play the same without it.",
                kind: Kind::AnyOf(&[one(
                    "bios_E.sms",
                    "Master System BIOS (Europe)",
                    8192,
                    &["840481177270d5642a14ca71ee72844c"],
                )]),
            },
            Group {
                label: "BIOS (USA)",
                importance: Importance::Optional,
                help: "The console's start-up screen. Games play the same without it.",
                kind: Kind::AnyOf(&[one(
                    "bios_U.sms",
                    "Master System BIOS (USA)",
                    8192,
                    &["840481177270d5642a14ca71ee72844c"],
                )]),
            },
            Group {
                label: "BIOS (Japan)",
                importance: Importance::Optional,
                help: "The console's start-up screen. Games play the same without it.",
                kind: Kind::AnyOf(&[one(
                    "bios_J.sms",
                    "Master System BIOS (Japan)",
                    8192,
                    &["24a519c53f67b00640d0048ef7089105"],
                )]),
            },
        ],
    },
    Spec {
        system: "gamegear",
        core_path: "",
        beside_games: false,
        groups: &[Group {
            label: "BIOS",
            importance: Importance::Optional,
            help: "The console's start-up screen. Games play the same without it.",
            kind: Kind::AnyOf(&[one(
                "bios.gg",
                "Game Gear BIOS",
                1024,
                &["672e104c3be3a238301aceffc3b23fd6"],
            )]),
        }],
    },
    Spec {
        system: "segacd",
        core_path: "",
        beside_games: false,
        groups: &[
            Group {
                label: "BIOS (USA)",
                importance: Importance::Required,
                help: "American Sega CD games need it. Genesis Plus GX picks the BIOS by the \
                       game's region, so keep the one for each region you play.",
                kind: Kind::AnyOf(&[one(
                    "bios_CD_U.bin",
                    "Sega CD BIOS (USA)",
                    131_072,
                    &["2efd74e3232ff260e371b99f84024f7f", "854b9150240a198070150e4566ae1290"],
                )]),
            },
            Group {
                label: "BIOS (Europe)",
                importance: Importance::Required,
                help: "European Mega-CD games need it.",
                kind: Kind::AnyOf(&[one(
                    "bios_CD_E.bin",
                    "Mega-CD BIOS (Europe)",
                    131_072,
                    &["e66fa1dc5820d254611fdcdba0662372"],
                )]),
            },
            Group {
                label: "BIOS (Japan)",
                importance: Importance::Required,
                help: "Japanese Mega-CD games need it.",
                kind: Kind::AnyOf(&[one(
                    "bios_CD_J.bin",
                    "Mega-CD BIOS (Japan)",
                    131_072,
                    &["278a9397d192149e84e820ac621a8edd"],
                )]),
            },
        ],
    },
    Spec {
        system: "pcenginecd",
        core_path: "",
        beside_games: false,
        groups: &[Group {
            label: "System Card",
            importance: Importance::Required,
            help: "Every CD game needs the Super CD-ROM² System Card 3.0. HuCard games (the \
                   PC Engine folder) need nothing.",
            kind: Kind::AnyOf(&[one(
                "syscard3.pce",
                "Super CD-ROM² System Card 3.0",
                262_144,
                &["38179df8f4ac870017db21ebcbf53114"],
            )]),
        }],
    },
    Spec {
        system: "atari7800",
        core_path: "",
        beside_games: false,
        groups: &[Group {
            label: "BIOS",
            importance: Importance::Optional,
            help: "Shows the Atari logo at start-up. Games play the same without it.",
            kind: Kind::AnyOf(&[
                one(
                    "7800 BIOS (U).rom",
                    "7800 BIOS (USA)",
                    4096,
                    &["0763f1ffb006ddbe32e52d497ee848ae"],
                ),
                one(
                    "7800 BIOS (E).rom",
                    "7800 BIOS (Europe)",
                    16_384,
                    &["397bb566584be7b9764e7a68974c4263"],
                ),
            ]),
        }],
    },
    Spec {
        system: "lynx",
        core_path: "",
        beside_games: false,
        groups: &[Group {
            label: "Boot ROM",
            importance: Importance::Required,
            help: "Handy cannot start a game without the Lynx boot ROM.",
            kind: Kind::AnyOf(&[one(
                "lynxboot.img",
                "Lynx boot ROM",
                512,
                &["fcd403db69f54290b51035d82f835e7b"],
            )]),
        }],
    },
    Spec {
        system: "intellivision",
        core_path: "",
        beside_games: false,
        groups: &[
            Group {
                label: "Executive ROM",
                importance: Importance::Required,
                help: "FreeIntv needs both the Executive ROM and the Graphics ROM.",
                kind: Kind::AnyOf(&[one(
                    "exec.bin",
                    "Intellivision Executive ROM",
                    8192,
                    &["62e761035cb657903761800f4437b8af"],
                )]),
            },
            Group {
                label: "Graphics ROM",
                importance: Importance::Required,
                help: "FreeIntv needs both the Executive ROM and the Graphics ROM.",
                kind: Kind::AnyOf(&[one(
                    "grom.bin",
                    "Intellivision Graphics ROM",
                    2048,
                    &["0cd5946c6473e42e8e4c2137785e427f"],
                )]),
            },
        ],
    },
    Spec {
        system: "colecovision",
        core_path: "",
        beside_games: false,
        groups: &[Group {
            label: "BIOS",
            importance: Importance::Required,
            help: "Gearcoleco cannot start a game without the ColecoVision OS-7 BIOS.",
            kind: Kind::AnyOf(&[Known {
                name: "colecovision.rom",
                alt: &["coleco.rom"],
                label: "ColecoVision OS-7 BIOS",
                size: 8192,
                md5: &["2c66f5911e5b42b8ebe113403548eee7"],
            }]),
        }],
    },
    Spec {
        system: "psx",
        core_path: "",
        beside_games: false,
        groups: &[Group {
            label: "BIOS",
            importance: Importance::Recommended,
            help: "PCSX-ReARMed's built-in BIOS plays most games; a real one fixes the rest and shows \
                   the start-up screen. Any one of these is enough, whatever the games' region.",
            kind: Kind::AnyOf(&[
                one("scph5501.bin", "SCPH-5501 (USA)", PSX, &["490f666e1afb15b7362b406ed1cea246"]),
                one(
                    "scph5500.bin",
                    "SCPH-5500 (Japan)",
                    PSX,
                    &["8dd7d5296a650fac7319bce665a6a53c"],
                ),
                one(
                    "scph5502.bin",
                    "SCPH-5502 (Europe)",
                    PSX,
                    &["32736f17079d0b2b7024407c39bd3050"],
                ),
                one("scph1001.bin", "SCPH-1001 (USA)", PSX, &["924e392ed05558ffdb115408c263dccf"]),
                one("scph7001.bin", "SCPH-7001 (USA)", PSX, &["1e68c231d0896b7eadcad1d7d8e76129"]),
                one(
                    "scph101.bin",
                    "SCPH-101 (PSone, USA)",
                    PSX,
                    &["6e3735ff4c7dc899ee98981385f6f3d0"],
                ),
            ]),
        }],
    },
    Spec {
        system: "fbneo",
        core_path: "",
        beside_games: true,
        groups: &[Group {
            label: "Neo Geo BIOS",
            importance: Importance::Optional,
            help: "Needed only for Neo Geo games kept in the arcade folder. FinalBurn Neo also finds \
                   it beside the games.",
            kind: Kind::AnyOf(&[NEOGEO]),
        }],
    },
    Spec {
        system: "neogeo",
        core_path: "",
        beside_games: true,
        groups: &[Group {
            label: "Neo Geo BIOS",
            importance: Importance::Required,
            help: "Every Neo Geo game needs it. FinalBurn Neo also finds it beside the games. Another \
                   version of the set may work: FinalBurn Neo checks the files inside when a game starts.",
            kind: Kind::AnyOf(&[NEOGEO]),
        }],
    },
    Spec {
        system: "dreamcast",
        core_path: "dc",
        beside_games: false,
        groups: &[
            Group {
                label: "Boot ROM",
                importance: Importance::Recommended,
                help: "Flycast's built-in BIOS plays many games, but not all; with the real one \
                       every game boots, through the console's start-up screen.",
                kind: Kind::AnyOf(&[Known {
                    name: "dc_boot.bin",
                    alt: &["dc_bios.bin"],
                    label: "Dreamcast boot ROM",
                    size: 2 * 1024 * 1024,
                    md5: &["e10c53c2f8b90bab96ead2d368858623"],
                }]),
            },
            Group {
                label: "Flash memory",
                importance: Importance::Optional,
                help: "The console's settings (language, region). Flycast makes a fresh one without it.",
                kind: Kind::AnyOf(&[one(
                    "dc_flash.bin",
                    "Dreamcast flash",
                    128 * 1024,
                    &["0a93f7940c455905bea6e392dfde92a4"],
                )]),
            },
        ],
    },
    Spec {
        system: "gamecube",
        core_path: "dolphin-emu/Sys/GC",
        beside_games: false,
        groups: &[
            Group {
                label: "IPL (USA)",
                importance: Importance::Optional,
                help: "The console's start-up animation and menus. Dolphin plays games without it.",
                kind: Kind::AnyOf(&[one(
                    "USA/IPL.bin",
                    "GameCube IPL (NTSC)",
                    2 * 1024 * 1024,
                    GC_NTSC,
                )]),
            },
            Group {
                label: "IPL (Europe)",
                importance: Importance::Optional,
                help: "The console's start-up animation and menus. Dolphin plays games without it.",
                kind: Kind::AnyOf(&[one(
                    "EUR/IPL.bin",
                    "GameCube IPL (PAL)",
                    2 * 1024 * 1024,
                    GC_PAL,
                )]),
            },
            Group {
                label: "IPL (Japan)",
                importance: Importance::Optional,
                help: "The console's start-up animation and menus. Dolphin plays games without it.",
                kind: Kind::AnyOf(&[one(
                    "JAP/IPL.bin",
                    "GameCube IPL (NTSC-J)",
                    2 * 1024 * 1024,
                    GC_NTSC,
                )]),
            },
        ],
    },
    Spec {
        system: "ps2",
        core_path: "pcsx2/bios",
        beside_games: false,
        groups: &[Group {
            label: "BIOS",
            importance: Importance::Required,
            help: "PCSX2 cannot start a game without one. Any region plays any game; put one or \
                   more dumps here under any name.",
            kind: Kind::Dumps { size: 4 * 1024 * 1024, known: PS2_DUMPS },
        }],
    },
];

/// Every PS2 BIOS dump in `System.dat`: `(md5, name)`.
const PS2_DUMPS: &[(&str, &str)] = &[
    ("32f2e4d5ff5ee11072a6bc45530f5765", "ps2-0100jd-20000117"),
    ("acf4730ceb38ac9d8c7d8e21f2614600", "ps2-0100j-20000117"),
    ("acf9968c8f596d2b15f42272082513d1", "ps2-0101jd-20000217"),
    ("b1459d7446c69e3e97e6ace3ae23dd1c", "ps2-0101j-20000217"),
    ("d3f1853a16c2ec18f3cd1ae655213308", "ps2-0101xd-20000224"),
    ("63e6fd9b3c72e0d7b920e80cf76645cd", "ps2-0110ad-20000727"),
    ("a20c97c02210f16678ca3010127caf36", "ps2-0110a-20000727"),
    ("8db2fbbac7413bf3e7154c1e0715e565", "ps2-0120a-20000902"),
    ("91c87cb2f2eb6ce529a2360f80ce2457", "ps2-0120ed-20000902"),
    ("3016b3dd42148a67e2c048595ca4d7ce", "ps2-0120ed-20000902-20030110"),
    ("b7fa11e87d51752a98b38e3e691cbf17", "ps2-0120e-20000902"),
    ("f63bc530bd7ad7c026fcd6f7bd0d9525", "ps2-0120j-20001027-185015"),
    ("cee06bd68c333fc5768244eae77e4495", "ps2-0120j-20001027-191435"),
    ("0bf988e9c7aaa4c051805b0fa6eb3387", "ps2-0150ad-20001228-20030520"),
    ("8accc3c49ac45f5ae2c5db0adc854633", "ps2-0150a-20001228"),
    ("6f9a6feb749f0533aaae2cc45090b0ed", "ps2-0150ed-20001228-20030520"),
    ("838544f12de9b0abc90811279ee223c8", "ps2-0150e-20001228"),
    ("bb6bbc850458fff08af30e969ffd0175", "ps2-0150jd-20010118"),
    ("815ac991d8bc3b364696bead3457de7d", "ps2-0150j-20010118"),
    ("b107b5710042abe887c0f6175f6e94bb", "ps2-0160a-20010427"),
    ("ab55cceea548303c22c72570cfd4dd71", "ps2-0160j-20010427"),
    ("18bcaadb9ff74ed3add26cdf709fff2e", "ps2-0160a-20010704"),
    ("491209dd815ceee9de02dbbc408c06d6", "ps2-0160e-20010704"),
    ("7200a03d51cacc4c14fcdfdbc4898431", "ps2-0160a-20011004"),
    ("8359638e857c8bc18c3c18ac17d9cc3c", "ps2-0160e-20011004"),
    ("352d2ff9b3f68be7e6fa7e6dd8389346", "ps2-0160h-20010730"),
    ("d5ce2c7d119f563ce04bc04dbc3a323e", "ps2-0160a-20020207"),
    ("0d2228e6fd4fb639c9c39d077a9ec10c", "ps2-0160e-20020319"),
    ("72da56fccb8fcd77bba16d1b6f479914", "ps2-0160j-20020426"),
    ("5b1f47fbeb277c6be2fccdd6344ff2fd", "ps2-0160e-20020426"),
    ("315a4003535dfda689752cb25f24785c", "ps2-0160h-20020426"),
    ("312ad4816c232a9606e56f946bc0678a", "ps2-0170j-20030206"),
    ("666018ffec65c5c7e04796081295c6c7", "ps2-0170ed-20030227"),
    ("6e69920fa6eef8522a1d688a11e41bc6", "ps2-0170e-20030227"),
    ("eb960de68f0c0f7f9fa083e9f79d0360", "ps2-0170ad-20030325"),
    ("8aa12ce243210128c5074552d3b86251", "ps2-0170a-20030325"),
    ("240d4c5ddd4b54069bdc4a3cd2faf99d", "ps2-0180cd-20030224"),
    ("1c6cd089e6c83da618fbf2a081eb4888", "ps2-0180j-20031028"),
    ("463d87789c555a4a7604e97d7db545d1", "ps2-0190j-20030623"),
    ("35461cecaa51712b300b2d6798825048", "ps2-0190a-20030623"),
    ("bd6415094e1ce9e05daabe85de807666", "ps2-0190e-20030623"),
    ("2e70ad008d4ec8549aada8002fdf42fb", "ps2-0190h-20030623"),
    ("b53d51edc7fc086685e31b811dc32aad", "ps2-0190r-20030623"),
    ("1b6e631b536247756287b916f9396872", "ps2-0190c-20030623"),
    ("00da1b177096cfd2532c8fa22b43e667", "ps2-0190j-20030822"),
    ("afde410bd026c16be605a1ae4bd651fd", "ps2-0190e-20030822"),
    ("81f4336c1de607dd0865011c0447052e", "ps2-0190a-20040329"),
    ("0eee5d1c779aa50e94edd168b4ebf42e", "ps2-0200j-20040614"),
    ("d333558cc14561c1fdc334c75d5f37b7", "ps2-0200a-20040614"),
    ("dc752f160044f2ed5fc1f4964db2a095", "ps2-0200e-20040614"),
    ("63ead1d74893bf7f36880af81f68a82d", "ps2-0200ed-20040614"),
    ("3e3e030c0f600442fa05b94f87a1e238", "ps2-0200h-20040614"),
    ("1ad977bb539fc9448a08ab276a836bbc", "ps2-0210j-20040917"),
    ("eb4f40fcf4911ede39c1bbfe91e7a89a", "ps2-0220j-20050620"),
    ("9959ad7a8685cad66206e7752ca23f8b", "ps2-0220ad-20050620"),
    ("929a14baca1776b00869f983aa6e14d2", "ps2-0220a-20050620"),
    ("573f7d4a430c32b3cc0fd0c41e104bbd", "ps2-0220e-20050620"),
    ("df63a604e8bff5b0599bd1a6c2721bd0", "ps2-0220h-20050620"),
    ("5b1ba4bb914406fae75ab8e38901684d", "ps2-0220j-20060210"),
    ("cb801b7920a7d536ba07b6534d2433ca", "ps2-0220a-20060210"),
    ("af60e6d1a939019d55e5b330d24b1c25", "ps2-0220e-20060210"),
    ("549a66d0c698635ca9fa3ab012da7129", "ps2-0220h-20060210"),
    ("5de9d0d730ff1e7ad122806335332524", "ps2-0220j-20060905"),
    ("21fe4cad111f7dc0f9af29477057f88d", "ps2-0220ad-20060905"),
    ("40c11c063b3b9409aa5e4058e984e30c", "ps2-0220a-20060905"),
    ("80bbb237a6af9c611df43b16b930b683", "ps2-0220e-20060905"),
    ("c37bce95d32b2be480f87dd32704e664", "ps2-0220h-20060905"),
    ("80ac46fa7e77b8ab4366e86948e54f83", "ps2-0230j-20080220"),
    ("21038400dc633070a78ad53090c53017", "ps2-0230a-20080220"),
    ("dc69f0643a3030aaa4797501b483d6c4", "ps2-0230e-20080220"),
    ("30d56e79d89fbddf10938fa67fe3f34e", "ps2-0230h-20080220"),
    ("93ea3bcee4252627919175ff1b16a1d9", "ps2-0250e-20100415"),
    ("d3e81e95db25f5a86a7b7474550a2155", "ps2-0250j-20100415"),
];

/// What a system uses, if anything.
pub fn spec(system: &str) -> Option<&'static Spec> {
    SPECS.iter().find(|s| s.system == system)
}

/// Where in the core's system directory a system's BIOS files belong (`""` is the root).
pub fn core_path(system: &str) -> &'static str {
    spec(system).map_or("", |s| s.core_path)
}

/// Creates a folder for every system (the recommended layout, so it is plain where each one's
/// files can go), when the BIOS folder can be written: a read-only share is left as it is.
pub fn prepare(bios_dir: &Path) {
    for s in crate::systems::SYSTEMS {
        if let Err(e) = std::fs::create_dir_all(bios_dir.join(s.id)) {
            tracing::debug!(path = %bios_dir.display(), error = %e, "BIOS folder not writable: layout left as it is");
            return;
        }
    }
}

/// The folders of `roms` holding the system's games (any of its folder names, in any case).
pub fn game_dirs(roms: &Path, system: &crate::systems::System) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(roms) else { return Vec::new() };
    entries
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_lowercase();
            system.folders.contains(&name.as_str()) && e.path().is_dir()
        })
        .map(|e| e.path())
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileState {
    /// Where the core looks, and a known good dump.
    Ok,
    /// Found by its name, but not a dump the list knows: used all the same.
    Unverified,
    /// No longer reported: files are found wherever they are.
    Misplaced,
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Everything required and recommended is there and verified.
    Ready,
    /// Playable, but a file is unverified.
    Warning,
    /// Playable, without a recommended file.
    Incomplete,
    /// A required file is missing: games will not start.
    Missing,
}

#[derive(Debug, Clone)]
pub struct FileReport {
    /// Where it belongs, relative to the system's folder.
    pub name: String,
    pub label: String,
    pub state: FileState,
    /// Where it was found, relative to the BIOS folder (`roms/…` beside the games).
    pub found_at: Option<String>,
    pub md5: Option<String>,
    /// The good dumps' MD5s, any one of which is right (empty: any content is fine, or, for a
    /// system that takes any number of dumps, too many to list).
    pub expected_md5: Vec<String>,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct GroupReport {
    pub group: &'static Group,
    pub satisfied: bool,
    pub files: Vec<FileReport>,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub state: State,
    pub groups: Vec<GroupReport>,
}

/// Whether a required file is missing.
pub fn missing_required(bios_dir: &Path, games: impl Fn() -> Vec<PathBuf>, system: &str) -> bool {
    let Some(spec) = spec(system) else { return false };
    let required = || spec.groups.iter().filter(|g| g.importance == Importance::Required);
    if required().next().is_none() {
        return false;
    }
    let all = listing(bios_dir);
    let games = if spec.beside_games { games() } else { Vec::new() };
    required().any(|g| !choose(bios_dir, &all, spec, g, &games).iter().any(|c| c.found()))
}

fn names(k: &Known) -> impl Iterator<Item = &'static str> {
    std::iter::once(k.name).chain(k.alt.iter().copied())
}

/// What was chosen for one file a core reads, or a dump of a system that takes any number.
#[derive(Debug, Clone)]
struct Choice {
    /// Where the core reads it, in its folder (`USA/IPL.bin`), or a dump's own name.
    name: String,
    label: String,
    state: FileState,
    path: Option<PathBuf>,
    md5: Option<String>,
    expected_md5: Vec<String>,
    size: u64,
    /// Found beside the games, where the core looks itself: not linked for it.
    beside_games: bool,
}

impl Choice {
    fn found(&self) -> bool {
        matches!(self.state, FileState::Ok | FileState::Unverified)
    }
}

/// The BIOS folder's files and sizes. A listing is reused while the folder and its subfolders
/// keep their modification times (a file added or removed in one changes it) and for at most a
/// minute (deeper changes): a share is slow to walk, and the systems list asks about every
/// system at once.
fn listing(bios_dir: &Path) -> Arc<Vec<(PathBuf, u64)>> {
    type Listed = HashMap<PathBuf, (Instant, Vec<SystemTime>, Arc<Vec<(PathBuf, u64)>>)>;
    static LISTED: LazyLock<Mutex<Listed>> = LazyLock::new(Default::default);
    const AT_MOST: Duration = Duration::from_secs(60);
    let stamp = folder_times(bios_dir);
    if let Ok(listed) = LISTED.lock()
        && let Some((at, was, files)) = listed.get(bios_dir)
        && at.elapsed() < AT_MOST
        && *was == stamp
    {
        return files.clone();
    }
    let files = Arc::new(list_files(bios_dir));
    if let Ok(mut listed) = LISTED.lock() {
        listed.insert(bios_dir.to_owned(), (Instant::now(), stamp, files.clone()));
    }
    files
}

/// The modification times of `dir` and of the folders in it, in name order.
fn folder_times(dir: &Path) -> Vec<SystemTime> {
    let time = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let mut folders: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|e| e.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default();
    folders.sort();
    std::iter::once(dir.to_owned()).chain(folders).filter_map(|p| time(&p)).collect()
}

/// `path`'s place in the BIOS folder, as lower-case names (`["dc", "dc_boot.bin"]`).
fn parts(bios_dir: &Path, path: &Path) -> Vec<String> {
    path.strip_prefix(bios_dir)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .collect()
}

/// Whether `path` is named `name` (which may hold folders: `USA/IPL.bin`), any letter case.
fn named(bios_dir: &Path, path: &Path, name: &str) -> bool {
    let have = parts(bios_dir, path);
    let want: Vec<String> = name.split('/').map(str::to_lowercase).collect();
    have.len() >= want.len() && have[have.len() - want.len()..] == want[..]
}

/// How well `path` fits `system`, lowest first: in the system's own folder, then in a folder
/// named like it (any of its ROM folder names, Batocera's `dc`), then anywhere; shallower
/// first, then by path.
fn rank(bios_dir: &Path, system: &str, path: &Path) -> (u8, usize, PathBuf) {
    let p = parts(bios_dir, path);
    let folders = &p[..p.len().saturating_sub(1)];
    let aliases = crate::systems::by_id(system).map_or(&[][..], |s| s.folders);
    let place = if folders.first().is_some_and(|f| *f == system) {
        0
    } else if folders.iter().any(|f| f == system || aliases.contains(&f.as_str())) {
        1
    } else {
        2
    };
    (place, p.len(), path.to_owned())
}

/// What a group's files are found as (see the module's notes).
fn choose(
    bios_dir: &Path,
    all: &[(PathBuf, u64)],
    spec: &Spec,
    group: &Group,
    games: &[PathBuf],
) -> Vec<Choice> {
    let best = |mut found: Vec<&PathBuf>| {
        found.sort_by_key(|p| rank(bios_dir, spec.system, p));
        found.first().map(|p| (*p).to_owned())
    };
    match &group.kind {
        Kind::AnyOf(known) => known
            .iter()
            .map(|k| {
                let choice = |state, path: Option<PathBuf>, md5: Option<String>, beside| Choice {
                    name: k.name.to_owned(),
                    label: k.label.to_owned(),
                    state,
                    path,
                    md5,
                    expected_md5: k.md5.iter().map(|m| (*m).to_owned()).collect(),
                    size: k.size,
                    beside_games: beside,
                };
                let state_of = |md5: Option<&str>| {
                    if k.md5.is_empty() || md5.is_some_and(|m| k.md5.contains(&m)) {
                        FileState::Ok
                    } else {
                        FileState::Unverified
                    }
                };
                // 1. A known dump, by its checksum: anywhere when no other file of the system
                // has that checksum, else only under its own name.
                if !k.md5.is_empty() {
                    let alone = k.md5.iter().all(|m| !shared_md5(spec, k, m));
                    let dumps: Vec<&PathBuf> = all
                        .iter()
                        .filter(|(p, len)| {
                            *len == k.size
                                && (alone || names(k).any(|n| named(bios_dir, p, n)))
                                && md5_of(p).is_some_and(|m| k.md5.contains(&m.as_str()))
                        })
                        .map(|(p, _)| p)
                        .collect();
                    if let Some(p) = best(dumps) {
                        let md5 = md5_of(&p);
                        return choice(FileState::Ok, Some(p), md5, false);
                    }
                }
                // 2. Its name, anywhere in the BIOS folder.
                let by_name: Vec<&PathBuf> = all
                    .iter()
                    .filter(|(p, _)| names(k).any(|n| named(bios_dir, p, n)))
                    .map(|(p, _)| p)
                    .collect();
                if let Some(p) = best(by_name) {
                    let md5 = md5_of(&p);
                    return choice(state_of(md5.as_deref()), Some(p), md5, false);
                }
                // 3. Beside the games, where the core looks itself: its exact name there.
                if spec.beside_games
                    && let Some(p) = games
                        .iter()
                        .flat_map(|d| names(k).map(move |n| d.join(n)))
                        .find(|p| exact_file(p))
                {
                    let md5 = md5_of(&p);
                    return choice(state_of(md5.as_deref()), Some(p), md5, true);
                }
                choice(FileState::Missing, None, None, false)
            })
            .collect(),
        Kind::Dumps { size, known } => {
            let mut files: Vec<Choice> = Vec::new();
            let mut dumps: Vec<&PathBuf> =
                all.iter().filter(|(_, len)| len == size).map(|(p, _)| p).collect();
            dumps.sort_by_key(|p| rank(bios_dir, spec.system, p));
            for p in dumps {
                let Some(md5) = md5_of(p) else { continue };
                let name = known.iter().find(|(m, _)| *m == md5).map(|(_, n)| *n);
                // An unknown file of the right size counts only in a folder of the system's.
                if name.is_none() && rank(bios_dir, spec.system, p).0 == 2 {
                    continue;
                }
                let file = p.file_name().map_or_else(String::new, |n| n.to_string_lossy().into());
                if files.iter().any(|f| f.name == file || f.md5.as_deref() == Some(&md5)) {
                    continue;
                }
                files.push(Choice {
                    name: file,
                    label: name.map_or_else(|| "Unknown dump".to_owned(), ps2_label),
                    state: if name.is_some() { FileState::Ok } else { FileState::Unverified },
                    path: Some(p.to_owned()),
                    md5: Some(md5),
                    expected_md5: Vec::new(),
                    size: *size,
                    beside_games: false,
                });
            }
            if files.is_empty() {
                files.push(Choice {
                    name: "*.bin".to_owned(),
                    label: "A BIOS dump".to_owned(),
                    state: FileState::Missing,
                    path: None,
                    md5: None,
                    expected_md5: Vec::new(),
                    size: *size,
                    beside_games: false,
                });
            }
            files
        }
    }
}

/// Whether another file of `spec` than `k` also has `md5` as a good dump.
fn shared_md5(spec: &Spec, k: &Known, md5: &str) -> bool {
    spec.groups.iter().any(|g| match &g.kind {
        Kind::AnyOf(files) => files.iter().any(|o| !std::ptr::eq(o, k) && o.md5.contains(&md5)),
        Kind::Dumps { .. } => false,
    })
}

/// Checks a system's BIOS files: `games` are its ROM folders, searched when its core looks
/// there too. `None` for a system that uses none.
pub fn check(bios_dir: &Path, games: &[PathBuf], system: &str) -> Option<Report> {
    let spec = spec(system)?;
    let all = listing(bios_dir);
    let rel = |p: &Path| -> String {
        if let Ok(r) = p.strip_prefix(bios_dir) {
            return slash(r);
        }
        games
            .iter()
            .find_map(|g| {
                let r = p.strip_prefix(g).ok()?;
                let dir = g.file_name()?.to_string_lossy();
                Some(format!("roms/{dir}/{}", slash(r)))
            })
            .unwrap_or_else(|| p.display().to_string())
    };
    let mut groups = Vec::new();
    for group in spec.groups {
        let files: Vec<FileReport> = choose(bios_dir, &all, spec, group, games)
            .into_iter()
            .map(|c| FileReport {
                found_at: c.path.as_deref().map(&rel),
                name: c.name,
                label: c.label,
                state: c.state,
                md5: c.md5,
                expected_md5: c.expected_md5,
                size: c.size,
            })
            .collect();
        let satisfied =
            files.iter().any(|f| matches!(f.state, FileState::Ok | FileState::Unverified));
        groups.push(GroupReport { group, satisfied, files });
    }
    let unmet = |i: Importance| groups.iter().any(|g| g.group.importance == i && !g.satisfied);
    let doubtful = groups.iter().flat_map(|g| &g.files).any(|f| f.state == FileState::Unverified);
    let state = if unmet(Importance::Required) {
        State::Missing
    } else if unmet(Importance::Recommended) {
        State::Incomplete
    } else if doubtful {
        State::Warning
    } else {
        State::Ready
    };
    Some(Report { state, groups })
}

/// Fills `dest` (a new, empty folder) with links to the BIOS files a session of `system` uses,
/// each under the name and in the place its core reads it (`USA/IPL.bin`), wherever it is in
/// `bios_dir`. Files the core finds beside the games are left to it. Returns how many.
pub fn link_session(bios_dir: &Path, system: &str, dest: &Path) -> std::io::Result<usize> {
    std::fs::create_dir_all(dest)?;
    let Some(spec) = spec(system) else { return Ok(0) };
    let all = listing(bios_dir);
    let mut linked = 0;
    for group in spec.groups {
        for c in choose(bios_dir, &all, spec, group, &[]) {
            if !c.found() || c.beside_games {
                continue;
            }
            let Some(source) = c.path else { continue };
            let link = dest.join(&c.name);
            if link.symlink_metadata().is_ok() {
                continue;
            }
            if let Some(parent) = link.parent() {
                std::fs::create_dir_all(parent)?;
            }
            symlink(&source, &link)?;
            linked += 1;
        }
    }
    Ok(linked)
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(not(unix))]
fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::fs::copy(target, link).map(drop)
}

/// `ps2-0230a-20080220` → "v2.30 USA (2008-02-20)".
fn ps2_label(name: &str) -> String {
    let mut parts = name.split('-').skip(1);
    let (Some(ver), Some(date)) = (parts.next(), parts.next()) else { return name.to_owned() };
    if ver.len() < 5 || date.len() != 8 || !ver.is_char_boundary(4) {
        return name.to_owned();
    }
    let (digits, kind) = ver.split_at(4);
    let region = match kind.chars().next() {
        Some('j') => "Japan",
        Some('a') => "USA",
        Some('e') => "Europe",
        Some('h') => "Asia",
        Some('c') => "China",
        Some('r') => "Russia",
        _ => "",
    };
    let dev = if kind.ends_with('d') { " (development)" } else { "" };
    format!(
        "v{}.{} {region}{dev} ({}-{}-{})",
        digits[..2].trim_start_matches('0'),
        &digits[2..],
        &date[..4],
        &date[4..6],
        &date[6..]
    )
    .replace("  ", " ")
}

/// A file at exactly this path, letter case included: the cores run on Linux, where
/// `SCPH5501.BIN` is not `scph5501.bin`, even when this server's disk ignores case (macOS).
fn exact_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else { return false };
    std::fs::read_dir(dir).is_ok_and(|mut e| e.any(|e| e.is_ok_and(|e| e.file_name() == name)))
}

fn slash(p: &Path) -> String {
    p.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
}

/// The files under `dir` with their sizes, a few levels deep and at most a few thousand.
fn list_files(dir: &Path) -> Vec<(PathBuf, u64)> {
    const MAX_DEPTH: usize = 6;
    const MAX_FILES: usize = 5000;
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_owned(), 0)];
    while let Some((d, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let Ok(meta) = std::fs::metadata(e.path()) else { continue };
            if meta.is_dir() {
                if depth < MAX_DEPTH {
                    stack.push((e.path(), depth + 1));
                }
            } else if meta.is_file() {
                out.push((e.path(), meta.len()));
                if out.len() >= MAX_FILES {
                    return out;
                }
            }
        }
    }
    out.sort();
    out
}

/// A file's MD5, remembered while its size and modification time stay the same.
fn md5_of(path: &Path) -> Option<String> {
    /// Path → (size, modification time, MD5).
    type Seen = HashMap<PathBuf, (u64, SystemTime, String)>;
    static SEEN: LazyLock<Mutex<Seen>> = LazyLock::new(Default::default);
    let meta = std::fs::metadata(path).ok()?;
    let key = (meta.len(), meta.modified().ok()?);
    if let Some((len, time, md5)) = SEEN.lock().ok()?.get(path)
        && (*len, *time) == key
    {
        return Some(md5.clone());
    }
    // BIOS files are a few megabytes at most.
    let md5 = hex::encode(Md5::digest(std::fs::read(path).ok()?));
    SEEN.lock().ok()?.insert(path.to_owned(), (key.0, key.1, md5.clone()));
    Some(md5)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sovemu-bios-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Content of `size` bytes whose MD5 is unknown.
    fn junk(size: u64) -> Vec<u8> {
        vec![0x5a; size as usize]
    }

    fn file<'a>(r: &'a Report, name: &str) -> &'a FileReport {
        r.groups.iter().flat_map(|g| &g.files).find(|f| f.name == name).unwrap()
    }

    #[test]
    fn the_table_is_sound() {
        for s in SPECS {
            let system = crate::systems::by_id(s.system).unwrap_or_else(|| panic!("{}", s.system));
            assert!(!s.groups.is_empty(), "{}", system.id);
            assert!(SPECS.iter().filter(|o| o.system == s.system).count() == 1, "{}", s.system);
            for g in s.groups {
                if let Kind::AnyOf(files) = &g.kind {
                    for k in *files {
                        for m in k.md5 {
                            assert!(
                                m.len() == 32 && m.bytes().all(|b| b.is_ascii_hexdigit()),
                                "{m}"
                            );
                            assert_eq!(m.to_lowercase(), *m);
                        }
                    }
                }
            }
        }
        assert_eq!(PS2_DUMPS.len(), 73);
        assert_eq!(core_path("ps2"), "pcsx2/bios");
        assert_eq!(core_path("dreamcast"), "dc");
        assert_eq!(core_path("snes"), "");
    }

    #[test]
    fn known_dumps_are_verified_and_others_flagged() {
        let bios = tmp("verify");
        std::fs::create_dir_all(bios.join("gba")).unwrap();
        std::fs::write(bios.join("gba/gba_bios.bin"), junk(16384)).unwrap();
        let r = check(&bios, &[], "gba").unwrap();
        assert_eq!(file(&r, "gba_bios.bin").state, FileState::Unverified);
        assert_eq!(r.state, State::Warning, "present but not a known dump");
        assert!(check(&bios, &[], "snes").is_none(), "SNES uses no BIOS");

        // Without its boot ROM, Dreamcast plays on Flycast's built-in BIOS.
        let r = check(&bios, &[], "dreamcast").unwrap();
        assert_eq!(r.state, State::Incomplete);
        assert_eq!(file(&r, "dc_boot.bin").state, FileState::Missing);
        std::fs::remove_dir_all(bios).ok();
    }

    #[test]
    fn required_files_decide_the_state() {
        let bios = tmp("required");
        let roms = tmp("required-roms");
        std::fs::create_dir_all(bios.join("neogeo")).unwrap();
        std::fs::create_dir_all(roms.join("NeoGeo")).unwrap();
        let games = || game_dirs(&roms, crate::systems::by_id("neogeo").unwrap());
        assert!(missing_required(&bios, games, "neogeo"));
        assert_eq!(check(&bios, &games(), "neogeo").unwrap().state, State::Missing);
        assert!(!missing_required(&bios, games, "snes"));
        assert!(!missing_required(&bios, games, "fbneo"), "optional for arcade");

        // FBNeo finds it beside the games too.
        std::fs::write(roms.join("NeoGeo/neogeo.zip"), b"zip").unwrap();
        assert!(!missing_required(&bios, games, "neogeo"));
        let r = check(&bios, &games(), "neogeo").unwrap();
        assert_eq!(file(&r, "neogeo.zip").found_at.as_deref(), Some("roms/NeoGeo/neogeo.zip"));
        assert_eq!(r.state, State::Warning, "another version of the set");
        std::fs::remove_dir_all(bios).ok();
        std::fs::remove_dir_all(roms).ok();
    }

    #[test]
    fn a_file_is_found_by_its_name_in_any_layout_and_any_case() {
        let bios = tmp("any-case");
        std::fs::create_dir_all(bios.join("psx")).unwrap();
        std::fs::write(bios.join("psx/SCPH5501.BIN"), junk(PSX)).unwrap();
        let r = check(&bios, &[], "psx").unwrap();
        let f = file(&r, "scph5501.bin");
        assert_eq!(f.state, FileState::Unverified, "found by name, not a known dump");
        assert_eq!(f.found_at.as_deref(), Some("psx/SCPH5501.BIN"));
        assert_eq!(r.state, State::Warning, "used, but worth a look");
        std::fs::remove_dir_all(&bios).ok();

        // Batocera's layout: the Dreamcast's in `dc/`, beside another copy deeper down.
        let bios = tmp("batocera");
        std::fs::create_dir_all(bios.join("dc")).unwrap();
        std::fs::create_dir_all(bios.join("old/stuff")).unwrap();
        std::fs::write(bios.join("dc/dc_boot.bin"), junk(2 << 20)).unwrap();
        std::fs::write(bios.join("old/stuff/dc_boot.bin"), junk(2 << 20)).unwrap();
        let r = check(&bios, &[], "dreamcast").unwrap();
        assert_eq!(
            file(&r, "dc_boot.bin").found_at.as_deref(),
            Some("dc/dc_boot.bin"),
            "a folder named like the system wins"
        );
        assert!(!missing_required(&bios, Vec::new, "dreamcast"));
        std::fs::remove_dir_all(bios).ok();
    }

    const fn known(name: &'static str, md5: &'static [&'static str]) -> Known {
        one(name, "Test", 4, md5)
    }

    // MD5 of b"bios".
    const BIOS_MD5: &str = "88264747405203a0502c8d242fdad7df";

    fn test_spec(files: &'static [Known]) -> Spec {
        Spec {
            system: "test",
            core_path: "",
            beside_games: false,
            groups: Box::leak(Box::new([Group {
                label: "Test",
                importance: Importance::Required,
                help: "",
                kind: Kind::AnyOf(files),
            }])),
        }
    }

    #[test]
    fn a_known_dump_is_found_under_any_name() {
        assert_eq!(md5_of_bytes(b"bios"), BIOS_MD5);
        let bios = tmp("by-md5");
        std::fs::create_dir_all(bios.join("misc")).unwrap();
        std::fs::write(bios.join("misc/renamed.rom"), b"bios").unwrap();
        std::fs::write(bios.join("expected.bin"), b"nope").unwrap();
        static FILES: [Known; 1] = [known("expected.bin", &[BIOS_MD5])];
        let spec = test_spec(&FILES);
        let all = list_files(&bios);
        let c = &choose(&bios, &all, &spec, &spec.groups[0], &[])[0];
        assert_eq!(c.state, FileState::Ok, "the checksum beats the name");
        assert_eq!(c.path.as_deref(), Some(bios.join("misc/renamed.rom").as_path()));
        std::fs::remove_dir_all(bios).ok();
    }

    #[test]
    fn a_checksum_two_files_share_also_needs_the_name() {
        let bios = tmp("shared-md5");
        std::fs::create_dir_all(bios.join("gc/USA")).unwrap();
        std::fs::write(bios.join("gc/USA/IPL.bin"), b"bios").unwrap();
        static FILES: [Known; 2] =
            [known("USA/IPL.bin", &[BIOS_MD5]), known("JAP/IPL.bin", &[BIOS_MD5])];
        let spec = test_spec(&FILES);
        let all = list_files(&bios);
        let picked: Vec<FileState> =
            choose(&bios, &all, &spec, &spec.groups[0], &[]).iter().map(|c| c.state).collect();
        assert_eq!(picked, [FileState::Ok, FileState::Missing], "the USA IPL is not the JAP one");
        std::fs::remove_dir_all(bios).ok();
    }

    #[test]
    fn a_session_gets_each_file_where_its_core_reads_it() {
        let bios = tmp("link");
        std::fs::create_dir_all(bios.join("gamecube/usa")).unwrap();
        std::fs::write(bios.join("gamecube/usa/ipl.BIN"), junk(2 << 20)).unwrap();
        std::fs::write(bios.join("SCPH5501.BIN"), junk(PSX)).unwrap();
        let dest = bios.join("../link-session-out");
        let _ = std::fs::remove_dir_all(&dest);
        assert_eq!(link_session(&bios, "gamecube", &dest.join("gc")).unwrap(), 1);
        let linked = std::fs::read_link(dest.join("gc/USA/IPL.bin")).unwrap();
        assert_eq!(linked, bios.join("gamecube/usa/ipl.BIN"), "named as the core reads it");
        assert_eq!(link_session(&bios, "psx", &dest.join("psx")).unwrap(), 1);
        assert!(dest.join("psx/scph5501.bin").exists());
        assert_eq!(link_session(&bios, "nes", &dest.join("nes")).unwrap(), 0, "nothing there");
        assert_eq!(
            std::fs::read(bios.join("SCPH5501.BIN")).unwrap().len() as u64,
            PSX,
            "untouched"
        );
        std::fs::remove_dir_all(dest).ok();
        std::fs::remove_dir_all(bios).ok();
    }

    #[test]
    fn ps2_dumps_under_any_name() {
        let bios = tmp("ps2");
        std::fs::create_dir_all(bios.join("ps2")).unwrap();
        assert!(missing_required(&bios, Vec::new, "ps2"));
        let r = check(&bios, &[], "ps2").unwrap();
        assert_eq!(r.state, State::Missing);
        assert_eq!(r.groups[0].files[0].state, FileState::Missing);

        std::fs::write(bios.join("ps2/SCPH-70012.bin"), junk(4 * 1024 * 1024)).unwrap();
        std::fs::write(bios.join("ps2/SCPH-70012.nvm"), b"nvm").unwrap();
        assert!(!missing_required(&bios, Vec::new, "ps2"));
        let r = check(&bios, &[], "ps2").unwrap();
        assert_eq!(r.groups[0].files.len(), 1, "only BIOS-sized files");
        assert_eq!(r.groups[0].files[0].state, FileState::Unverified);
        assert_eq!(r.state, State::Warning);
        std::fs::remove_dir_all(bios).ok();
    }

    #[test]
    fn ps2_labels() {
        assert_eq!(ps2_label("ps2-0230a-20080220"), "v2.30 USA (2008-02-20)");
        assert_eq!(ps2_label("ps2-0120ed-20000902"), "v1.20 Europe (development) (2000-09-02)");
        assert_eq!(ps2_label("ps2-0120j-20001027-185015"), "v1.20 Japan (2000-10-27)");
        assert_eq!(ps2_label("odd"), "odd");
    }

    fn md5_of_bytes(bytes: &[u8]) -> String {
        hex::encode(Md5::digest(bytes))
    }

    #[test]
    fn checksums_are_cached_until_the_file_changes() {
        let dir = tmp("md5");
        let p = dir.join("f");
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(md5_of(&p).as_deref(), Some("900150983cd24fb0d6963f7d28e17f72"));
        std::fs::write(&p, b"abcd").unwrap();
        assert_eq!(md5_of(&p).as_deref(), Some("e2fc714c4727ee9395f324cd2e7f331f"));
        std::fs::remove_dir_all(dir).ok();
    }
}
