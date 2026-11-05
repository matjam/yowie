//! Translated string tables with placeholders, plural rules and fallback
//! languages.
//!
//! - [`Table`] holds the messages for one language, loaded from JSON.
//! - [`Bundle`] holds the tables for every language plus the fallback
//!   chain, and hands out a [`Translator`] for the active language.
//! - [`plural`] picks the CLDR plural [`Category`] for a number.
//!
//! Message text uses `{name}` placeholders. `{{` and `}}` write literal
//! braces. A placeholder with no matching argument stays as written, and a
//! missing key becomes `[key]`.
//!
//! JSON tables are nested objects of strings. Nested keys join with dots
//! (`{"menu": {"play": "Play"}}` is `menu.play`). An object whose keys are
//! all plural categories (`zero`, `one`, `two`, `few`, `many`, `other`) is
//! one plural message.
//!
//! ```
//! use yowie_locale::{Args, Bundle};
//!
//! let mut bundle = Bundle::new(["en"]);
//! bundle.load_json("en", r#"{"greet": "Hello, {who}!",
//!     "arrows": {"one": "{n} arrow", "other": "{n} arrows"}}"#).unwrap();
//! let t = bundle.translator("en-GB");
//! assert_eq!(t.get_with("greet", &Args::new().set("who", "Ada")), "Hello, Ada!");
//! assert_eq!(t.count("arrows", 3), "3 arrows");
//! ```

#![warn(missing_docs)]

mod args;
mod bundle;
mod error;
mod plural;
mod table;

pub use args::{Args, Number};
pub use bundle::{Bundle, Translator, system_locale};
pub use error::LocaleError;
pub use plural::{Category, plural};
pub use table::Table;
