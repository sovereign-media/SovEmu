//! Structured settings from the environment, in the `__` hierarchy Sovereign uses for its own
//! (`Sovereign__DataSources__Radarr__0__Url`): lists and maps that a flat variable cannot hold.
//!
//! - Users: `SOVEMU_USERS__<n>__USERNAME`, `__PASSWORD` (or `__PASSWORD_FILE`), `__ADMIN`.
//! - Systems: `SOVEMU_SYSTEMS__<id>__CORE` (the core to use), `__ENABLED` (false hides it), and
//!   `__OPTIONS__<core option>` (libretro core options, over SovEmu's own).
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
                    other => {
                        bail!("{var}: unknown setting {other:?} (CORE, ENABLED or OPTIONS__<name>)")
                    }
                }
            }
        }
    }
    Ok(out)
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
        assert!(systems(&vars(&[("SOVEMU_SYSTEMS__n64__CORE", "x")])).is_err(), "unknown system");
        assert!(
            systems(&vars(&[("SOVEMU_SYSTEMS__nes__COLOUR", "red")])).is_err(),
            "unknown setting"
        );
    }
}
