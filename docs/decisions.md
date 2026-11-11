# Decisions

Each entry says what was chosen and why. `docs/parity.md` lists what each API does on each platform.

## Decided

### NSH 0.4.0

The build targets NSH 0.4.0, the release `nsh-biometrics` pins and the one CI installs (`NSH_VERSION: v0.4.0` from SuperNSH/nsh-builds). The nsh-core, nsh-engine and nsh-testing versions were verified on crates.io. `.refs/nsh` carries the `v0.4.0` tag, so the build read the exact published sources; that repository's HEAD is not 0.4.0 and was never used as an API reference.

### cpal on Windows and Linux, the AVAudioEngine tap on every Apple target

The brief's recommended path was "cpal on desktop". Windows and Linux take it (cpal 0.18.2, the WASAPI and ALSA hosts). macOS deviates from that line: it shares the Apple layer with iOS — objc2 0.6, block2 0.6 and objc2-avf-audio 0.3.2's AVAudioEngine input tap — instead of cpal. objc2-avf-audio is declared with `default-features = false` and an explicit feature list — std, alloc, AVAudioApplication, AVAudioBuffer, AVAudioEngine, AVAudioFile, AVAudioFormat, AVAudioIONode, AVAudioMixing, AVAudioNode, AVAudioSession, AVAudioSessionRoute, AVAudioSessionTypes, AVAudioSettings, AVAudioTypes, AVAudioTime — as `crates/nsh-microphone/Cargo.toml` records. AVAudioSessionTypes was added beyond the contract's original list, in a core fix round, because the Apple layer's session and route-change handling needs the types it exposes. The brief's own edge cases demand session handling cpal does not have: a route change must rebuild the audio graph, and a call interruption must stop the tap until an explicit restart, both AVAudioSession notifications. One Apple layer serves iOS and macOS, so the handling is written once. Android records through AudioRecord over JNI (jni 0.22.4 and ndk-context 0.1.1).

### Platforms in scope

iOS, macOS, Windows and Linux are real implementations. The web is an unsupported target: every backend call answers `ERR_MICROPHONE_NOT_AVAILABLE`, the precedent `nsh-biometrics` set. Device and emulator runs stay with the maintainers; CI has no devices.

### Stable error codes

Eight error kinds, each with a stable `ERR_MICROPHONE_*` code and one message, worded identically in the Rust crate and the TypeScript package, so an app matches on the code in either language. A platform's own status number travels with the error where the platform gives one: AVAudioSession error codes and osstatus on Apple, the JNI exception's detail on Android, cpal's error text on the desktop hosts.

### Stream chunks go through a sink, and pause drops them

Stream output is delivered as `AudioChunk`s through the `ChunkSink` handed to `start` (`onChunk` in TypeScript). A paused recording drops chunks instead of queuing them, so a resumed recording continues from live audio and memory stays flat over a long pause.

### The native TSX surface records to files only

NSH 0.4.0 has no verified bridge for streaming chunks from a Rust backend into TSX, so `NativeRecorder.start` refuses a stream output with `ERR_MICROPHONE_INVALID_ARGUMENT`. The surface stays file-output-only until a verified chunk callback exists in an NSH release. The Rust `Recorder` streams through any `Backend`, and the TypeScript `Recorder` streams through a custom backend; only the surface over the compiled crate is limited. Marked Partial in `docs/parity.md`.

### No demo app; the status page is the extra

`nsh-biometrics` ships `examples/demo` and the CI jobs that package, release and deploy it. This repo omits the demo deliberately: its extra is the static status page in `site/`, published by `.github/workflows/pages.yml`. ci.yml therefore stops at compile and test; pages.yml publishes the status page.

### Gates: authored into CI, not run in this task

AGENTS.md forbids building or testing on a developer machine, so the local gates of this build were pure formatting and parse checks: `rustfmt --check` on each area's Rust files, a YAML load of `.github/workflows/ci.yml`, and `cargo fmt --all -- --check` over the joined tree. Clippy, `cargo test`, and `nsh check`/`nsh build`/`nsh test` are authored into `.github/workflows/ci.yml`. The task also forbade commits and pushes, so no CI run has happened; every status claim in this repo stays Authored until the first green run on `master`.

### The five references (STEP 0)

All five reference clones resolved to their canonical repositories, and none failed: [record](https://github.com/llfbandit/record), [react-native-audio-api](https://github.com/software-mansion/react-native-audio-api), [cpal](https://github.com/RustAudio/cpal), [nsh](https://github.com/longbridge/nsh) and [biometrics](https://github.com/SuperNSH/biometrics). The run verified all five, and the `v0.4.0` tag in `.refs/nsh`, before any area started.
