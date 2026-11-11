//! The start-time refusals, permission paths and late answers, driven through `FakeBackend`.

use std::sync::{Arc, Mutex, PoisonError};

use nsh_microphone::{
    AudioChunk, ChunkSink, Encoder, ErrorKind, FakeBackend, Output, PermissionState, RecordConfig,
    Recorder, RecorderError, RecordingState, StoppedRecording,
};

/// A sink that drops every chunk: these tests watch the calls, not the audio.
fn sink() -> ChunkSink {
    Arc::new(|_: AudioChunk| {})
}

/// Yields until `ready` holds, so a spawned call has reached the backend.
async fn settle(ready: impl Fn() -> bool) {
    for _ in 0..1000 {
        if ready() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("the stop never reached the backend");
}

#[tokio::test]
async fn r_start_with_zero_sample_rate_is_invalid_argument() {
    let fake = FakeBackend::new();
    let recorder = Recorder::with_backend(fake.clone());
    // `RecordConfig` is non-exhaustive, so a test outside the crate sets fields one by one.
    let mut config = RecordConfig::default();
    config.sample_rate = 0;
    let error = recorder.start(&config, sink()).await.unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidArgument);
    assert_eq!(error.code(), "ERR_MICROPHONE_INVALID_ARGUMENT");
    assert_eq!(fake.starts(), 0);
    assert!(!fake.recording());
}

#[tokio::test]
async fn r_start_with_zero_or_three_channels_is_invalid_argument() {
    let fake = FakeBackend::new();
    let recorder = Recorder::with_backend(fake.clone());
    let mut config = RecordConfig::default();
    config.channels = 0;
    assert_eq!(
        recorder.start(&config, sink()).await.unwrap_err().kind(),
        ErrorKind::InvalidArgument
    );
    config.channels = 3;
    assert_eq!(
        recorder.start(&config, sink()).await.unwrap_err().kind(),
        ErrorKind::InvalidArgument
    );
    assert_eq!(fake.starts(), 0);

    // Stereo is the ceiling: two channels record.
    config.channels = 2;
    recorder.start(&config, sink()).await.unwrap();
    recorder.cancel().await.unwrap();
}

#[tokio::test]
async fn r_start_with_an_empty_file_path_is_invalid_argument() {
    let fake = FakeBackend::new();
    let recorder = Recorder::with_backend(fake.clone());
    let mut config = RecordConfig::default();
    config.output = Output::File {
        path: String::new(),
    };
    let error = recorder.start(&config, sink()).await.unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidArgument);
    assert_eq!(fake.starts(), 0);
}

#[tokio::test]
async fn r_start_with_zero_bit_rate_on_a_compressed_encoder_is_invalid_argument() {
    let fake = FakeBackend::new();
    let recorder = Recorder::with_backend(fake.clone());
    let mut config = RecordConfig::default();
    config.bit_rate = 0;
    let error = recorder.start(&config, sink()).await.unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidArgument);
    assert_eq!(fake.starts(), 0);

    // PCM carries no bit rate to negotiate, so zero is accepted there.
    config.encoder = Encoder::Wav;
    recorder.start(&config, sink()).await.unwrap();
    recorder.cancel().await.unwrap();
    config.encoder = Encoder::Pcm16bits;
    recorder.start(&config, sink()).await.unwrap();
    recorder.cancel().await.unwrap();
}

#[tokio::test]
async fn r_an_unsupported_encoder_is_format_not_supported() {
    let fake = FakeBackend::new();
    fake.set_encoder_supported(Ok(false));
    let recorder = Recorder::with_backend(fake.clone());
    let error = recorder
        .start(&RecordConfig::default(), sink())
        .await
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::FormatNotSupported);
    assert_eq!(error.code(), "ERR_MICROPHONE_FORMAT_NOT_SUPPORTED");
    assert_eq!(fake.starts(), 0);
    assert!(!fake.recording());

    // The refusal freed the gate: a supported encoder records next.
    fake.set_encoder_supported(Ok(true));
    recorder
        .start(&RecordConfig::default(), sink())
        .await
        .unwrap();
    assert!(fake.recording());
}

