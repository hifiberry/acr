use clap::{Parser, Subcommand};
use log::{info, warn};
use acr_store::attributecache::{self, AttributeCache, CacheEntry};
use audiocontrol_metadata::artistsplitter::ARTIST_SPLIT_CACHE_PREFIX;
use audiocontrol_metadata::musicbrainz::{ARTIST_MBID_CACHE_PREFIX, ARTIST_NOT_FOUND_CACHE_PREFIX};
use audiocontrol_metadata::image_meta::IMAGE_META_CACHE_PREFIX;
use std::path::{Path, PathBuf};
use chrono::DateTime;

#[derive(Parser)]
#[command(name = "audiocontrol_dump_cache")]
#[command(about = "A tool to manage AudioControl cache database")]
#[command(long_about = None)]
struct Cli {
    /// Path to the cache database directory
    #[arg(short, long, value_name = "DIR")]
    cache_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List cache entries
    List {
        /// Filter entries by prefix (e.g., "artist::", "theaudiodb::")
        #[arg(short, long)]
        prefix: Option<String>,
        
        /// Show detailed information including size and timestamps
        #[arg(short, long)]
        detailed: bool,

        /// Limit number of results
        #[arg(short, long)]
        limit: Option<usize>,

        /// Show artist MusicBrainz data (shortcut for --prefix "artist::mbid")
        #[arg(long)]
        artistmbid: bool,

        /// Show image metadata (shortcut for --prefix "image_meta:")
        #[arg(long)]
        imagemeta: bool,

        /// Show artist split cache (shortcut for --prefix "artist::split")
        #[arg(long)]
        artistsplit: bool,

        /// Show MusicBrainz negative cache (shortcut for --prefix "artist::not_found")
        #[arg(long)]
        artistnotfound: bool,
    },
    /// Clean cache entries
    Clean {
        /// Remove entries matching this prefix
        #[arg(short, long)]
        prefix: Option<String>,
        
        /// Remove all entries (use with caution!)
        #[arg(long)]
        all: bool,

        /// Remove entries older than specified days
        #[arg(long)]
        older_than_days: Option<u64>,

        /// Dry run - show what would be deleted without actually deleting
        #[arg(long)]
        dry_run: bool,

        /// Clean artist MusicBrainz data (shortcut for --prefix "artist::mbid")
        #[arg(long)]
        artistmbid: bool,

        /// Clean image metadata (shortcut for --prefix "image_meta:")
        #[arg(long)]
        imagemeta: bool,

        /// Clean artist split cache (shortcut for --prefix "artist::split")
        #[arg(long)]
        artistsplit: bool,

        /// Clean MusicBrainz negative cache (shortcut for --prefix "artist::not_found")
        #[arg(long)]
        artistnotfound: bool,
    },
    /// Show cache statistics
    Stats {
        /// Group statistics by prefix
        #[arg(short, long)]
        by_prefix: bool,
    },
}

fn determine_prefix(prefix: Option<&str>, artistmbid: bool, imagemeta: bool, artistsplit: bool, artistnotfound: bool) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let shortcut_count = [artistmbid, imagemeta, artistsplit, artistnotfound].iter().filter(|&&x| x).count();
    
    if shortcut_count > 1 {
        return Err("Cannot specify multiple shortcut options (--artistmbid, --imagemeta, --artistsplit, --artistnotfound) at once".into());
    }
    
    if prefix.is_some() && shortcut_count > 0 {
        return Err("Cannot specify both --prefix and shortcut options (--artistmbid, --imagemeta, --artistsplit, --artistnotfound)".into());
    }
    
    if artistmbid {
        Ok(Some(ARTIST_MBID_CACHE_PREFIX.trim_end_matches("::").to_string()))
    } else if imagemeta {
        Ok(Some(IMAGE_META_CACHE_PREFIX.trim_end_matches("::").to_string()))
    } else if artistsplit {
        Ok(Some(ARTIST_SPLIT_CACHE_PREFIX.trim_end_matches("::").to_string()))
    } else if artistnotfound {
        Ok(Some(ARTIST_NOT_FOUND_CACHE_PREFIX.trim_end_matches("::").to_string()))
    } else {
        Ok(prefix.map(|s| s.to_string()))
    }
}

