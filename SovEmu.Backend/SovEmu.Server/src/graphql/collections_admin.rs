//! Shared collections, kept by an admin for everyone (`collections_admin`): making, renaming and
//! deleting them and setting their games. Everyone reads them (`sharedCollections`, in
//! `library`); one declared in the environment is changed only there (`CONFLICT`).

use super::library::CollectionNode;
use super::*;
use crate::activity::{Entry, Kind};
use crate::collections::{self, Owner};

/// An admin's change to a shared collection, for the activity log.
fn changed(what: &str, name: impl Into<String>) -> Entry {
    Entry::new(Kind::SettingsChanged)
        .about("Shared collections")
        .detail(format!("{what} {}", name.into()))
}

/// A shared collection's name, for the activity log.
async fn name_of(ctx: &Context<'_>, id: i64) -> String {
    sqlx::query_scalar("SELECT name FROM collections WHERE id = ? AND user_id IS NULL")
        .bind(id)
        .fetch_optional(&state(ctx).db)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| format!("collection {id}"))
}

#[derive(Default)]
pub(super) struct CollectionsAdminMutation;

#[Object]
impl CollectionsAdminMutation {
    /// Admins: makes a collection everyone sees (`CONFLICT` if one has that name).
    async fn create_shared_collection(
        &self,
        ctx: &Context<'_>,
        name: String,
    ) -> Result<CollectionNode> {
        let me = admin(ctx).await?;
        let made = collections::create(state(ctx), Owner::Server, &name).await.api()?;
        record(ctx, &me, changed("Made", made.name.clone()));
        Ok(CollectionNode(made))
    }

    /// Admins: renames a shared collection.
    async fn rename_shared_collection(
        &self,
        ctx: &Context<'_>,
        id: i64,
        name: String,
    ) -> Result<bool> {
        let me = admin(ctx).await?;
        let was = name_of(ctx, id).await;
        collections::rename(state(ctx), Owner::Server, id, &name).await.api()?;
        record(ctx, &me, changed("Renamed", format!("{was} to {}", name.trim())));
        Ok(true)
    }

    /// Admins: deletes a shared collection (its games stay in the library).
    async fn delete_shared_collection(&self, ctx: &Context<'_>, id: i64) -> Result<bool> {
        let me = admin(ctx).await?;
        let name = name_of(ctx, id).await;
        collections::delete(state(ctx), Owner::Server, id).await.api()?;
        record(ctx, &me, changed("Deleted", name));
        Ok(true)
    }

    /// Admins: puts a game in a shared collection, or takes it out.
    async fn set_in_shared_collection(
        &self,
        ctx: &Context<'_>,
        collection_id: i64,
        game_id: i64,
        member: bool,
    ) -> Result<bool> {
        let me = admin(ctx).await?;
        collections::set_member(state(ctx), Owner::Server, collection_id, game_id, member)
            .await
            .api()?;
        let name = name_of(ctx, collection_id).await;
        let what = if member { "Added a game to" } else { "Took a game out of" };
        record(ctx, &me, changed(what, name));
        Ok(member)
    }
}