#[tokio::test]
async fn r_undetermined_permission_is_requested_once_then_start_proceeds() {
    let fake = FakeBackend::new();
    fake.set_check_permissions(Ok(PermissionState::Undetermined));
    fake.set_request_permissions(Ok(PermissionState::Granted));
    let recorder = Recorder::with_backend(fake.clone());
    recorder
        .start(&RecordConfig::default(), sink())
        .await
        .unwrap();
    assert_eq!(recorder.state(), RecordingState::Recording);
    assert!(fake.recording());
    assert_eq!(fake.starts(), 1);
}

#[tokio::test]
async fn r_denied_permission_is_permission_denied() {
    let fake = FakeBackend::new();
    let recorder = Recorder::with_backend(fake.clone());

    fake.set_check_permissions(Ok(PermissionState::Denied));
    let direct = recorder
        .start(&RecordConfig::default(), sink())
        .await
        .unwrap_err();
    assert_eq!(direct.kind(), ErrorKind::PermissionDenied);
    assert_eq!(direct.code(), "ERR_MICROPHONE_PERMISSION_DENIED");

    // An undetermined check followed by a denied request is the same refusal.
    fake.set_check_permissions(Ok(PermissionState::Undetermined));
    fake.set_request_permissions(Ok(PermissionState::Denied));
    let after_request = recorder
        .start(&RecordConfig::default(), sink())
        .await
        .unwrap_err();
    assert_eq!(after_request.kind(), ErrorKind::PermissionDenied);
    assert_eq!(fake.starts(), 0);

    // The refusals claimed no gate: a granted check records.
    fake.set_check_permissions(Ok(PermissionState::Granted));
    recorder
        .start(&RecordConfig::default(), sink())
        .await
        .unwrap();
    assert!(fake.recording());
}

#[tokio::test]
async fn r_a_cancelled_permission_prompt_reports_cancelled() {
    let fake = FakeBackend::new();
    let recorder = Recorder::with_backend(fake.clone());

    fake.set_check_permissions(Err(RecorderError::new(ErrorKind::Cancelled)));
    let direct = recorder
        .start(&RecordConfig::default(), sink())
        .await
        .unwrap_err();
    assert_eq!(direct.kind(), ErrorKind::Cancelled);
    assert_eq!(direct.code(), "ERR_MICROPHONE_CANCELLED");

    fake.set_check_permissions(Ok(PermissionState::Undetermined));
    fake.set_request_permissions(Err(RecorderError::new(ErrorKind::Cancelled)));
    let after_request = recorder
        .start(&RecordConfig::default(), sink())
        .await
        .unwrap_err();
    assert_eq!(after_request.kind(), ErrorKind::Cancelled);
    assert_eq!(fake.starts(), 0);
}

#[tokio::test]
async fn r_the_config_reaches_the_backend() {
    let fake = FakeBackend::new();
    let recorder = Recorder::with_backend(fake.clone());
    assert_eq!(fake.last_config(), None);

    let mut config = RecordConfig::default();
    config.sample_rate = 48_000;
    config.channels = 2;
    config.bit_rate = 96_000;
    config.encoder = Encoder::Opus;
    config.output = Output::File {
        path: "notes.opus".to_owned(),
    };
    recorder.start(&config, sink()).await.unwrap();
    assert_eq!(fake.last_config(), Some(config));
    recorder.cancel().await.unwrap();
}