/// Where this tool looks when `--cache-dir` is not given.
///
/// The metadata daemon's directory, not the player daemon's. Every prefix the
/// tool has a shortcut for -- `artist::mbid`, `image_meta:`, `artist::split`,
/// `artist::not_found` -- is written by the metadata daemon and by nothing
/// else, so `acr_store`'s own default (`/var/lib/audiocontrol/cache`, the
/// player daemon's attribute cache) held none of them once the two processes
/// split, and every invocation without the flag listed nothing at all and
/// exited 0.
///
/// Must agree with `services.datastore.attribute_cache.dbfile` in
/// `configs/metadata.json`; the test below reads that file and pins the two
/// together rather than trusting this comment.
const DEFAULT_CACHE_DIR: &str = "/var/lib/audiocontrol/metadata";

/// The directory to open, or a message explaining why there is none.
///
/// An explicit `--cache-dir` is taken as given -- it is how a cache copied off
/// a device gets inspected, and it may legitimately not exist yet. The default
/// is checked instead of assumed, because `AttributeCache` *creates* the
/// database it is pointed at: without this the tool would silently make an
/// empty cache, report that it holds nothing, exit 0, and leave the file
/// behind. An empty answer from a diagnostic tool is worse than a refusal.
fn resolve_cache_dir(explicit: Option<PathBuf>, default_dir: &Path) -> Result<PathBuf, String> {
    if let Some(dir) = explicit {
        return Ok(dir);
    }
    if default_dir.join("attributes.db").is_file() {
        return Ok(default_dir.to_path_buf());
    }
    Err(format!(
        "no cache database at {}; pass --cache-dir DIR to name the directory to read \
         (the metadata daemon's cache is configured as services.datastore.attribute_cache.dbfile \
         in /etc/audiocontrol/metadata.json, the player daemon's in audiocontrol.json)",
        default_dir.join("attributes.db").display()
    ))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();

    // The cache has to be told where it lives before anything reads it: the
    // library no longer carries a default path, so that neither daemon can
    // open the other's database by accident. This tool keeps its own default,
    // which is where the metadata daemon puts the cache -- and refuses rather
    // than reporting an empty cache when nothing is there to read.
    let cache_dir = resolve_cache_dir(cli.cache_dir, Path::new(DEFAULT_CACHE_DIR))?;
    info!("Using cache directory: {}", cache_dir.display());
    AttributeCache::initialize_global(&cache_dir)?;

    match &cli.command {
        Commands::List { prefix, detailed, limit, artistmbid, imagemeta, artistsplit, artistnotfound } => {
            let effective_prefix = determine_prefix(prefix.as_deref(), *artistmbid, *imagemeta, *artistsplit, *artistnotfound)?;
            list_cache_entries(effective_prefix.as_deref(), *detailed, *limit)?;
        }
        Commands::Clean { prefix, all, older_than_days, dry_run, artistmbid, imagemeta, artistsplit, artistnotfound } => {
            let effective_prefix = determine_prefix(prefix.as_deref(), *artistmbid, *imagemeta, *artistsplit, *artistnotfound)?;
            clean_cache_entries(effective_prefix.as_deref(), *all, *older_than_days, *dry_run)?;
        }
        Commands::Stats { by_prefix } => {
            show_cache_stats(*by_prefix)?;
        }
    }

    Ok(())
}

