//! `Recorder::on_amplitude_changed` on `Headless` with a manual clock: the level is read every
//! interval while a recording runs, stays quiet while none does, and ends with its subscription.

// nsh-testing exports `Headless` only off wasm32, so this test cannot exist there.
#![cfg(not(target_arch = "wasm32"))]

use std::{
    sync::{
        Arc, OnceLock,
        atomic::{AtomicUsize, Ordering::SeqCst},
    },
    time::{Duration, SystemTime},
};

use nsh_core::prelude::*;
use nsh_engine::{Clock, EngineConfig, Root};
use nsh_microphone::{Amplitude, AudioChunk, ChunkSink, FakeBackend, RecordConfig, Recorder};
use nsh_testing::Headless;

/// How many levels the listener was told about.
static HEARD: AtomicUsize = AtomicUsize::new(0);
static FAKE: OnceLock<FakeBackend> = OnceLock::new();
static RECORDER: OnceLock<Recorder> = OnceLock::new();

fn fake() -> FakeBackend {
    FAKE.get_or_init(FakeBackend::new).clone()
}

fn recorder() -> Recorder {
    RECORDER
        .get_or_init(|| Recorder::with_backend(fake()))
        .clone()
}

/// The probe: the last level heard, as text.
#[component]
pub fn Probe() -> impl IntoElement {
    let level = signal("no level".to_owned());
    let subscription =
        recorder().on_amplitude_changed(Duration::from_millis(100), move |amplitude| {
            HEARD.fetch_add(1, SeqCst);
            level.set(format!(
                "level {:.2} {:.2}",
                amplitude.average, amplitude.peak
            ));
        });
    // The subscription goes with the component, as its task does.
    cleanup(move || drop(subscription));
    text(move || level.get())
}

fn start() -> Result<Headless, String> {
    let config = EngineConfig {
        clock: Clock::manual(SystemTime::UNIX_EPOCH),
        ..EngineConfig::new(Root::of::<Probe>())
    };
    Headless::start(config).map_err(|e| format!("the engine did not start: {e}"))
}

/// Runs one call of the recorder to its end; the fake answers at once.
fn block_on<T>(future: impl std::future::Future<Output = T>) -> Result<T, String> {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .map(|runtime| runtime.block_on(future))
        .map_err(|e| format!("no runtime: {e}"))
}

#[test]
fn r_amplitude_is_heard_every_interval_while_a_recording_runs() -> Result<(), String> {
    let fake = fake();
    let recorder = recorder();
    fake.set_amplitude(Amplitude {
        average: 0.5,
        peak: 0.75,
    });
    let mut app = start()?;

    // Idle: the reads fail with an invalid state and say nothing.
    app.run_for(Duration::from_millis(350))?;
    assert_eq!(
        HEARD.load(SeqCst),
        0,
        "nothing records, so nothing is heard"
    );
    assert!(app.shows("no level"));

    let sink: ChunkSink = Arc::new(|_: AudioChunk| {});
    block_on(recorder.start(&RecordConfig::default(), sink))?.map_err(|e| e.to_string())?;
    app.run_for(Duration::from_millis(350))?;
    app.wait_for("level 0.50 0.75")?;
    let while_recording = HEARD.load(SeqCst);
    assert!(
        while_recording >= 3,
        "one read per 100 ms for 350 ms, heard {while_recording}"
    );

    // A paused recording still has a level.
    recorder.pause().map_err(|e| e.to_string())?;
    fake.set_amplitude(Amplitude {
        average: 0.25,
        peak: 0.5,
    });
    app.run_for(Duration::from_millis(250))?;
    app.wait_for("level 0.25 0.50")?;

    // A stopped recording is quiet again.
    block_on(recorder.stop())?.map_err(|e| e.to_string())?;
    let at_stop = HEARD.load(SeqCst);
    app.run_for(Duration::from_millis(500))?;
    assert_eq!(
        HEARD.load(SeqCst),
        at_stop,
        "no reads reach the listener once idle"
    );
    Ok(())
}
