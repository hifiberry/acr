use std::sync::Arc;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::io::Read;
use log::{debug, info, warn};
use once_cell::sync::Lazy;
use crate::data::artist::Artist;
use crate::helpers::coverart::{query_coverart, CoverartQuery, QueryOptions};
use crate::helpers::musicbrainz::{search_mbids_for_artist, MusicBrainzSearchResult};

/// Result of an artist image operation
#[derive(Debug)]
pub enum ArtistImageResult {
    /// Image found and cached successfully
    Found { cache_path: String },
    /// Image not found
    NotFound,
    /// Error occurred during operation
    Error(String),
}

/// Where a downloaded image is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageDestination {
    /// The provider cache, `{cache_dir}/{artist}/{type}.jpg`.
    Cache,
    /// The user directory, which wins the lookup.
    UserDirectory,
}

/// What an artist image lookup needs next.
///
/// Decided under the store's lock and carried out without it, so that no
/// network call is ever made while the mutex is held.
#[derive(Debug, PartialEq, Eq)]
pub enum ImageStep {
    /// Already on disk at this path.
    Ready(String),
    /// Fetch this URL and commit it.
    Fetch { url: String, image_type: String, destination: ImageDestination },
    /// Nothing recorded for this artist; ask the providers.
    AskProviders,
    /// Nothing to do.
    NotFound,
}

/// How long a single artist image download may take, in total.
///
/// Bounded rather than generous: the URL comes from a client through
/// `/coverart/artist/<b64>/update`, and a URL naming the daemon itself is
/// treated as remote and fetched rather than refused, so an unbounded wait
/// here is one client's way to hang a fetch forever.
const IMAGE_DOWNLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// How long a caller waits for someone else's in-flight download of the same
/// artist before giving up.
///
/// Deliberately shorter than `IMAGE_DOWNLOAD_TIMEOUT`: this caller is not
/// doing the fetch, it is waiting on one, and that fetch is itself allowed to
/// run for the full download timeout before failing. Waiting that long too
/// would mean waiting out the winner's own deadline and then still finding
/// nothing -- so this caller gives up first and reports no image, rather than
/// holding a second request open for the whole of the first one's deadline.
const IMAGE_DOWNLOAD_WAIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// How long to sleep between polls while waiting on someone else's download.
const IMAGE_DOWNLOAD_WAIT_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);

/// The largest artist image the daemon will take from a URL.
///
/// Generous for cover art -- a provider's artist background is a couple of
/// megabytes -- and small enough that the read cannot exhaust the memory of a
/// 1 GB device. The URL is client-supplied, so the bound is applied to the
/// read rather than checked after it.
const MAX_IMAGE_DOWNLOAD_BYTES: u64 = 16 * 1024 * 1024;

/// How many uploaded images one artist may keep.
///
/// A bound rather than a setting: the set exists to be picked from in a UI,
/// and a list past ten is a scroll rather than a choice. An upload past the
/// cap is refused and says so — evicting the oldest would silently discard a
/// picture someone deliberately chose.
pub const MAX_UPLOADS_PER_ARTIST: usize = 10;

/// Where one member of an artist's image set came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtistImageSource {
    /// `custom.jpg` or `cover.jpg`: fetched by the daemon from a URL.
    Download,
    /// A file the user uploaded, named by the hash of its own bytes.
    Upload,
}

/// One member of an artist's image set.
#[derive(Debug, Clone)]
pub struct ArtistImage {
    /// The file stem: `custom`, `cover`, or an upload's content hash.
    pub id: String,
    /// Absolute path on disk.
    pub path: String,
    pub source: ArtistImageSource,
}

/// The id of an uploaded image: the hash of the bytes themselves.
///
/// Content addressing makes a retried upload idempotent — the same bytes
/// resolve to the same file rather than growing the set — and it means the
/// bytes behind a name never change, so variants generated from them stay
/// valid for as long as the file exists.
pub fn upload_id(bytes: &[u8]) -> String {
    format!("{:x}", md5::compute(bytes))
}

/// The file extension for these bytes, if they are an image we can serve.
///
/// Taken from the content, never from anything a client said: the serving
/// route derives the `Content-Type` from the extension, so a `.jpg` holding a
/// PNG would be served under the wrong type.
///
/// Limited to the formats the upload path can actually decode: the `image`
/// crate backing `imageresize::validate` is built with only jpeg/png/webp
/// support, so accepting a GIF or BMP extension here would store a file the
/// daemon can never resize.
pub fn image_extension(bytes: &[u8]) -> Option<&'static str> {
    let mut cursor = std::io::Cursor::new(bytes);
    let (_, _, format) = crate::helpers::image_meta::detect_image_dimensions(&mut cursor).ok()?;
    match format.to_ascii_uppercase().as_str() {
        "JPEG" => Some("jpg"),
        "PNG" => Some("png"),
        "WEBP" => Some("webp"),
        _ => None,
    }
}

/// Whether a stored file's name claims one of the formats we serve.
///
/// The counterpart to [`image_extension`] for a file that is already on disk.
/// The name is trustworthy because we wrote it: the extension came from
/// sniffing the bytes at store time and never from anything a client said, so
/// deciding membership of an artist's set from it costs a directory entry
/// rather than a full read of every image.
fn has_servable_extension(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(extension.to_ascii_lowercase().as_str(), "jpg" | "jpeg" | "png" | "webp")
        })
}

/// The id named by one of this daemon's own artist-image URLs.
///
/// The selection a client posts to `/artist/<b64>/update` is a URL, and a
/// member of an artist's set is now one of the URLs it can post. Recognising
/// our own address means recording a pointer instead of fetching ourselves
/// over HTTP, and it is deliberately strict: the path must be exactly the
/// serving route, and the artist in it must be the artist being updated, so a
/// URL cannot select an image across artists. Anything with a scheme and host
/// is somebody else's URL, however its path ends.
pub fn local_image_id(url: &str, artist_name: &str) -> Option<String> {
    let expected_prefix = format!(
        "{}/coverart/artist/{}/image/",
        crate::constants::API_PREFIX,
        crate::helpers::url_encoding::encode_url_safe(artist_name)
    );
    let id = url.strip_prefix(&expected_prefix)?;
    if id.is_empty() || id.contains('/') || id.contains('?') {
        return None;
    }
    Some(id.to_string())
}

/// Configuration for the artist store
#[derive(Debug, Clone)]
pub struct ArtistStoreConfig {
    /// Base cache directory for artist images
    pub cache_dir: String,
    /// User directory for custom artist images (takes precedence over cache)
    pub user_dir: String,
    /// Whether to enable custom artist images from settings
    pub enable_custom_images: bool,
    /// Whether to automatically download missing images
    pub auto_download: bool,
}

impl Default for ArtistStoreConfig {
    fn default() -> Self {
        // Read configuration from settings database with fallback defaults
        let cache_dir = crate::helpers::settingsdb::get_string_with_default(
            "datastore.artist_store.cache_dir", 
            "/var/lib/audiocontrol/cache/artists"
        ).unwrap_or_else(|_| "/var/lib/audiocontrol/cache/artists".to_string());
        
        let user_dir = crate::helpers::settingsdb::get_string_with_default(
            "datastore.user_image_path", 
            "/var/lib/audiocontrol/user/images"
        ).unwrap_or_else(|_| "/var/lib/audiocontrol/user/images".to_string());
        
        let enable_custom_images = crate::helpers::settingsdb::get_bool_with_default(
            "datastore.artist_store.enable_custom_images", 
            true
        ).unwrap_or(true);
        
        let auto_download = crate::helpers::settingsdb::get_bool_with_default(
            "datastore.artist_store.auto_download", 
            true
        ).unwrap_or(true);

        Self {
            cache_dir,
            user_dir,
            enable_custom_images,
            auto_download,
        }
    }
}

/// Artist store for managing artist cover art download and caching
pub struct ArtistStore {
    /// Configuration
    config: ArtistStoreConfig,
    /// Cache of artist image paths
    image_cache: HashMap<String, String>,
    /// Artists with a download claimed right now, to prevent duplicate downloads.
    ///
    /// A plain set, not a flag per artist: `begin_download`/`finish_download`
    /// only ever run with `&mut self`, so nothing needs to observe or clone a
    /// flag independently of the lock that already serialises every access.
    downloading: std::collections::HashSet<String>,
}

impl Default for ArtistStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ArtistStore {
    /// Create a new artist store with default configuration
    pub fn new() -> Self {
        Self::with_config(ArtistStoreConfig::default())
    }

    /// Create a new artist store with custom configuration
    pub fn with_config(config: ArtistStoreConfig) -> Self {
        Self {
            config,
            image_cache: HashMap::new(),
            downloading: std::collections::HashSet::new(),
        }
    }

    /// Get the local cache path for an artist's cover art
    /// 
    /// # Arguments
    /// * `artist_name` - The name of the artist
    /// * `image_type` - Type of image ("custom", "cover", etc.)
    /// 
    /// # Returns
    /// The local cache path for the artist's image
    pub fn get_artist_image_path(&self, artist_name: &str, image_type: &str) -> String {
        let sanitized_name = crate::helpers::sanitize::filename_from_string(artist_name);
        format!("{}/{}/{}.jpg", self.config.cache_dir, sanitized_name, image_type)
    }

    /// Get the user directory path for an artist's custom cover art
    /// 
    /// # Arguments
    /// * `artist_name` - The name of the artist
    /// * `image_type` - Type of image ("custom", "cover", etc.)
    /// 
    /// # Returns
    /// The user directory path for the artist's image
    pub fn get_artist_user_image_path(&self, artist_name: &str, image_type: &str) -> String {
        let sanitized_name = crate::helpers::sanitize::filename_from_string(artist_name);
        format!("{}/artists/{}/{}.jpg", self.config.user_dir, sanitized_name, image_type)
    }

