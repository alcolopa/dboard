//! Transfer (part of the worker).

use super::*;

// ---------------------------------------------------------------------------------------------
// Export / import
// ---------------------------------------------------------------------------------------------

/// One record to import: (column, value) pairs, plus the raw JSON object when the source was JSON.
pub(crate) type Record = (Vec<(String, String)>, Option<String>);

impl Worker {
    pub(crate) fn xfer_progress(&self) -> impl FnMut(Progress) {
        let w = self.w.clone();
        move |p| {
            let f = p.fraction.unwrap_or(-1.0);
            ui(&w, move |st| {
                st.set_xfer_info(p.text.into());
                st.set_xfer_progress(f);
            })
        }
    }

    pub(crate) fn xfer_done(&self, info: String, error: String) {
        ui(&self.w, move |st| {
            st.set_xfer_busy(false);
            st.set_xfer_progress(-1.0);
            st.set_xfer_info(info.into());
            st.set_xfer_error(error.into());
        });
    }

    pub(crate) fn open_transfer(&mut self, mode: i32) {
        let Some(conn) = &self.conn else { return };
        let mongo = conn.db_type() == DbType::Mongo;
        let db = if conn.config.database.is_empty() { conn.config.display_name() } else { conn.config.database.clone() };
        let (title, note, path, a) = match mode {
            0 => {
                self.xfer_tables = self.selected_tables();
                self.xfer_format = 0;
                let labels: Vec<String> = vec![if mongo { "JSON" } else { "SQL script" }.into(), "CSV".into(), "HTML".into()];
                let scoped = !self.xfer_tables.is_empty();
                ui(&self.w, move |st| {
                    st.set_xfer_formats(strs(labels));
                    st.set_xfer_format(0);
                    st.set_xfer_scoped(scoped);
                });
                let (title, note) = self.xfer_texts(&db, 0);
                (title, note, self.xfer_default_path(&db, 0), true)
            }
            1 => {
                let kind = if mongo { "a dboard MongoDB export (.json)" } else { "a SQL script (.sql), including pg_dump and mysqldump files" };
                (
                    format!("Import into {db}"),
                    format!("Runs {kind} against this database. Existing objects with the same names will make statements fail, so use an empty database for a clean restore."),
                    String::new(),
                    true,
                )
            }
            _ => {
                let Some(t) = self.active_tab().filter(|t| t.kind == Kind::Table) else { return self.toast("Open a table first, then import rows into it") };
                (
                    format!("Import rows into {}", t.name),
                    "Reads a CSV / TSV file (first row = column names) or a JSON file (an array of objects, or one object per line) and inserts each record as a new row. Empty values use the column default.".to_string(),
                    String::new(),
                    true,
                )
            }
        };
        self.xfer_mode = mode;
        self.xfer_map.clear();
        self.xfer_targets.clear();
        ui(&self.w, |st| {
            st.set_xfer_map_files(strs(Vec::new()));
            st.set_xfer_preview("".into());
        });
        ui(&self.w, move |st| {
            st.set_xfer_mode(mode);
            st.set_xfer_title(title.into());
            st.set_xfer_note(note.into());
            st.set_xfer_path(path.into());
            st.set_xfer_opt_a(a);
            st.set_xfer_opt_b(true);
            st.set_xfer_info("".into());
            st.set_xfer_error("".into());
            st.set_xfer_busy(false);
            st.set_xfer_open(true);
        });
    }

    pub(crate) async fn xfer_browse(&mut self) {
        let mongo = self.is_mongo();
        let mut dlg = rfd::AsyncFileDialog::new();
        let picked = if self.xfer_mode == 0 {
            let _ = mongo;
            let ext = self.xfer_ext(self.xfer_format);
            dlg = dlg.set_title("Save export").add_filter(ext, &[ext]).set_directory(downloads_dir());
            dlg.set_file_name(format!("export.{ext}")).save_file().await
        } else {
            dlg = dlg.set_title("Choose a file to import").set_directory(downloads_dir());
            dlg = if self.xfer_mode == 1 { dlg.add_filter("SQL / JSON", &["sql", "json", "txt"]) } else { dlg.add_filter("CSV / TSV / JSON", &["csv", "tsv", "txt", "json"]) };
            dlg.pick_file().await
        };
        if let Some(f) = picked {
            let p = f.path().display().to_string();
            let (p2, hdr) = (p.clone(), true);
            ui(&self.w, move |st| st.set_xfer_path(p.into()));
            if self.xfer_mode == 2 {
                self.xfer_preview(p2, hdr);
            }
        }
    }

