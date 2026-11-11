//! The error model: every kind reports its stable code, and a platform status travels with its error.

use std::sync::Arc;

use nsh_microphone::{
    AudioChunk, ChunkSink, ErrorKind, FakeBackend, RecordConfig, Recorder, RecorderError,
};

/// A sink that drops every chunk: these tests watch the calls, not the audio.
fn sink() -> ChunkSink {
    Arc::new(|_: AudioChunk| {})
}

const ALL_KINDS: [(ErrorKind, &str); 8] = [
    (ErrorKind::NotAvailable, "ERR_MICROPHONE_NOT_AVAILABLE"),
    (
        ErrorKind::PermissionDenied,
        "ERR_MICROPHONE_PERMISSION_DENIED",
    ),
    (ErrorKind::Cancelled, "ERR_MICROPHONE_CANCELLED"),
    (ErrorKind::InProgress, "ERR_MICROPHONE_IN_PROGRESS"),
    (ErrorKind::InvalidState, "ERR_MICROPHONE_INVALID_STATE"),
    (
        ErrorKind::InvalidArgument,
        "ERR_MICROPHONE_INVALID_ARGUMENT",
    ),
    (
        ErrorKind::FormatNotSupported,
        "ERR_MICROPHONE_FORMAT_NOT_SUPPORTED",
    ),
    (ErrorKind::Backend, "ERR_MICROPHONE_BACKEND"),
];

#[test]
fn r_every_kind_reports_its_stable_code() {
    for (kind, code) in ALL_KINDS {
        let error = RecorderError::new(kind);
        assert_eq!(error.kind(), kind);
        assert_eq!(error.code(), code);
        // Display prints the code on the kind and the message on the error, as the TS side parses.
        assert_eq!(kind.to_string(), code);
        assert_eq!(error.to_string(), kind.message());
        assert!(!kind.message().is_empty());
        assert_eq!(error.status(), None);
    }
}

#[tokio::test]
async fn r_a_platform_status_travels_with_its_error() {
    let fake = FakeBackend::new();
    let recorder = Recorder::with_backend(fake.clone());
    fake.fail_next_stop(RecorderError::new(ErrorKind::Backend).with_status(-7));
    recorder
        .start(&RecordConfig::default(), sink())
        .await
        .unwrap();
    let error = recorder.stop().await.unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Backend);
    assert_eq!(error.code(), "ERR_MICROPHONE_BACKEND");
    assert_eq!(error.status(), Some(-7));
}
