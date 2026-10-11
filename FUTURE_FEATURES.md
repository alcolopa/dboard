# Future features

This document describes the planned feature backlog for dboard. All ten items below are planned work, not completed features. Priorities guide implementation order; release dates and database-engine coverage will be decided as each feature is scoped.

The goal is to improve daily database workflows, support larger datasets, and make data changes easier to understand and control.

## Feature overview

| Feature | What it adds | Priority |
| --- | --- | --- |
| Full-result exports | Stream entire query results to a file with progress and cancellation, beyond the 5,000-row display limit. | High |
| Smarter SQL autocomplete | Suggest tables, columns, and aliases at the cursor, including foreign-key join conditions. | High |
| Visual filter builder | Build grouped AND/OR conditions using column, operator, and value controls, with a SQL preview. | High |
| Follow relationships from a row | Open a referenced record or browse related child records directly from the grid. | High |
| Saved table views | Save column order, visibility, widths, sorting, and filters as named views. | High |
| Cancellable imports | Stop long imports with clear reporting of committed rows and rollback behavior. | High |
| Data profiling | Inspect null counts, distinct values, common values, ranges, and distributions. | Medium |
| Session and lock monitor | Inspect active queries, long transactions, and blocking relationships; stop queries when permitted. | Medium |
| Schema migration scripts | Turn schema differences into reviewable, dependency-aware migration scripts. | Medium |
| Masked exports | Replace sensitive fields with fake or redacted values when exporting development datasets. | Medium |

## 1. Full-result exports

**Priority: High**

Exporting a query should include its complete result, even when the grid only displays the first 5,000 rows. This will support reporting, analysis, and large data transfers without loading every row into memory.

### Planned scope

- Offer explicit choices to export the currently loaded rows or the complete query result.
- Stream rows to disk in bounded batches, beginning with CSV, JSON, and SQL formats.
- Show rows written, elapsed time, and cancellation status. Show a percentage only when the total is known.
- Explain when exporting requires running the query again and may capture newer data.
- Publish the final output file only after successful completion; clearly identify or remove incomplete output on cancellation or failure.
- Handle large Excel exports with worksheet limits and memory use accounted for before enabling that format.

### Completion criteria

A result larger than 5,000 rows exports completely without memory use growing with the entire dataset. Cancellation and failures leave an unambiguous file state, and complete-result export does not rerun a data-changing statement merely to obtain its results.

## 2. Smarter SQL autocomplete

**Priority: High**

Autocomplete should understand the cursor position and the surrounding query, so editing an existing statement is as useful as typing a new one.

### Planned scope

- Complete the token at the cursor, including text in the middle of a statement.
- Suggest schemas, tables, columns, SQL keywords, and aliases appropriate to the current context.
- Resolve qualified names such as `customers.email` and aliases such as `c.email`.
- Offer join conditions derived from foreign-key metadata.
- Respect engine-specific syntax, identifier quoting, and case behavior.
- Support keyboard navigation, acceptance, and dismissal without disrupting normal editor input.
- Refresh cached metadata after schema changes and keep suggestion generation responsive.

### Completion criteria

Suggestions work at arbitrary cursor positions, understand table aliases, and insert a valid completion without overwriting unrelated text. Foreign-key join suggestions use the correct columns, including composite keys.

## 3. Visual filter builder

**Priority: High**

Users should be able to construct common filters without writing a WHERE clause by hand, while still seeing what the app will execute.

### Planned scope

- Add filter rows with a column picker, type-appropriate operator, and value input.
- Support equality, comparisons, text matching, ranges, lists, and NULL checks where applicable.
- Allow nested groups joined by AND or OR.
- Preview the generated SQL and show bound values separately.
- Keep an advanced raw-filter mode available; explain when a raw expression cannot be represented visually.
- Validate values and bind them as parameters rather than concatenating user input into SQL.
- Define MongoDB support separately using a JSON query preview and its own operators.

### Completion criteria

Grouped conditions preserve their intended precedence, NULL checks behave correctly, and invalid values produce useful feedback before execution. The preview accurately represents the executed filter.

## 4. Follow relationships from a row

**Priority: High**

Relationship navigation should let users move through connected records without manually copying keys or writing lookup queries. For example, open a customer's orders from the customer row.

### Planned scope

- Extend foreign-key navigation into a consistent action for opening the referenced record in a new tab.
- Add a related-records action for tables that reference the selected row.
- Show the relationship name, target table, and applied filter in the destination view.
- Support composite keys, schema-qualified tables, and multiple relationships to the same table.
- Explain missing records, NULL references, and inaccessible tables.
- Preserve the source tab so users can return to their original record.
- Use declared SQL foreign keys initially; inferred MongoDB relationships require separate scope.

### Completion criteria

Parent and child navigation selects the correct records, including composite-key relationships, and leaves the source view intact. Existing FK navigation is reused and expanded rather than duplicated.

## 5. Saved table views

**Priority: High**

Users should be able to save reusable browsing layouts, such as a customer-support view showing only contact fields or an operations view filtered to recent failures.

### Planned scope

- Add column reordering and visibility controls.
- Save column order, hidden columns, widths, sorting, filters, and page size under a view name.
- Support several views for the same table, with create, rename, duplicate, and delete actions.
- Scope saved views to the connection, database, schema, and table.
- Allow a default view and a reset to the standard table layout.
- Store views locally and restore them across app restarts.
- Handle schema changes gracefully, identifying missing or renamed columns instead of silently applying an invalid view.

