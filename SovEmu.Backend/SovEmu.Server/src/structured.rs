//! Structured settings from the environment, in the `__` hierarchy Sovereign uses for its own
//! (`Sovereign__DataSources__Radarr__0__Url`): lists and maps that a flat variable cannot hold.
//!
//! - Users: `SOVEMU_USERS__<n>__USERNAME`, `__PASSWORD` (or `__PASSWORD_FILE`), `__ROLE`
//!   (`viewer`, the default, or `admin`; `__ADMIN=true` is the older spelling of `admin`).
//! - Systems: `SOVEMU_SYSTEMS__<id>__CORE` (the core to use), `__ENABLED` (false hides it),
//!   `__DECORATION` (the decoration set, or `none`), and `__OPTIONS__<core option>` (libretro core
//!   options, over SovEmu's own).
//! - Controller defaults: `SOVEMU_CONTROLLERS__<n>__ID` (a controller ID from
//!   docs/CONTROLLERS.md), `__BINDINGS` (`south:B,east:A,…`), and optionally `__SYSTEM` (a
//!   system id: the default for that system only) and `__NAME`.
//!
//! - Shared collections: `SOVEMU_COLLECTIONS__<n>__NAME` and `__GAMES__<m>`, each game as
//!   `<system id>/<file name>` (`nes/Super Stand-in (USA).nes`, wherever in the system's folders
//!   it lies).
//!
//! Keys after the prefix are case-insensitive except core option names and system ids, which are
//! passed through as written (lower-cased ids, as option names are).

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};

use sovemu_identity::Role;

/// A user declared in the environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserSpec {
    pub username: String,
    pub password: String,
    pub(crate) role: Role,
}

/// Per-system overrides.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SystemSpec {
    /// A core name (`snes9x`, …) to use instead of SovEmu's choice.
    pub core: Option<String>,
    pub enabled: Option<bool>,
    /// Core options, merged over SovEmu's defaults for the system.
    pub options: BTreeMap<String, String>,
    /// The decoration set its games are framed with (`none`: no frame).
    pub decoration: Option<String>,
    /// Frames its games' pictures run ahead, hiding their own input lag (0: off).
    pub run_ahead: Option<u32>,
}

/// Groups `PREFIX__<key>__<rest>=value` into key → (rest → value).
fn group(vars: &[(String, String)], prefix: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut out: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for (name, value) in vars {
        let Some(rest) = name.strip_prefix(prefix) else { continue };
        let Some((key, field)) = rest.split_once("__") else { continue };
        if key.is_empty() || field.is_empty() {
            continue;
        }
        out.entry(key.to_owned()).or_default().insert(field.to_owned(), value.clone());
    }
    out
}

fn parse_bool(var: &str, value: &str) -> Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        other => bail!("{var}: expected true or false, got {other:?}"),
    }
}

