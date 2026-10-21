# Progress

## Status (2026-10-21)

Plan approved: Linux + Android only, personal use, recommendations accepted.
**M0 (bootstrap) is done for Linux. The Android part is blocked**; see Known issues.

### Done
- Monorepo:
  - Rust workspace with crates `core/api`, `core/testkit`, `apps/cadenza/src-tauri` and `xtask`.
  - pnpm workspace with `ui` and `apps/cadenza`.
  - Lints: rustfmt, clippy `-D warnings`, eslint (typescript-eslint strict), prettier, tsc strict.
- Tauri 2 shell: `build_info` command over IPC, CSP set, icons. Runs in dev and release.
  Builds `.deb` and AppImage.
- UI foundation: design tokens (`ui/src/theme/tokens.css`), Default Light/Dark, bundled Inter,
  Reduce Motion, vitest + Testing Library.
- `cargo xtask`:
  - `fixtures`: generate + self-check
  - `test`: generates fixtures if missing
  - `bench`
  - `ci`: everything
- Fixture generator v1 (`tools/fixtures/generate.py`) and independent self-check (`check.py`).
  150 entries in nine groups: formats, multichannel, gapless, tags, encodings, artwork, cue,
  corrupt and library. All entries pass.
- `cadenza-testkit`: typed manifest access for Rust tests.
- GitHub Actions workflow (`.github/workflows/ci.yml`): a Linux job (checks, tests, bundles) and an
  Android APK job. **It has never run**, because there is no remote repository yet.
- `tools/setup-linux.sh` installs system packages.
- Demo script: `docs/demos/M0.md`.

### Next
1. Unblock Android (see below), then do the Android spike from ROADMAP M0.
2. M1 (Foundation) on Linux: library DB, tag reading, scanner, search, decode with gapless,
   shared-mode playback, and the first real UI.

## Known issues / blockers
- **Android SDK/NDK cannot be downloaded here.** The environment's network policy denies
  `dl.google.com`. Fix it by adding `dl.google.com` to the environment's allowed domains, or by
  building the Android job in GitHub Actions once a repo exists (the runner image includes the
  SDK/NDK). The spike also needs the owner's phone to run against the criteria in ARCHITECTURE §11.
- **No remote repository.** The session's GitHub token is invalid, so nothing has been pushed.
  The work exists only in this container until a repo is connected.
- Fixture warnings are expected: ffmpeg decodes AAC without applying the edit list's end trim
  (D18). They are warnings, not failures, because ffmpeg isn't the system under test.
- The 200k-track synthetic library for performance budgets is deferred to M1, where the scanner
  that needs it is built.

## Decisions
See ARCHITECTURE.md §13. Added during M0: D14–D19.
