//! The reactive availability read on `Headless`: it wakes only on change, and checks again on foreground.

use std::{
    sync::{
        Mutex, OnceLock, PoisonError,
        atomic::{AtomicUsize, Ordering::SeqCst},
    },
    time::{Duration, SystemTime},
};

use nsh_core::{platform::Lifecycle, prelude::*};
use nsh_engine::{Clock, EngineConfig, Root};
use nsh_microphone::{
    AvailabilityState, ErrorKind, FakeBackend, InputDevice, MicrophoneAvailability,
    PermissionState, Recorder, RecorderError, use_microphone_availability_for,
};
use nsh_testing::Headless;

static RUNS: AtomicUsize = AtomicUsize::new(0);
static FAKE: OnceLock<FakeBackend> = OnceLock::new();
/// The latest read's handle, which the Refresh button reaches without props.
static HANDLE: Mutex<Option<MicrophoneAvailability>> = Mutex::new(None);

fn fake() -> FakeBackend {
    FAKE.get_or_init(FakeBackend::new).clone()
}

/// The probe: the availability read as text, plus a button that refreshes it.
#[component]
pub fn Probe() -> impl IntoElement {
    view().child(reader()).child(
        view()
            .role(AccessibilityRole::Button)
            .label("Refresh")
            .on_press(|| {
                if let Some(handle) = HANDLE
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .as_ref()
                {
                    handle.refresh();
                }
            })
            .child(text("Refresh")),
    )
}

fn reader() -> impl IntoElement {
    let availability = use_microphone_availability_for(Recorder::with_backend(fake()));
    *HANDLE.lock().unwrap_or_else(PoisonError::into_inner) = Some(availability);
    text(move || {
        RUNS.fetch_add(1, SeqCst);
        match availability.state() {
            AvailabilityState::Checking => "checking".to_owned(),
            AvailabilityState::Ready(report) => format!(
                "ready {} {}",
                report.permission.name(),
                report.has_input_device
            ),
            AvailabilityState::Failed(error) => format!("failed {}", error.code()),
        }
    })
}

fn start() -> Result<Headless, String> {
    let config = EngineConfig {
        clock: Clock::manual(SystemTime::UNIX_EPOCH),
        ..EngineConfig::new(Root::of::<Probe>())
    };
    Headless::start(config).map_err(|e| format!("the engine did not start: {e}"))
}

fn leave_foreground(app: &mut Headless) -> Result<(), String> {
    app.set_environment("setLifecycle(background)", |e| {
        e.lifecycle = Lifecycle::Background;
    })?;
    app.run_for(Duration::from_millis(50))
}

fn return_to_foreground(app: &mut Headless) -> Result<(), String> {
    app.set_environment("setLifecycle(active)", |e| {
        e.lifecycle = Lifecycle::Active;
    })?;
    app.run_for(Duration::from_millis(50))
}

#[test]
fn r_availability_wakes_only_on_change() -> Result<(), String> {
    let fake = fake();
    fake.set_check_permissions(Ok(PermissionState::Granted));
    fake.set_devices(Ok(vec![InputDevice {
        id: "built-in-mic".to_owned(),
        label: Some("Built-in microphone".to_owned()),
    }]));
    let mut app = start()?;
    app.wait_for("ready granted true")?;

    // Back and to the foreground with the same answer: the binding must not run again.
    let settled = RUNS.load(SeqCst);
    leave_foreground(&mut app)?;
    return_to_foreground(&mut app)?;
    app.run_for(Duration::from_millis(100))?;
    assert_eq!(
        RUNS.load(SeqCst),
        settled,
        "the same answer must not run the binding again"
    );

    // The permission changed while the app was away; the foreground check must carry it in.
    leave_foreground(&mut app)?;
    fake.set_check_permissions(Ok(PermissionState::Denied));
    return_to_foreground(&mut app)?;
    app.wait_for("ready denied true")?;

    // A failed check is a change too, and it reports its code.
    leave_foreground(&mut app)?;
    fake.set_check_permissions(Err(RecorderError::new(ErrorKind::Backend)));
    return_to_foreground(&mut app)?;
    app.wait_for("failed ERR_MICROPHONE_BACKEND")?;

    // A refresh with a changed answer reaches the binding without leaving the foreground.
    fake.set_check_permissions(Ok(PermissionState::Granted));
    app.press("role:button[Refresh]")?;
    app.run_for(Duration::from_millis(50))?;
    app.wait_for("ready granted true")?;
    Ok(())
}
