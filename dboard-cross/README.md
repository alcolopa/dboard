# dboard-cross: Linux & Windows client

The native macOS app lives in `../dboard` (Swift/SwiftUI). This folder is a separate,
small and fast **Rust + [Slint](https://slint.dev)** client for Linux and Windows. It shares
no code with the Mac app; it follows the same feature spec (see the root README).

No web view, no runtime: a single native binary.

## Layout

| Crate | Purpose |
| :--- | :--- |
| `crates/core` | Headless library: PostgreSQL driver, SQL builders, production safety guards, edit history/undo, saved connections (JSON) + passwords in the OS keyring. |
| `crates/app`  | Slint UI: connect form, schema tree, virtualized data grid with instant cell editing, SQL tab, destructive-statement confirmation. |

## Status

- [x] PostgreSQL: connect, schema tree, paginated/sortable/filterable grid, instant cell edit (parameterised, PK-only), NULL toggle, undo, SQL tab, Production/Staging confirmation guard
- [ ] MySQL / MariaDB
- [ ] MongoDB
- [ ] SQL syntax highlighting & autocomplete, EXPLAIN visualizer, command palette, query history
- [ ] Windows/Linux release packaging

## Build & test

```bash
cd dboard-cross
cargo run -p dboard                # run the app
cargo build --release -p dboard    # optimised, stripped binary

# Unit tests always run; the Postgres integration test runs when this is set
# (host:port:user:password:database):
DBOARD_TEST_PG=localhost:5432:postgres:postgres:postgres cargo test -p dboard-core
```

Linux build dependencies (Debian/Ubuntu): `libfontconfig1-dev libxkbcommon-dev libxkbcommon-x11-dev libx11-dev libxcb1-dev libwayland-dev`.

## Design notes

- Every cell is fetched as text (`col::text`) and edits bind text parameters cast server-side
  (`SET "c" = $1::text::type WHERE "pk" = $2::text::type`), so any Postgres type works and
  primary-key lookups keep using their indexes.
- Tables without a primary key are read-only, matching the Mac app.
- Passwords go to the OS keyring (Windows Credential Manager / Secret Service), never to the JSON config.
