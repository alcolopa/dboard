# Contributing to dboard

Thanks for helping make database work easier. Bug reproductions, platform testing and documentation improvements are as useful as code.

## Before starting

Search existing issues. For a larger feature or a change to database writes, open an issue describing the user problem and proposed behavior before investing in an implementation. Pick a small, focused change for your first pull request. Current priorities are in [ROADMAP.md](ROADMAP.md).

Never include passwords, connection URLs with credentials, private schemas or production data in issues, screenshots or fixtures. Use a disposable local database and sanitized examples.

## Development setup

Follow the [README](README.md#build-from-source) to install Rust, a platform compiler and GUI dependencies. From `dboard-cross/`:

```sh
cargo run --locked -p dboard
cargo test --locked --workspace
```

Database integration tests need disposable local services and their environment variables. See the [developer guide](dboard-cross/README.md#build--test). Never point those tests at a production database. CI runs PostgreSQL 16 and MariaDB 10.11 integration tests and builds desktop binaries for all three platforms.

The `core` crate owns database behavior; the `app` crate owns the UI and workers. Slint files are under `crates/app/ui/`. Keep database operations off the UI thread.

## Make a pull request

1. Create a branch from `main` and keep the change focused.
2. Add a regression test for a behavior fix, especially SQL generation, write guards, data import/export or undo. Documentation changes do not need Rust tests.
3. Run the workspace tests for Rust changes. For UI changes, run the app and check the affected flow; mention which OS you tested.
4. Update documentation when behavior or setup changes.
5. Describe the problem, resulting behavior and validation in the pull request. Include screenshots for visual changes and disclose checks you could not run.

For website edits, serve `docs/` locally with `python3 -m http.server 8000 --directory docs` from the repository root. Check desktop and mobile widths, keyboard navigation, download links, and the fallback when GitHub's release API is unavailable.

Keep dependency changes necessary and explain them. Preserve third-party license notices. Contributions are provided under the repository's [MIT license](LICENSE). Follow the [code of conduct](CODE_OF_CONDUCT.md); report vulnerabilities through [SECURITY.md](SECURITY.md).
