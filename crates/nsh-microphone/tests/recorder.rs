//! The `Recorder` state machine on `FakeBackend`: one recording at a time, one answer per call, one shared gate.

use std::sync::Arc;

use nsh_microphone::{
    AudioChunk, ChunkSink, ErrorKind, FakeBackend, RecordConfig, Recorder, RecordingState,
};

/// A sink that drops every chunk: these tests watch the calls, not the audio.
fn sink() -> ChunkSink {
    Arc::new(|_: AudioChunk| {})
}

#[tokio::test]
async fn r_start_while_recording_is_refused_with_in_progress() {
    let fake = FakeBackend::new();
    let recorder = Recorder::with_backend(fake.clone());
    recorder
        .start(&RecordConfig::default(), sink())
        .await
        .unwrap();
    assert!(recorder.is_recording());
    assert!(fake.recording());
    let refused = recorder
        .start(&RecordConfig::default(), sink())
        .await
        .unwrap_err();
    assert_eq!(refused.kind(), ErrorKind::InProgress);
    assert_eq!(refused.code(), "ERR_MICROPHONE_IN_PROGRESS");
    assert_eq!(fake.starts(), 1);
}

#[tokio::test]
async fn r_pause_and_resume_track_the_state_machine() {
    let fake = FakeBackend::new();
    let recorder = Recorder::with_backend(fake.clone());
    assert_eq!(recorder.state(), RecordingState::Idle);

    recorder
        .start(&RecordConfig::default(), sink())
        .await
        .unwrap();
    assert_eq!(recorder.state(), RecordingState::Recording);
    assert!(recorder.is_recording());
    assert!(!recorder.is_paused());

    recorder.pause().unwrap();
    assert_eq!(recorder.state(), RecordingState::Paused);
    assert!(recorder.is_paused());
    assert!(!recorder.is_recording());
    assert_eq!(fake.pauses(), 1);

    recorder.resume().unwrap();
    assert_eq!(recorder.state(), RecordingState::Recording);
    assert_eq!(fake.resumes(), 1);

    let stopped = recorder.stop().await.unwrap();
    assert_eq!(stopped.path.as_deref(), Some("recording.m4a"));
    assert_eq!(stopped.duration_ms, Some(0));
    assert_eq!(recorder.state(), RecordingState::Idle);
    assert_eq!(fake.stops(), 1);
}

#[test]
fn r_pause_when_idle_is_invalid_state() {
    let fake = FakeBackend::new();
    let recorder = Recorder::with_backend(fake.clone());
    let error = recorder.pause().unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidState);
    assert_eq!(error.code(), "ERR_MICROPHONE_INVALID_STATE");
    assert_eq!(fake.pauses(), 0);
}

#[tokio::test]
async fn r_resume_when_recording_is_invalid_state() {
    let fake = FakeBackend::new();
    let recorder = Recorder::with_backend(fake.clone());
    recorder
        .start(&RecordConfig::default(), sink())
        .await
        .unwrap();
    let error = recorder.resume().unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidState);
    assert_eq!(fake.resumes(), 0);
}

#[tokio::test]
async fn r_stop_when_idle_is_invalid_state() {
    let fake = FakeBackend::new();
    let recorder = Recorder::with_backend(fake.clone());
    let error = recorder.stop().await.unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidState);
    assert_eq!(fake.stops(), 0);
}

#[tokio::test]
async fn r_stop_twice_resolves_once_then_invalid_state() {
    let fake = FakeBackend::new();
    let recorder = Recorder::with_backend(fake.clone());
    recorder
        .start(&RecordConfig::default(), sink())
        .await
        .unwrap();
    let stopped = recorder.stop().await.unwrap();
    assert_eq!(stopped.path.as_deref(), Some("recording.m4a"));
    let second = recorder.stop().await.unwrap_err();
    assert_eq!(second.kind(), ErrorKind::InvalidState);
    assert_eq!(fake.stops(), 1);
    assert_eq!(recorder.state(), RecordingState::Idle);
}

#[tokio::test]
async fn r_cancel_when_idle_is_invalid_state() {
    let fake = FakeBackend::new();
    let recorder = Recorder::with_backend(fake.clone());
    let error = recorder.cancel().await.unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidState);
    assert_eq!(fake.cancels(), 0);
}

#[tokio::test]
async fn r_amplitude_when_idle_is_invalid_state() {
    let fake = FakeBackend::new();
    let recorder = Recorder::with_backend(fake.clone());
    let error = recorder.amplitude().await.unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidState);
    assert_eq!(error.code(), "ERR_MICROPHONE_INVALID_STATE");
}

#[tokio::test]
async fn r_clones_share_one_gate() {
    let fake = FakeBackend::new();
    let recorder = Recorder::with_backend(fake.clone());
    let twin = recorder.clone();
    recorder
        .start(&RecordConfig::default(), sink())
        .await
        .unwrap();
    let refused = twin
        .start(&RecordConfig::default(), sink())
        .await
        .unwrap_err();
    assert_eq!(refused.kind(), ErrorKind::InProgress);
    assert_eq!(fake.starts(), 1);

    // One shared state machine: what one clone pauses, the other resumes.
    recorder.pause().unwrap();
    assert!(twin.is_paused());
    twin.resume().unwrap();
    assert!(recorder.is_recording());
    let stopped = twin.stop().await.unwrap();
    assert_eq!(stopped.path.as_deref(), Some("recording.m4a"));
    assert_eq!(recorder.state(), RecordingState::Idle);
    let error = recorder.amplitude().await.unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidState);
}

#[tokio::test]
async fn r_dispose_while_recording_stops_and_later_calls_report_not_available() {
    let fake = FakeBackend::new();
    let recorder = Recorder::with_backend(fake.clone());
    recorder
        .start(&RecordConfig::default(), sink())
        .await
        .unwrap();
    assert!(fake.recording());

    recorder.dispose().await.unwrap();
    assert!(!fake.recording());
    // A dispose while recording cancels: no stop answer, one backend dispose.
    assert_eq!(fake.cancels(), 1);
    assert_eq!(fake.stops(), 0);
    assert_eq!(fake.disposals(), 1);
    assert_eq!(recorder.state(), RecordingState::Idle);

    // Every later call on the handle answers NotAvailable, dispose included.
    assert_eq!(
        recorder.pause().unwrap_err().code(),
        "ERR_MICROPHONE_NOT_AVAILABLE"
    );
    assert_eq!(
        recorder.resume().unwrap_err().kind(),
        ErrorKind::NotAvailable
    );
    assert_eq!(
        recorder.check_permissions().await.unwrap_err().kind(),
        ErrorKind::NotAvailable
    );
    assert_eq!(
        recorder
            .start(&RecordConfig::default(), sink())
            .await
            .unwrap_err()
            .kind(),
        ErrorKind::NotAvailable
    );
    assert_eq!(
        recorder.amplitude().await.unwrap_err().kind(),
        ErrorKind::NotAvailable
    );
    assert_eq!(
        recorder.stop().await.unwrap_err().kind(),
        ErrorKind::NotAvailable
    );
    assert_eq!(
        recorder.cancel().await.unwrap_err().kind(),
        ErrorKind::NotAvailable
    );
    assert_eq!(
        recorder.dispose().await.unwrap_err().kind(),
        ErrorKind::NotAvailable
    );
    assert_eq!(fake.disposals(), 1, "the backend's dispose runs once");
}
