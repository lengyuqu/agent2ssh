# R5 Desktop Control Plane Research Report

## Scope

This research reviewed whether the desktop Settings surface should continue expanding as a local operator control plane, and which daemon capabilities are safe to expose there without adding new backend protocol surface.

The reviewed paths were:

- Desktop app Settings menu and topbar state.
- Local daemon `/health`, `/gate`, and `/console` surfaces.
- Existing CLI/MCP daemon health, doctor, metrics, and gate capabilities.
- Current roadmap guidance in `docs/plan.md` and control-plane architecture in `docs/architecture.md`.

## Findings

The daemon already exposes enough local read-only health state for a useful desktop operator surface:

- `/health` is unauthenticated and returns `ok`, `version`, `uptime_secs`, `config_dir_available`, `ssh_available`, and `pid`.
- `/gate` is authenticated and already powers pause/resume recovery from desktop.
- `/console` is the existing browser console entry point; the desktop should link to it rather than duplicating every console feature.

The desktop Settings menu is the right place for these controls because it is always visible, already houses language/setup/import controls, and is less disruptive than adding another full page module.

## Implemented Outcome

The desktop Settings menu now provides:

- Local daemon health status with version, PID, and last check time.
- Manual daemon health refresh using `/health`.
- Local daemon lifecycle controls for start, stop, and restart using the bundled sidecar.
- First-run setup wizard daemon start using the same desktop sidecar command.
- Execution gate status with active, paused, and unavailable states.
- Manual execution gate refresh.
- Web Console URL display, open action, and copy action.

Documentation was synchronized in:

- `README.md`
- `docs/architecture.md`
- `docs/guides/web-console-guide.md`
- `docs/api.yaml`

## Deferred Items

The following were intentionally not implemented during this pass:

- Remote daemon switching from the desktop Settings menu. Remote daemon operation already exists in CLI/API surfaces; adding it to Settings should wait for real multi-node dogfood.
- Full metrics and doctor reports in Settings. The menu should remain an operator summary. Detailed diagnostics belong in CLI/Web Console unless repeated user feedback shows otherwise.

Daemon lifecycle controls have been implemented for the bundled local sidecar. R1 package validation has since been updated in `docs/plan.md`; Windows packaged behavior was confirmed by user testing on 2026-06-22. New platform differences should now be tracked as explicit bug reports rather than broad validation debt.

## Validation

The implementation was verified with:

```bash
npm run build
(cd src-tauri && cargo test)
npm run tauri:build
```

The post-implementation regression was re-run on 2026-06-18. No test bugs were found. `npm run build` passed, `cargo test` passed with 161 unit tests, 27 CLI smoke tests, and 56 daemon integration tests, and `npm run tauri:build` produced the macOS `.app` and `.dmg` bundles.

Follow-up validation on 2026-06-22 confirmed the current closure baseline: Windows runtime smoke was user-confirmed, frontend/backend performance work completed, desktop i18n static audit reported 442 checked keys with 0 missing translations and 0 placeholder mismatches, and `npm run tauri:build` regenerated the macOS `.app` and `.dmg` bundles.

## Next Recommendation

No broad R5 follow-up remains. Cross-platform, performance, and desktop-control-plane follow-up should be limited to concrete bugs or regressions found during normal use.
