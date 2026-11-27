//! Structured settings from the environment, in the `__` hierarchy Sovereign uses for its own
//! (`Sovereign__DataSources__Radarr__0__Url`): lists and maps that a flat variable cannot hold.
//!
//! - Users: `SovEmu__Users__<n>__Username`, `__PASSWORD` (or `__PASSWORD_FILE`), `__ROLE`
//!   (`viewer`, the default, or `admin`; `__ADMIN=true` is the older spelling of `admin`).
//! - Systems: `SovEmu__Systems__<id>__Core` (the core to use), `__ENABLED` (false hides it),
//!   `__DECORATION` (the decoration set, or `none`), `__RUN_AHEAD` (frames), `__PRESCALE` (1 or 2),
//!   `__LIMITS__MEMORY_MEGABYTES`, `__LIMITS__CPUS`, `__LIMITS__THREADS` and
//!   `__LIMITS__OPEN_FILES` (what its sessions' workers may use, over the sessions' limits), and
//!   `__OPTIONS__<core option>` (libretro core options, over SovEmu's own).
//! - Controller defaults: `SovEmu__Controllers__<n>__Id` (a controller ID from
//!   docs/CONTROLLERS.md), `__BINDINGS` (`south:B,east:A,…`), and optionally `__SYSTEM` (a
//!   system id: the default for that system only) and `__NAME`.
//!
//! - Shared collections: `SovEmu__Collections__<n>__Name` and `__GAMES__<m>`, each game as
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

/// What a system's session workers may use, where its settings say (0: no limit).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LimitsSpec {
    pub memory_mb: Option<u64>,
    pub cpus: Option<f64>,
    pub threads: Option<u32>,
    pub open_files: Option<u64>,
}

/// Per-system overrides.
#[derive(Debug, Clone, Default, PartialEq)]
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
    /// How many times its games' pictures are scaled up before they are encoded (1 or 2).
    pub prescale: Option<u32>,
    /// What its sessions' workers may use.
    pub limits: LimitsSpec,
}

/// A setting's field as written in messages: `RUN_AHEAD` → `RunAhead`, `CORE` → `Core`.
fn pascal(field: &str) -> String {
    field
        .split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or_else(String::new, |f| {
                f.to_ascii_uppercase().to_string() + &c.as_str().to_ascii_lowercase()
            })
        })
        .collect()
}

