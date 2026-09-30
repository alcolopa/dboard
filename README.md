# dboard: Modern Native macOS Database Client

A production-quality, high-performance native macOS database management application inspired by **Sequel Pro**, reimagined from the ground up for modern developers with the visual polish and ergonomics of **TablePlus**, **DataGrip**, and **Linear**.

Built with **Swift & SwiftUI** targeting macOS 14.0+, utilizing native macOS AppKit split views, macOS Keychain Services for secure credential storage, and an extensible driver architecture.

---

## ✨ Highlights & Architecture

### ⚡ Critical Feature: Instant Cell Editing (Zero Friction)
- **No "Apply" / "Save" Button**: Double-click or press `Enter` to edit any cell. When you press `Enter`, `Tab`, or navigate away, the application **immediately executes the required database operation**.
- **Inline Sync Indicators**:
  - `Spinner`: Subtle inline indicator while executing async database write.
  - `Checkmark`: Emerald green checkmark flash confirming persistence.
  - `Error Badge`: Crimson badge with popover detailing the exact error and offering 1-click **Retry** or **Revert to Original**.
- **Safe Automatic Writes**:
  - Automatically identifies primary keys (including composite primary keys).
  - Employs strict parameterized statements (`$1, $2` for PostgreSQL; `?` for MySQL; `updateOne` for MongoDB).
  - If a table lacks a primary key or unique identifier, a safety banner notifies you and restricts inline mutations to protect data integrity.

### 🛡️ Production Safety & Safeguards
- **Environment Classifications**: Visually tag connections as **Production**, **Staging**, **Development**, or **Local**.
- **Visual Safety Badges**: Production connections feature a bright red indicator and lock shield icon.
- **Destructive Operation Guards**: Destructive SQL commands (`DROP TABLE`, `TRUNCATE`, `DELETE FROM`) on Production connections require typing confirmation before execution.
- **macOS Keychain Integration**: Passwords and connection secrets are stored in the OS Keychain and never logged or exposed in plaintext.

### ↩️ Edit History & Reversible Undo (`⌘Z`)
- Local edit history records every single cell update, row insertion, and custom DDL execution.
- Clear distinction between UI edit undo and actual database rollback.
- Reverts database operations using inverted parameterized statements.

---

## 🚀 Supported Database Engines

1. **PostgreSQL / PGSQL**:
   - Schemas (`public`, custom schemas)
   - Tables with primary keys, foreign keys, and column comments
   - Views & Materialized Views
   - Stored Functions & Routines (PL/pgSQL)
   - Sequences & Triggers
   - `EXPLAIN` and `EXPLAIN ANALYZE` visualizer
2. **MySQL / MariaDB**:
   - Databases, tables, views, procedures, functions, and triggers
   - Backtick escaping and indexed lookup diagnostics
   - Parameterized statements
3. **MongoDB**:
   - Dedicated NoSQL document experience (never forced into a relational SQL grid)
   - Document Card view, raw JSON editor with syntax and schema validation
   - Aggregation Pipeline visual builder (`$match`, `$group`, `$sort`, `$project`, `$limit`)
   - Collection statistics and index inspector

---

## 🖥️ User Interface Overview

### 1. Three-Pane Developer Layout
- **Left Sidebar**:
  - Connection switcher with live status dots (Connected, Connecting, Error).
  - Live object search filter.
  - Collapsible tree for Schemas, Tables, Views, Routines, Sequences, and MongoDB Collections.
  - Right-click context menus (Open Data, Inspect Structure, Query Table, Insert Row, Truncate, Drop, Copy Name).
- **Center Workspace**:
  - Multi-tab system with tab icons, pinned tabs, tab duplication, and reopen closed tabs (`⇧⌘T`).
  - Independent sessions for SQL queries, table data, MongoDB documents, and schema inspectors.
- **Right Inspector Panel (`⌥⌘I`)**:
  - **Live Activity Log**: Real-time stream of all executed queries and parameterized updates with millisecond durations.
  - **Edit History & Undo**: Interactive undo stack with one-click operation revert.
  - **Database Metadata**: Connection URI, tables count, indexes count, environment safety mode.