    /// The directory an artist's uploaded images live in.
    fn artist_uploads_dir(&self, artist_name: &str) -> String {
        let sanitized = crate::helpers::sanitize::filename_from_string(artist_name);
        format!("{}/artists/{}/uploads", self.config.user_dir, sanitized)
    }

    /// Test-only window onto the private uploads directory.
    #[cfg(test)]
    pub fn artist_uploads_dir_for_test(&self, artist_name: &str) -> String {
        self.artist_uploads_dir(artist_name)
    }

    /// Every image stored for this artist: the two well-known downloads and
    /// each upload.
    ///
    /// The filesystem is the source of truth, so a file put there by hand
    /// shows up and a file deleted by hand disappears. Anything in `uploads/`
    /// that is not a regular file with one of the extensions this daemon
    /// serves is skipped, so a stray `.DS_Store` costs the artist nothing.
    ///
    /// Classification is by name, never by content: this walk runs on the
    /// player-event path and on every listing, and an upload's extension was
    /// already sniffed from its bytes when it was written. Reading each file
    /// whole to re-derive that would cost tens of megabytes per listing for an
    /// artist with a full set. A file whose bytes turn out not to be an image
    /// after all is dropped by the listing route, which has to read the header
    /// for the dimensions anyway.
    pub fn artist_images(&self, artist_name: &str) -> Vec<ArtistImage> {
        let mut images = Vec::new();

        for id in ["custom", "cover"] {
            let path = self.get_artist_user_image_path(artist_name, id);
            if std::fs::metadata(&path).is_ok() {
                images.push(ArtistImage {
                    id: id.to_string(),
                    path,
                    source: ArtistImageSource::Download,
                });
            }
        }

        let uploads = self.artist_uploads_dir(artist_name);
        if let Ok(entries) = std::fs::read_dir(&uploads) {
            let mut found: Vec<ArtistImage> = Vec::new();
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
                // Variants live beside their original as `<stem>@<size>`; they
                // are derived files, not members of the set.
                if crate::helpers::imageresize::variant_size_of(stem).is_some() {
                    continue;
                }
                if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
                    continue;
                }
                if !has_servable_extension(&path) {
                    debug!("Skipping {} in {}: not a recognised image name", path.display(), uploads);
                    continue;
                }
                found.push(ArtistImage {
                    id: stem.to_string(),
                    path: path.to_string_lossy().into_owned(),
                    source: ArtistImageSource::Upload,
                });
            }
            found.sort_by(|a, b| a.id.cmp(&b.id));
            images.extend(found);
        }

        images
    }

    /// The path of one member of the set, or `None` when the id is not in it.
    pub fn artist_image_path(&self, artist_name: &str, id: &str) -> Option<String> {
        self.artist_images(artist_name)
            .into_iter()
            .find(|image| image.id == id)
            .map(|image| image.path)
    }

    /// Store uploaded bytes as a member of the artist's set and return its id.
    ///
    /// The id is the hash of the bytes, so storing the same image twice is the
    /// same member: a retry costs nothing and the cap is not consumed by it.
    /// The type is sniffed from the bytes, which both validates them — an HTML
    /// error page is not an image — and names the file.
    pub fn store_uploaded_image(&mut self, artist_name: &str, bytes: &[u8]) -> Result<String, String> {
        let Some(extension) = image_extension(bytes) else {
            return Err("Not a recognised image format".to_string());
        };
        let id = upload_id(bytes);

        let existing = self.artist_images(artist_name);
        let already_stored = existing.iter().any(|image| image.id == id);
        let uploads = existing
            .iter()
            .filter(|image| image.source == ArtistImageSource::Upload)
            .count();
        if !already_stored && uploads >= MAX_UPLOADS_PER_ARTIST {
            return Err(format!(
                "This artist already has the maximum of {} uploaded images; delete one first",
                MAX_UPLOADS_PER_ARTIST
            ));
        }

        let path = format!("{}/{}.{}", self.artist_uploads_dir(artist_name), id, extension);
        if let Some(parent) = std::path::Path::new(&path).parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
        }
        // Written through a temporary name and renamed into place, which is
        // what makes re-uploading the same bytes both safe and useful. The
        // serving route reads these files with the store lock released, so a
        // plain rewrite could hand a concurrent request a half-written
        // image; and a plain write that failed part way -- a full disk is the
        // realistic one -- would leave a truncated file that a retry could
        // never repair, because the name matches and the bytes do not. A
        // rename is atomic, so a reader sees either the previous complete file
        // or the new one, and a retry after a failure replaces whatever the
        // failure left behind.
        crate::helpers::imagecache::write_file_atomically(std::path::Path::new(&path), bytes)
            .map_err(|e| format!("Failed to write {}: {}", path, e))?;

        // Variants are generated from whatever was at this path before, and
        // this is now the one place where that can differ from what is here
        // afterwards: a re-upload repairing a damaged member would otherwise
        // keep serving the rung rendered from the damaged bytes, cached hard
        // by its ETag. The download path and the delete path already do this
        // for the same reason.
        crate::helpers::imageresize::remove_variants_of(&path);

        // The resolved-path memo is keyed by artist name and this changes what
        // that artist resolves to.
        self.image_cache.remove(artist_name);
        info!("Stored uploaded image {} for artist {}", id, artist_name);
        Ok(id)
    }

    /// Remove one member of the set, and the variants generated from it.
    ///
    /// Whether the member was selected is decided before the removal but acted
    /// on after it: a `remove_file` that fails must leave the artist exactly as
    /// it was, rather than reporting an error with the selection already gone.
    /// The decision cannot simply be repeated afterwards either — once the file
    /// is off disk the selection no longer resolves to any member, so it would
    /// look as though nothing had been selected.
    pub fn delete_artist_image(&mut self, artist_name: &str, id: &str) -> Result<(), String> {
        let images = self.artist_images(artist_name);
        let Some(path) = images.iter().find(|image| image.id == id).map(|image| image.path.clone()) else {
            return Err(format!("No image '{}' for artist '{}'", id, artist_name));
        };
        let was_selected = self
            .stored_selection(artist_name)
            .and_then(|stored| self.selection_member(artist_name, &stored, &images))
            .is_some_and(|selected| selected.id == id);

        std::fs::remove_file(&path).map_err(|e| format!("Failed to remove {}: {}", path, e))?;

        if was_selected {
            crate::helpers::settingsdb::remove(&format!("artist.image.{}", artist_name)).ok();
        }
        crate::helpers::imageresize::remove_variants_of(&path);
        crate::helpers::image_meta::clear_image_cache(&path).ok();
        self.image_cache.remove(artist_name);
        Ok(())
    }

    /// Record which member of the set is chosen.
    ///
    /// The pointer lives where the WebUI already puts it — the settings key
    /// `artist.image.{artist}` — so there is one record of "the chosen image"
    /// rather than two that can disagree.
    pub fn select_artist_image(&mut self, artist_name: &str, id: &str) -> Result<(), String> {
        if self.artist_image_path(artist_name, id).is_none() {
            return Err(format!("No image '{}' for artist '{}'", id, artist_name));
        }
        let url = format!(
            "{}/coverart/artist/{}/image/{}",
            crate::constants::API_PREFIX,
            crate::helpers::url_encoding::encode_url_safe(artist_name),
            id
        );
        crate::helpers::settingsdb::set_string(&format!("artist.image.{}", artist_name), &url)
            .map_err(|e| format!("Failed to record the selection: {}", e))?;
        self.image_cache.remove(artist_name);
        Ok(())
    }

    /// The URL recorded as this artist's selection, if there is a real one.
    ///
    /// An empty value is how the API clears a selection, so it is the same
    /// thing as no key at all.
    fn stored_selection(&self, artist_name: &str) -> Option<String> {
        let stored = crate::helpers::settingsdb::get_string(&format!("artist.image.{}", artist_name)).ok()??;
        if stored.is_empty() { None } else { Some(stored) }
    }

    /// Which member of `images` the stored selection `stored` resolves to.
    ///
    /// One of our own URLs names a member directly. Any other URL is a remote
    /// image, and the only place a remote selection is ever put is
    /// `custom.jpg` — so once it has been downloaded, `custom` is the member
    /// that selection is serving. Reporting nothing selected there would
    /// contradict the picture the artist is actually showing, which is the
    /// ordinary outcome of the WebUI's provider-candidate flow.
    ///
    /// Takes the already-walked set so that a caller which needs both the set
    /// and the selection pays for one walk of the directory, not two.
    fn selection_member<'a>(
        &self,
        artist_name: &str,
        stored: &str,
        images: &'a [ArtistImage],
    ) -> Option<&'a ArtistImage> {
        let wanted = local_image_id(stored, artist_name).unwrap_or_else(|| "custom".to_string());
        images.iter().find(|image| image.id == wanted)
    }

    /// Which member is selected, or `None` when nothing is.
    pub fn selected_image_id(&self, artist_name: &str) -> Option<String> {
        let stored = self.stored_selection(artist_name)?;
        let images = self.artist_images(artist_name);
        self.selection_member(artist_name, &stored, &images).map(|image| image.id.clone())
    }

    /// An artist's whole set together with the id of the selected member.
    ///
    /// The listing route needs both, and resolving them separately would walk
    /// the artist's directory twice per request.
    pub fn artist_images_with_selection(&self, artist_name: &str) -> (Vec<ArtistImage>, Option<String>) {
        let images = self.artist_images(artist_name);
        let selected = self
            .stored_selection(artist_name)
            .and_then(|stored| self.selection_member(artist_name, &stored, &images))
            .map(|image| image.id.clone());
        (images, selected)
    }

    /// Forget the path memoised for this artist.
    ///
    /// The memo is keyed by artist name and holds one resolved path, so it has
    /// to be dropped by every caller that changes what the artist resolves to
    /// — including the ones outside this module that write the selection key
    /// themselves.
    pub fn forget_memoised_path(&mut self, artist_name: &str) {
        self.image_cache.remove(artist_name);
    }

    /// Check if an artist image exists in cache
    /// 
    /// # Arguments
    /// * `artist_name` - The name of the artist
    /// * `image_type` - Type of image ("custom", "cover", etc.)
    /// 
    /// # Returns
    /// True if the image exists in cache
    pub fn has_cached_image(&self, artist_name: &str, image_type: &str) -> bool {
        let cache_path = self.get_artist_image_path(artist_name, image_type);
        std::fs::metadata(&cache_path).is_ok()
    }

    /// Get the cached image path for an artist if it exists
    /// 
    /// # Arguments
    /// * `artist_name` - The name of the artist
    /// 
    /// # Returns
    /// ArtistImageResult with the cache path if found
    pub fn get_cached_image(&mut self, artist_name: &str) -> ArtistImageResult {
        debug!("Checking cached image for artist: {}", artist_name);

        // Check cache first
        if let Some(cached_path) = self.image_cache.get(artist_name) {
            if std::fs::metadata(cached_path).is_ok() {
                debug!("Found cached image path for artist {}: {}", artist_name, cached_path);
                return ArtistImageResult::Found { cache_path: cached_path.clone() };
            } else {
                // Remove stale cache entry
                self.image_cache.remove(artist_name);
            }
        }

        // A selected member wins the chain below: the whole point of choosing
        // one is that it beats whatever precedence would otherwise apply.
        // This runs on every player event that resolves an artist image, so it
        // reads the selection first and walks the directory only when there is
        // something to resolve, and walks it exactly once.
        if let Some(stored) = self.stored_selection(artist_name) {
            let images = self.artist_images(artist_name);
            let selected = self
                .selection_member(artist_name, &stored, &images)
                .map(|image| image.path.clone());
            if let Some(path) = selected {
                self.image_cache.insert(artist_name.to_string(), path.clone());
                return ArtistImageResult::Found { cache_path: path };
            }
        }

        // Check user directory first (takes precedence over cache)
        let user_custom_path = self.get_artist_user_image_path(artist_name, "custom");
        if std::fs::metadata(&user_custom_path).is_ok() {
            debug!("Found user custom image for artist {}: {}", artist_name, user_custom_path);
            self.image_cache.insert(artist_name.to_string(), user_custom_path.clone());
            return ArtistImageResult::Found { cache_path: user_custom_path };
        }

        let user_cover_path = self.get_artist_user_image_path(artist_name, "cover");
        if std::fs::metadata(&user_cover_path).is_ok() {
            debug!("Found user cover image for artist {}: {}", artist_name, user_cover_path);
            self.image_cache.insert(artist_name.to_string(), user_cover_path.clone());
            return ArtistImageResult::Found { cache_path: user_cover_path };
        }

        // Check for custom image in cache directory
        if self.config.enable_custom_images {
            let custom_path = self.get_artist_image_path(artist_name, "custom");
            if std::fs::metadata(&custom_path).is_ok() {
                debug!("Found custom image for artist {}: {}", artist_name, custom_path);
                self.image_cache.insert(artist_name.to_string(), custom_path.clone());
                return ArtistImageResult::Found { cache_path: custom_path };
            }
        }

        // Check for regular cover image in cache directory
        let cover_path = self.get_artist_image_path(artist_name, "cover");
        if std::fs::metadata(&cover_path).is_ok() {
            debug!("Found cover image for artist {}: {}", artist_name, cover_path);
            self.image_cache.insert(artist_name.to_string(), cover_path.clone());
            return ArtistImageResult::Found { cache_path: cover_path };
        }

        debug!("No cached image found for artist: {}", artist_name);
        ArtistImageResult::NotFound
    }

    /// What an artist image lookup needs next.
    ///
    /// Exactly the decision logic that used to sit at the top of
    /// `get_or_download_artist_image`, and nothing else: this makes the
    /// decision under the store's lock, and the caller carries out whatever
    /// it says -- a fetch or a provider query -- once the lock is released.
    pub fn next_image_step(&mut self, artist_name: &str) -> ImageStep {
        // First check if we already have a cached image
        if let ArtistImageResult::Found { cache_path } = self.get_cached_image(artist_name) {
            return ImageStep::Ready(cache_path);
        }

        // If auto-download is disabled, return not found
        if !self.config.auto_download {
            return ImageStep::NotFound;
        }

        // Check for custom image URL in settings first
        if self.config.enable_custom_images {
            if let Some(custom_url) = self.stored_selection(artist_name) {
                if local_image_id(&custom_url, artist_name).is_some() {
                    // One of our own URLs is a pointer at a file, not something
                    // to fetch. `get_cached_image` above has already failed to
                    // resolve it, so the file is gone; handing `/api/...` to the
                    // HTTP client would only turn a recoverable miss into an
                    // error and leave the artist with no picture at all.
                    debug!(
                        "Selection for artist {} names a member that is gone ({}); falling back to the providers",
                        artist_name, custom_url
                    );
                } else {
                    debug!("Found custom image URL for artist {}: {}", artist_name, custom_url);
                    return ImageStep::Fetch {
                        url: custom_url,
                        image_type: "custom".to_string(),
                        destination: ImageDestination::Cache,
                    };
                }
            }
        }

        ImageStep::AskProviders
    }

    /// Store bytes fetched for an artist and update the store's records.
    ///
    /// The storing half of what `download_and_cache_image` and
    /// `download_and_store_user_image` used to do in one lock-held call: by
    /// the time this runs the bytes are already in hand, so this never blocks
    /// on the network.
    pub fn commit_downloaded_image(
        &mut self,
        artist_name: &str,
        bytes: &[u8],
        image_type: &str,
        destination: ImageDestination,
    ) -> ArtistImageResult {
        let path = match destination {
            ImageDestination::Cache => self.get_artist_image_path(artist_name, image_type),
            ImageDestination::UserDirectory => self.get_artist_user_image_path(artist_name, image_type),
        };

        match self.store_image(&path, bytes) {
            Ok(_) => {
                match destination {
                    ImageDestination::Cache => {
                        info!("Downloaded and cached {} image for artist {}", image_type, artist_name);
                    }
                    ImageDestination::UserDirectory => {
                        info!("Downloaded and stored {} image for artist {} in user directory", image_type, artist_name);
                    }
                }
                // Do not simply point the memo at what was just written: a
                // selection can be made while this fetch is in flight (an
                // upload picked in the WebUI, or a fresh `/update` call), and
                // that selection has to win over whichever download happens
                // to land last. `get_cached_image` re-derives what the
                // artist resolves to right now -- the selection first, then
                // the precedence chain -- and populates the memo itself, so
                // this only falls back to the path just written when nothing
                // else resolves at all. The memo has to be forgotten first:
                // `get_cached_image` checks it before the selection, so
                // leaving in place whatever it held from before this fetch
                // started -- possibly stale by now -- would make it win
                // instead of deferring to anything.
                self.image_cache.remove(artist_name);
                match self.get_cached_image(artist_name) {
                    ArtistImageResult::Found { cache_path } => ArtistImageResult::Found { cache_path },
                    _ => {
                        self.image_cache.insert(artist_name.to_string(), path.clone());
                        ArtistImageResult::Found { cache_path: path }
                    }
                }
            }
            Err(e) => {
                match destination {
                    ImageDestination::Cache => {
                        warn!("Failed to store {} image for artist {}: {}", image_type, artist_name, e);
                    }
                    ImageDestination::UserDirectory => {
                        warn!("Failed to store {} image for artist {} in user directory: {}", image_type, artist_name, e);
                    }
                }
                ArtistImageResult::Error(format!("Failed to store image: {}", e))
            }
        }
    }

    /// Claim the right to download this artist's image.
    ///
    /// `false` when another caller is already downloading for this artist --
    /// the caller must not fetch a second time. This is the flag that used to
    /// be read and written under the same lock that serialised the whole
    /// download, so a second caller could never observe it set; now that the
    /// fetch itself happens with the lock released, two callers really can
    /// race here, and this is what keeps them from duplicating the work.
    pub fn begin_download(&mut self, artist_name: &str) -> bool {
        if self.downloading.contains(artist_name) {
            debug!("Image already being downloaded for artist: {}", artist_name);
            return false;
        }

        self.downloading.insert(artist_name.to_string());
        true
    }

    /// Release the claim taken by [`Self::begin_download`].
    pub fn finish_download(&mut self, artist_name: &str) {
        self.downloading.remove(artist_name);
    }

    /// Whether a download is claimed for this artist right now.
    ///
    /// Lets a waiter tell "the winner is still working" from "the winner is
    /// done, one way or another" without waiting out its own timeout to find
    /// out: once this is `false` and `next_image_step` still isn't `Ready`,
    /// there is nothing left to wait for.
    pub fn is_downloading(&self, artist_name: &str) -> bool {
        self.downloading.contains(artist_name)
    }

    /// Looks up MusicBrainz IDs for an artist and returns them if found
    /// 
    /// This function searches for MusicBrainz IDs associated with the given artist name.
    /// 
    /// # Arguments
    /// * `artist_name` - The name of the artist to look up
    /// 
    /// # Returns
    /// A tuple containing:
    /// * `Vec<String>` - Vector of MusicBrainz IDs if found, empty vector otherwise
    /// * `bool` - true if this is a partial match (only some artists in a multi-artist name found)
    pub fn lookup_artist_mbids(&self, artist_name: &str) -> (Vec<String>, bool) {
        debug!("Looking up MusicBrainz IDs for artist: {}", artist_name);
        
        // Try to retrieve MusicBrainz ID using search_mbids_for_artist function
        // This is now a fully synchronous call since we replaced musicbrainz_rs with direct HTTP
        let search_result = search_mbids_for_artist(artist_name, true, false, true);
        
        match search_result {
            MusicBrainzSearchResult::Found(mbids, _) => {
                debug!("Found {} MusicBrainz ID(s) for artist {}: {:?}", 
                      mbids.len(), artist_name, mbids);
                (mbids, false) // Complete match
            },
            MusicBrainzSearchResult::FoundPartial(mbids, _) => {
                info!("Found {} partial MusicBrainz ID(s) for multi-artist {}: {:?}", 
                      mbids.len(), artist_name, mbids);
                (mbids, true) // Partial match
            },
            MusicBrainzSearchResult::NotFound => {
                info!("No MusicBrainz ID found for artist: {}", artist_name);
                (Vec::new(), false)
            },
            MusicBrainzSearchResult::Error(error) => {
                warn!("Error retrieving MusicBrainz ID for artist {}: {}", artist_name, error);
                (Vec::new(), false)
            }
        }
    }

    /// Updates artist data by fetching additional information like MusicBrainz IDs
    ///
    /// This function takes an artist and attempts to retrieve and set any missing data
    /// such as MusicBrainz IDs.
    ///
    /// This is the MusicBrainz and metadata half of what the module-level
    /// `update_data_for_artist` does; cover art resolution happens there,
    /// outside this store's lock, and this method never touches it.
    ///
    /// # Arguments
    /// * `artist` - The artist to update
    ///
    /// # Returns
    /// The updated artist
    pub fn update_data_for_artist(&mut self, mut artist: Artist) -> Artist {
        debug!("Updating data for artist: {}", artist.name);

        // Check if the artist already has MusicBrainz IDs set
        let has_mbid = match &artist.metadata {
            Some(meta) => !meta.mbid.is_empty(),
            None => false,
        };

        if !has_mbid {
            debug!("No MusicBrainz ID set for artist {}, attempting to retrieve it", artist.name);

            // Use the synchronous function to look up MusicBrainz IDs directly
            let (mbids, partial_match) = self.lookup_artist_mbids(&artist.name);
            let mbid_count = mbids.len();

            // Add each MusicBrainz ID to the artist if any were found
            for mbid in mbids {
                artist.add_mbid(mbid);
            }

            // if there is more than one mbid or it was a partial match, it's a multi-artist entry
            if mbid_count > 1 || partial_match {
                artist.is_multi = true; // Mark as multi-artist entry
                artist.clear_metadata(); // Clear metadata for multi-artist entries
                debug!("Cleared metadata for multi-artist entry: {}", artist.name);
            } else if mbid_count > 0 {
                info!("Updated artist '{}' with MusicBrainz data: {} ID(s)", artist.name, mbid_count);
                debug!("Added MusicBrainz ID(s) to artist {}", artist.name);
            }

            // Record if this is a partial match in the artist metadata
            if partial_match {
                debug!("Partial match found for multi-artist name: {}", artist.name);
                if let Some(meta) = &mut artist.metadata {
                    meta.is_partial_match = true;
                }
            }
        } else {
            debug!("Artist {} already has MusicBrainz ID(s)", artist.name);
        }

        artist
    }

    /// Clear cached image for an artist
    /// 
    /// # Arguments
    /// * `artist_name` - The name of the artist
    pub fn clear_cached_image(&mut self, artist_name: &str) {
        self.image_cache.remove(artist_name);
        
        // Remove user directory images
        let user_custom_path = self.get_artist_user_image_path(artist_name, "custom");
        let _ = std::fs::remove_file(&user_custom_path);
        
        let user_cover_path = self.get_artist_user_image_path(artist_name, "cover");
        let _ = std::fs::remove_file(&user_cover_path);
        
        // Remove cache directory images
        let custom_path = self.get_artist_image_path(artist_name, "custom");
        let _ = std::fs::remove_file(&custom_path);
        
        let cover_path = self.get_artist_image_path(artist_name, "cover");
        let _ = std::fs::remove_file(&cover_path);
        
        debug!("Cleared cached images for artist: {}", artist_name);
    }

    /// Store image data to a file
    /// 
    /// # Arguments
    /// * `cache_path` - The path to store the image
    /// * `image_data` - The image data to store
    /// 
    /// # Returns
    /// Result indicating success or failure
    fn store_image(&self, cache_path: &str, image_data: &[u8]) -> Result<(), String> {
        // Use the existing image cache functionality
        crate::helpers::imagecache::store_image(cache_path, image_data)
            .map_err(|e| e.to_string())
    }
}