fn list_cache_entries(prefix: Option<&str>, detailed: bool, limit: Option<usize>) -> Result<(), Box<dyn std::error::Error>> {
    if detailed {
        let entries = attributecache::list_entries(prefix)?;
        let entries_to_show = if let Some(limit) = limit {
            &entries[..entries.len().min(limit)]
        } else {
            &entries
        };

        if entries_to_show.is_empty() {
            info!("No cache entries found{}", 
                  prefix.map(|p| format!(" with prefix '{}'", p)).unwrap_or_default());
            return Ok(());
        }

        println!("Cache Entries ({}{})", 
                 entries_to_show.len(),
                 if entries.len() > entries_to_show.len() { 
                     format!(" of {} total", entries.len()) 
                 } else { 
                     String::new() 
                 });
        println!("{:-<120}", "");
        println!("{:<60} {:>10} {:>20} {:>20}", "Key", "Size", "Created", "Updated");
        println!("{:-<120}", "");

        for entry in entries_to_show {
            let created = DateTime::from_timestamp(entry.created_at, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            
            let updated = DateTime::from_timestamp(entry.updated_at, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "Unknown".to_string());

            let size_str = format_size(entry.size_bytes);
            
            println!("{:<60} {:>10} {:>20} {:>20}", 
                     truncate_key(&entry.key, 60), 
                     size_str, 
                     created, 
                     updated);
        }
    } else {
        let keys = attributecache::list_keys(prefix)?;
        let keys_to_show = if let Some(limit) = limit {
            &keys[..keys.len().min(limit)]
        } else {
            &keys
        };

        if keys_to_show.is_empty() {
            info!("No cache entries found{}", 
                  prefix.map(|p| format!(" with prefix '{}'", p)).unwrap_or_default());
            return Ok(());
        }

        println!("Cache Keys ({}{})", 
                 keys_to_show.len(),
                 if keys.len() > keys_to_show.len() { 
                     format!(" of {} total", keys.len()) 
                 } else { 
                     String::new() 
                 });
        println!("{:-<80}", "");

        for key in keys_to_show {
            println!("{}", key);
        }
    }

    Ok(())
}

fn clean_cache_entries(prefix: Option<&str>, all: bool, older_than_days: Option<u64>, dry_run: bool) -> Result<(), Box<dyn std::error::Error>> {
    if all && prefix.is_some() {
        return Err("Cannot specify both --all and --prefix options".into());
    }

    // `--all` clears the cache outright, so an age given with it would be
    // silently dropped and far more deleted than was asked for.
    if all && older_than_days.is_some() {
        return Err("Cannot specify both --all and --older-than-days options".into());
    }

    if !all && prefix.is_none() && older_than_days.is_none() {
        return Err("Must specify either --all, --prefix, or --older-than-days".into());
    }

    if all {
        if dry_run {
            let entries = attributecache::list_entries(None)?;
            println!("Would delete {} cache entries (dry run)", entries.len());
            return Ok(());
        }

        warn!("Clearing ALL cache entries!");
        attributecache::clear()?;
        info!("All cache entries cleared");
        return Ok(());
    }

    // Age is checked before the prefix-only branch, so that a prefix given
    // together with an age narrows the age sweep instead of replacing it:
    // `--prefix X --older-than-days N` used to delete everything under X
    // whatever its age.
    if let Some(days) = older_than_days {
        let entries = attributecache::list_entries(prefix)?;
        let old = entries_older_than(&entries, days, chrono::Utc::now().timestamp());
        let scope = prefix.map(|p| format!(" with prefix '{}'", p)).unwrap_or_default();

        if dry_run {
            println!("Would delete {} cache entries{} older than {} days (dry run)", old.len(), scope, days);
            print_sample(&old);
            return Ok(());
        }

        let mut deleted = 0;
        for entry in &old {
            if attributecache::remove(&entry.key)? {
                deleted += 1;
            }
        }
        info!("Deleted {} cache entries{} older than {} days", deleted, scope, days);
        return Ok(());
    }

    if let Some(prefix) = prefix {
        if dry_run {
            let entries = attributecache::list_entries(Some(prefix))?;
            println!("Would delete {} cache entries with prefix '{}' (dry run)", entries.len(), prefix);
            print_sample(&entries.iter().collect::<Vec<_>>());
            return Ok(());
        }

        let deleted = attributecache::remove_by_prefix(prefix)?;
        info!("Deleted {} cache entries with prefix '{}'", deleted, prefix);
        return Ok(());
    }

    Ok(())
}

/// The first few keys of what a dry run would delete.
fn print_sample(entries: &[&CacheEntry]) {
    for entry in &entries[..entries.len().min(10)] {
        println!("  - {}", entry.key);
    }
    if entries.len() > 10 {
        println!("  ... and {} more", entries.len() - 10);
    }
}

/// The entries last written more than `days` days before `now`.
///
/// Judged on `updated_at` rather than `created_at`: an entry that was
/// refreshed yesterday is not old, whenever its key first appeared. This is
/// the age the caller asked for, which is not the cache's own configured
/// maximum -- `attributecache::cleanup` applies that one, and was what this
/// option ran before whatever number it was given.
fn entries_older_than(entries: &[CacheEntry], days: u64, now: i64) -> Vec<&CacheEntry> {
    let age_seconds = i64::try_from(days.saturating_mul(86_400)).unwrap_or(i64::MAX);
    let cutoff = now.saturating_sub(age_seconds);
    entries.iter().filter(|entry| entry.updated_at < cutoff).collect()
}

fn show_cache_stats(by_prefix: bool) -> Result<(), Box<dyn std::error::Error>> {
    let entries = attributecache::list_entries(None)?;
    
    if entries.is_empty() {
        info!("Cache is empty");
        return Ok(());
    }

    let total_size: usize = entries.iter().map(|e| e.size_bytes).sum();
    let total_count = entries.len();

    println!("Cache Statistics");
    println!("{:-<50}", "");
    println!("Total entries: {}", total_count);
    println!("Total size: {}", format_size(total_size));
    
    if let (Some(oldest), Some(newest)) = (
        entries.iter().min_by_key(|e| e.created_at),
        entries.iter().max_by_key(|e| e.created_at)
    ) {
        let oldest_date = DateTime::from_timestamp(oldest.created_at, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        let newest_date = DateTime::from_timestamp(newest.created_at, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        
        println!("Oldest entry: {}", oldest_date);
        println!("Newest entry: {}", newest_date);
    }

    if by_prefix {
        let mut prefix_stats = std::collections::HashMap::new();
        
        for entry in &entries {
            let prefix = extract_prefix(&entry.key);
            let stats = prefix_stats.entry(prefix).or_insert((0, 0));
            stats.0 += 1;
            stats.1 += entry.size_bytes;
        }

        let mut sorted_prefixes: Vec<_> = prefix_stats.iter().collect();
        sorted_prefixes.sort_by_key(|(_, (count, _))| std::cmp::Reverse(*count));

        println!("\nBy Prefix:");
        println!("{:-<50}", "");
        println!("{:<30} {:>10} {:>10}", "Prefix", "Count", "Size");
        println!("{:-<50}", "");

        for (prefix, (count, size)) in sorted_prefixes {
            println!("{:<30} {:>10} {:>10}", prefix, count, format_size(*size));
        }
    }

    Ok(())
}

fn extract_prefix(key: &str) -> String {
    if let Some(pos) = key.find("::") {
        let prefix_end = pos + 2;
        if let Some(next_pos) = key[prefix_end..].find("::") {
            key[..prefix_end + next_pos + 2].to_string()
        } else {
            key[..prefix_end].to_string()
        }
    } else {
        "other".to_string()
    }
}

fn format_size(bytes: usize) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} {}", bytes, UNITS[unit_index])
    } else {
        format!("{:.1} {}", size, UNITS[unit_index])
    }
}

