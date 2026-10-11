# Agent instructions for dboard

## Performance is the number one engineering priority

Performance is our number one engineering priority. Every feature and code change
must keep the app responsive and use CPU, memory, disk and database round trips
efficiently. Treat avoidable work and performance regressions as defects, not as
cleanup to postpone. Correctness, data integrity, security and database operation
ordering remain mandatory constraints on every optimization.

Read [dboard-cross/PERFORMANCE.md](dboard-cross/PERFORMANCE.md) before changing app,
UI or database code. Preserve its existing optimizations and regression coverage.

- Keep database, file and expensive computation work off the UI thread. Any
  synchronous UI work must be small, bounded and justified; preserve the cached
  editor analysis and viewport-only drawing.
- Virtualize large lists and grids. Create editors and other expensive widgets
  only when needed. Update changed model items in place rather than rebuilding
  everything for a local change.
- Bound work and memory where semantics allow: use pagination, streaming, batches,
  capped suggestions and bounded concurrency. A display cap does not bound query
  memory. Never silently truncate complete exports or change query semantics.
- Borrow or move data instead of cloning whole tabs, metadata or result sets.
  Copy only the fields needed across worker/UI boundaries; exclude inactive
  results, original rows and staging history from display snapshots.
- Avoid repeated scans, parsing, formatting, allocations and database requests in
  loops or interaction handlers. Use indexed lookups, one-pass aggregation,
  cached analysis with correct invalidation, and precomputed sort/type keys.
  Do not introduce quadratic work where a straightforward linear approach exists.
- Coalesce obsolete transient input updates, but never drop writes or reorder
  actions across tab/session switches. Preserve cancellation and ordering barriers.
- Prefer the simplest efficient implementation. Do not add speculative caches,
  unbounded tasks, polling, dependencies or complexity without a measured need.
- For changes to hot paths, assess input size, time complexity, allocations,
  retained memory and round trips. Run relevant regression tests; compare release
  builds on representative workloads when claiming a speedup or when latency or
  memory may regress. Report measurements and checks that could not be run.

## Verification and existing work

The working tree may contain ongoing user changes. Preserve unrelated edits.
Follow CONTRIBUTING.md for development and validation. Documentation-only changes
do not require Rust tests. Runtime performance claims require release measurements;
passing tests or a static review alone does not establish a latency or memory budget.
