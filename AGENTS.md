# GoToCS

GoToCS is a Tauri 2 desktop app that creates a local Omarchy VM on Apple Silicon macOS or Windows x86_64. The frontend uses React/TypeScript, the host backend uses Rust, and guest provisioning uses Python.

## Sources of truth

- Product parameters: `product.config.json`.
- Platform runtime, download and resource parameters: `runtime-profiles.json`.
- Build-time Windows assets: `asset-sources.json`.
- Local macOS asset paths: `build-assets.local.json`, copied from the tracked example and never committed.
- Build instructions: `docs/build.md`; implementation: `docs/architecture.md`; download source: `docs/image-mirror.md`.
- Resource-pack manifests are generated from the platform profiles. Do not edit only generated output.

## Behavior to preserve

- Guest application traffic must use the user's selected node. An independent network guard must block fallback to direct traffic when sing-box is unavailable.
- Set guest language, region and fixed timezone from the selected node's measured exit IP. Do not infer them from node names or host settings.
- Clipboard transfer is host-to-guest only. Access to the host over SSH is optional and defaults to off.
- Mark an environment configured only after the guest returns verified `ready`. A running process, successful build or `restart_required` does not establish acceptance.
- Preserve existing VM images, selected storage locations, partial downloads and Windows directory junctions. No automatic migration, deletion or graphics-mode retry.
- Subscription URLs, node credentials, SSH private keys and login state must not be printed or committed.

## Changes and verification

Read before editing and keep changes focused. Preserve unrelated work. Choose checks appropriate to the change: frontend type/build checks, Rust tests and platform checks, Python tests, or real UI verification. Clearly separate build results from Windows/macOS device acceptance.

Generated runtime packages, large images, local configuration, test evidence and user data are excluded from Git. Do not install global dependencies, change host system settings, deploy services, or publish releases without the user's authorization. Source updates and software releases are separate actions.
