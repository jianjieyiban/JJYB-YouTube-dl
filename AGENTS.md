# AGENTS

## Overview
This project uses Vue 3 with TypeScript for the frontend and Rust for the backend.
Vue code lives in `src/` and `src/components`, while the Rust Tauri backend lives in `src-tauri/`.

## Testing
Before committing changes, run the following commands:

### JavaScript/TypeScript
- `npm run lint:fix` to apply ESLint fixes and formatting.
- `npm run test:unit` to run unit tests.
- `npm run test:e2e` to run end-to-end tests.
- `npm run build` to ensure the app builds.

To run e2e tests locally, install Playwright browsers and required system packages:

- `npx playwright install --with-deps`

### Rust (inside `src-tauri/`)
- `cargo fmt --all` to format Rust code.
- `cargo clippy --all-targets -- -D warnings` to lint.
- `cargo test` to run backend tests.

Linux requires platform libraries such as `glib-2.0`; report missing prerequisites rather than weakening checks.

## Release and regression rules
- Keep package.json, package-lock.json, Cargo.toml, Cargo.lock and tauri.conf.json versions consistent. Update user documentation and rebuild the executable before relabeling portable artifacts; recompute SHA256 after copying.
- Run full-repository `cargo fmt --manifest-path=src-tauri/Cargo.toml --all -- --check`, frontend tests/build/lint, Rust tests and strict Clippy. Check remote CI after pushing; never describe pending or failed checks as passed.
- Download options can contain nested Vue proxies. Copy known plain fields and arrays explicitly; do not structuredClone reactive options. Cover failures before IPC and surface errors to the task.
- Verify download changes in the native app using actual output files. Thumbnail-only tasks must skip media, track the after_video thumbnail manifest, and verify a real image before success. Keep local HTTP fixture results separate from real-site acceptance.
- Discover and validate existing tools in portable/sibling/managed bin directories and PATH before installing missing tools. Keep explicit update checks separate; never force a redundant download just to start the app.
- Preserve current user settings, downloads, required tools, vendor sources and licenses during cleanup. Delete only inspected obsolete/test artifacts; never publish Cookie files, runtime profiles, caches or logs.
- Keep the upstream attribution and known HTTP 429 / unsigned updater limitations visible. Do not claim external-site or signed-upgrade acceptance without evidence.

## Temporary files and production startup
- Never create temporary files at drive roots such as `C:\tmp` or `D:\tmp`. Use a named subdirectory of the operating system temporary directory for short-lived staging and remove it after use. Keep build caches in the configured development/cache locations; preserve signing keys and user profiles.
- The libsodium dependency falls back to `/tmp/LATEST` when its source-build path contains spaces. Use an existing verified MSVC library via `SODIUM_LIB_DIR`, or a non-root, space-free build/cache path when a source rebuild is required. Do not silently leave fallback directories behind.
- Production builds must enable `custom-protocol` and embed frontend assets. Verify native startup with the development server stopped; keep the compile-time guard and release regression tests.
- Ordinary startup must not require an update transaction; only update child processes validate transaction paths and readiness. Keep isolation IPC permissions synchronized with settings commands.

## Searching
Use `rg` for searching the codebase; avoid recursive `grep` or `ls` commands.

## Commit style
Write commit messages in the form `type: summary`, e.g. `fix: update progress bar logic`.
