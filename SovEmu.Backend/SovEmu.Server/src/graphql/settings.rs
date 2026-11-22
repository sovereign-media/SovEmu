//! Emulator settings: each system's core options and run-ahead.

use super::*;

/// Where an emulator setting's value comes from.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
pub(super) enum SettingSource {
    /// The emulator's own default.
    Core,
    /// SovEmu's default for the system.
    Sovemu,
    /// Changed by an admin.
    Admin,
    /// Set by `SOVEMU_SYSTEMS__<id>__OPTIONS__<name>`, which always wins (locked).
    Environment,
}

/// An emulator setting (a libretro core option).
#[derive(SimpleObject)]
pub(super) struct EmulatorSetting {
    pub(super) key: String,
    pub(super) description: String,
    /// What it does, in the emulator's words (empty where it does not say).
    pub(super) info: String,
    /// The key of the emulator's category for it.
    pub(super) category: Option<String>,
    pub(super) choices: Vec<String>,
    /// A display label per choice.
    pub(super) labels: Vec<String>,
    /// The emulator's default.
    pub(super) default: String,
    pub(super) value: String,
    pub(super) source: SettingSource,
}

/// How many frames a system's pictures run ahead of its games, hiding the games' own input lag:
/// a press shows up that many frames sooner. Each frame costs that many more emulated frames.
#[derive(SimpleObject)]
pub(super) struct RunAheadSetting {
    /// 0: off.
    pub(super) frames: i32,
    /// The most a system may run ahead.
    pub(super) max: i32,
    /// `SOVEMU` when nothing sets it (1 frame on the systems that support it); `ENVIRONMENT` is
    /// `SOVEMU_SYSTEMS__<id>__RUN_AHEAD`, which always wins (locked).
    pub(super) source: SettingSource,
    /// Whether the system can run ahead at all (the 2D systems).
    pub(super) supported: bool,
}

/// A group of settings the emulator declares ("Video", "Audio" …).
#[derive(SimpleObject)]
pub(super) struct SettingCategory {
    pub(super) key: String,
    pub(super) description: String,
    pub(super) info: String,
}

/// A system's emulator settings.
#[derive(SimpleObject)]
pub(super) struct EmulatorSettings {
    pub(super) system: String,
    /// The emulator's name and version.
    pub(super) core: String,
    pub(super) settings: Vec<EmulatorSetting>,
    pub(super) categories: Vec<SettingCategory>,
    /// The emulator lists its settings only with a game loaded, and none has run yet.
    pub(super) needs_game: bool,
    /// The admin's changes as `SOVEMU_SYSTEMS__…` lines, to move into configuration.
    pub(super) environment: Vec<String>,
    pub(super) run_ahead: RunAheadSetting,
}

impl From<crate::settings::Source> for SettingSource {
    fn from(source: crate::settings::Source) -> Self {
        use crate::settings::Source;
        match source {
            Source::Core => SettingSource::Core,
            Source::Sovemu => SettingSource::Sovemu,
            Source::Admin => SettingSource::Admin,
            Source::Environment => SettingSource::Environment,
        }
    }
}

impl From<crate::settings::SystemSettings> for EmulatorSettings {
    fn from(s: crate::settings::SystemSettings) -> Self {
        Self {
            system: s.system,
            core: s.core,
            settings: s
                .settings
                .into_iter()
                .map(|x| EmulatorSetting {
                    key: x.key,
                    description: x.description,
                    info: x.info,
                    category: x.category,
                    choices: x.choices,
                    labels: x.labels,
                    default: x.default,
                    value: x.value,
                    source: x.source.into(),
                })
                .collect(),
            categories: s
                .categories
                .into_iter()
                .map(|c| SettingCategory { key: c.key, description: c.description, info: c.info })
                .collect(),
            needs_game: s.needs_game,
            environment: s.environment,
            run_ahead: RunAheadSetting {
                frames: s.run_ahead.frames as i32,
                max: sovemu_protocol::ipc::MAX_RUN_AHEAD as i32,
                source: s.run_ahead.source.into(),
                supported: s.run_ahead.supported,
            },
        }
    }
}

/// A change to one emulator setting.
#[derive(InputObject)]
pub(super) struct SettingChange {
    pub(super) key: String,
    /// The new value; null goes back to the default.
    pub(super) value: Option<String>,
}

#[derive(Default)]
pub(super) struct SettingsQuery;

#[Object]
impl SettingsQuery {
    /// Admins: a system's emulator settings.
    async fn emulator_settings(&self, ctx: &Context<'_>, system: ID) -> Result<EmulatorSettings> {
        admin(ctx).await?;
        crate::settings::get_settings(state(ctx), &system).await.api().map(Into::into)
    }
}

#[derive(Default)]
pub(super) struct SettingsMutation;

#[Object]
impl SettingsMutation {
    /// Admins: changes a system's emulator settings. Applies to the next game started.
    async fn change_emulator_settings(
        &self,
        ctx: &Context<'_>,
        system: ID,
        changes: Vec<SettingChange>,
    ) -> Result<EmulatorSettings> {
        admin(ctx).await?;
        let changes: BTreeMap<String, Option<String>> =
            changes.into_iter().map(|c| (c.key, c.value)).collect();
        crate::settings::put_settings(state(ctx), &system, changes).await.api().map(Into::into)
    }

    /// Admins: how many frames a system's pictures run ahead of its games (0: off), or with no
    /// `frames` back to SovEmu's default (1 frame on the systems that support it). Applies to the next game started. Refused while
    /// `SOVEMU_SYSTEMS__<id>__RUN_AHEAD` sets it.
    async fn set_run_ahead(
        &self,
        ctx: &Context<'_>,
        system: ID,
        frames: Option<i32>,
    ) -> Result<EmulatorSettings> {
        admin(ctx).await?;
        let frames = match frames {
            Some(f) if f < 0 => {
                return Err(crate::error::ApiError::bad_request("frames cannot be negative")).api();
            }
            f => f.map(|f| f as u32),
        };
        crate::settings::set_run_ahead(state(ctx), &system, frames).await.api().map(Into::into)
    }
}