    pub(crate) async fn xfer_run(&mut self, path: String, a: bool, b: bool) {
        let path = path.trim().to_string();
        if path.is_empty() {
            return;
        }
        if self.xfer_mode != 0 && self.read_only() {
            return self.xfer_done(String::new(), "This connection is read-only, so nothing can be imported.".into());
        }
        match self.xfer_mode {
            0 if self.xfer_format == 0 && self.xfer_tables.is_empty() => self.run_export(path, a, b).await,
            0 => self.run_export_tables(path).await,
            1 => {
                if self.protected() {
                    let env = self.env.label();
                    self.ask(Pending::ImportDatabase(path.clone(), a), format!("Import into {env}"), format!("Run every statement in {path} against this {env} database?"));
                } else {
                    self.run_import_db(path, a).await;
                }
            }
            _ => {
                self.xfer_stop_first = b;
                if self.protected() {
                    let env = self.env.label();
                    self.ask(Pending::ImportRows(path.clone(), a), format!("Import rows into {env}"), format!("Insert every record in {path} as a new row?"));
                } else {
                    self.run_import_rows(path, a).await;
                }
            }
        }
    }

    fn db_label(&self) -> String {
        match &self.conn {
            Some(c) if c.config.database.is_empty() => c.config.display_name(),
            Some(c) => c.config.database.clone(),
            None => String::new(),
        }
    }

    /// The tables an export covers: the sidebar selection, else every table in the database.
    fn xfer_table_list(&self) -> Vec<(String, String)> {
        if !self.xfer_tables.is_empty() {
            return self.xfer_tables.clone();
        }
        self.conn.as_ref().map(|c| c.metadata.tables.iter().map(|t| (t.schema.clone(), t.name.clone())).collect()).unwrap_or_default()
    }

