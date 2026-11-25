//! SovEmu's ROM library, everything about it that is not the database: which systems there are
//! and how their folders are named, walking a library into games (parts folded together, soft
//! patches found), release labels for games that share a title, BIOS files, the game
//! information services' clients, the local cache and mirror of a network library, and
//! decorations (bezels).
//!
//! It reads files and asks services; the server stores what it learns. Nothing here opens the
//! database, so the server's tables are the only place the library's state lives.

pub mod artwork;
pub mod bios;
pub mod cache;
pub mod cores;
pub mod decorations;
pub mod info;
pub mod labels;
pub mod layout;
pub mod mirror;
pub mod scan;
pub mod systems;
pub mod text;
