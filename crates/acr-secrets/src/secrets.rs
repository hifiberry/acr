// This file includes the generated secrets constants at compile time.
include!(concat!(env!("OUT_DIR"), "/generated_secrets.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    /// Every constant the two daemons read is generated into *this* crate and
    /// deobfuscates to something usable.
    ///
    /// Compiling this test is half of what it asserts: the six accessors and
    /// their constants have to exist here, under these names, with these
    /// signatures. The assertions cover the rest -- a generator that emitted a
    /// constant it could not decode leaves the accessor returning an empty
    /// string rather than failing the build.
    ///
    /// Deliberately *not* asserted: that an accessor round-trips its own
    /// constant. The generated accessor is defined as the deobfuscation of the
    /// constant, so that comparison holds however broken the obfuscation is,
    /// and it would be coverage of nothing.
    ///
    /// Nothing here prints a value: the assertions name the accessor and use
    /// `assert!`, never `assert_eq!`, so a machine that does have a
    /// `secrets.txt` cannot leak one through a failure message.
    ///
    /// **This is not the guard against a lost `secrets.txt`.** It asserts
    /// non-emptiness, and the generator's fallback `"unknown"` is non-empty --
    /// so it stays green in exactly the silent-failure state that matters,
    /// where the build produced constants from no input at all. What it pins
    /// is the interface: six accessors exist, are callable, and return
    /// something. `the_generator_still_searches_the_repository_root` below is
    /// the guard, because it is emitted from the same array `main()` iterates.
    /// Strengthen that one; strengthening this one would only make it demand a
    /// real secrets file on every machine that runs the tests.
    #[test]
    fn every_generated_accessor_yields_a_usable_value() {
        for (name, value, obf) in [
            (
                "secrets_encryption_key",
                secrets_encryption_key(),
                SECRETS_ENCRYPTION_KEY_OBF,
            ),
            ("lastfm_api_key", lastfm_api_key(), LASTFM_API_KEY_OBF),
            (
                "lastfm_api_secret",
                lastfm_api_secret(),
                LASTFM_API_SECRET_OBF,
            ),
            ("artistdb_api_key", artistdb_api_key(), ARTISTDB_API_KEY_OBF),
            (
                "spotify_oauth_url",
                spotify_oauth_url(),
                SPOTIFY_OAUTH_URL_OBF,
            ),
            (
                "spotify_proxy_secret",
                spotify_proxy_secret(),
                SPOTIFY_PROXY_SECRET_OBF,
            ),
        ] {
            assert!(!obf.is_empty(), "{} has no obfuscated constant", name);
            assert!(!value.is_empty(), "{} deobfuscated to an empty string", name);
        }
    }

    /// The generator must still look in the repository root.
    ///
    /// `build.rs` runs with the current directory set to `CARGO_MANIFEST_DIR`,
    /// so every path it searches is relative to *this crate*. The HiFiBerry OS
    /// build copies `secrets.txt` to the repository root, and moving this crate
    /// to a different depth under the workspace would silently stop the
    /// generator finding it: the build would succeed, every constant would be
    /// the `unknown` placeholder, and the first failure would be on a device.
    ///
    /// `SECRETS_SEARCH_PATHS` is emitted from the same list `build.rs`
    /// actually searches, so dropping the repository-root entry fails here.
    #[test]
    fn the_generator_still_searches_the_repository_root() {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let searched: Vec<std::path::PathBuf> =
            SECRETS_SEARCH_PATHS.iter().map(|p| manifest.join(p)).collect();
        let reaches_root = searched.iter().any(|candidate| {
            let dir = match candidate.parent() {
                Some(dir) => dir,
                None => return false,
            };
            dir.join("Cargo.lock").is_file() && dir.join("crates").is_dir()
        });
        assert!(
            reaches_root,
            "no path the generator searches lands in the repository root; it searches {:?}",
            SECRETS_SEARCH_PATHS
        );
    }
}
