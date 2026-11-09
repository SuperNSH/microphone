# Microphone and recording: brief

Recording to file or stream for voice notes and level meters. Not speech-to-text.

## Options

- cpal: Rust capture on desktop
- AVAudioEngine input tap: Apple
- AudioRecord or MediaRecorder with a foreground service: Android
- Reference only: react-native-audio-api, Flutter record

## Recommended path

cpal on desktop. Port the Apple and Android handling rules into the native layers.

## Edge cases to design for

- Route changes rebuild the audio graph (headphones, Bluetooth)
- Call interruptions stop the tap; restart explicitly
- Sample-rate mismatch: record at hardware rate, resample
- Android 14+ needs the microphone foreground service type declared
- Revoked permission can yield silence, not an error: detect and report

## Research status

Research done 2026-11-09 by web search. Versions and licences are unverified until checked on the registry before pinning. Full catalogue: the research notes for the SuperNSH library set.
