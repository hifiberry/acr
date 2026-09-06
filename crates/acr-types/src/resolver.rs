use crate::OrderResult;

/// The two synchronous questions the player daemon asks MusicBrainz.
pub trait Resolver: Send + Sync {
    fn title_order(&self, part1: &str, part2: &str) -> OrderResult;
    /// `None` means "one artist". Called only for names containing a separator.
    fn artist_split(&self, name: &str, separators: &[String]) -> Option<Vec<String>>;
}
