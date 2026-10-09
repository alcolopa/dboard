# dboard (macOS, Linux, Windows)

dboard is a single Rust + [Slint](https://slint.dev) codebase that runs on macOS, Linux and Windows.
A native desktop UI without a web view. See the [project README](../README.md) for downloads and [contribution guide](../CONTRIBUTING.md) to get involved.


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
- **Workspace**: sidebar with everything in the database (schemas → tables, views, collections, routines with their
  argument lists, sequences, triggers, types, indexes, events, extensions; live filter; right-click menu: open, open in a
  new tab, view / copy definition, run a routine in a query tab), any number of tabs (the strip scrolls; `Ctrl+Tab`,
  `Ctrl+1…9`, middle-click closes; pin, duplicate, reopen closed), virtualised data grid (column resize, sort, WHERE / JSON
  filter, pagination, page size), type-aware cells (NULL badge, boolean toggle, JSON editor, FK / PK markers).
- **Database switcher**: a drop-down in the top bar lists every database on the server; picking one reconnects / `USE`s
  it. MySQL and MongoDB also offer "All databases".
- **Selecting and copying**: click a cell, ranges with Shift+click, click a column header to select the whole
  column (Shift extends), click a row number for a row, the corner for everything. `Ctrl/Cmd+C` copies as tab-separated
  text (pastes cleanly into spreadsheets), `Ctrl/Cmd+Shift+C` adds the header row; the right-click menus also copy as CSV,
  JSON or SQL `INSERT`. Arrow keys move the selection.
- **Instant editing**: double-click, Enter or F2 saves immediately via a parameterised statement; `Tab` / `Shift+Tab` move
  to the neighbouring cell; "Edit row" edits every field of a row in one form. Works on tables with a primary key and, on
  PostgreSQL, on tables without one (the row is addressed by its `ctid`). `Ctrl/Cmd+V` pastes a value, a column or a block
  of cells (confirmation for more than one). Insert row, delete row (with confirmation), truncate, drop.
- **Undo**: cell edits, pastes and deleted rows are all recorded. `Ctrl/Cmd+Z` (or the top-bar button) reverts the last
  one; the Inspector's *Changes* tab and the History drawer list every change with its own *Undo* button (an older edit
  is refused while a newer edit of the same cell exists).
- **Users & access** (Database ▾ → *Users & access…*): list server accounts, add a user with a password and an access level
  for the current database (no access / read only / read & write / full control, or a server administrator), change a
  password, change a level, drop a user, and see what each user can do. PostgreSQL roles, MySQL / MariaDB accounts
  (`'user'@'host'`) and MongoDB users are supported.
- **Export / import**: *Database ▾ → Export database…* writes a SQL script (PostgreSQL: schemas, types, sequences, tables
  with constraints, data, functions, views, indexes, triggers, foreign keys; MySQL / MariaDB: `CREATE TABLE`, data, views,
  routines, triggers, events) or, for MongoDB, one JSON file (documents and indexes). *Import database…* runs a SQL script
  (including `pg_dump` files with `COPY` blocks and `mysqldump` files with `DELIMITER`) or restores a MongoDB export; on
  PostgreSQL it is all-or-nothing. *Import rows…* loads a CSV / TSV / JSON file into the open table.
- **Query editor**: run (Ctrl+Enter), EXPLAIN / EXPLAIN ANALYZE plan tree, autocomplete chips, persistent history,
  saved queries in folders. MongoDB: `db.coll.find({...}).sort({...}).limit(n)`, `aggregate([...])`, stage templates,
  whole-document JSON editor.
- **Safety**: destructive statements on Production/Staging require typing `CONFIRM`.
- **Also**: structure + DDL view, routine definitions, inspector (activity log, changes, metadata, query history and saved queries; **pin** it to
  keep it open across restarts, otherwise it closes when you click away; both modes reserve workspace space),
  Ctrl+K command palette, Ctrl+P object/column search, CSV / JSON / SQL export of results, light and dark themes.

Keyboard (`Cmd` replaces `Ctrl` on macOS; the toolkit maps it, so the same shortcuts work natively everywhere):
Ctrl+K palette · Ctrl+P search · Ctrl+N / Ctrl+T new query · Ctrl+Enter run · Ctrl+S save query · Ctrl+W close tab ·
Ctrl+Shift+T reopen · Ctrl+Tab / Ctrl+Shift+Tab / Ctrl+1…9 switch tabs · Ctrl+R refresh · Ctrl+Alt+I inspector ·
Ctrl+, preferences · Ctrl+Q quit · in the grid: Ctrl+C copy (Shift: with header) · Ctrl+V paste · Ctrl+A select all ·
Ctrl+Z undo · arrows move (Shift extends) · Enter / F2 edit · Tab next cell · Esc clear selection. Text fields keep
their own Ctrl+C / V / X / A / Z.

