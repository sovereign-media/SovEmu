//! Structured settings from the environment, in the `__` hierarchy Sovereign uses for its own
//! (`Sovereign__DataSources__Radarr__0__Url`): lists and maps that a flat variable cannot hold.
//!
//! - Users: `SOVEMU_USERS__<n>__USERNAME`, `__PASSWORD` (or `__PASSWORD_FILE`), `__ADMIN`.
//! - Systems: `SOVEMU_SYSTEMS__<id>__CORE` (the core to use), `__ENABLED` (false hides it),
//!   `__DECORATION` (the decoration set, or `none`), and `__OPTIONS__<core option>` (libretro core
//!   options, over SovEmu's own).
//! - Controller defaults: `SOVEMU_CONTROLLERS__<n>__ID` (a controller ID from
//!   docs/CONTROLLERS.md), `__BINDINGS` (`south:B,east:A,…`), and optionally `__SYSTEM` (a
//!   system id: the default for that system only) and `__NAME`.
//!
//! Keys after the prefix are case-insensitive except core option names and system ids, which are
//! passed through as written (lower-cased ids, as option names are).

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};

/// A user declared in the environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserSpec {
    pub username: String,
    pub password: String,
    pub admin: bool,
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
pub fn users(
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
            let admin =
                f.get("ADMIN").map(|v| parse_bool(&var("ADMIN"), v)).transpose()?.unwrap_or(false);
            Ok(UserSpec { username: username.clone(), password, admin })
        })
        .collect()
}

/// System overrides from `SOVEMU_SYSTEMS__<id>__…`, keyed by lower-case system id.
pub fn systems(vars: &[(String, String)]) -> Result<BTreeMap<String, SystemSpec>> {
    let mut out: BTreeMap<String, SystemSpec> = BTreeMap::new();
    for (id, fields) in group(vars, "SOVEMU_SYSTEMS__") {
        let id = id.to_ascii_lowercase();
        if crate::systems::by_id(&id).is_none() {
            bail!(
                "SOVEMU_SYSTEMS__{id}: unknown system (known: {})",
                crate::systems::SYSTEMS.iter().map(|s| s.id).collect::<Vec<_>>().join(", ")
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
                    other => bail!(
                        "{var}: unknown setting {other:?} (CORE, ENABLED, DECORATION or OPTIONS__<name>)"
                    ),
                }
            }
        }
    }
    Ok(out)
}

/// Default controller mappings from `SOVEMU_CONTROLLERS__<n>__…`, ordered by index. A list
/// rather than a map by ID, since IDs such as `pad:2dc8:6101` cannot be part of a variable name.
pub fn controllers(vars: &[(String, String)]) -> Result<Vec<crate::controllers::ControllerSpec>> {
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
            ("SOVEMU_USERS__0__ADMIN", "true"),
            ("SOVEMU_OTHER", "x"),
        ]);
        let users = users(&v, |path| Ok(format!("from {path}"))).unwrap();
        assert_eq!(
            users,
            [
                UserSpec { username: "mum".into(), password: "password123".into(), admin: true },
                UserSpec {
                    username: "kid".into(),
                    password: "from /run/secrets/kid".into(),
                    admin: false
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
}
