// Phase plan for this repo's build. The executable script is
// .zcode/run-microphone-parity.workflow.ts; this file records the shape it followed.

export type Phase = "implement" | "audit" | "gate" | "join";

export const workflow = {
  repo: "SuperNSH/microphone",
  areas: [
    "goals",
    "parity",
    "parity-rn",
    "core",
    "apple",
    "android",
    "windows",
    "linux",
    "web",
    "nsh-surface",
    "ts-package",
    "rust-tests",
    "ts-tests",
    "ci-meta",
  ],
  // Per area: implement, up to 3 audit rounds with a fix round between each,
  // then a scoped formatting gate (rustfmt --check; a YAML load for ci.yml).
  perAreaPhases: ["implement", "audit", "gate"] as Phase[],
  // Join: one cross-area consistency audit plus cargo fmt --all -- --check over
  // the whole tree, then one routed fix round and one re-check.
  joinPhases: ["join"] as Phase[],
  // AGENTS.md: nothing builds or tests on a developer machine. Local gates are
  // formatting and parsing only; the compile/test gates are authored in
  // .github/workflows/ci.yml and run on GitHub Actions after a push.
  localGates: [
    "rustfmt --check (area-scoped)",
    "cargo fmt --all -- --check (join)",
    "YAML load of .github/workflows/ci.yml",
  ],
  ciGates: [
    "cargo fmt --all -- --check",
    "cargo clippy -p nsh-microphone --all-targets -- -D warnings (host matrix and cross targets)",
    "cargo test -p nsh-microphone --all-targets",
    "packages/nsh-microphone: nsh check; nsh build with no warnings; nsh test",
  ],
};
