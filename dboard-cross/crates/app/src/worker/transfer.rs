//! Transfer (part of the worker).

use super::*;

// ---------------------------------------------------------------------------------------------
// Export / import
// ---------------------------------------------------------------------------------------------

/// One record to import: (column, value) pairs, plus the raw JSON object when the source was JSON.
pub(crate) type Record = (Vec<(String, String)>, Option<String>);

impl Worker {
    pub(crate) fn xfer_progress(&self) -> impl FnMut(String) {
        let w = self.w.clone();
        move |m| ui(&w, move |st| st.set_xfer_info(m.into()))
    }

    pub(crate) fn xfer_done(&self, info: String, error: String) {
        ui(&self.w, move |st| {
            st.set_xfer_busy(false);
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
                let ext = if mongo { "json" } else { "sql" };
                let name = format!("{}-{}.{ext}", sanitize(&db), chrono::Local::now().format("%Y%m%d-%H%M%S"));
                (
                    format!("Export {db}"),
                    if mongo {
                        "Writes every collection (documents and indexes) to one JSON file that this app can import again.".to_string()
                    } else {
                        "Writes a SQL script that recreates the database: tables, constraints, indexes, views, routines, triggers and data. It runs on an empty database with this app, psql or mysql.".to_string()
                    },
                    downloads_dir().join(name).display().to_string(),
                    true,
                )
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
            let ext = if mongo { "json" } else { "sql" };
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
        match self.xfer_mode {
            0 => self.run_export(path, a, b).await,
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

    pub(crate) async fn run_export(&mut self, path: String, schema: bool, data: bool) {
        if !schema && !data {
            return self.xfer_done(String::new(), "Choose structure, data, or both.".into());
        }
        ui(&self.w, |st| {
            st.set_xfer_busy(true);
            st.set_xfer_error("".into());
            st.set_xfer_info("Exporting…".into());
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
        let Some(tab) = self.active_tab().filter(|t| t.kind == Kind::Table).cloned() else { return };
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
        let Some(tab) = self.active_tab().filter(|t| t.kind == Kind::Table).cloned() else { return };
        let Some(table) = self.conn.as_ref().and_then(|c| c.table(&tab.schema, &tab.name)).cloned() else { return };
        let all_cols: Vec<String> = table.columns.iter().map(|c| c.name.clone()).collect();
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
                progress(format!("Processed {n} of {total}…"));
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