/// Download an image from a URL.
///
/// Takes no store state and holds no lock -- this is the network call that
/// used to run with the artist store's mutex held.
///
/// # Arguments
/// * `url` - The URL to download the image from
///
/// # Returns
/// Result with the image data or an error message
pub(crate) fn fetch_image(url: &str) -> Result<Vec<u8>, String> {
    debug!("Downloading image from URL: {}", url);

    // Bounded even though the lock is no longer held across it. The URL
    // comes from a client through /coverart/artist/<b64>/update, and a URL
    // naming the device itself is treated as remote and fetched, so the
    // daemon can still be asked to call itself -- that now costs a worker
    // and a socket rather than the whole store, but a request with no
    // deadline is still a request nothing ends. ureq's `timeout` is a
    // deadline for the socket phases -- connect, write, and the body read
    // -- not just the connect. It does not cover name resolution, which
    // ureq performs before the deadline applies, so a hostname whose
    // resolver is blackholed can still block here for as long as the
    // resolver takes.
    match ureq::get(url).timeout(IMAGE_DOWNLOAD_TIMEOUT).call() {
        Ok(response) => {
            // Read one byte past the cap: if it arrives, the body is over
            // the limit and we know that without having buffered the rest.
            // The deadline alone bounds nothing useful here -- fifteen
            // seconds of LAN throughput is hundreds of megabytes into a
            // Vec on a device with a gigabyte of RAM, and the allocation
            // failure that follows takes the whole daemon with it.
            let mut bytes = Vec::new();
            let mut reader = response.into_reader().take(MAX_IMAGE_DOWNLOAD_BYTES.saturating_add(1));
            if let Err(e) = reader.read_to_end(&mut bytes) {
                return Err(format!("Failed to read image data: {}", e));
            }

            if bytes.len() as u64 > MAX_IMAGE_DOWNLOAD_BYTES {
                return Err(format!(
                    "Downloaded image exceeds the {} byte limit",
                    MAX_IMAGE_DOWNLOAD_BYTES
                ));
            }

            if bytes.is_empty() {
                return Err("Downloaded image is empty".to_string());
            }

            debug!("Successfully downloaded image: {} bytes", bytes.len());
            Ok(bytes)
        },
        Err(e) => {
            Err(format!("HTTP request failed: {}", e))
        }
    }
}

