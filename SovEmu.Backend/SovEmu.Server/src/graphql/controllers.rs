//! Controller layouts and mappings, the user's and the admin's defaults.

use super::*;
use crate::activity::{Entry, Kind};

/// An admin's change to a controller model's default, on one system or every system.
fn controller_entry(controller: &str, system: Option<&ID>, what: &str) -> Entry {
    let on = system.map_or_else(|| "every system".to_owned(), |s| s.to_string());
    Entry::new(Kind::SettingsChanged)
        .about(format!("Controller {controller}"))
        .detail(format!("{what} ({on})"))
}

/// The default controller mappings (admins).
#[derive(SimpleObject)]
pub(super) struct ControllerDefaults {
    /// From the environment (locked) and from the admin page.
    pub(super) mappings: Vec<crate::controllers::ControllerMapping>,
    /// The admin page's defaults as `SovEmu__Controllers__<n>__…` lines.
    pub(super) environment: Vec<String>,
}

#[derive(Default)]
pub(super) struct ControllersQuery;

#[Object]
impl ControllersQuery {
    /// Diagrams of the player's controllers, by family (`xbox`, `playstation`, `nintendo`,
    /// `generic`), keyed by input (`south`, `l1` …).
    async fn controller_layouts(&self) -> Vec<crate::controls::PadLayout> {
        crate::controls::controller_layouts()
    }

    /// Admins: the default controller mappings every user starts from, and the admin's as
    /// environment lines.
    async fn controller_defaults(&self, ctx: &Context<'_>) -> Result<ControllerDefaults> {
        admin(ctx).await?;
        let state = state(ctx);
        Ok(ControllerDefaults {
            mappings: crate::controllers::defaults(state).await.api()?,
            environment: crate::controllers::environment_lines(state).await.api()?,
        })
    }

    /// The controller mappings in force for this user on a system (or, with none, on every
    /// system), one per controller model (docs/CONTROLLERS.md): their own for the system, their
    /// own for every system, the default for the system, the default for every system. A model
    /// missing here uses the clients' built-in defaults.
    async fn controller_mappings(
        &self,
        ctx: &Context<'_>,
        system: Option<ID>,
    ) -> Result<Vec<crate::controllers::ControllerMapping>> {
        let user = user(ctx).await?;
        crate::controllers::list(state(ctx), &user, system.as_ref().map(|s| s.as_str())).await.api()
    }
}

#[derive(Default)]
pub(super) struct ControllersMutation;

#[Object]
impl ControllersMutation {
    /// Saves this user's mapping for a controller model on a system, or on every system without
    /// one, replacing its defaults wherever they play.
    async fn save_controller_mapping(
        &self,
        ctx: &Context<'_>,
        controller: String,
        system: Option<ID>,
        #[graphql(default)] name: String,
        bindings: Vec<crate::controllers::Binding>,
    ) -> Result<crate::controllers::ControllerMapping> {
        let user = user(ctx).await?;
        let state = state(ctx);
        crate::controllers::save(
            state,
            &user,
            &controller,
            system.as_ref().map(|s| s.as_str()),
            &name,
            bindings,
        )
        .await
        .api()
    }

    /// Admins: sets the default mapping for a controller model on a system (or every system),
    /// for every user without their own. A model whose default is in the environment cannot be changed here (CONFLICT).
    async fn save_controller_default(
        &self,
        ctx: &Context<'_>,
        controller: String,
        system: Option<ID>,
        #[graphql(default)] name: String,
        bindings: Vec<crate::controllers::Binding>,
    ) -> Result<crate::controllers::ControllerMapping> {
        let me = admin(ctx).await?;
        let state = state(ctx);
        let saved = crate::controllers::save_default(
            state,
            &controller,
            system.as_ref().map(|s| s.as_str()),
            &name,
            bindings,
        )
        .await
        .api()?;
        record(ctx, &me, controller_entry(&controller, system.as_ref(), "default mapping saved"));
        Ok(saved)
    }

    /// Admins: forgets the admin's default for a controller model. Returns whether it had one.
    async fn reset_controller_default(
        &self,
        ctx: &Context<'_>,
        controller: String,
        system: Option<ID>,
    ) -> Result<bool> {
        let me = admin(ctx).await?;
        let had = crate::controllers::reset_default(
            state(ctx),
            &controller,
            system.as_ref().map(|s| s.as_str()),
        )
        .await
        .api()?;
        if had {
            record(
                ctx,
                &me,
                controller_entry(&controller, system.as_ref(), "default mapping reset"),
            );
        }
        Ok(had)
    }

    /// Puts a controller model back on the defaults (an admin's, else the built-in ones).
    /// Returns whether it had a mapping of the user's own.
    async fn reset_controller_mapping(
        &self,
        ctx: &Context<'_>,
        controller: String,
        system: Option<ID>,
    ) -> Result<bool> {
        let user = user(ctx).await?;
        crate::controllers::reset(
            state(ctx),
            &user,
            &controller,
            system.as_ref().map(|s| s.as_str()),
        )
        .await
        .api()
    }
}
