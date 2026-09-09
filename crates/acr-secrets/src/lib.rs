//! The encrypted key-value store both AudioControl daemons hold credentials
//! in: the metadata process keeps the Last.fm session key, and the player
//! process keeps the Spotify account. Each daemon reads and writes only its
//! own keys, and -- now that the two processes are split -- each does so
//! through its own file, never the other's: `save_to_file` serialises the
//! whole in-memory map with a truncating write, so two daemons sharing one
//! file would have the second writer erase whatever the first had just
//! written, including a real credential neither daemon meant to lose. See
//! [`security_store`]'s module doc for which file holds which keys.
//!
//! The build-time obfuscated secrets live here too, in [`secrets`]. The
//! generator used to belong to `audiocontrol-metadata`, which worked only
//! while one binary linked both halves: the player daemon needs the Spotify
//! OAuth proxy URL and secret, and a shared crate cannot depend on one of its
//! own callers to get them. Generating them here gives both daemons the same
//! constants without either depending on the other.
//!
//! `SecurityStore::initialize` and `initialize_with_defaults` still take the
//! encryption key as an argument rather than reading [`secrets`] themselves.
//! That is what lets a caller open a store under a key of its own -- which
//! every test here does, and which `change_encryption_key` needs.
pub mod secrets;
pub mod security_store;