    /// File extension for an export format (0 script / JSON, 1 CSV, 2 HTML).
    fn xfer_ext(&self, format: i32) -> &'static str {
        match format {
            1 => "csv",
            2 => "html",
            _ if self.is_mongo() => "json",
            _ => "sql",
        }
    }

    /// CSV (and MongoDB JSON of picked collections) writes one file per table into a folder.
    fn xfer_multi_files(&self, format: i32) -> bool {
        let per_table = format == 1 || (format == 0 && self.is_mongo() && !self.xfer_tables.is_empty());
        per_table && self.xfer_table_list().len() != 1
    }

    fn xfer_default_path(&self, db: &str, format: i32) -> String {
        let tables = self.xfer_table_list();
        let base = if self.xfer_tables.len() == 1 { sanitize(&self.xfer_tables[0].1) } else { sanitize(db) };
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let name = if self.xfer_multi_files(format) && !tables.is_empty() { format!("{base}-{stamp}") } else { format!("{base}-{stamp}.{}", self.xfer_ext(format)) };
        downloads_dir().join(name).display().to_string()
    }

    fn xfer_texts(&self, db: &str, format: i32) -> (String, String) {
        let mongo = self.is_mongo();
        let n = self.xfer_tables.len();
        let title = match n {
            0 => format!("Export {db}"),
            1 => format!("Export table {}", self.xfer_tables[0].1),
            _ => format!("Export {n} tables"),
        };
        let what = if n == 0 { "every table".to_string() } else { format!("the {n} selected table(s)") };
        let note = match format {
            1 if self.xfer_multi_files(1) => format!("Writes {what} as CSV files (one per table, with a header row) into a new folder. The path below is that folder."),
            1 => format!("Writes {what} as one CSV file with a header row."),
            2 => format!("Writes {what} to one HTML file with a table for each."),
            _ if n > 0 && mongo => format!("Writes {what} as JSON files (one per collection) into a new folder."),
            _ if n > 0 => format!("Writes {what} as SQL INSERT statements (data only; no structure)."),
            _ if mongo => "Writes every collection (documents and indexes) to one JSON file that this app can import again.".to_string(),
            _ => "Writes a SQL script that recreates the database: tables, constraints, indexes, views, routines, triggers and data. It runs on an empty database with this app, psql or mysql.".to_string(),
        };
        (title, note)
    }

    /// The format drop-down changed: keep the path's name but give it the right extension.
    pub(crate) fn xfer_set_format(&mut self, format: i32, path: String) {
        self.xfer_format = format;
        let db = self.db_label();
        let p = std::path::PathBuf::from(path.trim());
        let new_path = if path.trim().is_empty() {
            self.xfer_default_path(&db, format)
        } else if self.xfer_multi_files(format) {
            p.with_extension("").display().to_string()
        } else {
            p.with_extension(self.xfer_ext(format)).display().to_string()
        };
        let (title, note) = self.xfer_texts(&db, format);
        ui(&self.w, move |st| {
            st.set_xfer_title(title.into());
            st.set_xfer_note(note.into());
            st.set_xfer_path(new_path.into());
        });
    }

    /// Export chosen tables (or every table) as CSV, HTML, SQL INSERTs or, for MongoDB, JSON.
    pub(crate) async fn run_export_tables(&mut self, path: String) {
        let format = self.xfer_format;
        let tables = self.xfer_table_list();
        if tables.is_empty() {
            return self.xfer_done(String::new(), "There are no tables to export.".into());
        }
        ui(&self.w, |st| {
            st.set_xfer_busy(true);
            st.set_xfer_error("".into());
            st.set_xfer_info("Exporting…".into());
            st.set_xfer_progress(0.0);
        });
        let (multi, mongo, dialect) = (self.xfer_multi_files(format), self.is_mongo(), self.dialect());
        let mut progress = self.xfer_progress();
        let Some(conn) = self.conn.as_mut() else { return };
        let res = write_tables(conn, std::path::Path::new(&path), format, &tables, multi, mongo, dialect, &mut progress).await;
        match res {
            Ok((n, rows)) => {
                self.log_activity(None, &format!("EXPORT {n} table(s) to {path}"));
                self.xfer_done(format!("Done: {n} table(s), {rows} row(s) written to {path}"), String::new());
            }
            Err(e) => self.xfer_done(String::new(), e),
        }
    }

    pub(crate) async fn run_export(&mut self, path: String, schema: bool, data: bool) {
        if !schema && !data {
            return self.xfer_done(String::new(), "Choose structure, data, or both.".into());
        }
        ui(&self.w, |st| {
            st.set_xfer_busy(true);
            st.set_xfer_error("".into());
            st.set_xfer_info("Exporting…".into());
            st.set_xfer_progress(0.0);
        });
        let mut progress = self.xfer_progress();
        let res = match self.conn.as_mut() {
            Some(c) => c.export_database(std::path::Path::new(&path), &DumpOptions { schema, data }, &mut progress).await,
            None => return,
        };
        match res {
            Ok(st) => {
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                self.log_activity(None, &format!("EXPORT DATABASE {path}"));
                self.xfer_done(format!("Done: {} table(s), {} row(s), {} other object(s) · {:.1} KB written to {path}", st.tables, st.rows, st.objects, size as f64 / 1024.0), String::new());
            }
            Err(e) => self.xfer_done(String::new(), e.to_string()),
        }
    }

    pub(crate) async fn run_import_db(&mut self, path: String, stop: bool) {
        ui(&self.w, |st| {
            st.set_xfer_busy(true);
            st.set_xfer_error("".into());
            st.set_xfer_info("Importing…".into());
            st.set_xfer_progress(0.0);
        });
        let mut progress = self.xfer_progress();
        let res = match self.conn.as_mut() {
            Some(c) => c.import_database(std::path::Path::new(&path), &ImportOptions { stop_on_error: stop }, &mut progress).await,
            None => return,
        };
        match res {
            Ok(st) => {
                self.log_activity(None, &format!("IMPORT DATABASE {path}"));
                let mut info = format!("Done: {} statement(s) run", st.statements);
                if st.rows_copied > 0 {
                    info.push_str(&format!(", {} row(s) loaded", st.rows_copied));
                }
                let err = if st.errors.is_empty() {
                    String::new()
                } else {
                    format!("{} problem(s):\n{}", st.errors.len(), st.errors.iter().take(5).cloned().collect::<Vec<_>>().join("\n"))
                };
                self.xfer_done(info, err);
                self.rebuild_tree();
                self.load_databases().await;
                self.load_active().await;
            }
            Err(e) => {
                self.xfer_done(String::new(), e.to_string());
                self.rebuild_tree();
            }
        }
    }

    /// Records (column name, value) from a CSV / TSV / JSON file, plus the raw JSON object when the
    /// source was JSON (MongoDB keeps nested values that way).
    pub(crate) fn read_records(path: &str, header: bool, columns: &[String]) -> Result<Vec<Record>, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("Cannot read {path}: {e}"))?;
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let trimmed = text.trim_start_matches('\u{feff}').trim_start();
        let lower = path.to_lowercase();
        if lower.ends_with(".json") || trimmed.starts_with('[') || trimmed.starts_with('{') {
            let values: Vec<serde_json::Value> = match serde_json::from_str::<serde_json::Value>(trimmed) {
                Ok(serde_json::Value::Array(a)) => a,
                Ok(v @ serde_json::Value::Object(_)) => vec![v],
                _ => trimmed
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .enumerate()
                    .map(|(i, l)| serde_json::from_str(l.trim().trim_end_matches(',')).map_err(|e| format!("Line {}: {e}", i + 1)))
                    .collect::<Result<_, _>>()?,
            };
            return values
                .into_iter()
                .enumerate()
                .map(|(i, v)| {
                    let serde_json::Value::Object(m) = &v else { return Err(format!("Record {} is not a JSON object", i + 1)) };
                    let pairs = m
                        .iter()
                        .filter(|(_, x)| !x.is_null())
                        .map(|(k, x)| (k.clone(), match x { serde_json::Value::String(s) => s.clone(), other => other.to_string() }))
                        .collect();
                    Ok((pairs, Some(v.to_string())))
                })
                .collect();
        }
        let sep = if lower.ends_with(".tsv") || (trimmed.lines().next().unwrap_or("").matches('\t').count() > trimmed.lines().next().unwrap_or("").matches(',').count()) { '\t' } else { ',' };
        let mut grid = export::parse_delimited(&text, sep);
        let names: Vec<String> = if header {
            if grid.is_empty() {
                return Ok(Vec::new());
            }
            grid.remove(0).into_iter().map(|h| h.trim().to_string()).collect()
        } else {
            columns.to_vec()
        };
        Ok(grid
            .into_iter()
            .map(|r| (names.iter().cloned().zip(r).filter(|(_, v)| !v.is_empty()).collect(), None))
            .collect())
    }

    /// Read the chosen file, list its columns and propose a mapping onto the open table.
    pub(crate) fn xfer_preview(&mut self, path: String, header: bool) {
        let path = path.trim().to_string();
        if self.xfer_mode != 2 || path.is_empty() || self.is_mongo() {
            return;
        }
        let Some(tab) = self.active_tab().filter(|t| t.kind == Kind::Table).map(Tab::request_snapshot) else { return };
        let Some(table) = self.conn.as_ref().and_then(|c| c.table(&tab.schema, &tab.name)).cloned() else { return };
        let all_cols: Vec<String> = table.columns.iter().map(|c| c.name.clone()).collect();
        let records = match Self::read_records(&path, header, &all_cols) {
            Ok(r) => r,
            Err(e) => return self.xfer_done(String::new(), e),
        };
        let mut files: Vec<String> = Vec::new();
        for (pairs, _) in records.iter().take(200) {
            for (k, _) in pairs {
                if !files.contains(k) {
                    files.push(k.clone());
                }
            }
        }
        self.xfer_map = files.iter().map(|f| (f.clone(), all_cols.iter().find(|c| c.eq_ignore_ascii_case(f.trim())).cloned())).collect();
        self.xfer_targets = std::iter::once("(skip this column)".to_string()).chain(all_cols.iter().cloned()).collect();
        let preview = records
            .iter()
            .take(3)
            .enumerate()
            .map(|(i, (pairs, _))| format!("{}: {}", i + 1, pairs.iter().map(|(k, v)| format!("{k}={}", one_line(v))).collect::<Vec<_>>().join(", ")))
            .collect::<Vec<_>>()
            .join("\n");
        let total = records.len();
        self.push_xfer_map();
        ui(&self.w, move |st| {
            st.set_xfer_preview(preview.into());
            st.set_xfer_info(format!("{total} record(s) found. Check the column mapping, then Import.").into());
            st.set_xfer_error("".into());
        });
    }

    fn push_xfer_map(&self) {
        let files: Vec<String> = self.xfer_map.iter().map(|(f, _)| f.clone()).collect();
        let sel: Vec<i32> = self.xfer_map.iter().map(|(_, t)| t.as_ref().and_then(|t| self.xfer_targets.iter().position(|x| x == t)).unwrap_or(0) as i32).collect();
        let targets = self.xfer_targets.clone();
        ui(&self.w, move |st| {
            st.set_xfer_map_files(strs(files));
            st.set_xfer_map_targets(strs(targets));
            st.set_xfer_map_sel(ModelRc::new(VecModel::from(sel)));
        });
    }

    pub(crate) fn xfer_map_pick(&mut self, i: usize, j: usize) {
        let target = if j == 0 { None } else { self.xfer_targets.get(j).cloned() };
        if let Some(slot) = self.xfer_map.get_mut(i) {
            slot.1 = target;
        }
    }

    pub(crate) async fn run_import_rows(&mut self, path: String, header: bool) {
        let Some(tab) = self.active_tab().filter(|t| t.kind == Kind::Table).map(Tab::request_snapshot) else { return };
        let Some(table) = self.conn.as_ref().and_then(|c| c.table(&tab.schema, &tab.name)).cloned() else { return };
        let all_cols: Vec<String> = table.columns.iter().map(|c| c.name.clone()).collect();
        if !self.hook_gate(&format!("IMPORT rows into {}.{} from {path}", tab.schema, tab.name)) {
            let why = self.tabs.get(self.active.unwrap_or(0)).map(|t| t.banner.clone()).unwrap_or_default();
            return self.xfer_done(String::new(), why);
        }
        let stop_first = self.xfer_stop_first;
        ui(&self.w, |st| {
            st.set_xfer_busy(true);
            st.set_xfer_error("".into());
            st.set_xfer_info("Reading file…".into());
        });
        let records = match Self::read_records(&path, header, &all_cols) {
            Ok(r) => r,
            Err(e) => return self.xfer_done(String::new(), e),
        };
        // The mapping chosen in the dialog wins; without one, names are matched ignoring case.
        let mapped: std::collections::HashMap<String, Option<String>> = self.xfer_map.iter().cloned().collect();
        let resolve = |name: &str| -> Result<Option<String>, String> {
            if let Some(m) = mapped.get(name) {
                return Ok(m.clone());
            }
            table.columns.iter().find(|c| c.name.eq_ignore_ascii_case(name.trim())).map(|c| Some(c.name.clone())).ok_or_else(|| format!("the table has no column “{name}”"))
        };
        let mongo = self.is_mongo();
        let total = records.len();
        let mut progress = self.xfer_progress();
        let (mut ok, mut errors): (usize, Vec<String>) = (0, Vec::new());
        for (n, (pairs, raw)) in records.into_iter().enumerate() {
            let result: Result<(), String> = async {
                let conn = self.conn.as_mut().ok_or("Not connected")?;
                if let (true, Some(json)) = (mongo, raw) {
                    return conn.insert_document(&tab.schema, &tab.name, &json).await.map_err(|e| e.to_string());
                }
                let mut vals = Vec::new();
                for (k, v) in pairs {
                    if let Some(col) = resolve(&k)? {
                        vals.push((col, v));
                    }
                }
                conn.insert_row(&tab.schema, &tab.name, &vals).await.map_err(|e| e.to_string())
            }
            .await;
            match result {
                Ok(()) => ok += 1,
                Err(e) if stop_first => {
                    self.xfer_done(String::new(), format!("Record {} failed: {e}\n{ok} record(s) before it were inserted and kept.", n + 1));
                    self.load_active().await;
                    return;
                }
                Err(e) => errors.push(format!("record {}: {e}", n + 1)),
            }
            if n % 50 == 0 {
                progress(Progress::at(format!("Processed {n} of {total}…"), n, total));
            }
        }
        self.log_activity(None, &format!("IMPORT {ok} rows into {}.{}", tab.schema, tab.name));
        if errors.is_empty() {
            self.xfer_done(format!("Done: {ok} row(s) inserted into {}.", tab.name), String::new());
        } else {
            let report = downloads_dir().join(format!("import-errors-{}.txt", chrono::Local::now().format("%Y%m%d-%H%M%S")));
            let _ = std::fs::write(&report, errors.join("\n"));
            let head = errors.iter().take(5).cloned().collect::<Vec<_>>().join("\n");
            self.xfer_done(
                format!("{ok} row(s) inserted into {}, {} skipped.", tab.name, errors.len()),
                format!("{head}{}\nFull report: {}", if errors.len() > 5 { "\n…" } else { "" }, report.display()),
            );
        }
        self.load_active().await;
    }
}

