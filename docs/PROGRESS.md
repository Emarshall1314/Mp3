# Progress

## Status
Planning: `ARCHITECTURE.md` and `ROADMAP.md` drafted 2026-10-21 and **awaiting owner approval**.
No code yet.

## Next
1. Owner answers the open questions (ARCHITECTURE §14).
2. M0 bootstrap.

## Known issues / blockers
- The build container has no Xcode or Android SDK, so the mobile spike (M0) needs a Mac and devices, or a later session with mobile toolchains.
- `ffmpeg` and `sox` are not installed in the container. They are needed by the fixture generator and get installed in M0.
- The container is ephemeral. The repo is local only until a GitHub remote is provided.

## Decisions
See ARCHITECTURE.md §13.
