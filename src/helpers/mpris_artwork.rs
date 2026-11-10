//! Cover art that an MPRIS player names as a local file.
//!
//! An `mpris:artUrl` of `file:///…`, or a bare absolute path as
//! shairport-sync 5.6 sends, is useless to a browser. The resolver imports
//! such a file into the image cache and returns the URL clients load it from.
//! Remote `http(s)` URLs pass through unchanged.
//!
//! Anything on the bus can claim any `artUrl`, so local files are imported
//! only from configured directories (after resolving symlinks), only if they
//! are real images, and only up to [`MAX_ARTWORK_BYTES`]. Otherwise an MPRIS
//! client could get an arbitrary readable file served over HTTP.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use log::debug;
use parking_lot::Mutex;

use crate::helpers::imagecache;

/// Largest local artwork file that is imported.
pub const MAX_ARTWORK_BYTES: u64 = 10 * 1024 * 1024;

/// How long imported artwork stays in the image cache.
const ARTWORK_EXPIRY: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Where imported artwork goes.
pub trait ArtworkStore: Send + Sync {
    /// Store `data` under `key` (`mpris/<md5>.<ext>`) and return the URL
    /// clients use to load it.
    fn store(&self, key: &str, data: &[u8]) -> Result<String, String>;
}

/// The image cache, served under `/api/imagecache/`.
pub struct ImageCacheArtworkStore;

impl ArtworkStore for ImageCacheArtworkStore {
    fn store(&self, key: &str, data: &[u8]) -> Result<String, String> {
        imagecache::store_image_with_expiry(key, data, Some(SystemTime::now() + ARTWORK_EXPIRY))?;
        Ok(format!("/api/imagecache/{}", key))
    }
}

/// File extension for image data, recognised by its magic bytes.
pub fn image_extension(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if data.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("png")
    } else if data.starts_with(b"GIF8") {
        Some("gif")
    } else if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

/// The last file resolved: (raw art URL, file length, mtime, result).
type Memo = Option<(String, u64, SystemTime, Option<String>)>;

/// Turns an `mpris:artUrl` into a URL a browser can load.
pub struct ArtworkResolver {
    allowed_dirs: Vec<PathBuf>,
    store: Arc<dyn ArtworkStore>,
    memo: Mutex<Memo>,
}

impl ArtworkResolver {
    pub fn new(allowed_dirs: Vec<PathBuf>, store: Arc<dyn ArtworkStore>) -> Self {
        Self { allowed_dirs, store, memo: Mutex::new(None) }
    }

    /// The URL to show for `art_url`, or `None` if there is no usable artwork.
    ///
    /// Only `file://` URLs and absolute paths are treated as local files.
    /// Anything else (`http(s)`, `data:` …) is passed through unchanged.
    pub fn resolve(&self, art_url: &str) -> Option<String> {
        let art_url = art_url.trim();
        if art_url.is_empty() {
            return None;
        }
        let Some(path) = local_path(art_url) else {
            if is_file_url(art_url) {
                // file:// with no usable path, e.g. "file://(null)".
                return None;
            }
            return Some(art_url.to_string());
        };
        let canonical = match std::fs::canonicalize(&path) {
            Ok(p) => p,
            Err(e) => {
                debug!("MPRIS artwork {} not usable: {}", path.display(), e);
                return None;
            }
        };
        if !self.is_allowed(&canonical) {
            debug!("MPRIS artwork {} is outside the allowed directories", canonical.display());
            return None;
        }
        // Open once and check what was opened: a file swapped between a path
        // check and the read cannot slip through, a FIFO cannot block the
        // read, and the read is bounded whatever the file does meanwhile.
        let mut file = match open_no_follow(&canonical) {
            Ok(f) => f,
            Err(e) => {
                debug!("MPRIS artwork {} cannot be opened: {}", canonical.display(), e);
                return None;
            }
        };
        let meta = file.metadata().ok()?;
        if !meta.is_file() || meta.len() > MAX_ARTWORK_BYTES {
            debug!("MPRIS artwork {} is not a regular file or too large ({} bytes)", canonical.display(), meta.len());
            return None;
        }
        let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);

