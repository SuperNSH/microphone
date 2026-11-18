# nsh-microphone

Recording to file or stream for voice notes and level meters. Not speech-to-text.

Part of the Native Shell (NSH) capability set. Same layout as the other SuperNSH repos: a Rust crate and a TypeScript package, docs in `docs/`, CI on GitHub Actions, and a web status page on GitHub Pages at https://supernsh.github.io/microphone/.

## Status

**Verified by CI.** Run https://github.com/SuperNSH/microphone/actions/runs/44381766793 passed every job on master: format, rust (Linux, macOS, Windows), cross-check (iOS, iOS simulator, Android, wasm) and typescript. The platform layers are compiled and linted there but not run on devices. Not implemented: detecting a revoked permission that yields silence (see docs/parity.md).

## Approach

cpal 0.18.2 on Windows and Linux. The Apple layer — an AVAudioEngine input tap through objc2-avf-audio — serves iOS and macOS both, because the session, interruption and route-change handling in [docs/brief.md](docs/brief.md)'s edge cases needs AVAudioSession, which cpal does not wrap. Android records through AudioRecord over JNI. The web is an unsupported target and answers `ERR_MICROPHONE_NOT_AVAILABLE`. Why macOS does not use cpal is recorded in [docs/decisions.md](docs/decisions.md).

## Layout

- `Cargo.toml`: the workspace; `crates/nsh-microphone` is the only member.
- `crates/nsh-microphone/`: the Rust crate — recorder, error model, backend seam, platform layers.
- `packages/nsh-microphone/`: the TypeScript package, same API names and behavior.
- `docs/`: brief, goals, decisions and parity.
- `site/`: static status page, published by `.github/workflows/pages.yml`.
- `.zcode/workflow.ts`: the phases this repo was built through.

## Checks

CI (`.github/workflows/ci.yml`) runs four gates: format (`cargo fmt --all -- --check`), rust (clippy and tests for the crate on Linux, macOS and Windows), cross-check (clippy for iOS, the iOS simulator, Android and wasm) and typescript (`nsh check`, `nsh build` with no warnings, `nsh test`). Every compile and test runs in GitHub Actions. Nothing is built on a developer machine.

## Licence

MIT. See [LICENSE](LICENSE).

Repository: https://github.com/SuperNSH/microphone
