//! How the server looks in every client: the background behind every screen (one for everyone,
//! the admin's choice), and each system's photo (`System.photoUrl`, in `library`).

use super::*;

/// A background clients draw behind every screen: a transparent image laid over the surface
/// colour, scaled to cover the screen.
#[derive(SimpleObject)]
pub(super) struct Background {
    /// Its name in settings (`SOVEMU_BACKGROUND`): `lattice`, or a file's name in the folders.
    pub(super) id: String,
    /// As shown: "Lattice", "Sunset beach".
    pub(super) name: String,
    /// The image (WebP, PNG or JPEG): a URL signed for this user.
    pub(super) url: String,
    /// One SovEmu ships, rather than the admin's own.
    pub(super) builtin: bool,
}

impl Background {
    pub(super) fn of(ctx: &Context<'_>, user: &User, image: &crate::appearance::Image) -> Self {
        let path = format!("/media/backgrounds/{}", image.id);
        Self {
            id: image.id.clone(),
            name: image.name.clone(),
            url: state(ctx).assets.sign(user.id, &path),
            builtin: image.builtin(),
        }
    }
}

/// The backgrounds there are, and the one chosen.
#[derive(SimpleObject)]
pub(super) struct AppearanceSettings {
    /// The built-in ones, then the admin's own (from `<data>/backgrounds` and
    /// `SOVEMU_BACKGROUNDS_DIRS`).
    pub(super) backgrounds: Vec<Background>,
    /// A background's id, `random` (a different one each visit) or `none`.
    pub(super) chosen: String,
    /// `SOVEMU_BACKGROUND` sets it: the page cannot change it.
    pub(super) from_environment: bool,
    /// The choice as the line that sets it, to move it into the environment.
    pub(super) environment: String,
}

#[derive(Default)]
pub(super) struct AppearanceQuery;

#[Object]
impl AppearanceQuery {
    /// The background to draw behind every screen now: the one chosen, or one at random each
    /// time it is asked (`random`); null for none.
    async fn background(&self, ctx: &Context<'_>) -> Result<Option<Background>> {
        let user = user(ctx).await?;
        let image = crate::appearance::current(state(ctx)).await.api()?;
        Ok(image.map(|i| Background::of(ctx, &user, &i)))
    }

    /// Admins: every background, and the one chosen.
    async fn appearance(&self, ctx: &Context<'_>) -> Result<AppearanceSettings> {
        let user = admin(ctx).await?;
        let (chosen, from_environment) = crate::appearance::chosen(state(ctx)).await.api()?;
        let backgrounds = state(ctx)
            .appearance()
            .backgrounds
            .iter()
            .map(|i| Background::of(ctx, &user, i))
            .collect();
        Ok(AppearanceSettings {
            backgrounds,
            environment: format!("SOVEMU_BACKGROUND={chosen}"),
            chosen,
            from_environment,
        })
    }
}

#[derive(Default)]
pub(super) struct AppearanceMutation;

#[Object]
impl AppearanceMutation {
    /// Admins: the background for everyone: a background's id, `random` or `none`. `CONFLICT`
    /// while `SOVEMU_BACKGROUND` sets it; `NOT_FOUND` for a background that is not there.
    async fn set_background(&self, ctx: &Context<'_>, id: String) -> Result<bool> {
        admin(ctx).await?;
        crate::appearance::choose(state(ctx), id.trim()).await.api()?;
        Ok(true)
    }
}
