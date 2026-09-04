//! Process-wide stores over configured paths: the SQLite attribute cache,
//! the settings DB, the image cache with its variant purge, and the
//! background job registry. Each daemon initialises its own.
pub mod attributecache;
pub mod backgroundjobs;
pub mod imagecache;
pub mod imagepurge;
pub mod settingsdb;
