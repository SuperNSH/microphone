import { mount, signal, Text, View } from "nsh";
import { asRecorderError, memoryBackend, Recorder } from "./src";

// The entry runs on a memory backend, so opening it never touches a real microphone.
const recorder = new Recorder({ backend: memoryBackend() });

const message = signal("idle");

function record() {
  recorder.start({ encoder: "wav", output: { kind: "file", path: "note.wav" } }).then(
    () => message.set(`state ${recorder.state()}`),
    (error: unknown) => message.set(asRecorderError(error).code),
  );
}

function stop() {
  recorder.stop().then(
    (recording) => message.set(`stopped ${recording.path}`),
    (error: unknown) => message.set(asRecorderError(error).code),
  );
}

function App() {
  return (
    <View class="flex-1 gap-3 p-4">
      <Text testID="title" class="text-foreground">nsh-microphone</Text>
      <Text testID="message" class="text-foreground">{message()}</Text>
      <Text testID="record" role="button" onPress={record} class="rounded-(--radius) bg-muted px-3 py-2">
        Record
      </Text>
      <Text testID="stop" role="button" onPress={stop} class="rounded-(--radius) bg-muted px-3 py-2">
        Stop
      </Text>
    </View>
  );
}

mount(App);
