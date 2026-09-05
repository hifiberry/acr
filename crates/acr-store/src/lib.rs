//! Process-wide stores over configured paths: the SQLite attribute cache,
//! the settings DB, the image cache with its variant purge, the background
//! job registry, and the genre cleanup table. Each daemon initialises its own.
pub mod attributecache;
pub mod backgroundjobs;
pub mod genre_cleanup;
pub mod imagecache;
pub mod imagepurge;
pub mod settingsdb;