/// Shorten `key` to at most `max_len` characters, marking the cut.
///
/// Counted and cut in characters, not bytes: keys carry artist names, and a
/// byte slice that lands inside a multibyte character panics.
fn truncate_key(key: &str, max_len: usize) -> String {
    if key.chars().count() <= max_len {
        key.to_string()
    } else {
        let kept: String = key.chars().take(max_len.saturating_sub(3)).collect();
        format!("{}...", kept)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `DEFAULT_CACHE_DIR` is the directory holding the *metadata* daemon's
    /// attribute cache, read out of the shipped configuration rather than
    /// restated here.
    ///
    /// Every prefix this tool knows a shortcut for -- `artist::mbid`,
    /// `image_meta:`, `artist::split`, `artist::not_found` -- is written only
    /// by the metadata daemon. Pointed at the player daemon's cache the tool
    /// answers every question with an empty list and exit 0, which is the one
    /// failure mode a diagnostic tool must not have.
    #[test]
    fn default_cache_dir_is_the_metadata_daemons() {
        let config = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../configs/metadata.json");
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&config).expect("configs/metadata.json"))
                .expect("configs/metadata.json is not valid JSON");
        let dbfile = json["services"]["datastore"]["attribute_cache"]["dbfile"]
            .as_str()
            .expect("services.datastore.attribute_cache.dbfile");

        assert_eq!(
            Path::new(dbfile).parent().unwrap(),
            Path::new(DEFAULT_CACHE_DIR),
            "the default cache directory must be the one configs/metadata.json puts attributes.db in"
        );
    }

    fn entry(key: &str, updated_at: i64) -> CacheEntry {
        CacheEntry {
            key: key.to_string(),
            size_bytes: 1,
            created_at: 0,
            updated_at,
            expires_at: None,
        }
    }

    /// The number given is the age applied -- not the cache's configured
    /// maximum, and not "everything".
    #[test]
    fn older_than_days_selects_by_the_age_it_was_given() {
        let day = 86_400;
        let now = 100 * day;
        let entries = vec![
            entry("artist::mbid::old", now - 10 * day),
            entry("artist::mbid::edge", now - 7 * day),
            entry("artist::mbid::fresh", now - day),
        ];

        let keys = |days| -> Vec<String> {
            entries_older_than(&entries, days, now).iter().map(|e| e.key.clone()).collect()
        };

        assert_eq!(keys(7), vec!["artist::mbid::old"]);
        assert_eq!(keys(30), Vec::<String>::new());
        assert_eq!(keys(0), vec!["artist::mbid::old", "artist::mbid::edge", "artist::mbid::fresh"]);
        // An age no clock reaches selects nothing rather than overflowing.
        assert_eq!(keys(u64::MAX), Vec::<String>::new());
    }

    #[test]
    fn a_short_key_is_shown_whole() {
        assert_eq!(truncate_key("artist::mbid::björk", 60), "artist::mbid::björk");
    }

    /// Keys carry artist names, so the cut can land inside a multibyte
    /// character. A byte slice there panics and takes the listing with it.
    #[test]
    fn a_long_multibyte_key_is_cut_on_a_character_boundary() {
        let key = format!("artist::mbid::{}", "ö".repeat(100));
        let shown = truncate_key(&key, 60);
        assert_eq!(shown.chars().count(), 60);
        assert!(shown.ends_with("..."));

        // Every cut position, including the ones inside a character.
        let key = "日本語のアーティスト名";
        for max_len in 0..20 {
            assert!(truncate_key(key, max_len).chars().count() <= max_len.max(3));
        }
    }

    /// `--cache-dir` is taken exactly as given, existing or not: the tool is
    /// also used to inspect a copy of a cache pulled off a device.
    #[test]
    fn an_explicit_cache_dir_is_used_as_given() {
        let explicit = PathBuf::from("/somewhere/else");
        let resolved = resolve_cache_dir(Some(explicit.clone()), Path::new("/unused"))
            .expect("an explicit directory is never refused");
        assert_eq!(resolved, explicit);
    }

    /// With no `--cache-dir` and no database at the default path, the tool
    /// refuses rather than opening an empty cache.
    ///
    /// `AttributeCache` creates the database it is pointed at, so without this
    /// the tool would create an empty one and then report, successfully, that
    /// it holds nothing -- indistinguishable from a cache that really is
    /// empty, and it would leave a stray file behind as well.
    #[test]
    fn a_missing_default_cache_is_refused() {
        let empty = tempfile::tempdir().unwrap();
        let err = resolve_cache_dir(None, empty.path())
            .expect_err("a default directory with no attributes.db must be refused");
        assert!(err.contains("--cache-dir"), "the error must name the way out: {}", err);
        assert!(
            err.contains(&empty.path().display().to_string()),
            "the error must name the directory it looked in: {}",
            err
        );
    }

    /// A default directory that does hold a cache is used without a flag.
    #[test]
    fn a_present_default_cache_needs_no_flag() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("attributes.db"), b"").unwrap();
        let resolved = resolve_cache_dir(None, dir.path()).expect("a populated default is used");
        assert_eq!(resolved, dir.path());
    }
}
