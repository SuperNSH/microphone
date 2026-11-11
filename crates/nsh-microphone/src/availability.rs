//! The reactive availability read: the answer is a signal that wakes its bindings only when it
//! changes.

use std::{cell::Cell, rc::Rc};

use nsh_core::platform::Lifecycle;
use nsh_core::prelude::*;

use crate::{error::RecorderError, options::PermissionState, recorder::Recorder};

/// What the microphone can do right now.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct Availability {
    /// Whether recording is allowed, as the platform last answered.
    pub permission: PermissionState,
    /// Whether a microphone exists to record from.
    pub has_input_device: bool,
}

impl Default for Availability {
    /// No answer and no device: what a check replaces. `PermissionState` has no default, so
    /// `Undetermined` is named here.
    fn default() -> Self {
        Self {
            permission: PermissionState::Undetermined,
            has_input_device: false,
        }
    }
}

/// Where the availability read stands.
#[derive(Debug, Clone, PartialEq)]
pub enum AvailabilityState {
    /// The first check has not answered yet.
    Checking,
    /// The last check answered.
    Ready(Availability),
    /// The last check failed. The error says why.
    Failed(RecorderError),
}

/// A reactive handle on [`Availability`]: a `Copy` handle bindings may hold freely. A binding on
/// [`MicrophoneAvailability::state`] runs again only when the answer changes.
#[derive(Clone, Copy)]
pub struct MicrophoneAvailability {
    state: Signal<AvailabilityState>,
    refresh: Stored<Rc<dyn Fn()>>,
}

impl MicrophoneAvailability {
    /// The current state. Reading it in a binding subscribes to changes.
    pub fn state(&self) -> AvailabilityState {
        self.state.get()
    }

    /// Checks again now, without waiting for the app to return to the foreground.
    pub fn refresh(&self) {
        let check = self.refresh.get();
        check();
    }
}

/// The platform's availability, read reactively in the current owner. See
/// [`MicrophoneAvailability`].
pub fn use_microphone_availability() -> MicrophoneAvailability {
    use_microphone_availability_for(Recorder::new())
}

/// [`use_microphone_availability`] for a given `Recorder`, such as one built on a fake backend.
pub fn use_microphone_availability_for(recorder: Recorder) -> MicrophoneAvailability {
    let owner = Owner::current();
    let state = signal(AvailabilityState::Checking);
    // Dropping a task cancels it, so a new check replaces a check still in flight.
    let inflight = stored(None::<Spawned<()>>);
    // The handle can outlive its owner, and a check then has no owner to land in.
    let gone = Rc::new(Cell::new(false));
    let check: Rc<dyn Fn()> = {
        let gone = Rc::clone(&gone);
        Rc::new(move || {
            if gone.get() {
                return;
            }
            let recorder = recorder.clone();
            let task = owner.run(|| {
                spawn(async move {
                    let answer = match read(&recorder).await {
                        Ok(availability) => AvailabilityState::Ready(availability),
                        Err(error) => AvailabilityState::Failed(error),
                    };
                    state.set(answer);
                })
            });
            drop(inflight.replace(Some(task)));
        })
    };
    // A permission can be revoked in the system settings, so returning to the foreground
    // re-checks.
    let on_foreground = check.clone();
    effect(move || {
        if lifecycle() == Lifecycle::Active {
            on_foreground();
        }
    });
    cleanup({
        let gone = Rc::clone(&gone);
        move || {
            gone.set(true);
            drop(inflight.replace(None));
        }
    });
    MicrophoneAvailability {
        state,
        refresh: stored(check),
    }
}

async fn read(recorder: &Recorder) -> Result<Availability, RecorderError> {
    Ok(Availability {
        permission: recorder.check_permissions().await?,
        has_input_device: !recorder.list_input_devices().await?.is_empty(),
    })
}
