/// A source of a Spotify Web API access token, for the librespot backend.
pub trait AccessTokenSource: Send + Sync {
    /// `None` when no account is linked or the token cannot be refreshed.
    fn access_token(&self) -> Option<String>;
}
