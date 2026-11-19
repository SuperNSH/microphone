//! `Recorder::on_state_changed`: each change of state is heard once, in order, and only while subscribed.

use std::sync::{Arc, Mutex, PoisonError};

use nsh_microphone::{
    AudioChunk, ChunkSink, ErrorKind, FakeBackend, RecordConfig, Recorder, RecorderError,
    RecordingState, Subscription,
};

/// A sink that drops every chunk: these tests watch the states, not the audio.
fn sink() -> ChunkSink {
    Arc::new(|_: AudioChunk| {})
}

type Heard = Arc<Mutex<Vec<RecordingState>>>;

/// Listens to `recorder` and collects every state it is told about.
fn listen(recorder: &Recorder) -> (Heard, Subscription) {
    let heard: Heard = Arc::new(Mutex::new(Vec::new()));
    let collector = Arc::clone(&heard);
    let subscription = recorder.on_state_changed(move |state| {
        collector
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(state);
    });
    (heard, subscription)
}

fn heard(heard: &Heard) -> Vec<RecordingState> {
    heard.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

#[tokio::test]
async fn r_state_listener_hears_each_change_once_in_order() {
    let recorder = Recorder::with_backend(FakeBackend::new());
    let (states, _subscription) = listen(&recorder);

    recorder
        .start(&RecordConfig::default(), sink())
        .await
        .unwrap();
    recorder.pause().unwrap();
    // A refused pause changes nothing, so it announces nothing.
    assert_eq!(
        recorder.pause().unwrap_err().kind(),
        ErrorKind::InvalidState
    );
    recorder.resume().unwrap();
    recorder.stop().await.unwrap();

    assert_eq!(
        heard(&states),
        vec![
            RecordingState::Recording,
            RecordingState::Paused,
            RecordingState::Recording,
            RecordingState::Idle,
        ]
    );
}

#[tokio::test]
async fn r_a_failed_start_announces_its_claim_and_its_release() {
    let fake = FakeBackend::new();
    let recorder = Recorder::with_backend(fake.clone());
    let (states, _subscription) = listen(&recorder);
    fake.fail_next_start(RecorderError::new(ErrorKind::Backend));

    let refused = recorder
        .start(&RecordConfig::default(), sink())
        .await
        .unwrap_err();

    assert_eq!(refused.kind(), ErrorKind::Backend);
    assert_eq!(
        heard(&states),
        vec![RecordingState::Recording, RecordingState::Idle]
    );
    assert_eq!(recorder.state(), RecordingState::Idle);
}

#[tokio::test]
async fn r_cancel_and_dispose_announce_idle() {
    let cancelled = Recorder::with_backend(FakeBackend::new());
    let (states, _subscription) = listen(&cancelled);
    cancelled
        .start(&RecordConfig::default(), sink())
        .await
        .unwrap();
    cancelled.cancel().await.unwrap();
    assert_eq!(
        heard(&states),
        vec![RecordingState::Recording, RecordingState::Idle]
    );

    let disposed = Recorder::with_backend(FakeBackend::new());
    let (states, _subscription) = listen(&disposed);
    disposed
        .start(&RecordConfig::default(), sink())
        .await
        .unwrap();
    disposed.pause().unwrap();
    disposed.dispose().await.unwrap();
    assert_eq!(
        heard(&states),
        vec![
            RecordingState::Recording,
            RecordingState::Paused,
            RecordingState::Idle,
        ]
    );
}

#[tokio::test]
async fn r_a_stopped_or_dropped_subscription_hears_nothing_more() {
    let recorder = Recorder::with_backend(FakeBackend::new());
    let (stopped, subscription) = listen(&recorder);
    let (dropped, kept) = listen(&recorder);
    let (still_on, _still_on) = listen(&recorder);
    subscription.stop();
    drop(kept);

    recorder
        .start(&RecordConfig::default(), sink())
        .await
        .unwrap();

    assert!(heard(&stopped).is_empty());
    assert!(heard(&dropped).is_empty());
    assert_eq!(heard(&still_on), vec![RecordingState::Recording]);
}

#[tokio::test]
async fn r_clones_of_one_recorder_share_the_announcements() {
    let recorder = Recorder::with_backend(FakeBackend::new());
    let other = recorder.clone();
    let (states, _subscription) = listen(&recorder);

    other.start(&RecordConfig::default(), sink()).await.unwrap();

    assert_eq!(heard(&states), vec![RecordingState::Recording]);
}

#[tokio::test]
async fn r_a_listener_may_read_the_recorder_back() {
    let recorder = Recorder::with_backend(FakeBackend::new());
    let seen: Heard = Arc::new(Mutex::new(Vec::new()));
    let (reader, collector) = (recorder.clone(), Arc::clone(&seen));
    let _subscription = recorder.on_state_changed(move |_| {
        // The listener runs outside the gate's lock, so this read cannot deadlock.
        collector
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(reader.state());
    });

    recorder
        .start(&RecordConfig::default(), sink())
        .await
        .unwrap();
    recorder.pause().unwrap();

    assert_eq!(
        heard(&seen),
        vec![RecordingState::Recording, RecordingState::Paused]
    );
}