        let mut memo = self.memo.lock();
        if let Some((url, len, time, result)) = memo.as_ref() {
            if url == art_url && *len == meta.len() && *time == mtime {
                return result.clone();
            }
        }
        let mut data = Vec::new();
        if (&mut file).take(MAX_ARTWORK_BYTES + 1).read_to_end(&mut data).is_err() {
            return None;
        }
        let result = self.import(&canonical, &data);
        *memo = Some((art_url.to_string(), meta.len(), mtime, result.clone()));
        result
    }

    fn is_allowed(&self, canonical: &Path) -> bool {
        self.allowed_dirs.iter().any(|dir| {
            std::fs::canonicalize(dir).map(|d| canonical.starts_with(&d)).unwrap_or(false)
        })
    }

    fn import(&self, canonical: &Path, data: &[u8]) -> Option<String> {
        if data.len() as u64 > MAX_ARTWORK_BYTES {
            return None;
        }
        let Some(ext) = image_extension(data) else {
            debug!("MPRIS artwork {} is not a recognised image", canonical.display());
            return None;
        };
        let key = format!("mpris/{:x}.{}", md5::compute(data), ext);
        match self.store.store(&key, data) {
            Ok(url) => Some(url),
            Err(e) => {
                debug!("Could not store MPRIS artwork {}: {}", canonical.display(), e);
                None
            }
        }
    }
}

fn is_file_url(art_url: &str) -> bool {
    art_url.as_bytes().get(..7).is_some_and(|p| p.eq_ignore_ascii_case(b"file://"))
}

/// Open without following a final symlink and without blocking on a FIFO.
fn open_no_follow(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}

