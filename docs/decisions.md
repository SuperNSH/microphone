# Decisions

Each entry says what was chosen and why. `docs/parity.md` lists what each API does on each platform.

## Decided

### NSH 0.7.1

The build targets NSH 0.7.1, the release CI installs (`NSH_VERSION: v0.7.1` from SuperNSH/nsh-builds). It moved from 0.4.0 on 2026-11-12 with no source change, and to 0.5.1, a bug-fix release with no API changes, on 2026-11-17, to 0.6.0 on 2026-11-18, and to 0.7.1 on 2026-11-25. The nsh-core, nsh-engine and nsh-testing versions were verified on crates.io. API names are read from the published `v0.7.1` sources; NSH's HEAD is never used as an API reference.

### cpal on Windows and Linux, the AVAudioEngine tap on every Apple target

The brief's recommended path was "cpal on desktop". Windows and Linux take it (cpal 0.18.2, the WASAPI and ALSA hosts). macOS deviates from that line: it shares the Apple layer with iOS — objc2 0.6, block2 0.6 and objc2-avf-audio 0.3.2's AVAudioEngine input tap — instead of cpal. objc2-avf-audio is declared with `default-features = false` and an explicit feature list — std, alloc, AVAudioApplication, AVAudioBuffer, AVAudioEngine, AVAudioFile, AVAudioFormat, AVAudioIONode, AVAudioMixing, AVAudioNode, AVAudioSession, AVAudioSessionRoute, AVAudioSessionTypes, AVAudioSettings, AVAudioTypes, AVAudioTime — as `crates/nsh-microphone/Cargo.toml` records. AVAudioSessionTypes was added beyond the contract's original list, in a core fix round, because the Apple layer's session and route-change handling needs the types it exposes. The brief's own edge cases demand session handling cpal does not have: a route change must rebuild the audio graph, and a call interruption must stop the tap until an explicit restart, both AVAudioSession notifications. One Apple layer serves iOS and macOS, so the handling is written once. Android records through AudioRecord over JNI (jni 0.22.4 and ndk-context 0.1.1).

### Platforms in scope

iOS, macOS, Windows and Linux are real implementations. The web is an unsupported target: every backend call answers `ERR_MICROPHONE_NOT_AVAILABLE`, the precedent `nsh-biometrics` set. Device and emulator runs stay with the maintainers; CI has no devices.

### Stable error codes

Eight error kinds, each with a stable `ERR_MICROPHONE_*` code and one message, worded identically in the Rust crate and the TypeScript package, so an app matches on the code in either language. A platform's own status number travels with the error where the platform gives one: AVAudioSession error codes and osstatus on Apple, the JNI exception's detail on Android, cpal's error text on the desktop hosts.

### Stream chunks go through a sink, and pause drops them

Stream output is delivered as `AudioChunk`s through the `ChunkSink` handed to `start` (`onChunk` in TypeScript). A paused recording drops chunks instead of queuing them, so a resumed recording continues from live audio and memory stays flat over a long pause.

### The native TSX surface streams through a callback

NSH 0.6.0 lets a crate keep a TSX function as a `Callback<T>` and call it from another thread with `post` (the platform sink sends each chunk over a channel, and a task started with `CallCx::start` posts it on the engine thread); the `Stop` a listening call returns ends it. `NativeRecorder.onChunk` is built on that: `rustBackend` sets it before a stream `start` and ends it after `stop`, `cancel` and `dispose`. The samples cross as a number array, copied into an `Int16Array`; a `Vec<u8>` would be smaller, but this is the form that needs no byte packing. The same mechanism would carry `onError`; that stays open because the platform layers have no error channel (`docs/parity.md`). What no CI run shows is audio on a device.

The first version of this surface refused a stream output with `ERR_MICROPHONE_INVALID_ARGUMENT`, because the build then targeted NSH 0.4.0 and no chunk callback had been verified.

### Amplitude and state events: the crate polls, the platform does not push

`onStateChanged` is a listener list on the recorder's gate, fired after each change outside the lock. `onAmplitudeChanged` reads `amplitude()` on a timer: no platform layer meters on its own schedule, so a push would only move the timer into each layer. A Rust call needs an NSH owner (it spawns a task), and the TSX one uses `setInterval`.

### No demo app; the status page is the extra

`nsh-biometrics` ships `examples/demo` and the CI jobs that package, release and deploy it. This repo omits the demo deliberately: its extra is the static status page in `site/`, published by `.github/workflows/pages.yml`. ci.yml therefore stops at compile and test; pages.yml publishes the status page.

### Gates: run in CI only

AGENTS.md forbids building or testing on a developer machine, so every gate runs in `.github/workflows/ci.yml`. Run https://github.com/SuperNSH/microphone/actions/runs/44381766793 on master passed all of them. The typescript job installs `libasound2-dev` because `nsh` compiles the crate on Linux and links ALSA.

### The five references (STEP 0)

All five reference clones resolved to their canonical repositories, and none failed: [record](https://github.com/llfbandit/record), [react-native-audio-api](https://github.com/software-mansion/react-native-audio-api), [cpal](https://github.com/RustAudio/cpal), [nsh](https://github.com/longbridge/nsh) and [biometrics](https://github.com/SuperNSH/biometrics). The run verified all five, and the `v0.4.0` tag in `.refs/nsh`, before any area started.
