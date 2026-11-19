import { mount, signal, Text, View } from "nsh";
import { Recorder, memoryBackend } from "../../src";

const backend = memoryBackend();
const recorder = new Recorder({ backend });

const level = signal("level none");
const heard = signal(0);
let heardCount = 0;

// The listener runs on setInterval, which the test's manual clock drives.
const stopListening = recorder.onAmplitudeChanged(100, (amplitude) => {
  heardCount += 1;
  heard.set(heardCount);
  level.set(`level ${amplitude.average} ${amplitude.peak}`);
});

function App() {
  return (
    <View class="flex-1 gap-3 p-4">
      <Text testID="level" class="text-foreground">{level()}</Text>
      <Text testID="heard" class="text-foreground">{`heard ${heard()}`}</Text>
      <Text testID="record" role="button" onPress={() => void recorder.start({})} class="rounded-(--radius) bg-muted px-3 py-2">
        Record
      </Text>
      <Text testID="pause" role="button" onPress={() => void recorder.pause()} class="rounded-(--radius) bg-muted px-3 py-2">
        Pause
      </Text>
      <Text testID="stop" role="button" onPress={() => void recorder.stop()} class="rounded-(--radius) bg-muted px-3 py-2">
        Stop
      </Text>
      <Text
        testID="loud"
        role="button"
        onPress={() => backend.setAmplitude({ average: 0.5, peak: 0.75 })}
        class="rounded-(--radius) bg-muted px-3 py-2"
      >
        Loud
      </Text>
      <Text testID="unsubscribe" role="button" onPress={stopListening} class="rounded-(--radius) bg-muted px-3 py-2">
        Unsubscribe
      </Text>
    </View>
  );
}

mount(App);