/// Users from `SOVEMU_USERS__<n>__…`, ordered by index. `read_file` reads `__PASSWORD_FILE`.
pub(crate) fn users(
    vars: &[(String, String)],
    read_file: impl Fn(&str) -> Result<String>,
) -> Result<Vec<UserSpec>> {
    let mut indexed: Vec<(u32, BTreeMap<String, String>)> = Vec::new();
    for (key, fields) in group(vars, "SOVEMU_USERS__") {
        let n: u32 = key
            .parse()
            .with_context(|| format!("SOVEMU_USERS__{key}: the index must be a number"))?;
        let fields = fields.into_iter().map(|(k, v)| (k.to_ascii_uppercase(), v)).collect();
        indexed.push((n, fields));
    }
    indexed.sort_by_key(|(n, _)| *n);
    indexed
        .into_iter()
        .map(|(n, f)| {
            let var = |field: &str| format!("SOVEMU_USERS__{n}__{field}");
            let username = f
                .get("USERNAME")
                .filter(|u| !u.is_empty())
                .with_context(|| format!("{} is required", var("USERNAME")))?;
            let password = match (
                f.get("PASSWORD").filter(|p| !p.is_empty()),
                f.get("PASSWORD_FILE").filter(|p| !p.is_empty()),
            ) {
                (Some(p), _) => p.clone(),
                (None, Some(path)) => {
                    read_file(path).with_context(|| format!("reading {}", var("PASSWORD_FILE")))?
                }
                (None, None) => {
                    bail!("{} or {} is required", var("PASSWORD"), var("PASSWORD_FILE"))
                }
            };
            let role = f
                .get("ROLE")
                .filter(|r| !r.is_empty())
                .map(|r| r.parse::<Role>().map_err(|e| anyhow::anyhow!("{}: {e}", var("ROLE"))))
                .transpose()?;
            let admin = f.get("ADMIN").map(|v| parse_bool(&var("ADMIN"), v)).transpose()?;
            let role = match (role, admin) {
                (Some(role), Some(admin)) if admin != (role == Role::Admin) => {
                    bail!("{} and {} disagree: set only {}", var("ROLE"), var("ADMIN"), var("ROLE"))
                }
                (Some(role), _) => role,
                (None, Some(true)) => Role::Admin,
                (None, _) => Role::Viewer,
            };
            Ok(UserSpec { username: username.clone(), password, role })
        })
        .collect()
}