/// The local path an art URL names, if it names one.
fn local_path(art_url: &str) -> Option<PathBuf> {
    let raw = if is_file_url(art_url) {
        let rest = &art_url[7..];
        rest.strip_prefix("localhost").unwrap_or(rest)
    } else if art_url.starts_with('/') {
        art_url
    } else {
        return None;
    };
    let decoded = urlencoding::decode(raw).ok()?;
    let path = PathBuf::from(decoded.as_ref());
    if path.is_absolute() {
        Some(path)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F'];
    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    /// Records how often it was asked to store, and answers like the image cache.
    struct RecordingStore {
        calls: AtomicUsize,
    }

    impl RecordingStore {
        fn new() -> Arc<Self> {
            Arc::new(Self { calls: AtomicUsize::new(0) })
        }
        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl ArtworkStore for RecordingStore {
        fn store(&self, key: &str, _data: &[u8]) -> Result<String, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(format!("/api/imagecache/{}", key))
        }
    }

    fn resolver(dir: &Path, store: &Arc<RecordingStore>) -> ArtworkResolver {
        ArtworkResolver::new(vec![dir.to_path_buf()], store.clone() as Arc<dyn ArtworkStore>)
    }

    fn expected_url(data: &[u8], ext: &str) -> String {
        format!("/api/imagecache/mpris/{:x}.{}", md5::compute(data), ext)
    }

    #[test]
    fn non_ascii_urls_do_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecordingStore::new();
        let r = resolver(dir.path(), &store);
        // byte 7 falls inside the second '€'
        assert_eq!(r.resolve("ab€€"), Some("ab€€".to_string()));
        assert_eq!(r.resolve("file€€x"), Some("file€€x".to_string()));
        assert_eq!(r.resolve("file://€/x.jpg"), None);
        assert_eq!(store.calls(), 0);
    }

    #[test]
    fn http_urls_pass_through() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecordingStore::new();
        let r = resolver(dir.path(), &store);
        assert_eq!(r.resolve("https://x/y.jpg"), Some("https://x/y.jpg".to_string()));
        assert_eq!(r.resolve("http://x/y.jpg"), Some("http://x/y.jpg".to_string()));
        assert_eq!(store.calls(), 0);
    }

    #[test]
    fn file_url_in_allowed_dir_is_imported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cover.jpg");
        std::fs::write(&path, JPEG).unwrap();
        let store = RecordingStore::new();
        let r = resolver(dir.path(), &store);
        let url = format!("file://{}", path.display());
        assert_eq!(r.resolve(&url), Some(expected_url(JPEG, "jpg")));
    }

    #[test]
    fn bare_path_is_imported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cover.jpg");
        std::fs::write(&path, JPEG).unwrap();
        let store = RecordingStore::new();
        let r = resolver(dir.path(), &store);
        assert_eq!(r.resolve(&path.display().to_string()), Some(expected_url(JPEG, "jpg")));
    }

    #[test]
    fn percent_encoded_path_is_decoded() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("cover a.png"), PNG).unwrap();
        let store = RecordingStore::new();
        let r = resolver(dir.path(), &store);
        let url = format!("file://{}/cover%20a.png", dir.path().display());
        assert_eq!(r.resolve(&url), Some(expected_url(PNG, "png")));
    }

    #[test]
    fn other_schemes_pass_through() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecordingStore::new();
        let r = resolver(dir.path(), &store);
        let data = "data:image/png;base64,iVBORw0KGgo=";
        assert_eq!(r.resolve(data), Some(data.to_string()));
        assert_eq!(r.resolve("HTTPS://X/y.jpg"), Some("HTTPS://X/y.jpg".to_string()));
        assert_eq!(store.calls(), 0);
    }

    #[test]
    fn file_localhost_url_is_imported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cover.jpg");
        std::fs::write(&path, JPEG).unwrap();
        let store = RecordingStore::new();
        let r = resolver(dir.path(), &store);
        assert_eq!(r.resolve(&format!("file://localhost{}", path.display())), Some(expected_url(JPEG, "jpg")));
    }

    #[test]
    fn fifo_is_refused_without_blocking() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pipe.jpg");
        let c = std::ffi::CString::new(path.display().to_string()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o644) }, 0);
        let store = RecordingStore::new();
        let r = resolver(dir.path(), &store);
        let (tx, rx) = std::sync::mpsc::channel();
        let url = path.display().to_string();
        std::thread::spawn(move || { tx.send(r.resolve(&url)).ok(); });
        let got = rx.recv_timeout(std::time::Duration::from_secs(3)).expect("resolve blocked on a FIFO");
        assert_eq!(got, None);
    }

    #[test]
    fn null_and_empty_give_none() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecordingStore::new();
        let r = resolver(dir.path(), &store);
        assert_eq!(r.resolve("file://(null)"), None);
        assert_eq!(r.resolve(""), None);
        assert_eq!(store.calls(), 0);
    }

    #[test]
    fn file_outside_allowed_dirs_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let path = other.path().join("cover.jpg");
        std::fs::write(&path, JPEG).unwrap();
        let store = RecordingStore::new();
        let r = resolver(dir.path(), &store);
        assert_eq!(r.resolve(&format!("file://{}", path.display())), None);
        assert_eq!(store.calls(), 0);
    }

    #[test]
    fn symlink_escaping_allowed_dir_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let target = other.path().join("secret.jpg");
        std::fs::write(&target, JPEG).unwrap();
        let link = dir.path().join("evil.jpg");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let store = RecordingStore::new();
        let r = resolver(dir.path(), &store);
        assert_eq!(r.resolve(&link.display().to_string()), None);
        assert_eq!(store.calls(), 0);
    }

    #[test]
    fn non_image_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.jpg");
        std::fs::write(&path, b"hello").unwrap();
        let store = RecordingStore::new();
        let r = resolver(dir.path(), &store);
        assert_eq!(r.resolve(&path.display().to_string()), None);
        assert_eq!(store.calls(), 0);
    }

    #[test]
    fn oversized_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.jpg");
        let mut data = JPEG.to_vec();
        data.resize(MAX_ARTWORK_BYTES as usize + 1, 0);
        std::fs::write(&path, &data).unwrap();
        let store = RecordingStore::new();
        let r = resolver(dir.path(), &store);
        assert_eq!(r.resolve(&path.display().to_string()), None);
        assert_eq!(store.calls(), 0);
    }

    #[test]
    fn unchanged_file_is_read_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cover.jpg");
        std::fs::write(&path, JPEG).unwrap();
        let store = RecordingStore::new();
        let r = resolver(dir.path(), &store);
        let url = path.display().to_string();
        let first = r.resolve(&url);
        let second = r.resolve(&url);
        assert_eq!(first, second);
        assert_eq!(store.calls(), 1);

        let mut changed = JPEG.to_vec();
        changed.extend_from_slice(b"different-and-longer");
        std::fs::write(&path, &changed).unwrap();
        assert_eq!(r.resolve(&url), Some(expected_url(&changed, "jpg")));
        assert_eq!(store.calls(), 2);
    }

    #[test]
    fn image_extension_recognises_formats() {
        assert_eq!(image_extension(JPEG), Some("jpg"));
        assert_eq!(image_extension(PNG), Some("png"));
        assert_eq!(image_extension(b"GIF89a...."), Some("gif"));
        assert_eq!(image_extension(b"RIFF\x00\x00\x00\x00WEBPVP8 "), Some("webp"));
        assert_eq!(image_extension(b"RIFF\x00\x00\x00\x00WAVEfmt "), None);
        assert_eq!(image_extension(b""), None);
    }
}