#[tokio::test]
async fn r_chunks_pause_and_resume_with_the_recording() {
    let fake = FakeBackend::new();
    let recorder = Recorder::with_backend(fake.clone());
    let delivered: Arc<Mutex<Vec<usize>>> = Arc::new(Mutex::new(Vec::new()));
    let sink: ChunkSink = {
        let delivered = Arc::clone(&delivered);
        Arc::new(move |chunk| {
            delivered
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(chunk.data.len());
        })
    };
    recorder
        .start(&RecordConfig::default(), sink)
        .await
        .unwrap();

    assert!(fake.push_chunk(AudioChunk {
        data: vec![1, 2],
        sample_rate: 44_100,
        channels: 1,
    }));
    recorder.pause().unwrap();
    assert!(
        !fake.push_chunk(AudioChunk {
            data: vec![3, 4, 5],
            sample_rate: 44_100,
            channels: 1,
        }),
        "a paused recording drops chunks"
    );
    recorder.resume().unwrap();
    assert!(fake.push_chunk(AudioChunk {
        data: vec![6, 7, 8, 9],
        sample_rate: 44_100,
        channels: 1,
    }));

    // Only the chunks delivered while Recording reached the sink.
    assert_eq!(
        *delivered.lock().unwrap_or_else(PoisonError::into_inner),
        vec![2, 4]
    );
    assert_eq!(fake.chunks(), 2);
    assert_eq!(fake.stops(), 0);
    recorder.stop().await.unwrap();
    assert_eq!(fake.stops(), 1);
}

#[tokio::test]
async fn r_a_late_stop_answer_after_cancel_is_ignored() {
    let fake = FakeBackend::new();
    let recorder = Recorder::with_backend(fake.clone());
    recorder
        .start(&RecordConfig::default(), sink())
        .await
        .unwrap();
    fake.hold_stop();

    // The stop future is !Send (`BackendFuture` adds no Send), so it drives on this task.
    let mut call = std::pin::pin!(recorder.stop());
    tokio::select! {
        _ = call.as_mut() => panic!("the stop resolved while the backend held it"),
        _ = tokio::task::yield_now() => {}
    }
    settle(|| fake.stops() == 1).await;

    recorder.cancel().await.unwrap();
    assert_eq!(recorder.state(), RecordingState::Idle);
    assert_eq!(fake.cancels(), 1);
    // The cancelled stop resolves once, as a cancel, without the recording's output.
    let error = call.as_mut().await.unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Cancelled);

    // The system's late answer arrives for a generation the gate no longer holds.
    assert!(fake.finish_stop(Ok(StoppedRecording::default())));
    assert_eq!(recorder.state(), RecordingState::Idle);

    // Nothing of the dropped answer disturbed the gate: a new recording runs.
    recorder
        .start(&RecordConfig::default(), sink())
        .await
        .unwrap();
    assert!(fake.recording());
}

#[tokio::test]
async fn r_a_backend_error_keeps_its_code_and_status() {
    let fake = FakeBackend::new();
    fake.fail_next_start(RecorderError::new(ErrorKind::NotAvailable).with_status(42));
    let recorder = Recorder::with_backend(fake.clone());
    let error = recorder
        .start(&RecordConfig::default(), sink())
        .await
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::NotAvailable);
    assert_eq!(error.code(), "ERR_MICROPHONE_NOT_AVAILABLE");
    assert_eq!(error.status(), Some(42));
    assert_eq!(fake.starts(), 0);

    // The failed start freed the gate: the next one records.
    recorder
        .start(&RecordConfig::default(), sink())
        .await
        .unwrap();
    assert!(fake.recording());
}

const ALL_ENCODERS: [(Encoder, &str); 10] = [
    (Encoder::AacLc, "aacLc"),
    (Encoder::AacEld, "aacEld"),
    (Encoder::AacHe, "aacHe"),
    (Encoder::AmrNb, "amrNb"),
    (Encoder::AmrWb, "amrWb"),
    (Encoder::Flac, "flac"),
    (Encoder::Opus, "opus"),
    (Encoder::Pcm16bits, "pcm16bits"),
    (Encoder::Vorbis, "vorbis"),
    (Encoder::Wav, "wav"),
];

#[test]
fn r_each_encoder_has_the_name_tsx_uses() {
    for (encoder, name) in ALL_ENCODERS {
        assert_eq!(encoder.name(), name);
    }
    // The names are unique, so the TSX side can map both ways.
    let mut names: Vec<&str> = ALL_ENCODERS.iter().map(|(_, name)| *name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), ALL_ENCODERS.len());
}
