//! The errors a recording call reports. Each has a stable code; a message says what to do.

use std::fmt;

/// What went wrong, as a stable code; see [`RecorderError::code`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// No recording here: no microphone, a target with no backend, or a disposed handle.
    NotAvailable,
    /// Recording was refused; the user or the system said no.
    PermissionDenied,
    /// The operation was cancelled.
    Cancelled,
    /// A recording already runs; one runs at a time.
    InProgress,
    /// The recorder's state does not allow the call.
    InvalidState,
    /// An argument is empty or out of range.
    InvalidArgument,
    /// The platform cannot encode or write this format.
    FormatNotSupported,
    /// The platform's audio service failed; `status` holds its own code where it gives one.
    Backend,
}

/// A failed recording call or availability query.
///
/// `Display` prints the message. Neither field holds reason text or any user data.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}", .kind.message())]
pub struct RecorderError {
    kind: ErrorKind,
    status: Option<i64>,
}

impl RecorderError {
    /// An error of `kind`, with no platform status.
    pub fn new(kind: ErrorKind) -> Self {
        Self { kind, status: None }
    }

    /// The same error, carrying the platform's own status code.
    #[must_use]
    pub fn with_status(mut self, status: i64) -> Self {
        self.status = Some(status);
        self
    }

    /// What went wrong.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The platform's status code, when the platform gave one.
    pub fn status(&self) -> Option<i64> {
        self.status
    }

    /// The stable code, such as `ERR_MICROPHONE_CANCELLED`. Apps match on this.
    pub fn code(&self) -> &'static str {
        self.kind.code()
    }
}

impl ErrorKind {
    /// The stable code of this kind.
    pub fn code(self) -> &'static str {
        match self {
            ErrorKind::NotAvailable => "ERR_MICROPHONE_NOT_AVAILABLE",
            ErrorKind::PermissionDenied => "ERR_MICROPHONE_PERMISSION_DENIED",
            ErrorKind::Cancelled => "ERR_MICROPHONE_CANCELLED",
            ErrorKind::InProgress => "ERR_MICROPHONE_IN_PROGRESS",
            ErrorKind::InvalidState => "ERR_MICROPHONE_INVALID_STATE",
            ErrorKind::InvalidArgument => "ERR_MICROPHONE_INVALID_ARGUMENT",
            ErrorKind::FormatNotSupported => "ERR_MICROPHONE_FORMAT_NOT_SUPPORTED",
            ErrorKind::Backend => "ERR_MICROPHONE_BACKEND",
        }
    }

    /// What to do about it, in one sentence.
    pub fn message(self) -> &'static str {
        match self {
            ErrorKind::NotAvailable => "Microphone recording is not available on this device.",
            ErrorKind::PermissionDenied => {
                "Recording was refused. Allow microphone access in the system settings."
            }
            ErrorKind::Cancelled => "The operation was cancelled.",
            ErrorKind::InProgress => "A recording is already running. Stop or cancel it first.",
            ErrorKind::InvalidState => "The recorder is in a state that does not allow this call.",
            ErrorKind::InvalidArgument => "An argument is empty or not allowed on this platform.",
            ErrorKind::FormatNotSupported => {
                "The encoder or format is not supported on this platform."
            }
            ErrorKind::Backend => "The platform's audio service failed.",
        }
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}
