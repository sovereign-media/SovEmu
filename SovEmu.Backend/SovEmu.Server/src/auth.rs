//! Accounts and sign-in are `sovemu_identity`'s (`SovEmu.Identity`): local users with roles, and
//! the OAuth 2.1 / OpenID Connect provider every client signs in through. The server uses its
//! types under these names.

pub(crate) use sovemu_identity::{Credentials, NewUser, Role, User};
