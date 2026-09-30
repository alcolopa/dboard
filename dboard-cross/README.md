# dboard-cross: Linux & Windows client

The native macOS app lives in `../dboard` (Swift/SwiftUI). This folder is a separate, small and fast
**Rust + [Slint](https://slint.dev)** client for Linux and Windows. It shares no code with the Mac app;
it follows the same feature spec (see the root README). No web view, no runtime: one native binary.

## Layout

| Crate | Purpose |
| :--- | :--- |
| `crates/core` | Headless library: PostgreSQL / MySQL·MariaDB / MongoDB drivers behind one `Conn` API, dialect-aware SQL builders, production safety guards, edit history + undo, versioned config store, OS-keyring passwords. |
| `crates/app`  | Slint UI + a worker thread that owns the connection and all persisted state. |

## Features

- **Connections**: create/edit/duplicate/delete any number of saved connections; **no default host, user or
  database**; engine, environment (Production/Staging/Development/Local) and SSL mode per connection;
  MongoDB connection strings; *Test connection*.
- **Persistence** (survives restarts and updates): connections, settings, query history and saved queries live in
  the OS config dir (`~/.config/dboard`, `%APPDATA%\dboard`; override with `DBOARD_CONFIG_DIR`). Files are versioned,
  written atomically, ignore unknown fields, and a file that fails to parse is copied to `*.corrupt-<ts>` instead of
  being overwritten. The earlier bare-array format is migrated automatically.
- **Passwords** are stored only in the OS keyring (Secret Service / Windows Credential Manager), keyed by connection id,
  never in the JSON. If no keyring exists the app says so and asks again next time.
- **Workspace**: sidebar (schemas → tables / views / collections / routines / sequences, live filter, right-click menu),
  multi-tab (pin, duplicate, reopen closed), virtualised data grid (column resize, sort, WHERE / JSON filter, pagination,
  page size), type-aware cells (NULL badge, boolean toggle, JSON editor, FK / PK markers).
- **Instant editing**: double-click, Enter saves immediately via a parameterised statement; PK-only; saving / saved / error
  indicators; undo (button or palette). Insert row, delete row (with confirmation), truncate, drop.
- **Query editor**: run (Ctrl+Enter), EXPLAIN / EXPLAIN ANALYZE plan tree, autocomplete chips, persistent history,
  saved queries in folders. MongoDB: `db.coll.find({...}).sort({...}).limit(n)`, `aggregate([...])`, stage templates,
  whole-document JSON editor.
- **Safety**: destructive statements on Production/Staging require typing `CONFIRM`.
- **Also**: structure + DDL view, routine definitions, inspector (activity log, edit history, metadata),
  Ctrl+K command palette, Ctrl+P object/column search, CSV / JSON / SQL export, light and dark themes.

Keyboard: Ctrl+K palette · Ctrl+P search · Ctrl+N new query · Ctrl+Enter run · Ctrl+W close tab · Ctrl+Shift+T reopen ·
Ctrl+R refresh · Ctrl+Alt+I inspector (Cmd instead of Ctrl on macOS builds).

## Not done / known limits

- SSH tunnels; syntax **highlighting** in the SQL editor (Slint's text editor can't colour spans); column reordering;
  ER diagram; importing data.
- Autocomplete assumes you are typing at the end of the text.
- Query results are capped at 5,000 displayed rows; export writes what is loaded.
- Tested against PostgreSQL 16, MariaDB 10.11 and a MongoDB-wire-compatible server (FerretDB) - not a real `mongod`.
  Windows is built by CI but has not been run by hand.

## Build & test

```bash
cd dboard-cross
cargo run -p dboard                # run the app
cargo build --release -p dboard    # optimised, stripped binary
cargo test --workspace             # unit tests; integration tests run when their env var is set:
DBOARD_TEST_PG=localhost:5432:postgres:PASSWORD:postgres \
DBOARD_TEST_MYSQL=127.0.0.1:3306:USER:PASSWORD:DB \
DBOARD_TEST_MONGO=127.0.0.1:27017:USER:PASSWORD:DB cargo test -p dboard-core --test db
```

Linux build dependencies (Debian/Ubuntu): `libfontconfig1-dev libxkbcommon-dev libxkbcommon-x11-dev libx11-dev libxcb1-dev libwayland-dev`.

## Design notes

- Every cell is fetched as text and edits bind text parameters cast server-side
  (`SET "c" = $1::text::type WHERE "pk" = $2::text::type`), so any type works and PK lookups keep their indexes.
- Tables without a primary key, and views, are read-only. Browsing orders by primary key so edited rows don't jump.
- The right-click menu is drawn in-window (the toolkit's native menu doesn't appear on every Linux backend).