/// The highest-graded image the coverart providers have for this artist.
///
/// Fast providers only: this runs while a caller waits for an artist image,
/// and a slow provider's answer reaches the cache by its own route. Holds no
/// lock -- `query_coverart` is a network call.
fn best_provider_image(artist_name: &str) -> Option<String> {
    let results = query_coverart(
        &CoverartQuery::Artist(artist_name.to_string()),
        &QueryOptions::default(),
    );

    if results.is_empty() {
        debug!("No cover art found for artist {}", artist_name);
        return None;
    }

    // Find the highest-rated image across all providers
    let mut best_image: Option<&crate::helpers::coverart::ImageInfo> = None;
    let mut best_grade = -10; // Start lower to allow grade -1 images

    for result in &results {
        for image in &result.images {
            let grade = image.grade.unwrap_or(0);
            if grade > best_grade {
                best_grade = grade;
                best_image = Some(image);
            }
        }
    }

    match best_image {
        Some(best_image) => {
            debug!("Found best image for artist {} with grade {}: {}", artist_name, best_grade, best_image.url);
            Some(best_image.url.clone())
        }
        None => {
            debug!("No images with valid grades found for artist {}", artist_name);
            None
        }
    }
}

/// Global singleton instance of the artist store
static ARTIST_STORE: Lazy<Arc<Mutex<ArtistStore>>> = Lazy::new(|| {
    Arc::new(Mutex::new(ArtistStore::new()))
});

/// Get the global artist store instance
pub fn get_artist_store() -> Arc<Mutex<ArtistStore>> {
    ARTIST_STORE.clone()
}

/// Convenience function to get cached image for an artist
/// 
/// # Arguments
/// * `artist_name` - The name of the artist
/// 
/// # Returns
/// Option with the cache path if found
pub fn get_artist_cached_image(artist_name: &str) -> Option<String> {
    let store_arc = get_artist_store();
    let mut store = store_arc.lock();
    match store.get_cached_image(artist_name) {
        ArtistImageResult::Found { cache_path } => Some(cache_path),
        _ => None,
    }
}

/// Releases a download claim when dropped, even if the code between the
/// claim and its ordinary release point unwinds.
///
/// `begin_download`/`finish_download` are plain store methods, not RAII on
/// their own, so a panic between the two would otherwise strand the claim --
/// and that is realistic here: `best_provider_image` fans out to third-party
/// providers, and `fetch_image` reads a client-supplied URL. `parking_lot`
/// mutexes do not poison, so a stranded claim would not clear itself; every
/// later lookup for that artist would lose the race to `begin_download`,
/// wait out `IMAGE_DOWNLOAD_WAIT_TIMEOUT`, and return `None`, for the rest of
/// the process's life.
///
/// Only ever constructed after `begin_download` has actually succeeded, and
/// held until after any commit for this claim has completed: local
/// variables drop in reverse declaration order, so as long as a guard is
/// declared *before* this one, that guard's `Drop` -- releasing the store
/// lock used for the commit -- runs first, and this one's `Drop` -- which
/// re-locks the store to call `finish_download` -- runs after. A waiter must
/// never be able to observe the claim gone while the result it was waiting
/// for is still uncommitted, and this ordering is what guarantees that.
struct DownloadClaim {
    store: Arc<Mutex<ArtistStore>>,
    artist_name: String,
}