/// Stream the rows of each table to disk page by page. Returns (tables, rows) written.
#[allow(clippy::too_many_arguments)]
async fn write_tables(
    conn: &mut Conn,
    target: &std::path::Path,
    format: i32,
    tables: &[(String, String)],
    multi: bool,
    mongo: bool,
    d: Dialect,
    progress: &mut dyn FnMut(Progress),
) -> Result<(usize, u64), String> {
    use std::io::{BufWriter, Write};
    const PAGE: i64 = 2000;
    let open = |p: &std::path::Path| -> Result<BufWriter<std::fs::File>, String> {
        if let Some(parent) = p.parent().filter(|x| !x.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| format!("Cannot write {}: {e}", p.display()))?;
        }
        std::fs::File::create(p).map(BufWriter::new).map_err(|e| format!("Cannot write {}: {e}", p.display()))
    };
    let werr = |e: std::io::Error| format!("Cannot write {}: {e}", target.display());
    let ext = match format {
        1 => "csv",
        2 => "html",
        _ if mongo => "json",
        _ => "sql",
    };
    let mut shared = if multi {
        std::fs::create_dir_all(target).map_err(|e| format!("Cannot create {}: {e}", target.display()))?;
        None
    } else {
        let mut w = open(target)?;
        if format == 2 {
            w.write_all(export::html_start().as_bytes()).map_err(werr)?;
        }
        Some(w)
    };
    let mut total_rows = 0u64;
    for (i, (schema, name)) in tables.iter().enumerate() {
        let label = if schema.is_empty() { name.clone() } else { format!("{schema}.{name}") };
        progress(Progress::at(format!("Exporting {label}…"), i, tables.len()));
        let mut own = if multi { Some(open(&target.join(format!("{}.{ext}", sanitize(&label))))?) } else { None };
        let out = own.as_mut().or(shared.as_mut()).ok_or("no output")?;
        let qualified = if schema.is_empty() { d.quote(name) } else { format!("{}.{}", d.quote(schema), d.quote(name)) };
        let page = Page { limit: PAGE, sort_ascending: true, ..Default::default() };
        let (mut offset, mut count) = (0i64, 0usize);
        let mut json_rows: Vec<Vec<Cell>> = Vec::new();
        let mut json_cols: Vec<ExportCol> = Vec::new();
        loop {
            let rows = conn.fetch_page(schema, name, &Page { offset, ..page.clone() }).await.map_err(|e| format!("{label}: {e}"))?;
            let cols: Vec<ExportCol> = rows
                .columns
                .iter()
                .map(|c| ExportCol { name: c.clone(), type_name: conn.table(schema, name).and_then(|t| t.columns.iter().find(|x| &x.name == c)).map(|x| x.type_name.clone()).unwrap_or_default() })
                .collect();
            let n = rows.rows.len();
            if offset == 0 {
                match format {
                    1 => {}
                    2 => out.write_all(export::html_table_start(&label, &cols).as_bytes()).map_err(werr)?,
                    _ if !mongo => out.write_all(format!("-- {label}\n").as_bytes()).map_err(werr)?,
                    _ => {}
                }
            }
            match format {
                1 => out.write_all(export::csv(&cols, &rows.rows, offset == 0).as_bytes()).map_err(werr)?,
                2 => out.write_all(export::html_rows(&rows.rows).as_bytes()).map_err(werr)?,
                _ if mongo => {
                    json_cols = cols;
                    json_rows.extend(rows.rows);
                }
                _ => out.write_all(export::sql_inserts(&cols, &rows.rows, &qualified, d).as_bytes()).map_err(werr)?,
            }
            count += n;
            offset += n as i64;
            if (n as i64) < PAGE {
                break;
            }
        }
        match format {
            2 => out.write_all(export::html_table_end(count).as_bytes()).map_err(werr)?,
            0 if mongo => out.write_all(export::json(&json_cols, &json_rows).as_bytes()).map_err(werr)?,
            0 => out.write_all(b"\n").map_err(werr)?,
            _ => {}
        }
        if let Some(w) = own.as_mut() {
            w.flush().map_err(werr)?;
        }
        total_rows += count as u64;
    }
    if let Some(mut w) = shared {
        if format == 2 {
            w.write_all(export::html_end().as_bytes()).map_err(werr)?;
        }
        w.flush().map_err(werr)?;
    }
    Ok((tables.len(), total_rows))
}