## Not done / known limits

- Column reordering; autocomplete assumes you are typing at the end of the text. SSH tunnels, syntax colors and ER diagrams are implemented (see below).
- Copy / paste work on the rows loaded in the grid (the current page), not the whole table; use Export for that.
- Exports are portable scripts, not byte-identical backups: owners, grants, comments, partitioning options and
  PostgreSQL extension-owned objects are not written. Imports cannot be cancelled once started. MongoDB has been tested and works; MongoDB user management remains unverified. PostgreSQL 16 and MariaDB 10.11 have been tested end to end.
- Inline editing of tables without a primary key is PostgreSQL-only; MySQL has no exact row id and stays read-only.
- The file picker on Linux needs the XDG desktop portal; without one *Browse…* does nothing and you can type the path.
- Query results are capped at 5,000 displayed rows; export writes what is loaded.
- Tested against PostgreSQL 16, MariaDB 10.11 and MongoDB.
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
- MySQL tables without a primary key and views are read-only; PostgreSQL tables can use `ctid` for inline editing. Browsing orders by primary key when available.
- The right-click menu is drawn in-window (the toolkit's native menu doesn't appear on every Linux backend).

## Newer features

- **Editor**: syntax colours, line numbers, bracket matching, `{{variables}}` that prompt for values, SQL snippets, *Run selection*.
- **Safety**: Production banner, read-only connections (the session itself is read-only on PostgreSQL / MySQL / SQLite), staged edits you review as old → new and apply in one transaction, transaction mode (Begin / Commit / Rollback), Stop button and a statement timeout (View → Query Timeout), and a local audit log of every write (View → Audit Log, stored in `audit.log` in the config folder).
- **Connections**: several open at once as tabs (reopened on launch), grouped by environment, SSH tunnel through your system `ssh`, TLS CA / client certificates, paste a `postgres://` / `mysql://` URL, SQLite files.
- **Password references**: the password field accepts `env:NAME`, `op://vault/item/field` (1Password CLI), `aws-rds-iam[:region]` (AWS CLI) or `gcloud-sql-iam` (gcloud CLI), resolved each time you connect.
- **ER diagrams**: use **Export diagram…** in the diagram footer to save the complete canvas as an SVG image.
- **Data**: import wizard with column mapping, preview and an error report; generate sample rows (database menu); pin a result and compare against it; export to CSV / JSON / SQL / Excel; ER diagram; schema diff (right-click a connection tab); plan view that flags the slowest node.
- **Backups**: File → Back Up Database Now, and File → Automatic Backups (every hour / 6 h / daily while dboard is open, keeping the newest 3 / 7 / 30) into `Downloads/dboard-backups/<connection>/`.
- **Hooks**: Help → Edit Hooks File… opens `hooks.json` in the config folder:

  ```json
  [{"event": "before_write", "command": "/usr/local/bin/check-ticket", "args": [], "timeout_secs": 10}]
  ```

  Events: `before_write` (a non-zero exit blocks the write and its output is shown), `after_write`, `on_connect`, `on_disconnect`. The command gets a JSON object on stdin (`event`, `connection`, `environment`, `database`, `statement`) and the same values as `DBOARD_*` environment variables. A hooks file that does not parse blocks writes rather than silently disabling your policy.
- **macOS releases** are signed and notarized when the repository has the secrets listed in `.github/workflows/release.yml`.

### Local macOS builds and Keychain access

Use a persistent local signing identity to keep Keychain authorization across updates:

```sh
packaging/macos/setup-local-signing.sh # once: private key is stored in Keychain
packaging/macos/run-local.sh           # build and reopen the signed app
```

The local app stays at `target/local/dboard.app` and uses `com.alcolopa.dboard`.
When switching from an ad-hoc build, macOS may ask once for each existing saved
password; choose **Always Allow** to authorize the new signed identity. Keychain
can still prompt when it is locked or its access policy changes. Launch subsequent
builds through `run-local.sh`; `cargo run` launches an ad-hoc binary with a different
identity. The local certificate is for development and does not provide Developer ID
notarization. The bundler automatically reuses it when no release identity is supplied.