impl DownloadClaim {
    fn new(store: Arc<Mutex<ArtistStore>>, artist_name: String) -> Self {
        Self { store, artist_name }
    }
}

impl Drop for DownloadClaim {
    fn drop(&mut self) {
        self.store.lock().finish_download(&self.artist_name);
    }
}

/// Wait for another caller's in-flight download of this artist to land.
///
/// Polls with no lock held between checks: each iteration takes the lock
/// only long enough to ask `next_image_step` and `is_downloading`, and drops
/// it again before sleeping, so a slow winner never has to contend with this
/// thread for the store -- only its own fetch has to finish. Gives up as
/// soon as the winner's claim is gone and the artist still isn't `Ready` --
/// its fetch failed or found nothing, so there is nothing left to wait for
/// -- rather than always paying the full `IMAGE_DOWNLOAD_WAIT_TIMEOUT`.
fn wait_for_in_flight_download(artist_name: &str) -> Option<String> {
    let deadline = std::time::Instant::now() + IMAGE_DOWNLOAD_WAIT_TIMEOUT;
    loop {
        let (step, still_downloading) = {
            let store_arc = get_artist_store();
            let mut store = store_arc.lock();
            (store.next_image_step(artist_name), store.is_downloading(artist_name))
        };
        if let ImageStep::Ready(path) = step {
            return Some(path);
        }
        if !still_downloading {
            return None;
        }

        if std::time::Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(IMAGE_DOWNLOAD_WAIT_POLL_INTERVAL);
    }
}

/// Get or download artist cover art.
///
/// The only place this sequences a lookup: decide under the store's lock,
/// release it, do whatever network work the decision calls for, then
/// re-acquire the lock only to commit. No guard is ever held across
/// `best_provider_image` or `fetch_image`.
///
/// The claim taken by `begin_download` covers the provider query as well as
/// the fetch -- querying providers is the more expensive half on a
/// constrained device, so N concurrent callers for the same artist must not
/// each pay for it before N-1 are turned away. A caller that is turned away
/// waits for the winner instead of giving up outright, since a lookup this
/// function used to serve by finding the winner's result already in the
/// cache once the shared lock let it through.
///
/// # Arguments
/// * `artist_name` - The name of the artist
///
/// # Returns
/// Option with the cache path if found or downloaded
pub fn get_or_download_artist_image(artist_name: &str) -> Option<String> {
    let store_arc = get_artist_store();

    let step = {
        let mut store = store_arc.lock();
        store.next_image_step(artist_name)
    };

    match step {
        ImageStep::Ready(path) => return Some(path),
        ImageStep::NotFound => return None,
        ImageStep::Fetch { .. } | ImageStep::AskProviders => {}
    }

    let claimed = {
        let mut store = store_arc.lock();
        store.begin_download(artist_name)
    };
    if !claimed {
        return wait_for_in_flight_download(artist_name);
    }
    // Declared before the commit's lock guard further down, so that guard's
    // Drop always runs first: see the type's own doc comment for why that
    // order is load-bearing, not incidental.
    let _claim = DownloadClaim::new(store_arc.clone(), artist_name.to_string());

    let resolved = match step {
        ImageStep::Fetch { url, image_type, destination } => Some((url, image_type, destination)),
        ImageStep::AskProviders => {
            best_provider_image(artist_name).map(|url| (url, "cover".to_string(), ImageDestination::Cache))
        }
        ImageStep::Ready(_) | ImageStep::NotFound => None, // handled above; kept exhaustive rather than panicking
    };

    // No provider had anything for this artist: nothing to fetch or commit.
    // `_claim` releases the download claim when it drops on the way out.
    let Some((url, image_type, destination)) = resolved else {
        return None;
    };

    let fetch_result = fetch_image(&url);

    let mut store = store_arc.lock();
    match fetch_result {
        Ok(bytes) => match store.commit_downloaded_image(artist_name, &bytes, &image_type, destination) {
            ArtistImageResult::Found { cache_path } => Some(cache_path),
            _ => None,
        },
        Err(e) => {
            warn!("Failed to download image for artist {} from URL {}: {}", artist_name, url, e);
            None
        }
    }
}

/// Attach the coverart API URL to an artist's metadata when an image was found.
///
/// Free of store state, so the orchestrator above can call this after its
/// lock is already released.
fn apply_coverart_metadata(mut artist: Artist, found: bool) -> Artist {
    if !found {
        debug!("No image available for artist {}", artist.name);
        return artist;
    }

    // Initialize metadata if needed
    if artist.metadata.is_none() {
        artist.metadata = Some(crate::data::ArtistMeta::new());
    }

    // Add the cached image to the artist metadata
    if let Some(ref mut metadata) = artist.metadata {
        // Generate proper API URL for artist image
        let encoded_name = crate::helpers::url_encoding::encode_url_safe(&artist.name);
        let api_url = format!("{}/coverart/artist/{}/image", crate::constants::API_PREFIX, encoded_name);
        metadata.thumb_url = vec![api_url];
        debug!("Updated artist {} with coverart API image URL: /api/coverart/artist/{}/image", artist.name, encoded_name);
    }

    artist
}

/// Update an artist with cover art information.
///
/// Resolves the image through the unlocked orchestrator above before the
/// store is touched at all, so this never holds the store's lock across the
/// network calls that resolution can make.
///
/// # Arguments
/// * `artist` - The artist to update
///
/// # Returns
/// The updated artist with image URLs in metadata
pub fn update_artist_with_coverart(artist: Artist) -> Artist {
    debug!("Updating artist {} with cover art", artist.name);
    let found = get_or_download_artist_image(&artist.name).is_some();
    apply_coverart_metadata(artist, found)
}

/// Convenience function to lookup MusicBrainz IDs for an artist
/// 
/// # Arguments
/// * `artist_name` - The name of the artist
/// 
/// # Returns
/// A tuple containing:
/// * `Vec<String>` - Vector of MusicBrainz IDs if found, empty vector otherwise
/// * `bool` - true if this is a partial match (only some artists in a multi-artist name found)
pub fn lookup_artist_mbids(artist_name: &str) -> (Vec<String>, bool) {
    let store_arc = get_artist_store();
    let store = store_arc.lock();
    store.lookup_artist_mbids(artist_name)
}

/// Update artist data, including metadata and cover art.
///
/// The MusicBrainz lookup runs under the store's lock, exactly as it always
/// has; cover art resolution runs afterwards, through the unlocked
/// orchestrator, so the lock is never held across it.
///
/// # Arguments
/// * `artist` - The artist to update
///
/// # Returns
/// The updated artist with metadata and cover art information
pub fn update_data_for_artist(artist: Artist) -> Artist {
    let mut artist = {
        let store_arc = get_artist_store();
        let mut store = store_arc.lock();
        store.update_data_for_artist(artist)
    };

    // If the artist has MusicBrainz IDs, update from the coverart system
    if artist.metadata.as_ref().is_some_and(|meta| !meta.mbid.is_empty()) {
        debug!("Artist {} has MusicBrainz ID(s), updating with cover art system", artist.name);
        artist = update_artist_with_coverart(artist);
    } else {
        // For artists without MusicBrainz IDs, still try coverart system with artist name only
        debug!("Artist {} has no MusicBrainz ID, trying cover art by name only", artist.name);
        artist = update_artist_with_coverart(artist);
    }

    // Note: LastFM metadata is now handled by the unified coverart system
    // No need for separate LastFM calls as the coverart system includes LastFM provider

    // Handle artists without MusicBrainz IDs but with existing thumbnails
    if artist.metadata.as_ref().is_some_and(|meta| meta.mbid.is_empty()) {
        // Check if the artist has thumbnail images
        let has_thumbnails = match &artist.metadata {
            Some(meta) => !meta.thumb_url.is_empty(),
            None => false,
        };

        if has_thumbnails {
            debug!("Artist {} has thumbnail image(s) but no MusicBrainz ID, skipping updates", artist.name);
        }
    }

    // Store the updated metadata in cache
    if let Some(metadata) = &artist.metadata {
        // Create a cache key using the artist's name
        let cache_key = format!("artist::metadata::{}", artist.name);

        // Store the metadata in the attribute cache
        match crate::helpers::attributecache::set(&cache_key, metadata) {
            Ok(_) => debug!("Stored metadata for artist {} in attribute cache", artist.name),
            Err(e) => warn!("Failed to store metadata for artist {} in attribute cache: {}", artist.name, e),
        }

        // If the artist has MusicBrainz IDs, store them separately for faster lookup
        if !metadata.mbid.is_empty() {
            let mbid_key = format!("artist::mbid::{}", artist.name);
            if let Err(e) = crate::helpers::attributecache::set(&mbid_key, &metadata.mbid) {
                warn!("Failed to store MusicBrainz IDs for artist {} in attribute cache: {}", artist.name, e);
            }
        }
    }

    // Return the potentially updated artist
    artist
}

