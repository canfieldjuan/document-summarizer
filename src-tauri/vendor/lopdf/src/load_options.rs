use crate::Object;

/// Decimal representation selected once for a load or content decode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NumericPrecision {
    #[default]
    Legacy,
    Precise,
}

/// Type alias for the filter function used during PDF loading.
///
/// The function receives an object ID and a mutable reference to the object,
/// and returns `Some((id, object))` to keep it or `None` to discard it.
pub type FilterFunc = fn((u32, u16), &mut Object) -> Option<((u32, u16), Object)>;

/// Options for loading PDF documents.
///
/// Use this struct to configure password, object filtering, and strictness
/// when loading a PDF. The default is lenient parsing with no password or filter.
///
/// # Examples
///
/// ```no_run
/// use lopdf::{Document, LoadOptions};
///
/// // Load with a password
/// let doc = Document::load_with_options(
///     "encrypted.pdf",
///     LoadOptions::with_password("secret"),
/// );
///
/// // Load with strict parsing
/// let doc = Document::load_with_options(
///     "document.pdf",
///     LoadOptions { strict: true, ..Default::default() },
/// );
/// ```
#[derive(Clone, Default)]
pub struct LoadOptions {
    /// Password for encrypted PDFs.
    pub password: Option<String>,
    /// Object filter applied during loading.
    pub filter: Option<FilterFunc>,
    /// When `true`, reject non-conforming PDFs instead of silently accepting them.
    /// Defaults to `false` (lenient parsing).
    pub strict: bool,
    /// Defaults to the original 32-bit representation.
    pub numeric_precision: NumericPrecision,
}

impl std::fmt::Debug for LoadOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadOptions")
            .field("password", &self.password.as_ref().map(|_| "***"))
            .field("filter", &self.filter.map(|_| "fn(..)"))
            .field("strict", &self.strict)
            .field("numeric_precision", &self.numeric_precision)
            .finish()
    }
}

impl LoadOptions {
    /// Create options with a password for encrypted PDFs.
    pub fn with_password(password: &str) -> Self {
        Self {
            password: Some(password.to_string()),
            ..Default::default()
        }
    }

    /// Create options with an object filter.
    pub fn with_filter(filter: FilterFunc) -> Self {
        Self {
            filter: Some(filter),
            ..Default::default()
        }
    }
}
