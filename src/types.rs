/// Controls how CLI commands format their output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable text.
    Human,
    /// Compact single-line JSON.
    Json,
    /// Indented, pretty-printed JSON.
    PrettyJson,
}

/// Error type for all fallible cosmicmsg operations.
#[derive(Debug)]
pub enum Error {
    /// Failed to connect to the Wayland display.
    Connect(String),
    /// A required Wayland protocol is not advertised by the compositor.
    ProtocolNotAvailable(String),
    /// No workspace or window matched the given selector.
    NotFound(String),
    /// The selector matched more than one item.
    AmbiguousMatch(String),
    /// Any other runtime error.
    Other(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Connect(s) => write!(f, "failed to connect to Wayland display: {s}"),
            Error::ProtocolNotAvailable(s) => write!(f, "protocol not available: {s}"),
            Error::NotFound(s) => write!(f, "not found: {s}"),
            Error::AmbiguousMatch(s) => write!(f, "ambiguous match: {s}"),
            Error::Other(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for Error {}
