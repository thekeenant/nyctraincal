# Contributing

## Releasing a new version

1. Install `cargo-release` (one-time):
   ```bash
   cargo install cargo-release
   ```

2. Bump the version and release:
   ```bash
   cargo release patch   # 0.1.0 → 0.1.1
   cargo release minor   # 0.1.0 → 0.2.0
   cargo release major   # 0.1.0 → 1.0.0
   ```

   This will update `Cargo.toml`, create a commit, tag it (e.g. `v0.1.1`), and push — which triggers the Docker workflow to publish the new image to `ghcr.io/thekeenant/nyctraincal`.