/// Convenience function to clear cached image for an artist
/// 
/// # Arguments
/// * `artist_name` - The name of the artist
pub fn clear_artist_cached_image(artist_name: &str) {
    let store_arc = get_artist_store();
    let mut store = store_arc.lock();
    store.clear_cached_image(artist_name);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;
    use serial_test::serial;

    /// Repoint the process-wide settings database at a temporary directory, once.
    ///
    /// The selection lives in the same global settings database the daemon
    /// uses for everything else, so a test that calls `select_artist_image`
    /// must not write to the real device path either. `#[serial]` on every
    /// caller keeps this from racing the settingsdb crate's own `#[serial]`
    /// tests, which share the same global; distinct artist names per test
    /// keep them from colliding with each other through the one database this
    /// leaves in place for the rest of the run.
    fn init_test_settings_db() {
        use std::sync::Once;
        static INIT: Once = Once::new();

        INIT.call_once(|| {
            let temp_dir = TempDir::new().expect("settings db temp dir");
            crate::helpers::settingsdb::SettingsDb::initialize_global(temp_dir.path())
                .expect("settings db should initialize");
            std::mem::forget(temp_dir);
        });
    }

    /// Create a test artist store with temporary directories
    fn create_test_store() -> (ArtistStore, TempDir, TempDir) {
        let cache_temp_dir = TempDir::new().expect("Failed to create temp cache dir");
        let user_temp_dir = TempDir::new().expect("Failed to create temp user dir");
        
        let config = ArtistStoreConfig {
            cache_dir: cache_temp_dir.path().to_string_lossy().to_string(),
            user_dir: user_temp_dir.path().to_string_lossy().to_string(),
            enable_custom_images: true,
            auto_download: true,
        };
        
        let store = ArtistStore::with_config(config);
        (store, cache_temp_dir, user_temp_dir)
    }

    #[test]
    fn test_user_directory_precedence() {
        let (mut store, _cache_temp, _user_temp) = create_test_store();
        let artist_name = "Test Artist";
        
        // Use the sanitized name format
        let sanitized_name = crate::helpers::sanitize::filename_from_string(artist_name);
        
        // Create user directory structure
        let user_artist_dir = Path::new(&store.config.user_dir).join("artists").join(&sanitized_name);
        fs::create_dir_all(&user_artist_dir).expect("Failed to create user artist dir");
        
        // Create cache directory structure (cache_dir already includes 'artists')
        let cache_artist_dir = Path::new(&store.config.cache_dir).join(&sanitized_name);
        fs::create_dir_all(&cache_artist_dir).expect("Failed to create cache artist dir");
        
        // Create a dummy image in cache
        let cache_image_path = cache_artist_dir.join("cover.jpg");
        fs::write(&cache_image_path, b"cache image data").expect("Failed to write cache image");
        
        // Create a dummy image in user directory
        let user_image_path = user_artist_dir.join("cover.jpg");
        fs::write(&user_image_path, b"user image data").expect("Failed to write user image");
        
        // Test that user directory takes precedence
        match store.get_cached_image(artist_name) {
            ArtistImageResult::Found { cache_path } => {
                assert!(cache_path.contains(&store.config.user_dir), 
                    "User directory should take precedence over cache directory. Got: {}", cache_path);
                
                // Verify the content is from user directory
                let content = fs::read(&cache_path).expect("Failed to read image");
                assert_eq!(content, b"user image data");
            },
            _ => panic!("Should have found image in user directory"),
        }
    }

    #[test] 
    fn test_get_artist_image_paths() {
        let (store, _cache_temp, _user_temp) = create_test_store();
        
        let cache_path = store.get_artist_image_path("Metallica", "cover");
        // Use the sanitized filename format (filename_from_string converts to lowercase)
        assert!(cache_path.contains("/metallica/cover.jpg"));
        assert!(cache_path.starts_with(&store.config.cache_dir));
        
        let user_path = store.get_artist_user_image_path("Metallica", "custom");
        assert!(user_path.contains("/artists/metallica/custom.jpg"));
        assert!(user_path.starts_with(&store.config.user_dir));
    }

    #[tokio::test]
    #[serial]
    async fn test_metallica_cover_download() {
        init_test_settings_db();
        let (mut store, _cache_temp, _user_temp) = create_test_store();
        let artist_name = "Metallica";

        // This test drives the same phases the unlocked orchestrator does --
        // decide, fetch, commit -- by hand, since the store no longer owns a
        // method that does all three under one lock. `next_image_step` reads
        // the shared settings database, so this needs `#[serial]` and its own
        // init like every other test that touches it -- a stray
        // `artist.image.Metallica` key left by an unrelated test, or a race
        // on first initialization, must not turn a network-optional test
        // into a hard failure.
        // Note: This requires internet connectivity and working cover art providers
        match store.next_image_step(artist_name) {
            ImageStep::AskProviders => {
                match best_provider_image(artist_name) {
                    Some(url) => match fetch_image(&url) {
                        Ok(bytes) => match store.commit_downloaded_image(artist_name, &bytes, "cover", ImageDestination::Cache) {
                            ArtistImageResult::Found { cache_path } => {
                                assert!(Path::new(&cache_path).exists(), "Downloaded image file should exist");

                                let metadata = fs::metadata(&cache_path).expect("Failed to get file metadata");
                                assert!(metadata.len() > 0, "Downloaded image should not be empty");
                                assert!(metadata.len() > 1024, "Image should be larger than 1KB");
                                assert!(metadata.len() < 10_000_000, "Image should be smaller than 10MB");

                                println!("Successfully downloaded Metallica cover: {} bytes", metadata.len());
                            }
                            ArtistImageResult::Error(e) => {
                                println!("Warning: Error storing Metallica cover: {} (this may be expected in test environment)", e);
                            }
                            ArtistImageResult::NotFound => panic!("commit_downloaded_image never returns NotFound"),
                        },
                        Err(e) => {
                            println!("Warning: Error downloading Metallica cover: {} (this may be expected in test environment)", e);
                        }
                    },
                    None => {
                        println!("Warning: No cover art found for Metallica (this may be expected in test environment)");
                    }
                }
            }
            other => {
                // A fresh artist with no selection should ask the providers,
                // but this is a network-optional test: tolerate any other
                // step rather than failing the build over test-environment
                // noise (a leftover settings key, auto-download disabled by
                // some other global default, and so on).
                println!("Warning: expected to ask the providers for Metallica, got {:?} (this may be expected in test environment)", other);
            }
        }
    }

    #[test]
    fn test_cache_invalidation() {
        let (mut store, _cache_temp, _user_temp) = create_test_store();
        let artist_name = "Cache Test Artist";
        
        // Use the sanitized name format
        let sanitized_name = crate::helpers::sanitize::filename_from_string(artist_name);
        
        // Create cache directory structure (cache_dir already includes 'artists')
        let cache_artist_dir = Path::new(&store.config.cache_dir).join(&sanitized_name);
        fs::create_dir_all(&cache_artist_dir).expect("Failed to create cache artist dir");
        
        // Create a dummy image
        let image_path = cache_artist_dir.join("cover.jpg");
        fs::write(&image_path, b"test image data").expect("Failed to write test image");
        
        // First call should find the image and cache the path
        match store.get_cached_image(artist_name) {
            ArtistImageResult::Found { cache_path } => {
                assert_eq!(cache_path, image_path.to_string_lossy());
                assert!(store.image_cache.contains_key(artist_name));
            },
            _ => panic!("Should have found the test image"),
        }
        
        // Remove the file
        fs::remove_file(&image_path).expect("Failed to remove test image");
        
        // Second call should detect the missing file and remove from cache
        match store.get_cached_image(artist_name) {
            ArtistImageResult::NotFound => {
                assert!(!store.image_cache.contains_key(artist_name));
            },
            _ => panic!("Should not have found the removed image"),
        }
    }

    #[test]
    fn test_download_prevention() {
        let (mut store, _cache_temp, _user_temp) = create_test_store();

        // Disable auto-download
        store.config.auto_download = false;

        let result = store.next_image_step("NonExistent Artist");
        match result {
            ImageStep::NotFound => {
                // This is expected when auto-download is disabled
            },
            _ => panic!("Should return NotFound when auto-download is disabled"),
        }
    }

    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        use image::{DynamicImage, RgbaImage};
        let img = DynamicImage::ImageRgba8(RgbaImage::from_pixel(w, h, image::Rgba([10, 120, 200, 255])));
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        buf.into_inner()
    }

    fn temp_user_dir() -> tempfile::TempDir { tempfile::TempDir::new().unwrap() }

    fn store_in(dir: &tempfile::TempDir) -> ArtistStore {
        ArtistStore::with_config(ArtistStoreConfig {
            cache_dir: dir.path().join("cache").to_string_lossy().into_owned(),
            user_dir: dir.path().join("user").to_string_lossy().into_owned(),
            enable_custom_images: true,
            auto_download: false,
        })
    }

    fn artist_dir(store: &ArtistStore, artist: &str) -> String {
        let path = store.get_artist_user_image_path(artist, "custom");
        std::path::Path::new(&path).parent().unwrap().to_string_lossy().into_owned()
    }

    fn write_file(path: &str, bytes: &[u8]) {
        let p = std::path::Path::new(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    #[test]
    fn an_upload_id_is_the_content_hash_and_is_stable() {
        let bytes = b"the same bytes";
        assert_eq!(upload_id(bytes), upload_id(bytes));
        assert_eq!(upload_id(bytes).len(), 32);
        assert!(upload_id(bytes).chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(upload_id(bytes), upload_id(b"other bytes"));
    }

    /// The extension comes from the bytes, never from what a client called the
    /// image: the serving route derives a content type from the file name.
    #[test]
    fn an_extension_is_sniffed_from_the_bytes() {
        assert_eq!(image_extension(&png_bytes(8, 8)), Some("png"));
        assert_eq!(image_extension(b"<html>not an image</html>"), None);
    }

    #[test]
    fn the_set_lists_the_downloads_and_the_uploads() {
        let store = store_in(&temp_user_dir());
        write_file(&store.get_artist_user_image_path("Artist", "custom"), &png_bytes(8, 8));
        let id = upload_id(&png_bytes(16, 16));
        write_file(&format!("{}/uploads/{}.png", artist_dir(&store, "Artist"), id), &png_bytes(16, 16));

        let images = store.artist_images("Artist");

        assert_eq!(images.len(), 2);
        let custom = images.iter().find(|i| i.id == "custom").expect("custom is a member");
        assert_eq!(custom.source, ArtistImageSource::Download);
        let upload = images.iter().find(|i| i.id == id).expect("the upload is a member");
        assert_eq!(upload.source, ArtistImageSource::Upload);
    }

    /// A stray file that is not named like an image we serve is omitted rather
    /// than failing the listing for the whole artist. Bytes are not consulted
    /// here: the walk runs on the player-event path, and the extension of an
    /// upload was sniffed from its bytes when it was written.
    #[test]
    fn a_file_that_is_not_named_like_an_image_is_omitted() {
        let store = store_in(&temp_user_dir());
        let uploads = format!("{}/uploads", artist_dir(&store, "Artist"));
        write_file(&format!("{}/.DS_Store", uploads), b"not an image");
        write_file(&format!("{}/notes.txt", uploads), b"not an image either");

        assert!(store.artist_images("Artist").is_empty());
    }

    /// The listing must not read every file whole just to classify it: an
    /// artist with a full set of multi-megabyte uploads would otherwise cost
    /// tens of megabytes of reads and allocation per request.
    #[test]
    fn classifying_the_set_does_not_read_the_files() {
        let store = store_in(&temp_user_dir());
        let uploads = format!("{}/uploads", artist_dir(&store, "Artist"));
        // Bytes that no sniffer would accept, under a name we wrote ourselves.
        write_file(&format!("{}/{}.png", uploads, "a".repeat(32)), b"header-only classification");

        assert_eq!(store.artist_images("Artist").len(), 1);
    }

    #[test]
    fn an_unknown_id_has_no_path() {
        let store = store_in(&temp_user_dir());
        assert_eq!(store.artist_image_path("Artist", "nope"), None);
    }

    #[test]
    fn an_upload_is_stored_under_its_own_hash() {
        let dir = temp_user_dir();
        let mut store = store_in(&dir);
        let bytes = png_bytes(16, 16);

        let id = store.store_uploaded_image("Artist", &bytes).expect("the upload is stored");

        assert_eq!(id, upload_id(&bytes));
        assert_eq!(store.artist_image_path("Artist", &id).map(|p| std::fs::read(p).unwrap()), Some(bytes));
    }

    /// A client that retries after a timeout must not end up with two copies.
    #[test]
    fn uploading_the_same_bytes_twice_yields_one_member() {
        let dir = temp_user_dir();
        let mut store = store_in(&dir);
        let bytes = png_bytes(16, 16);

        let first = store.store_uploaded_image("Artist", &bytes).unwrap();
        let second = store.store_uploaded_image("Artist", &bytes).unwrap();

        assert_eq!(first, second);
        assert_eq!(store.artist_images("Artist").len(), 1);
    }

    #[test]
    fn an_upload_past_the_cap_is_refused_and_says_so() {
        let dir = temp_user_dir();
        let mut store = store_in(&dir);
        for i in 0..MAX_UPLOADS_PER_ARTIST {
            store.store_uploaded_image("Artist", &png_bytes(16, 16 + i as u32)).unwrap();
        }

        let err = store
            .store_uploaded_image("Artist", &png_bytes(64, 64))
            .expect_err("the cap is enforced");

        assert!(err.contains(&MAX_UPLOADS_PER_ARTIST.to_string()), "the refusal names the cap: {}", err);
        assert_eq!(store.artist_images("Artist").len(), MAX_UPLOADS_PER_ARTIST);
    }

    /// Re-uploading bytes that are already stored is not a new member, so it
    /// must not be refused when the set is full.
    #[test]
    fn a_re_upload_is_allowed_when_the_set_is_full() {
        let dir = temp_user_dir();
        let mut store = store_in(&dir);
        let mut ids = Vec::new();
        for i in 0..MAX_UPLOADS_PER_ARTIST {
            ids.push(store.store_uploaded_image("Artist", &png_bytes(16, 16 + i as u32)).unwrap());
        }

        let again = store.store_uploaded_image("Artist", &png_bytes(16, 16)).expect("a re-upload is allowed");

        assert_eq!(again, ids[0]);
    }

    /// A re-upload repairs a member whose bytes are not the ones its name
    /// promises.
    ///
    /// The write can fail part way — a full disk is the realistic case — and
    /// leave a truncated file behind. Since the name is the hash of the bytes,
    /// nothing else will ever notice the mismatch, so re-uploading the same
    /// image has to be the repair. Writing through a temporary name and
    /// renaming is what makes that safe to do while a reader may be looking.
    #[test]
    fn a_re_upload_repairs_a_damaged_member() {
        let dir = temp_user_dir();
        let mut store = store_in(&dir);
        let bytes = png_bytes(16, 16);
        let id = store.store_uploaded_image("Artist", &bytes).unwrap();
        let path = store.artist_image_path("Artist", &id).expect("the member is stored");
        write_file(&path, b"truncated");

        let again = store.store_uploaded_image("Artist", &bytes).expect("a re-upload is allowed");

        assert_eq!(again, id);
        assert_eq!(
            std::fs::read(&path).unwrap(),
            bytes,
            "the damaged member should hold the uploaded bytes again"
        );
        assert_eq!(store.artist_images("Artist").len(), 1, "and still be one member");
    }

    /// A temporary file left behind by an interrupted atomic write must not
    /// appear in the set.
    ///
    /// `write_file_atomically` creates its temporary beside the destination
    /// and removes it on both failure paths, but a crash between create and
    /// rename leaves one there, so the listing has to exclude it on its own
    /// rather than by trusting the writer.
    #[test]
    fn a_leftover_temporary_file_is_not_a_member() {
        let dir = temp_user_dir();
        let mut store = store_in(&dir);
        let bytes = png_bytes(16, 16);
        let id = store.store_uploaded_image("Artist", &bytes).unwrap();
        let uploads = store.artist_uploads_dir_for_test("Artist");
        write_file(&format!("{}/.{}.png.1234.0.tmp", uploads, id), &bytes);

        let members = store.artist_images("Artist");

        assert_eq!(members.len(), 1, "only the renamed file is a member: {:?}", members);
        assert_eq!(members[0].id, id);
    }

    #[test]
    fn bytes_that_are_not_an_image_are_refused() {
        let dir = temp_user_dir();
        let mut store = store_in(&dir);
        assert!(store.store_uploaded_image("Artist", b"<html>").is_err());
    }

    #[test]
    fn deleting_a_member_removes_it_and_its_variants() {
        let dir = temp_user_dir();
        let mut store = store_in(&dir);
        let id = store.store_uploaded_image("Artist", &png_bytes(16, 16)).unwrap();
        let variant = format!("{}/{}@200.png", store.artist_uploads_dir_for_test("Artist"), id);
        write_file(&variant, &png_bytes(8, 8));

        store.delete_artist_image("Artist", &id).expect("the member is deleted");

        assert!(store.artist_image_path("Artist", &id).is_none());
        assert!(!std::path::Path::new(&variant).exists(), "the variant went with it");
    }

    #[test]
    fn deleting_an_unknown_member_is_an_error() {
        let dir = temp_user_dir();
        let mut store = store_in(&dir);
        assert!(store.delete_artist_image("Artist", "nope").is_err());
    }

    #[test]
    fn a_local_image_url_names_its_id() {
        let b64 = crate::helpers::url_encoding::encode_url_safe("The Beatles");
        let url = format!("/api/coverart/artist/{}/image/custom", b64);
        assert_eq!(local_image_id(&url, "The Beatles"), Some("custom".to_string()));
    }

    /// A URL for a different artist must not select an image for this one, and
    /// a remote host that merely ends in the same path is not local at all.
    #[test]
    fn a_url_for_another_artist_or_host_is_not_local() {
        let other = crate::helpers::url_encoding::encode_url_safe("Someone Else");
        assert_eq!(local_image_id(&format!("/api/coverart/artist/{}/image/custom", other), "The Beatles"), None);

        let b64 = crate::helpers::url_encoding::encode_url_safe("The Beatles");
        assert_eq!(
            local_image_id(&format!("https://evil.test/api/coverart/artist/{}/image/custom", b64), "The Beatles"),
            None
        );
        assert_eq!(local_image_id("https://example.test/cover.jpg", "The Beatles"), None);
    }

    #[test]
    #[serial]
    fn a_selected_upload_wins_over_an_existing_custom_image() {
        init_test_settings_db();
        let dir = temp_user_dir();
        let mut store = store_in(&dir);
        let artist = "Selection Winner Artist";
        write_file(&store.get_artist_user_image_path(artist, "custom"), &png_bytes(8, 8));
        let id = store.store_uploaded_image(artist, &png_bytes(16, 16)).unwrap();

        store.select_artist_image(artist, &id).expect("the upload is selectable");

        let ArtistImageResult::Found { cache_path } = store.get_cached_image(artist) else {
            panic!("an image should be found");
        };
        assert_eq!(Some(cache_path), store.artist_image_path(artist, &id));
        assert_eq!(store.selected_image_id(artist).as_deref(), Some(id.as_str()));
    }

    #[test]
    #[serial]
    fn deleting_the_selected_image_falls_back_to_the_chain() {
        init_test_settings_db();
        let dir = temp_user_dir();
        let mut store = store_in(&dir);
        let artist = "Selection Fallback Artist";
        let custom = store.get_artist_user_image_path(artist, "custom");
        write_file(&custom, &png_bytes(8, 8));
        let id = store.store_uploaded_image(artist, &png_bytes(16, 16)).unwrap();
        store.select_artist_image(artist, &id).unwrap();

        store.delete_artist_image(artist, &id).unwrap();

        assert_eq!(store.selected_image_id(artist), None);
        let ArtistImageResult::Found { cache_path } = store.get_cached_image(artist) else {
            panic!("the chain still finds the custom image");
        };
        assert_eq!(cache_path, custom);
    }

    #[test]
    #[serial]
    fn selecting_an_unknown_id_is_refused_and_changes_nothing() {
        init_test_settings_db();
        let dir = temp_user_dir();
        let mut store = store_in(&dir);
        let artist = "Selection Unknown Artist";
        assert!(store.select_artist_image(artist, "nope").is_err());
        assert_eq!(store.selected_image_id(artist), None);
    }

    /// The ordinary WebUI flow: a provider candidate is posted as a remote URL
    /// and the daemon downloads it to `custom.jpg`. `custom` is what is being
    /// served, so `custom` is what the listing must report as selected.
    #[test]
    #[serial]
    fn a_remote_selection_reports_custom_as_the_selected_member() {
        init_test_settings_db();
        let dir = temp_user_dir();
        let store = store_in(&dir);
        let artist = "Remote Selection Artist";
        write_file(&store.get_artist_user_image_path(artist, "custom"), &png_bytes(8, 8));
        crate::helpers::settingsdb::set_string(
            &format!("artist.image.{}", artist),
            "https://provider.test/portrait.jpg",
        )
        .unwrap();

        assert_eq!(store.selected_image_id(artist).as_deref(), Some("custom"));
        let (images, selected) = store.artist_images_with_selection(artist);
        assert_eq!(selected.as_deref(), Some("custom"));
        assert_eq!(images.len(), 1);
    }

    /// Until the download lands there is no `custom.jpg`, and a remote
    /// selection must not claim a member that does not exist.
    #[test]
    #[serial]
    fn a_remote_selection_with_no_custom_file_selects_nothing() {
        init_test_settings_db();
        let dir = temp_user_dir();
        let store = store_in(&dir);
        let artist = "Remote Selection Pending Artist";
        crate::helpers::settingsdb::set_string(
            &format!("artist.image.{}", artist),
            "https://provider.test/portrait.jpg",
        )
        .unwrap();

        assert_eq!(store.selected_image_id(artist), None);
    }

    /// A selection of ours whose file has since gone is a stale pointer, not a
    /// URL to fetch: handing `/api/coverart/...` to the HTTP client turns a
    /// recoverable miss into an error and leaves the artist with no picture.
    /// `next_image_step` must send this case to the providers, never to `Fetch`.
    #[test]
    #[serial]
    fn a_local_selection_whose_file_is_gone_is_not_fetched_over_http() {
        init_test_settings_db();
        let dir = temp_user_dir();
        let mut store = store_in(&dir);
        store.config.auto_download = true;
        let artist = "Vanished Selection Artist";
        let url = format!(
            "{}/coverart/artist/{}/image/{}",
            crate::constants::API_PREFIX,
            crate::helpers::url_encoding::encode_url_safe(artist),
            "a".repeat(32)
        );
        crate::helpers::settingsdb::set_string(&format!("artist.image.{}", artist), &url).unwrap();

        let step = store.next_image_step(artist);

        assert_eq!(
            step,
            ImageStep::AskProviders,
            "our own address must never reach the download path"
        );
    }

    /// A removal that fails must leave the artist exactly as it was. A
    /// directory sitting where `custom.jpg` belongs lists as a member and
    /// cannot be removed with `remove_file`, which is the failure to observe.
    #[test]
    #[serial]
    fn a_failed_removal_leaves_the_selection_alone() {
        init_test_settings_db();
        let dir = temp_user_dir();
        let mut store = store_in(&dir);
        let artist = "Failed Delete Artist";
        std::fs::create_dir_all(store.get_artist_user_image_path(artist, "custom")).unwrap();
        store.select_artist_image(artist, "custom").expect("custom lists as a member");

        assert!(store.delete_artist_image(artist, "custom").is_err(), "a directory cannot be removed");

        assert_eq!(
            store.selected_image_id(artist).as_deref(),
            Some("custom"),
            "the selection must survive a removal that did not happen"
        );
    }

    #[test]
    #[serial]
    fn next_image_step_is_ready_when_a_member_is_on_disk() {
        init_test_settings_db();
        let dir = temp_user_dir();
        let mut store = store_in(&dir);
        let artist = "On Disk Artist";
        let path = store.get_artist_user_image_path(artist, "custom");
        write_file(&path, &png_bytes(8, 8));

        assert_eq!(store.next_image_step(artist), ImageStep::Ready(path));
    }

    #[test]
    #[serial]
    fn next_image_step_is_not_found_when_auto_download_is_off() {
        init_test_settings_db();
        let dir = temp_user_dir();
        let mut store = store_in(&dir);
        store.config.auto_download = false;

        assert_eq!(store.next_image_step("No Auto Download Artist"), ImageStep::NotFound);
    }

    /// A stored selection that is not one of our own URLs is a remote image to
    /// fetch into the cache under `custom`, exactly as the WebUI's provider
    /// candidate flow expects.
    #[test]
    #[serial]
    fn next_image_step_is_fetch_for_a_remote_selection() {
        init_test_settings_db();
        let dir = temp_user_dir();
        let mut store = store_in(&dir);
        store.config.auto_download = true;
        let artist = "Remote Fetch Artist";
        let url = "https://provider.test/portrait.jpg".to_string();
        crate::helpers::settingsdb::set_string(&format!("artist.image.{}", artist), &url).unwrap();

        assert_eq!(
            store.next_image_step(artist),
            ImageStep::Fetch { url, image_type: "custom".to_string(), destination: ImageDestination::Cache }
        );
    }

    /// The earlier version of this test (and its user-directory sibling)
    /// committed against an artist with no selection and no memo, so the
    /// deferred-to answer coincidentally equalled the path just written --
    /// it would have passed unchanged against a blind
    /// `image_cache.insert(path)`, which is the bug the re-check in
    /// `commit_downloaded_image` exists to prevent. This one seeds a
    /// competing selection (a user `custom.jpg`, which precedence prefers
    /// over anything in the cache directory) and a stale memo entry naming a
    /// real, pre-existing cache file before committing a `Cache` download,
    /// so it only passes when the commit truly defers to what the artist
    /// resolves to now.
    #[test]
    #[serial]
    fn commit_downloaded_image_defers_to_a_selection_made_before_it_lands() {
        init_test_settings_db();
        let dir = temp_user_dir();
        let mut store = store_in(&dir);
        let artist = "Selection Wins Over Commit Artist";

        // The competing selection: user directory wins the precedence chain
        // over anything in the cache, with no settings key required.
        let selected_path = store.get_artist_user_image_path(artist, "custom");
        write_file(&selected_path, &png_bytes(4, 4));

        // A stale memo entry naming a real, pre-existing cache file --
        // standing in for whatever a concurrent lookup resolved to before
        // the selection above was in place. It has to exist on disk, or
        // `get_cached_image`'s own staleness check would discard it before
        // this test could tell the two code paths apart.
        let stale_path = store.get_artist_image_path(artist, "cover");
        write_file(&stale_path, b"old cover bytes");
        store.image_cache.insert(artist.to_string(), stale_path.clone());

        let bytes = png_bytes(8, 8);
        let result = store.commit_downloaded_image(artist, &bytes, "cover", ImageDestination::Cache);

        let ArtistImageResult::Found { cache_path } = result else { panic!("the write should succeed: {:?}", result) };
        assert_eq!(cache_path, selected_path, "the selection must win over both the stale memo and the file just committed");
        assert_eq!(
            store.image_cache.get(artist),
            Some(&selected_path),
            "the memo must agree with the selection, not the stale entry or the new file"
        );
    }

    #[test]
    #[serial]
    fn commit_downloaded_image_writes_to_the_user_directory_and_populates_the_memo() {
        init_test_settings_db();
        let dir = temp_user_dir();
        let mut store = store_in(&dir);
        let artist = "User Commit Artist";
        let bytes = png_bytes(8, 8);

        let result = store.commit_downloaded_image(artist, &bytes, "custom", ImageDestination::UserDirectory);

        let ArtistImageResult::Found { cache_path } = result else { panic!("the write should succeed: {:?}", result) };
        assert_eq!(cache_path, store.get_artist_user_image_path(artist, "custom"));
        assert_eq!(std::fs::read(&cache_path).unwrap(), bytes);
        assert_eq!(store.image_cache.get(artist), Some(&cache_path));
    }

    /// Pins the begin/finish mechanics in isolation: a claim refuses a second
    /// claim while it stands, and releasing it lets a new one through. This
    /// does not by itself prove the flag was ever dead -- a single-threaded
    /// test like this one would have passed against the old, lock-held
    /// `download_and_cache_image` too, since that method's check-then-insert
    /// was the same shape. What made the flag dead was lock scope: it was
    /// read and written under the same mutex that serialised the whole
    /// download, so a second caller could never reach the check while it was
    /// set. No unit test in this module observes that; it took two threads
    /// actually racing under the real lock.
    #[test]
    fn begin_download_refuses_a_second_claim_until_finish_download_releases_it() {
        let dir = temp_user_dir();
        let mut store = store_in(&dir);
        let artist = "In Flight Artist";

        assert!(store.begin_download(artist), "the first claim succeeds");
        assert!(!store.begin_download(artist), "a second claim must be refused while one is in flight");

        store.finish_download(artist);

        assert!(store.begin_download(artist), "finish_download must release the claim");
    }

    /// `begin_download`/`finish_download` are plain methods, not RAII on
    /// their own, so nothing but `DownloadClaim` stops a panic between the
    /// two from stranding the claim forever -- `parking_lot` mutexes do not
    /// poison, so there is no other mechanism that would ever clear it.
    /// `best_provider_image` and `fetch_image` both run in that window in
    /// production, and both can realistically panic (a provider fanning out
    /// to third parties, a client-supplied URL), so this has to hold even
    /// when the code between the claim and its ordinary release point
    /// unwinds.
    #[test]
    fn a_panic_between_claim_and_release_still_frees_the_claim() {
        let dir = temp_user_dir();
        let store = Arc::new(Mutex::new(store_in(&dir)));
        let artist = "Panicking Download Artist";

        assert!(store.lock().begin_download(artist), "the claim succeeds");

        let guarded_store = store.clone();
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _claim = DownloadClaim::new(guarded_store, artist.to_string());
            panic!("simulated failure between the claim and its ordinary release");
        }));

        assert!(unwound.is_err(), "the panic must propagate, not be swallowed");
        assert!(
            store.lock().begin_download(artist),
            "the claim must be released by unwinding through the guard, not left stranded"
        );
    }
}