/// Groups `PREFIX__<key>__<rest>=value` into key → (rest → value).
fn group(vars: &[(String, String)], prefix: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut out: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for (name, value) in vars {
        // Any letter case, as .NET reads its configuration: `SOVEMU__USERS__0__USERNAME` too.
        let Some(rest) = name
            .get(..prefix.len())
            .filter(|p| p.eq_ignore_ascii_case(prefix))
            .map(|_| &name[prefix.len()..])
        else {
            continue;
        };
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

/// Users from `SovEmu__Users__<n>__…`, ordered by index.
pub(crate) fn users(vars: &[(String, String)]) -> Result<Vec<UserSpec>> {
    let mut indexed: Vec<(u32, BTreeMap<String, String>)> = Vec::new();
    for (key, fields) in group(vars, "SovEmu__Users__") {
        let n: u32 = key
            .parse()
            .with_context(|| format!("SovEmu__Users__{key}: the index must be a number"))?;
        let fields = fields.into_iter().map(|(k, v)| (k.to_ascii_uppercase(), v)).collect();
        indexed.push((n, fields));
    }
    indexed.sort_by_key(|(n, _)| *n);
    indexed
        .into_iter()
        .map(|(n, f)| {
            let var = |field: &str| format!("SovEmu__Users__{n}__{field}");
            let username = f
                .get("USERNAME")
                .filter(|u| !u.is_empty())
                .with_context(|| format!("{} is required", var("Username")))?;
            let password = f
                .get("PASSWORD")
                .filter(|p| !p.is_empty())
                .with_context(|| format!("{} is required", var("Password")))?
                .clone();
            let role = f
                .get("ROLE")
                .filter(|r| !r.is_empty())
                .map(|r| r.parse::<Role>().map_err(|e| anyhow::anyhow!("{}: {e}", var("Role"))))
                .transpose()?
                .unwrap_or(Role::Viewer);
            Ok(UserSpec { username: username.clone(), password, role })
        })
        .collect()
}

/// System overrides from `SovEmu__Systems__<id>__…`, keyed by lower-case system id.
pub(crate) fn systems(vars: &[(String, String)]) -> Result<BTreeMap<String, SystemSpec>> {
    let mut out: BTreeMap<String, SystemSpec> = BTreeMap::new();
    for (id, fields) in group(vars, "SovEmu__Systems__") {
        let id = id.to_ascii_lowercase();
        if sovemu_library::systems::by_id(&id).is_none() {
            bail!(
                "SovEmu__Systems__{id}: unknown system (known: {})",
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
            let var = format!("SovEmu__Systems__{id}__{}", pascal(&field));
            if let Some(option) =
                field.get(..9).filter(|p| p.eq_ignore_ascii_case("OPTIONS__")).map(|_| &field[9..])
            {
                spec.options.insert(option.to_owned(), value);
            } else {
                match field.to_ascii_uppercase().replace('_', "").as_str() {
                    "CORE" => spec.core = Some(value.trim().to_owned()).filter(|c| !c.is_empty()),
                    "ENABLED" => spec.enabled = Some(parse_bool(&var, &value)?),
                    "DECORATION" => {
                        spec.decoration = Some(value.trim().to_owned()).filter(|d| !d.is_empty())
                    }
                    "RUNAHEAD" => spec.run_ahead = Some(parse_run_ahead(&var, &id, &value)?),
                    "PRESCALE" => spec.prescale = Some(parse_prescale(&var, &value)?),
                    limit if limit.starts_with("LIMITS") => {
                        parse_limit(&id, &limit["LIMITS".len()..], &value, &mut spec.limits)?
                    }
                    other => bail!(
                        "{var}: unknown setting {other:?} (Core, Enabled, Decoration, RunAhead, Prescale, Limits__<limit> or Options__<name>)"
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

/// One of `SovEmu__Systems__<id>__Limits__*` (`limit` upper-cased without underscores).
fn parse_limit(id: &str, limit: &str, value: &str, limits: &mut LimitsSpec) -> Result<()> {
    fn number<T: std::str::FromStr>(var: &str, value: &str, what: &str) -> Result<T>
    where
        T::Err: std::error::Error + Send + Sync + 'static,
    {
        value.trim().parse().with_context(|| format!("{var}: {what}, 0 for no limit"))
    }
    let var = |name: &str| format!("SovEmu__Systems__{id}__Limits__{name}");
    match limit {
        "MEMORYMEGABYTES" => {
            limits.memory_mb = Some(number(&var("MemoryMegabytes"), value, "MiB")?);
        }
        "CPUS" => {
            let var = var("Cpus");
            let cpus: f64 = number(&var, value, "a number of CPUs (1.5 is a core and a half)")?;
            if !cpus.is_finite() || cpus < 0.0 {
                bail!("{var}: a number of CPUs, 0 for no limit (got {value:?})");
            }
            limits.cpus = Some(cpus);
        }
        "THREADS" => limits.threads = Some(number(&var("Threads"), value, "a number of threads")?),
        "OPENFILES" => {
            limits.open_files = Some(number(&var("OpenFiles"), value, "a number of files")?)
        }
        other => bail!(
            "{}: unknown limit (MemoryMegabytes, Cpus, Threads or OpenFiles)",
            var(&pascal(other.trim_start_matches('_')))
        ),
    }
    Ok(())
}

/// A pre-scale: 1 (off) or 2, as `SovEmu__Streaming__Video__Prescale` takes.
fn parse_prescale(var: &str, value: &str) -> Result<u32> {
    match value.trim() {
        "1" => Ok(1),
        "2" => Ok(2),
        other => bail!("{var}: 1 (off) or 2, got {other:?}"),
    }
}

/// Default controller mappings from `SovEmu__Controllers__<n>__…`, ordered by index. A list
/// rather than a map by ID, since IDs such as `pad:2dc8:6101` cannot be part of a variable name.
pub(crate) fn controllers(
    vars: &[(String, String)],
) -> Result<Vec<crate::controllers::ControllerSpec>> {
    let mut indexed: Vec<(u32, crate::controllers::ControllerSpec)> = Vec::new();
    for (key, fields) in group(vars, "SovEmu__Controllers__") {
        let n: u32 = key
            .parse()
            .with_context(|| format!("SovEmu__Controllers__{key}: the index must be a number"))?;
        let var = |field: &str| format!("SovEmu__Controllers__{n}__{field}");
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
                        .map_err(|e| anyhow::anyhow!("{}: {e}", var("System")))?;
                }
                "BINDINGS" => {
                    spec.bindings = crate::controllers::parse_bindings(&value)
                        .map_err(|e| anyhow::anyhow!("{}: {e}", var("Bindings")))?;
                    has_bindings = true;
                }
                other => bail!(
                    "{}: unknown setting {other:?} (Id, System, Name or Bindings)",
                    var(other)
                ),
            }
        }
        if spec.controller.is_empty() || !has_bindings {
            bail!("SovEmu__Controllers__{n}: needs both __Id and __Bindings");
        }
        crate::controllers::check(&spec.controller, &spec.name, &spec.bindings)
            .map_err(|e| anyhow::anyhow!("SovEmu__Controllers__{n}: {e}"))?;
        if let Some((m, _)) =
            indexed.iter().find(|(_, s)| s.controller == spec.controller && s.system == spec.system)
        {
            let on =
                if spec.system.is_empty() { String::new() } else { format!(" on {}", spec.system) };
            bail!(
                "SovEmu__Controllers__{n}: {}{on} is already set by SovEmu__Controllers__{m}",
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

/// Shared collections from `SovEmu__Collections__<n>__…`, ordered by index.
pub(crate) fn collections(vars: &[(String, String)]) -> Result<Vec<CollectionSpec>> {
    let mut indexed: Vec<(u32, CollectionSpec)> = Vec::new();
    for (key, fields) in group(vars, "SovEmu__Collections__") {
        let n: u32 = key
            .parse()
            .with_context(|| format!("SovEmu__Collections__{key}: the index must be a number"))?;
        let var = |field: &str| format!("SovEmu__Collections__{n}__{field}");
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
                bail!("{}: unknown setting {field:?} (Name or Games__<m>)", var(&field));
            }
        }
        if name.is_empty() {
            bail!("{} is required", var("Name"));
        }
        if let Some((m, _)) = indexed.iter().find(|(_, c)| c.name == name) {
            bail!(
                "SovEmu__Collections__{n}: {name:?} is already declared by SovEmu__Collections__{m}"
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
    fn users_by_index_in_any_letter_case() {
        let v = vars(&[
            ("SovEmu__Users__1__Username", "kid"),
            ("SOVEMU__USERS__1__PASSWORD", "password789"),
            ("SovEmu__Users__0__Username", "mum"),
            ("SovEmu__Users__0__Password", "password123"),
            ("SovEmu__Users__0__Role", "Admin"),
            ("SovEmu__Users__Prune", "true"),
            ("SOVEMU_OTHER", "x"),
        ]);
        let users = users(&v).unwrap();
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
                    password: "password789".into(),
                    role: Role::Viewer
                },
            ]
        );
    }

    #[test]
    fn user_errors_name_the_variable() {
        let missing = users(&vars(&[("SovEmu__Users__0__Username", "a")])).unwrap_err();
        assert!(format!("{missing:#}").contains("SovEmu__Users__0__Password"));
        let bad_index = users(&vars(&[("SovEmu__Users__x__Username", "a")])).unwrap_err();
        assert!(format!("{bad_index:#}").contains("index"));
        let bad_role = users(&vars(&[
            ("SovEmu__Users__0__Username", "a"),
            ("SovEmu__Users__0__Password", "p"),
            ("SovEmu__Users__0__Role", "owner"),
        ]))
        .unwrap_err();
        assert!(format!("{bad_role:#}").contains("SovEmu__Users__0__Role"));
    }

    #[test]
    fn system_overrides() {
        let v = vars(&[
            ("SovEmu__Systems__psx__Options__pcsx_rearmed_cd_readahead", "512"),
            ("SovEmu__Systems__PSX__Core", "mednafen_psx"),
            ("SovEmu__Systems__gamegear__Enabled", "false"),
        ]);
        let s = systems(&v).unwrap();
        assert_eq!(s["psx"].core.as_deref(), Some("mednafen_psx"));
        assert_eq!(s["psx"].options["pcsx_rearmed_cd_readahead"], "512");
        assert_eq!(s["gamegear"].enabled, Some(false));
        assert!(
            systems(&vars(&[("SovEmu__Systems__saturn__Core", "x")])).is_err(),
            "unknown system"
        );
        assert!(
            systems(&vars(&[("SovEmu__Systems__nes__COLOUR", "red")])).is_err(),
            "unknown setting"
        );
    }

    #[test]
    fn prescale_is_1_or_2() {
        let s = systems(&vars(&[
            ("SovEmu__Systems__gb__Prescale", "2"),
            ("SovEmu__Systems__SNES__PRESCALE", " 1 "),
        ]))
        .unwrap();
        assert_eq!(s["gb"].prescale, Some(2));
        assert_eq!(s["snes"].prescale, Some(1), "any letter case");
        for bad in ["3", "0", "2x", ""] {
            let err = systems(&vars(&[("SovEmu__Systems__nes__Prescale", bad)])).unwrap_err();
            assert!(format!("{err:#}").contains("SovEmu__Systems__nes__Prescale"), "{bad:?}");
        }
    }

    #[test]
    fn limits_per_system() {
        let s = systems(&vars(&[
            ("SovEmu__Systems__gamecube__Limits__MemoryMegabytes", "4096"),
            ("SovEmu__Systems__gamecube__Limits__Cpus", "2.5"),
            ("SovEmu__Systems__GAMECUBE__LIMITS__THREADS", "0"),
            ("SovEmu__Systems__nes__Limits__Open_Files", "64"),
        ]))
        .unwrap();
        let gc = &s["gamecube"].limits;
        assert_eq!((gc.memory_mb, gc.cpus, gc.threads), (Some(4096), Some(2.5), Some(0)));
        assert_eq!(s["nes"].limits.open_files, Some(64), "underscores ignored, as elsewhere");
        for (var, bad) in [
            ("SovEmu__Systems__nes__Limits__MemoryMegabytes", "lots"),
            ("SovEmu__Systems__nes__Limits__Cpus", "-1"),
            ("SovEmu__Systems__nes__Limits__Threads", "1.5"),
        ] {
            let err = systems(&vars(&[(var, bad)])).unwrap_err();
            assert!(format!("{err:#}").contains(var), "{var}={bad}: {err:#}");
        }
        let err = systems(&vars(&[("SovEmu__Systems__nes__Limits__Disk", "1")])).unwrap_err();
        assert!(format!("{err:#}").contains("unknown limit"), "{err:#}");
    }

    #[test]
    fn run_ahead_is_a_few_frames_on_a_2d_system() {
        let s = systems(&vars(&[("SovEmu__Systems__snes__RunAhead", "2")])).unwrap();
        assert_eq!(s["snes"].run_ahead, Some(2));
        let s = systems(&vars(&[("SovEmu__Systems__n64__RunAhead", "0")])).unwrap();
        assert_eq!(s["n64"].run_ahead, Some(0), "off is fine anywhere");
        for (var, value, why) in [
            ("SovEmu__Systems__nes__RunAhead", "5", "more than the maximum"),
            ("SovEmu__Systems__nes__RunAhead", "on", "not a number"),
            ("SovEmu__Systems__n64__RunAhead", "1", "a 3D system"),
        ] {
            assert!(systems(&vars(&[(var, value)])).is_err(), "{why}");
        }
    }

    #[test]
    fn reads_controller_defaults() {
        use crate::controllers::{Binding, PadButton};
        let specs = controllers(&vars(&[
            ("SovEmu__Controllers__1__Id", "keyboard"),
            ("SovEmu__Controllers__1__Bindings", "KeyJ:B"),
            ("SovEmu__Controllers__0__Id", "pad:2dc8:6101"),
            ("SovEmu__Controllers__0__name", "8BitDo SN30 Pro"),
            ("SovEmu__Controllers__0__Bindings", "south:A, east:B"),
        ]))
        .unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].controller, "pad:2dc8:6101");
        assert_eq!(specs[0].name, "8BitDo SN30 Pro");
        assert_eq!(specs[1].bindings, vec![Binding { input: "KeyJ".into(), button: PadButton::B }]);
        assert_eq!(specs[0].system, "");

        let per_system = controllers(&vars(&[
            ("SovEmu__Controllers__0__Id", "xbox"),
            ("SovEmu__Controllers__0__Bindings", "south:A"),
            ("SovEmu__Controllers__1__Id", "xbox"),
            ("SovEmu__Controllers__1__System", "megadrive"),
            ("SovEmu__Controllers__1__Bindings", "south:B"),
        ]))
        .unwrap();
        assert_eq!(
            per_system[1].system, "megadrive",
            "the same model on one system is not a repeat"
        );

        let fails = |v: &[(&str, &str)]| format!("{:#}", controllers(&vars(v)).unwrap_err());
        assert!(fails(&[("SovEmu__Controllers__0__Id", "xbox")]).contains("__Bindings"));
        let unknown_system = [
            ("SovEmu__Controllers__0__Id", "xbox"),
            ("SovEmu__Controllers__0__System", "atari"),
            ("SovEmu__Controllers__0__Bindings", ""),
        ];
        assert!(fails(&unknown_system).contains("not a system"));
        let key_on_pad = [
            ("SovEmu__Controllers__0__Id", "xbox"),
            ("SovEmu__Controllers__0__Bindings", "KeyZ:B"),
        ];
        assert!(fails(&key_on_pad).contains("KeyZ"));
        let bad_button = [
            ("SovEmu__Controllers__0__Id", "xbox"),
            ("SovEmu__Controllers__0__Bindings", "south:Q"),
        ];
        assert!(fails(&bad_button).contains("RetroPad"));
        assert!(
            fails(&[
                ("SovEmu__Controllers__0__Id", "xbox"),
                ("SovEmu__Controllers__0__Bindings", ""),
                ("SovEmu__Controllers__1__Id", "xbox"),
                ("SovEmu__Controllers__1__Bindings", ""),
            ])
            .contains("already set")
        );
    }

    #[test]
    fn collections_are_read_in_order_with_their_games() {
        let got = collections(&vars(&[
            ("SovEmu__Collections__1__Name", "Party  games"),
            ("SovEmu__Collections__1__Games__1", "snes/B (USA).sfc"),
            ("SovEmu__Collections__1__Games__0", "NES/A (USA).nes"),
            ("SovEmu__Collections__0__Name", "Classics"),
        ]))
        .unwrap();
        assert_eq!(got[0], CollectionSpec { name: "Classics".into(), games: vec![] });
        assert_eq!(got[1].name, "Party games");
        assert_eq!(
            got[1].games,
            [("nes".into(), "A (USA).nes".into()), ("snes".into(), "B (USA).sfc".into())]
        );

        for (bad, says) in [
            (vec![("SovEmu__Collections__0__Games__0", "nes/A.nes")], "Name is required"),
            (
                vec![
                    ("SovEmu__Collections__0__Name", "X"),
                    ("SovEmu__Collections__0__Games__0", "A.nes"),
                ],
                "<system id>/<file name>",
            ),
            (
                vec![
                    ("SovEmu__Collections__0__Name", "X"),
                    ("SovEmu__Collections__0__Games__0", "zx/A.nes"),
                ],
                "unknown system",
            ),
            (
                vec![("SovEmu__Collections__0__Name", "X"), ("SovEmu__Collections__1__Name", "X")],
                "already declared",
            ),
            (vec![("SovEmu__Collections__0__Title", "X")], "unknown setting"),
        ] {
            let err = collections(&vars(&bad)).unwrap_err().to_string();
            assert!(err.contains(says), "{err}");
        }
    }
}