### 2. Table Data Browser
- Spreadsheet-like virtualized grid with column resizing, reordering, and sorting.
- Type-aware cell renderers & inline editors:
  - `NULL` badge (1-click NULL toggle button)
  - Booleans (1-click toggle)
  - JSON (formatted modal editor with validation)
  - Foreign keys (target table badges)
  - Dates and timestamps
- Visual Condition Filter Builder + raw SQL `WHERE` clause input.
- Pagination bar with page size selection (25, 50, 100, 500, 1000) and latency timer.

### 3. SQL Query Editor
- Syntax highlighting and autocomplete suggestions (tables, columns, SQL keywords).
- Execute query or selection (`⌘↵`).
- Query execution timer and row counter.
- Visual `EXPLAIN / EXPLAIN ANALYZE` execution plan tree with cost and timing breakdown.
- Query History drawer and saved query folders (**Users**, **Analytics**, **Production**, **Debugging**).

### 4. Command Palette (`⌘K`) & Global Search (`⌘P`)
- **Command Palette (`⌘K`)**: Fuzzy search across commands, tables, saved queries, and connection switching.
- **Global Object Search (`⌘P`)**: Instant search across all tables, columns, routines, views, and collections.

---

## ⌨️ Keyboard Shortcuts

| Shortcut | Action |
| :--- | :--- |
| **`⌘K`** | Open Command Palette |
| **`⌘P`** | Global Object Search |
| **`⌘N`** | New SQL Query Tab |
| **`⌘↵`** | Execute Query / Current Statement |
| **`⌘W`** | Close Active Tab |
| **`⇧⌘T`** | Reopen Recently Closed Tab |
| **`⌘R`** | Refresh Database Metadata |
| **`⌘Z`** | Undo Most Recent Database Edit |
| **`⌥⌘I`** | Toggle Context Inspector Panel |
| **`Double-Click / ↵`** | Edit Cell in Table Grid |
| **`↵ / Tab / Click outside`** | **Instant Save Cell Edit to Database** |

---

## 🐧🪟 Linux & Windows

The Swift/SwiftUI app above is macOS-only. A separate, small native client for **Linux and Windows** (Rust + Slint,
no web view) lives in [`dboard-cross/`](dboard-cross/README.md). It supports PostgreSQL, MySQL/MariaDB and MongoDB,
stores connections and settings persistently (passwords in the OS keyring) and mirrors the workspace described here.

## 🌐 Website

A landing page lives in [`docs/`](docs/index.html) and deploys automatically to GitHub Pages via `.github/workflows/pages.yml` whenever `docs/` changes on `main`. One-time setup: in **Settings → Pages**, set **Source** to **GitHub Actions**.

## 📦 Building & Running

### Option 0: Download a Release
Every push of a `v*` tag (or a manual run of the **Release** workflow) builds on GitHub Actions and attaches to a [GitHub Release](../../releases): `dboard-<tag>-macos.zip` (drag `dboard.app` to `/Applications`), `dboard-<tag>-windows-x86_64.zip` (unzip, run `dboard.exe`) and `dboard-<tag>-linux-x86_64.tar.gz` (extract, run `./dboard`). The Windows exe is unsigned, so SmartScreen may warn on first launch (More info → Run anyway).

Since the app isn't notarized/signed, macOS Gatekeeper will block the first launch. Either right-click → **Open** and confirm, or run:
```bash
xattr -cr /Applications/dboard.app
```

### Option 1: Standalone Build Script
Run the automated build script to compile the native `dboard.app` bundle:
```bash
./build.sh run
```

### Option 2: Xcode Project
Open `dboard.xcodeproj` directly in Xcode:
```bash
open dboard.xcodeproj
```
All Swift files in `dboard/` are automatically synchronized via Xcode's synchronized groups. Select **My Mac** and click **Run (⌘R)**.
