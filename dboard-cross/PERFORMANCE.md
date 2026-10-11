# Performance review

Reviewed on 2026-10-09. This records an optimization pass, not a claim that every
workload has reached its performance limit.

## Changes

- Grid inputs are created on first edit instead of for every instantiated cell.
  Rows remain virtualized, and an existing editor is retained for focus handling.
- Saved queries, query history and saved connections now use virtualized lists.
- SQL highlighting draws only the visible lines. Highlighting and bracket matching
  share cached analysis; long tokens no longer repeatedly count their characters.
  Bracket positions are stored only for actual brackets.
- Autocomplete borrows database metadata, stops at eight unique suggestions and
  avoids copying the statement into tab state a second time.
- The palette normalizes its query once, retains only the best 50 matches in stable
  order and constructs labels/actions only for candidates entering those results.
- Adjacent typing, search and column-resize commands can replace obsolete updates.
  Database operations and tab/session changes remain ordering barriers.
- Display snapshots exclude inactive result sets, original rows and staging
  history. Reloads, imports, inserts and cell edits copy only the context they need.
  Displayed cell values move into their models instead of being copied again.
- Sidebar metadata is grouped by schema in one pass; selection updates change
  existing tree items. Connection sort keys are computed once per connection.
- Staged overlays and review row tracking use indexed lookups. Inspector undo
  availability is computed in one reverse pass. Activity items update in place.
- CSV/TSV/SQL formatters append directly to their output, JSON serializes one row
  at a time, and Excel/JSON/SQL column types are classified once per export.
- Configuration writes serialize borrowed data through a buffered writer before
  atomic rename. Audit loading parses only the newest requested entries.
- PostgreSQL catalog reads are pipelined; MongoDB inspects at most four collections
  concurrently and avoids formatting sampled documents just to discover columns.
- PostgreSQL/MySQL query results are converted while streaming, removing the
  additional complete buffer of driver rows/messages.

## Verification

The optimized release build succeeded. All 125 unit, headless UI and SQLite tests
passed. The 10 external-database test entry points returned early because their
test-server configuration was absent. Clippy completed without errors; existing
warnings and a test-only sorting-style warning remain.

Regression coverage includes Unicode and long tokens, bracket matching, viewport
highlighting, autocomplete priority, stable palette ranking, command ordering,
staged edits with duplicate rows, snapshot contents, export compatibility, undo
availability, and creating/saving/reopening a grid editor through keyboard events.

An optimized standalone syntax comparison used a SELECT statement containing a
64,000-character quoted Unicode value. The previous implementation took about
170 ms per call; the revised analysis plus tokenization took about 0.46 ms.
This is a synthetic syntax benchmark, not an end-to-end scrolling measurement.
Differential checks also compared the previous and new syntax results. Bracket
positions after tabs now use the same visual columns as highlighting.

Run `cargo test --locked --workspace` and `cargo clippy --locked --workspace --all-targets`.
The download test needs a localhost listener. External database tests require the
disposable database configuration documented in README.md; without it they return
early. SQLite tests run against temporary local databases.

## Runtime checks still needed

Use `cargo run --release --locked -p dboard` for user-facing performance comparisons.
The release profile already enables optimization level 3 and full LTO.

Measure scrolling with representative wide tables, long values, large schemas and
large query results on the real renderer. Columns are not horizontally virtualized,
and arbitrary queries still retain their returned data before the UI's 5,000-row
display cap. Slow database statements also occupy the session's serial command
worker. Further changes to these areas should preserve editing, result counts,
export completeness and operation ordering, and be guided by runtime profiles.

## Preservation review — 2026-10-11

A static review of the current working tree found all optimization mechanisms
listed above still present. This review does not rerun the 2026-10-09 benchmarks
or certify runtime performance of the current UI changes.

| Principle | Current implementation |
| --- | --- |
| Lazy grid editors and virtualized rows/lists | `crates/app/ui/grid.slint`, `query.slint`, `connections.slint` |
| Shared cached syntax/bracket analysis and visible-line drawing | `crates/app/src/highlight.rs`, `main.rs`, `ui/editor.slint` |
| Borrowed metadata and eight unique autocomplete suggestions | `crates/app/src/suggest.rs`, `worker/query.rs` |
| Stable top-50 palette results and one-time query normalization | `crates/app/src/worker/palette.rs` |
| Adjacent transient command coalescing with ordering barriers | `crates/app/src/worker/mod.rs`, `main.rs` |
| Minimal request/display snapshots and moved cell values | `crates/app/src/worker/mod.rs`, `tabs.rs`, `transfer.rs`, `editing.rs` |
| One-pass schema grouping, in-place selection and cached connection sort keys | `crates/app/src/worker/tree.rs`, `mod.rs` |
| Indexed staging/review, reverse-pass undo availability and in-place activity | `crates/app/src/worker/staging.rs`, `palette.rs`, `mod.rs`, `crates/core/src/edit.rs` |
| Direct export formatting and once-per-export type classification | `crates/app/src/export.rs` |
| Buffered borrowed configuration serialization and limited audit parsing | `crates/core/src/config.rs` |
| Pipelined PostgreSQL catalog reads and four concurrent MongoDB inspections | `crates/core/src/pg.rs`, `mongo.rs` |
| Streamed PostgreSQL/MySQL driver-result conversion | `crates/core/src/pg.rs`, `mysql.rs` |

The runtime checks and limitations above still apply. SQLite also performs an
exact `COUNT(*)` when loading a table page (`crates/core/src/sqlite.rs`); this can
add latency on large tables or expensive filters and should be profiled before
changing count behavior. Audit parsing is limited to requested newest entries,
but the underlying audit files are still read into strings; it is not bounded
file I/O.

Repository-wide [agent instructions](../AGENTS.md) now make performance the number
one engineering priority and require preserving these mechanisms, avoiding
unnecessary work and measuring relevant changes without compromising correctness.
