//! Placeholder for the build-time secrets this crate reads.
//!
//! The real file is generated from `secrets.txt` by a build script; that
//! script and the obfuscation it emits arrive with the next commit. Until
//! then these accessors keep the crate compiling and answer "unknown", which
//! every caller already treats as a credential that will not authenticate.
//!
//! Every caller is behind `#[cfg(not(test))]`, so nothing in the test build
//! reaches these — hence the allow.
#![allow(dead_code)]

pub fn lastfm_api_key() -> String {
    "unknown".into()
}

pub fn lastfm_api_secret() -> String {
    "unknown".into()
}

pub fn artistdb_api_key() -> String {
    "unknown".into()
}

pub fn secrets_encryption_key() -> String {
    "unknown".into()
}

pub fn spotify_oauth_url() -> String {
    "unknown".into()
}

pub fn spotify_proxy_secret() -> String {
    "unknown".into()
}
