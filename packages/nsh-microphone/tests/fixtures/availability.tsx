import { mount, signal, Text, View } from "nsh";
import {
  type AvailabilityState,
  type MicrophoneAvailability,
  Recorder,
  memoryBackend,
  recorderError,
  useMicrophoneAvailability,
  type RecorderBackend,
} from "../../src";

/** How many times the permission was read. A signal, so the screen shows it. */
const reads = signal(0);
// Counted outside the signal: reading it in the check would subscribe the check's effect to it.
let readCount = 0;
let failNextCheck = false;
const memory = memoryBackend();
// The memory backend has no read hook, so the counter and the scripted failure live in a wrapper.
const backend: RecorderBackend = {
  ...memory,
  checkPermissions: async () => {
    if (failNextCheck) {
      failNextCheck = false;
      throw recorderError("ERR_MICROPHONE_NOT_AVAILABLE");
    }
    readCount += 1;
    reads.set(readCount);
    return memory.checkPermissions();
  },
};
const recorder = new Recorder({ backend });
// The handle escapes its component on purpose: a button outside the read refreshes it.
let handle: MicrophoneAvailability | null = null;

function describe(state: AvailabilityState): string {
  if (state.status === "checking") return "availability checking";
  if (state.status === "failed") return `availability failed ${state.error.code}`;
  return `availability ${state.availability.permission} device ${state.availability.hasInputDevice}`;
}

function Reader() {
  const availability = useMicrophoneAvailability({ recorder });
  handle = availability;
  let runs = 0;
  // The line counts its own runs, so a test sees whether a read ran the binding again.
  const line = () => {
    runs += 1;
    return `${describe(availability.state())} runs ${runs}`;
  };
  return <Text testID="availability" class="text-foreground">{line()}</Text>;
}

function App() {
  return (
    <View class="flex-1 gap-3 p-4">
      <Reader />
      <Text testID="reads" class="text-foreground">{`reads ${reads()}`}</Text>
      <Text
        testID="fail"
        role="button"
        onPress={() => {
          failNextCheck = true;
        }}
        class="rounded-(--radius) bg-muted px-3 py-2"
      >
        Fail
      </Text>
      <Text
        testID="refresh"
        role="button"
        onPress={() => handle?.refresh()}
        class="rounded-(--radius) bg-muted px-3 py-2"
      >
        Refresh
      </Text>
    </View>
  );
}

mount(App);
