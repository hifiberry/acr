//! The encrypted key-value store both AudioControl daemons hold credentials
//! in: the metadata process keeps the Last.fm session key here, and the
//! player process will keep the Spotify account once it owns playback. Each
//! daemon reads and writes its own keys; neither reads the other's entries,
//! and they share nothing beyond the one store file.
//!
//! This crate deliberately does not know how to produce the default
//! encryption key. That key comes from a build-time obfuscated secret
//! (`secrets.txt`, compiled in by `audiocontrol-metadata`'s `build.rs`), and
//! reaching back into that crate from here would recreate the dependency
//! this extraction exists to remove -- a shared crate cannot depend on one of
//! its own callers. So `SecurityStore::initialize` and
//! `initialize_with_defaults` take the key as an argument, and each binary
//! supplies its own.
pub mod security_store;
