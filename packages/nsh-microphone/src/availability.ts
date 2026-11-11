import { cleanup, effect, lifecycle, signal } from "nsh";
import { asRecorderError, type RecorderError } from "./errors";
import type { PermissionState } from "./options";
import { Recorder } from "./recorder";

/** What the device can do for recording right now. */
export interface Availability {
  permission: PermissionState;
  hasInputDevice: boolean;
}

/** Where the availability read stands. */
export type AvailabilityState =
  | { status: "checking" }
  | { status: "ready"; availability: Availability }
  | { status: "failed"; error: RecorderError };

/** A reactive read of `Availability`. It checks at start, and again on each return to the foreground. */
export interface MicrophoneAvailability {
  state(): AvailabilityState;
  refresh(): void;
}

export interface UseMicrophoneAvailabilityOptions {
  /** The recorder to ask. Defaults to the Rust crate's. */
  recorder?: Recorder;
}

// NSH 0.4.0's web runtime ships no `lifecycle` accessor: the import stays `undefined` there.
// Treat that as active, so the first read still runs; the foreground re-check comes back with an NSH release that has it.
function lifecycleState(): "active" | "inactive" | "background" {
  return typeof lifecycle === "function" ? lifecycle() : "active";
}

/**
 * Reads what the device can do, as a reactive value. A binding on `state()` runs again only when
 * the answer changes. The read ends when the component goes.
 */
export function useMicrophoneAvailability(
  options: UseMicrophoneAvailabilityOptions = {},
): MicrophoneAvailability {
  const recorder = options.recorder ?? new Recorder();
  const state = signal<AvailabilityState>({ status: "checking" });
  let shown = "checking";
  let latest = 0;
  let gone = false;

  const show = (next: AvailabilityState) => {
    const key = describeKey(next);
    if (key === shown) return;
    shown = key;
    state.set(next);
  };

  const check = () => {
    // The handle can outlive its component, and a read then has no owner to land in.
    if (gone) return;
    const attempt = ++latest;
    readAll(recorder).then(
      (next) => {
        if (attempt === latest) show(next);
      },
      (error: unknown) => {
        if (attempt === latest) show({ status: "failed", error: asRecorderError(error) });
      },
    );
  };

  effect(() => {
    if (lifecycleState() === "active") check();
  });
  cleanup(() => {
    gone = true;
    latest += 1;
  });
  return { state: () => state(), refresh: check };
}

async function readAll(recorder: Recorder): Promise<AvailabilityState> {
  const permission = await recorder.checkPermissions();
  const devices = await recorder.listInputDevices();
  return { status: "ready", availability: { permission, hasInputDevice: devices.length > 0 } };
}

function describeKey(state: AvailabilityState): string {
  if (state.status === "ready") return `ready ${JSON.stringify(state.availability)}`;
  if (state.status === "failed") return `failed ${state.error.code}`;
  return "checking";
}
