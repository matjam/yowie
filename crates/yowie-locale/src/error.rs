use thiserror::Error;

/// Errors from loading a translation table.
#[derive(Debug, Error)]
pub enum LocaleError {
    /// The text is not valid JSON, or its top level is not an object.
    #[error("locale {lang}: invalid JSON: {source}")]
    Json {
        /// Language of the table being loaded.
        lang: String,
        /// The parse error.
        source: serde_json::Error,
    },
    /// An entry is neither a string nor an object, or a plural form is not
    /// a string.
    #[error("locale {lang}: entry {key} must be a string or an object of strings")]
    InvalidEntry {
        /// Language of the table being loaded.
        lang: String,
        /// Dotted key of the offending entry.
        key: String,
    },
}