/// System overrides from `SOVEMU_SYSTEMS__<id>__…`, keyed by lower-case system id.
pub(crate) fn systems(vars: &[(String, String)]) -> Result<BTreeMap<String, SystemSpec>> {
    let mut out: BTreeMap<String, SystemSpec> = BTreeMap::new();
    for (id, fields) in group(vars, "SOVEMU_SYSTEMS__") {
        let id = id.to_ascii_lowercase();
        if sovemu_library::systems::by_id(&id).is_none() {
            bail!(
                "SOVEMU_SYSTEMS__{id}: unknown system (known: {})",
                sovemu_library::systems::SYSTEMS
                    .iter()
                    .map(|s| s.id)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        // `psx` and `PSX` name the same system: merge their settings.
        let spec = out.entry(id.clone()).or_default();
        for (field, value) in fields {
            let var = format!("SOVEMU_SYSTEMS__{id}__{field}");
            if let Some(option) =
                field.strip_prefix("OPTIONS__").or_else(|| field.strip_prefix("options__"))
            {
                spec.options.insert(option.to_owned(), value);
            } else {
                match field.to_ascii_uppercase().as_str() {
                    "CORE" => spec.core = Some(value.trim().to_owned()).filter(|c| !c.is_empty()),
                    "ENABLED" => spec.enabled = Some(parse_bool(&var, &value)?),
                    "DECORATION" => {
                        spec.decoration = Some(value.trim().to_owned()).filter(|d| !d.is_empty())
                    }
                    "RUN_AHEAD" => spec.run_ahead = Some(parse_run_ahead(&var, &id, &value)?),
                    other => bail!(
                        "{var}: unknown setting {other:?} (CORE, ENABLED, DECORATION, RUN_AHEAD or OPTIONS__<name>)"
                    ),
                }
            }
        }
    }
    Ok(out)
}

/// Frames of run-ahead for `system`: 0 to [`MAX_RUN_AHEAD`], and only where it can keep up.
fn parse_run_ahead(var: &str, system: &str, value: &str) -> Result<u32> {
    use sovemu_protocol::ipc::MAX_RUN_AHEAD;
    let frames: u32 = value
        .trim()
        .parse()
        .with_context(|| format!("{var}: a number of frames, 0 (off) to {MAX_RUN_AHEAD}"))?;
    if frames > MAX_RUN_AHEAD {
        bail!("{var}: at most {MAX_RUN_AHEAD} frames (got {frames})");
    }
    let supported = sovemu_library::systems::by_id(system).is_some_and(|s| s.run_ahead());
    if frames > 0 && !supported {
        bail!("{var}: run-ahead is for the 2D systems; {system} is too costly to run ahead");
    }
    Ok(frames)
}

/// Default controller mappings from `SOVEMU_CONTROLLERS__<n>__…`, ordered by index. A list
/// rather than a map by ID, since IDs such as `pad:2dc8:6101` cannot be part of a variable name.
pub(crate) fn controllers(
    vars: &[(String, String)],
) -> Result<Vec<crate::controllers::ControllerSpec>> {
    let mut indexed: Vec<(u32, crate::controllers::ControllerSpec)> = Vec::new();
    for (key, fields) in group(vars, "SOVEMU_CONTROLLERS__") {
        let n: u32 = key
            .parse()
            .with_context(|| format!("SOVEMU_CONTROLLERS__{key}: the index must be a number"))?;
        let var = |field: &str| format!("SOVEMU_CONTROLLERS__{n}__{field}");
        let mut spec = crate::controllers::ControllerSpec {
            controller: String::new(),
            system: String::new(),
            name: String::new(),
            bindings: Vec::new(),
        };
        let mut has_bindings = false;
        for (field, value) in fields {
            match field.to_ascii_uppercase().as_str() {
                "ID" => spec.controller = value.trim().to_owned(),
                "NAME" => spec.name = value.trim().to_owned(),
                "SYSTEM" => {
                    spec.system = crate::controllers::system_key(Some(&value))
                        .map_err(|e| anyhow::anyhow!("{}: {e}", var("SYSTEM")))?;
                }
                "BINDINGS" => {
                    spec.bindings = crate::controllers::parse_bindings(&value)
                        .map_err(|e| anyhow::anyhow!("{}: {e}", var("BINDINGS")))?;
                    has_bindings = true;
                }
                other => bail!(
                    "{}: unknown setting {other:?} (ID, SYSTEM, NAME or BINDINGS)",
                    var(other)
                ),
            }
        }
        if spec.controller.is_empty() || !has_bindings {
            bail!("SOVEMU_CONTROLLERS__{n}: needs both __ID and __BINDINGS");
        }
        crate::controllers::check(&spec.controller, &spec.name, &spec.bindings)
            .map_err(|e| anyhow::anyhow!("SOVEMU_CONTROLLERS__{n}: {e}"))?;
        if let Some((m, _)) =
            indexed.iter().find(|(_, s)| s.controller == spec.controller && s.system == spec.system)
        {
            let on =
                if spec.system.is_empty() { String::new() } else { format!(" on {}", spec.system) };
            bail!(
                "SOVEMU_CONTROLLERS__{n}: {}{on} is already set by SOVEMU_CONTROLLERS__{m}",
                spec.controller
            );
        }
        indexed.push((n, spec));
    }
    indexed.sort_by_key(|(n, _)| *n);
    Ok(indexed.into_iter().map(|(_, spec)| spec).collect())
}

/// A shared collection declared in the environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionSpec {
    pub name: String,
    /// Its games, in order, as (system id, file name).
    pub games: Vec<(String, String)>,
}

/// Shared collections from `SOVEMU_COLLECTIONS__<n>__…`, ordered by index.
pub(crate) fn collections(vars: &[(String, String)]) -> Result<Vec<CollectionSpec>> {
    let mut indexed: Vec<(u32, CollectionSpec)> = Vec::new();
    for (key, fields) in group(vars, "SOVEMU_COLLECTIONS__") {
        let n: u32 = key
            .parse()
            .with_context(|| format!("SOVEMU_COLLECTIONS__{key}: the index must be a number"))?;
        let var = |field: &str| format!("SOVEMU_COLLECTIONS__{n}__{field}");
        let mut name = String::new();
        let mut games: Vec<(u32, (String, String))> = Vec::new();
        for (field, value) in fields {
            let upper = field.to_ascii_uppercase();
            if upper == "NAME" {
                name = value.split_whitespace().collect::<Vec<_>>().join(" ");
            } else if let Some(m) = upper.strip_prefix("GAMES__") {
                let m: u32 = m
                    .parse()
                    .with_context(|| format!("{}: the index must be a number", var(&field)))?;
                games.push((m, game_ref(&var(&field), &value)?));
            } else {
                bail!("{}: unknown setting {upper:?} (NAME or GAMES__<m>)", var(&field));
            }
        }
        if name.is_empty() {
            bail!("{} is required", var("NAME"));
        }
        if let Some((m, _)) = indexed.iter().find(|(_, c)| c.name == name) {
            bail!(
                "SOVEMU_COLLECTIONS__{n}: {name:?} is already declared by SOVEMU_COLLECTIONS__{m}"
            );
        }
        games.sort_by_key(|(m, _)| *m);
        indexed
            .push((n, CollectionSpec { name, games: games.into_iter().map(|(_, g)| g).collect() }));
    }
    indexed.sort_by_key(|(n, _)| *n);
    Ok(indexed.into_iter().map(|(_, c)| c).collect())
}

/// `<system id>/<file name>`.
fn game_ref(var: &str, value: &str) -> Result<(String, String)> {
    let (system, file) = value
        .trim()
        .split_once('/')
        .filter(|(s, f)| !s.is_empty() && !f.is_empty() && !f.contains('/'))
        .with_context(|| format!("{var}: a game as <system id>/<file name>, got {value:?}"))?;
    let system = system.to_ascii_lowercase();
    if sovemu_library::systems::by_id(&system).is_none() {
        bail!("{var}: unknown system {system:?}");
    }
    Ok((system, file.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
    }

    #[test]
    fn users_by_index_with_passwords_from_files() {
        let v = vars(&[
            ("SOVEMU_USERS__1__USERNAME", "kid"),
            ("SOVEMU_USERS__1__PASSWORD_FILE", "/run/secrets/kid"),
            ("SOVEMU_USERS__0__USERNAME", "mum"),
            ("SOVEMU_USERS__0__PASSWORD", "password123"),
            ("SOVEMU_USERS__0__ROLE", "Admin"),
            ("SOVEMU_USERS__2__USERNAME", "dad"),
            ("SOVEMU_USERS__2__PASSWORD", "password456"),
            ("SOVEMU_USERS__2__ADMIN", "true"),
            ("SOVEMU_OTHER", "x"),
        ]);
        let users = users(&v, |path| Ok(format!("from {path}"))).unwrap();
        assert_eq!(
            users,
            [
                UserSpec {
                    username: "mum".into(),
                    password: "password123".into(),
                    role: Role::Admin
                },
                UserSpec {
                    username: "kid".into(),
                    password: "from /run/secrets/kid".into(),
                    role: Role::Viewer
                },
                UserSpec {
                    username: "dad".into(),
                    password: "password456".into(),
                    role: Role::Admin
                },
            ]
        );
    }

    #[test]
    fn user_errors_name_the_variable() {
        let missing =
            users(&vars(&[("SOVEMU_USERS__0__USERNAME", "a")]), |_| Ok(String::new())).unwrap_err();
        assert!(format!("{missing:#}").contains("SOVEMU_USERS__0__PASSWORD"));
        let bad_index =
            users(&vars(&[("SOVEMU_USERS__x__USERNAME", "a")]), |_| Ok(String::new())).unwrap_err();
        assert!(format!("{bad_index:#}").contains("index"));
        let bad_bool = users(
            &vars(&[
                ("SOVEMU_USERS__0__USERNAME", "a"),
                ("SOVEMU_USERS__0__PASSWORD", "p"),
                ("SOVEMU_USERS__0__ADMIN", "maybe"),
            ]),
            |_| Ok(String::new()),
        )
        .unwrap_err();
        assert!(format!("{bad_bool:#}").contains("ADMIN"));
        let user = |extra: &[(&str, &str)]| {
            let mut v =
                vars(&[("SOVEMU_USERS__0__USERNAME", "a"), ("SOVEMU_USERS__0__PASSWORD", "p")]);
            v.extend(vars(extra));
            users(&v, |_| Ok(String::new()))
        };
        let bad_role = user(&[("SOVEMU_USERS__0__ROLE", "owner")]).unwrap_err();
        assert!(format!("{bad_role:#}").contains("SOVEMU_USERS__0__ROLE"));
        let disagree =
            user(&[("SOVEMU_USERS__0__ROLE", "viewer"), ("SOVEMU_USERS__0__ADMIN", "true")])
                .unwrap_err();
        assert!(format!("{disagree:#}").contains("disagree"));
    }

    #[test]
    fn system_overrides() {
        let v = vars(&[
            ("SOVEMU_SYSTEMS__psx__OPTIONS__pcsx_rearmed_cd_readahead", "512"),
            ("SOVEMU_SYSTEMS__PSX__CORE", "mednafen_psx"),
            ("SOVEMU_SYSTEMS__gamegear__ENABLED", "false"),
        ]);
        let s = systems(&v).unwrap();
        assert_eq!(s["psx"].core.as_deref(), Some("mednafen_psx"));
        assert_eq!(s["psx"].options["pcsx_rearmed_cd_readahead"], "512");
        assert_eq!(s["gamegear"].enabled, Some(false));
        assert!(
            systems(&vars(&[("SOVEMU_SYSTEMS__saturn__CORE", "x")])).is_err(),
            "unknown system"
        );
        assert!(
            systems(&vars(&[("SOVEMU_SYSTEMS__nes__COLOUR", "red")])).is_err(),
            "unknown setting"
        );
    }

    #[test]
    fn run_ahead_is_a_few_frames_on_a_2d_system() {
        let s = systems(&vars(&[("SOVEMU_SYSTEMS__snes__RUN_AHEAD", "2")])).unwrap();
        assert_eq!(s["snes"].run_ahead, Some(2));
        let s = systems(&vars(&[("SOVEMU_SYSTEMS__n64__RUN_AHEAD", "0")])).unwrap();
        assert_eq!(s["n64"].run_ahead, Some(0), "off is fine anywhere");
        for (var, value, why) in [
            ("SOVEMU_SYSTEMS__nes__RUN_AHEAD", "5", "more than the maximum"),
            ("SOVEMU_SYSTEMS__nes__RUN_AHEAD", "on", "not a number"),
            ("SOVEMU_SYSTEMS__n64__RUN_AHEAD", "1", "a 3D system"),
        ] {
            assert!(systems(&vars(&[(var, value)])).is_err(), "{why}");
        }
    }

    #[test]
    fn reads_controller_defaults() {
        use crate::controllers::{Binding, PadButton};
        let specs = controllers(&vars(&[
            ("SOVEMU_CONTROLLERS__1__ID", "keyboard"),
            ("SOVEMU_CONTROLLERS__1__BINDINGS", "KeyJ:B"),
            ("SOVEMU_CONTROLLERS__0__ID", "pad:2dc8:6101"),
            ("SOVEMU_CONTROLLERS__0__name", "8BitDo SN30 Pro"),
            ("SOVEMU_CONTROLLERS__0__BINDINGS", "south:A, east:B"),
        ]))
        .unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].controller, "pad:2dc8:6101");
        assert_eq!(specs[0].name, "8BitDo SN30 Pro");
        assert_eq!(specs[1].bindings, vec![Binding { input: "KeyJ".into(), button: PadButton::B }]);
        assert_eq!(specs[0].system, "");

        let per_system = controllers(&vars(&[
            ("SOVEMU_CONTROLLERS__0__ID", "xbox"),
            ("SOVEMU_CONTROLLERS__0__BINDINGS", "south:A"),
            ("SOVEMU_CONTROLLERS__1__ID", "xbox"),
            ("SOVEMU_CONTROLLERS__1__SYSTEM", "megadrive"),
            ("SOVEMU_CONTROLLERS__1__BINDINGS", "south:B"),
        ]))
        .unwrap();
        assert_eq!(
            per_system[1].system, "megadrive",
            "the same model on one system is not a repeat"
        );

        let fails = |v: &[(&str, &str)]| format!("{:#}", controllers(&vars(v)).unwrap_err());
        assert!(fails(&[("SOVEMU_CONTROLLERS__0__ID", "xbox")]).contains("__BINDINGS"));
        let unknown_system = [
            ("SOVEMU_CONTROLLERS__0__ID", "xbox"),
            ("SOVEMU_CONTROLLERS__0__SYSTEM", "atari"),
            ("SOVEMU_CONTROLLERS__0__BINDINGS", ""),
        ];
        assert!(fails(&unknown_system).contains("not a system"));
        let key_on_pad =
            [("SOVEMU_CONTROLLERS__0__ID", "xbox"), ("SOVEMU_CONTROLLERS__0__BINDINGS", "KeyZ:B")];
        assert!(fails(&key_on_pad).contains("KeyZ"));
        let bad_button =
            [("SOVEMU_CONTROLLERS__0__ID", "xbox"), ("SOVEMU_CONTROLLERS__0__BINDINGS", "south:Q")];
        assert!(fails(&bad_button).contains("RetroPad"));
        assert!(
            fails(&[
                ("SOVEMU_CONTROLLERS__0__ID", "xbox"),
                ("SOVEMU_CONTROLLERS__0__BINDINGS", ""),
                ("SOVEMU_CONTROLLERS__1__ID", "xbox"),
                ("SOVEMU_CONTROLLERS__1__BINDINGS", ""),
            ])
            .contains("already set")
        );
    }

    #[test]
    fn collections_are_read_in_order_with_their_games() {
        let got = collections(&vars(&[
            ("SOVEMU_COLLECTIONS__1__NAME", "Party  games"),
            ("SOVEMU_COLLECTIONS__1__GAMES__1", "snes/B (USA).sfc"),
            ("SOVEMU_COLLECTIONS__1__GAMES__0", "NES/A (USA).nes"),
            ("SOVEMU_COLLECTIONS__0__NAME", "Classics"),
        ]))
        .unwrap();
        assert_eq!(got[0], CollectionSpec { name: "Classics".into(), games: vec![] });
        assert_eq!(got[1].name, "Party games");
        assert_eq!(
            got[1].games,
            [("nes".into(), "A (USA).nes".into()), ("snes".into(), "B (USA).sfc".into())]
        );

        for (bad, says) in [
            (vec![("SOVEMU_COLLECTIONS__0__GAMES__0", "nes/A.nes")], "NAME is required"),
            (
                vec![
                    ("SOVEMU_COLLECTIONS__0__NAME", "X"),
                    ("SOVEMU_COLLECTIONS__0__GAMES__0", "A.nes"),
                ],
                "<system id>/<file name>",
            ),
            (
                vec![
                    ("SOVEMU_COLLECTIONS__0__NAME", "X"),
                    ("SOVEMU_COLLECTIONS__0__GAMES__0", "zx/A.nes"),
                ],
                "unknown system",
            ),
            (
                vec![("SOVEMU_COLLECTIONS__0__NAME", "X"), ("SOVEMU_COLLECTIONS__1__NAME", "X")],
                "already declared",
            ),
            (vec![("SOVEMU_COLLECTIONS__0__TITLE", "X")], "unknown setting"),
        ] {
            let err = collections(&vars(&bad)).unwrap_err().to_string();
            assert!(err.contains(says), "{err}");
        }
    }
}