### Completion criteria

Switching views reliably restores the saved layout and filter. Views remain isolated between connections, survive restarts, and continue to open with useful feedback after schema changes.

## 6. Cancellable imports

**Priority: High**

Long imports should offer a stop action and explain precisely what happened to the data when the operation ends.

### Planned scope

- Add cancellation controls to row imports and database-script imports.
- Check for cancellation between parsing, reading, and writing batches; use driver cancellation where supported.
- Display progress, processed rows or statements, and committed work when known.
- Explain the transaction strategy before starting: a whole-import transaction or committed batches.
- Roll back uncommitted work where the engine supports it.
- Report partial completion for engines or statements that cannot be rolled back, including implicitly committed DDL.
- Preserve a useful error report after cancellation and explain whether restarting could duplicate data.

### Completion criteria

Cancellation is responsive at supported boundaries, returns the connection to a usable state, and accurately distinguishes committed, rolled-back, and unprocessed work. The UI never promises rollback for operations that cannot provide it.

## 7. Data profiling

**Priority: Medium**

Profiling will help users understand unfamiliar data and spot quality problems before writing queries or editing records.

### Planned scope

- Provide profiling for a table or selected columns.
- Show row counts, NULL counts and percentages, and distinct-value counts.
- Show common values and their frequencies.
- Provide min/max and distributions for compatible numeric and date columns, plus useful text-length statistics.
- Offer a bounded sample mode and an explicit full-data mode.
- Label sampled, estimated, and exact statistics, and indicate whether the current filter is included.
- Allow cancellation and keep expensive analysis off the UI thread.

### Completion criteria

Statistics are correct for their declared scope, compatible with column types, and clearly labeled when sampled or estimated. Profiling large tables remains controllable and does not silently trigger an expensive full scan.

## 8. Session and lock monitor

**Priority: Medium**

A server activity view will help diagnose slow queries and transactions that block other work.

### Planned scope

- List active sessions and queries, with database, user, duration, and execution state where available.
- Identify long-running transactions and show lock waits and blocking relationships.
- Provide manual refresh and an optional visible refresh interval.
- Offer query cancellation or session termination only when supported and permitted.
- Distinguish cancellation from termination and confirm consequential actions with the affected session's details.
- Explain missing monitoring permissions and engine-specific limitations.
- Start with PostgreSQL and MySQL/MariaDB; evaluate MongoDB monitoring separately. SQLite has no equivalent server-wide session monitor.

### Completion criteria

Blocking relationships match the server's reported state, refresh remains responsive, and permission failures are clear. Actions identify the intended session and explain their effect before execution.

## 9. Schema migration scripts

**Priority: Medium**

Schema diff should help produce a migration that a developer can review, save, and run through their normal deployment process.

### Planned scope

- Extend existing schema diff and SQL rendering into a workflow with explicit source and target selection.
- Generate a script that transforms the target toward the source schema.
- Order supported changes around dependencies such as tables, indexes, and foreign keys.
- Highlight destructive changes and keep them excluded or commented out until deliberately included.
- Show unsupported differences and assumptions rather than silently omitting them.
- Allow copying or saving the script without automatically executing it.
- Describe transaction limitations and provide reversal guidance only where a safe reversal is possible; dropped data cannot be restored by a reverse schema script.

### Completion criteria

Supported scripts apply successfully to representative test schemas, the transformation direction is unmistakable, and destructive or unsupported changes remain visible. Existing schema-diff capabilities are extended rather than presented as entirely new functionality.

## 10. Masked exports

**Priority: Medium**

Users should be able to create development datasets by transforming selected sensitive fields during export, without changing the source database.

### Planned scope

- Select columns to redact, replace with NULL, or populate with type-compatible fake values.
- Provide suitable replacements for names, email addresses, phone numbers, and other common fields.
- Preview transformed sample rows before starting the export.
- Support consistent replacement when the same original value appears repeatedly.
- Preserve relationships and uniqueness where configured, detecting conflicts that could make the exported dataset invalid.
- Save reusable masking profiles scoped to the relevant connection and tables.
- Apply transformations while streaming, so masking can work with full-result exports.
- Explain that unselected fields and combinations of retained values may still identify people; masking is not a guarantee of anonymization.

### Completion criteria

Selected fields are transformed in the output, the source remains unchanged, and exported values satisfy the configured type and relationship rules. A masking failure stops the export rather than silently emitting the original sensitive value.

## Suggested delivery order

1. **Saved table views** — deliver column organization and reusable browsing layouts.
2. **Follow relationships from a row** — build on existing FK metadata and navigation.
3. **Full-result exports** — remove the loaded-results export limitation and establish streaming infrastructure.
4. **Smarter SQL autocomplete** — improve the editor's everyday usefulness.
5. **Visual filter builder** — make common browsing filters easier to construct.
6. **Cancellable imports** — give long data transfers a clear stop and recovery path.
7. **Data profiling** — add insight into table contents and data quality.
8. **Session and lock monitor** — support server-side troubleshooting.
9. **Schema migration scripts** — develop the existing diff capabilities into a review workflow.
10. **Masked exports** — build on streaming exports to create transformed datasets.

This is an initial sequence, not a fixed release schedule. Reliability fixes and platform validation from [ROADMAP.md](ROADMAP.md) continue alongside feature work. Each feature should document supported engines, provide clear empty/error/busy states, and receive appropriate behavioral and UI validation before being marked complete.
