//! Query (part of the worker).

use super::*;

/// Signalled by the Stop button (from the UI thread) to interrupt the running statement.
pub(crate) static CANCEL: tokio::sync::Notify = tokio::sync::Notify::const_new();

async fn exec_cancellable(c: &mut Conn, stmt: &str) -> dboard_core::Result<Rows> {
    // drop a stale permit from an earlier Stop click
    let _ = tokio::time::timeout(Duration::ZERO, CANCEL.notified()).await;
    let canceller = c.canceller();
    let mut fut = Box::pin(c.execute_query(stmt));
    tokio::select! {
        r = &mut fut => r,
        _ = CANCEL.notified() => {
            if canceller.cancel().await {
                fut.await
            } else {
                drop(fut);
                Err(dboard_core::Error::Db("Cancelled.".into()))
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Query editor, history, saved queries
// ---------------------------------------------------------------------------------------------

pub(crate) fn is_read_only(sql: &str) -> bool {
    let first = sql.trim_start().split_whitespace().next().unwrap_or("").to_uppercase();
    matches!(first.as_str(), "SELECT" | "WITH" | "VALUES" | "SHOW" | "EXPLAIN" | "TABLE" | "DESCRIBE" | "DESC")
}

pub(crate) fn changes_schema(sql: &str) -> bool {
    let first = sql.trim_start().split_whitespace().next().unwrap_or("").to_uppercase();
    matches!(first.as_str(), "CREATE" | "ALTER" | "DROP" | "RENAME")
}

impl Worker {
    pub(crate) fn push_saved_and_history(&self) {
        let saved: Vec<(String, String, String, String)> = self.saved.iter().map(|q| (q.id.clone(), q.name.clone(), q.folder.clone(), one_line(&q.sql))).collect();
        let hist: Vec<(String, String, bool)> = self
            .history
            .iter()
            .take(100)
            .map(|h| (one_line(&h.sql), format!("{} · {:.1} ms{}", when(h.at), h.duration_ms, if h.ok { "" } else { " · failed" }), h.ok))
            .collect();
        ui(&self.w, move |st| {
            st.set_saved(ModelRc::new(VecModel::from(
                saved.into_iter().map(|(id, name, folder, sql)| SavedItem { id: id.into(), name: name.into(), folder: folder.into(), sql: sql.into() }).collect::<Vec<_>>(),
            )));
            st.set_history(ModelRc::new(VecModel::from(
                hist.into_iter().map(|(sql, meta, ok)| HistoryItem { sql: sql.into(), meta: meta.into(), ok }).collect::<Vec<_>>(),
            )));
        });
    }

    pub(crate) fn record_history(&mut self, sql: &str, ms: f64, ok: bool) {
        let cid = self.conn.as_ref().map(|c| c.config.id.clone()).unwrap_or_default();
        self.history.retain(|h| !(h.sql == sql && h.connection_id == cid));
        self.history.insert(0, HistoryEntry { sql: sql.to_string(), connection_id: cid, at: config::now_secs(), duration_ms: ms, ok });
        self.history.truncate(500);
        let _ = self.store.save_history(&self.history);
        self.push_saved_and_history();
    }

    pub(crate) fn query_edited(&mut self, text: String) {
        let mongo = self.is_mongo();
        let enabled = self.settings.autocomplete;
        if let Some(t) = self.active_mut() {
            t.query_text = text.clone();
        }
        let tables = self.conn.as_ref().map(|c| c.metadata.tables.clone()).unwrap_or_default();
        let s = if enabled { suggest::suggest(&text, &tables, mongo) } else { Vec::new() };
        ui(&self.w, move |st| st.set_suggestions(strs(s)));
    }

    pub(crate) fn set_query_text(&mut self, text: String) {
        if let Some(t) = self.active_mut() {
            t.query_text = text.clone();
        }
        ui(&self.w, move |st| {
            st.set_query_text(text.into());
            st.set_suggestions(strs(Vec::new()));
        });
    }

    pub(crate) async fn run_query(&mut self, text: String) {
        if text.trim().is_empty() {
            return;
        }
        if let Some(t) = self.active_mut() {
            t.query_text = text.clone();
        }
        self.run_with_vars(text).await;
    }

    /// Whether `text` only reads data. SQL is judged by its statements, MongoDB by the command.
    fn is_read_only_text(&self, text: &str) -> bool {
        if self.is_mongo() {
            let t: String = text.split_whitespace().collect::<Vec<_>>().join("");
            let writes = [".insert", ".update", ".delete", ".drop", ".remove", ".replace", ".bulkWrite", ".createIndex", ".rename", "$out", "$merge"];
            return !writes.iter().any(|w| t.contains(w));
        }
        use dboard_core::split::{split_all, Stmt};
        split_all(self.dialect(), text).into_iter().all(|s| match s {
            Stmt::Sql(t) => t.trim().is_empty() || is_read_only(&t),
            Stmt::Copy { .. } => false,
        })
    }

    pub(crate) async fn run_snippet(&mut self, text: String) {
        if text.trim().is_empty() {
            return;
        }
        self.run_with_vars(text).await;
    }

    /// Ask for `{{variable}}` values first when the text has any, then run.
    async fn run_with_vars(&mut self, text: String) {
        let vars = crate::vars::find(&text);
        if vars.is_empty() {
            return self.run_guarded(text).await;
        }
        let items: Vec<(String, String, String)> = vars
            .iter()
            .map(|v| (v.name.clone(), if v.default.is_empty() { String::new() } else { format!("default: {}", v.default) }, self.var_values.get(&v.name).cloned().unwrap_or_else(|| v.default.clone())))
            .collect();
        for (n, _, val) in &items {
            self.var_values.insert(n.clone(), val.clone());
        }
        self.var_pending = Some((text, items.iter().map(|i| i.0.clone()).collect()));
        ui(&self.w, move |st| {
            let v: Vec<FieldItem> = items.into_iter().map(|(n, h, val)| FieldItem { name: n.into(), type_name: "".into(), hint: h.into(), value: val.into(), is_null: false }).collect();
            st.set_vars_items(ModelRc::new(VecModel::from(v)));
            st.set_vars_open(true);
        });
    }

    /// Safety check for destructive statements, then execute.
    pub(crate) async fn run_guarded(&mut self, text: String) {
        if self.read_only() && !self.is_read_only_text(&text) {
            return self.set_banner("This connection is read-only: only queries that read data can run.", true);
        }
        if !self.is_mongo() && self.protected() && safety::is_destructive(&text) {
            let preview = one_line(&text);
            return self.ask(Pending::Query(text), "Destructive statement".into(), preview);
        }
        self.run_sql(text).await;
    }

    pub(crate) async fn run_sql(&mut self, sql: String) {
        let Some(i) = self.active else { return };
        // SQL scripts with several statements get one result set per statement.
        let stmts: Vec<String> = if self.is_mongo() {
            vec![sql.clone()]
        } else {
            use dboard_core::split::{split_all, Stmt};
            let parts: Vec<String> = split_all(self.dialect(), &sql)
                .into_iter()
                .filter_map(|s| match s {
                    Stmt::Sql(t) if !t.trim().is_empty() => Some(t),
                    _ => None,
                })
                .collect();
            if parts.len() > 1 { parts } else { vec![sql.clone()] }
        };
        let multi = stmts.len() > 1;
        let mut sets: Vec<ResultSet> = Vec::new();
        let mut failure: Option<String> = None;
        let mut total_ms = 0.0;
        let mut schema_changed = false;
        ui(&self.w, |st| st.set_running(true));
        for (n, stmt) in stmts.iter().enumerate() {
            let res = match self.conn.as_mut() {
                Some(c) => exec_cancellable(c, stmt).await,
                None => return,
            };
            match res {
                Ok(r) => {
                    total_ms += r.duration_ms;
                    let count = r.rows.len();
                    let mut rows = r.rows;
                    rows.truncate(RESULT_CAP);
                    let cols: Vec<ColMeta> = r.columns.iter().map(|n| ColMeta::plain(n)).collect();
                    let info = if count > RESULT_CAP { format!("{count} rows (showing the first {RESULT_CAP})") } else { format!("{count} row(s)") };
                    let what = if cols.is_empty() { stmt.trim_start().split_whitespace().next().unwrap_or("OK").to_uppercase() } else { format!("{count} row(s)") };
                    sets.push(ResultSet { label: format!("{}: {what}", n + 1), widths: auto_widths(&cols, &rows), cols, rows, info, timing: format!("{:.1} ms", r.duration_ms) });
                    schema_changed |= changes_schema(stmt);
                }
                Err(e) => {
                    failure = Some(if multi { format!("Statement {} failed: {e}", n + 1) } else { e.to_string() });
                    break;
                }
            }
        }
        ui(&self.w, |st| st.set_running(false));
        match failure {
            Some(msg) => {
                let t = &mut self.tabs[i];
                t.banner = msg.clone();
                t.banner_err = true;
                self.record_history(&sql, 0.0, false);
                self.log_activity(None, &format!("ERROR {msg}"));
            }
            None => {
                // Show the last statement that returned columns (usually the final SELECT).
                let idx = sets.iter().rposition(|s| !s.cols.is_empty()).unwrap_or(sets.len().saturating_sub(1));
                let t = &mut self.tabs[i];
                t.banner = String::new();
                t.banner_err = false;
                t.editable = false;
                t.page.offset = 0;
                if let Some(s) = sets.get(idx) {
                    Self::apply_set(t, s);
                }
                if multi {
                    t.timing = format!("{total_ms:.1} ms total");
                }
                t.results = if multi { sets } else { Vec::new() };
                t.result_idx = idx;
                self.record_history(&sql, total_ms, true);
                self.log_activity(Some(total_ms), &sql);
            }
        }
        if schema_changed {
            if let Some(c) = self.conn.as_mut() {
                let _ = c.refresh_metadata().await;
            }
            self.rebuild_tree();
        }
        self.show_active();
    }

    fn apply_set(t: &mut Tab, s: &ResultSet) {
        t.cols = s.cols.clone();
        t.widths = s.widths.clone();
        t.rows = s.rows.clone();
        t.page_info = s.info.clone();
        t.timing = s.timing.clone();
    }

    pub(crate) fn pick_result(&mut self, idx: usize) {
        let Some(i) = self.active else { return };
        let t = &mut self.tabs[i];
        let Some(s) = t.results.get(idx).cloned() else { return };
        Self::apply_set(t, &s);
        t.result_idx = idx;
        self.show_active();
    }

    pub(crate) async fn explain_query(&mut self, text: String, analyze: bool) {
        if text.trim().is_empty() {
            return;
        }
        if let Some(t) = self.active_mut() {
            t.query_text = text.clone();
        }
        // EXPLAIN ANALYZE really executes the statement.
        if analyze && self.protected() && !is_read_only(&text) {
            return self.ask(Pending::Explain(text.clone(), analyze), "EXPLAIN ANALYZE runs the statement".into(), one_line(&text));
        }
        self.run_explain(text, analyze).await;
    }

    pub(crate) async fn run_explain(&mut self, sql: String, analyze: bool) {
        let Some(i) = self.active else { return };
        let res = match self.conn.as_mut() {
            Some(c) => c.explain(&sql, analyze).await,
            None => return,
        };
        match res {
            Ok(r) => {
                let cols: Vec<ColMeta> = r.columns.iter().map(|n| ColMeta::plain(n)).collect();
                let t = &mut self.tabs[i];
                t.widths = auto_widths(&cols, &r.rows);
                if let Some(w) = t.widths.first_mut() {
                    *w = w.max(420.0);
                }
                t.cols = cols;
                t.page_info = format!("Execution plan{}", if analyze { " (analyze)" } else { "" });
                t.timing = format!("{:.1} ms", r.duration_ms);
                t.rows = r.rows;
                t.editable = false;
                t.banner = String::new();
                self.log_activity(Some(r.duration_ms), &format!("EXPLAIN {}", sql));
            }
            Err(e) => {
                let t = &mut self.tabs[i];
                t.banner = e.to_string();
                t.banner_err = true;
            }
        }
        self.show_active();
    }

    pub(crate) fn insert_template(&mut self, which: &str) {
        if !self.is_mongo() {
            let active = self.active_tab().filter(|t| t.kind == Kind::Table).map(|t| t.name.clone());
            let d = self.dialect();
            let text = self.conn.as_ref().and_then(|c| crate::snippets::sql_snippet(which, d, &c.metadata.tables, active.as_deref()));
            return match text {
                Some(t) => self.set_query_text(t),
                None => self.toast("No suitable table or foreign key found for that snippet."),
            };
        }
        let coll = self
            .active_tab()
            .filter(|t| t.kind == Kind::Table)
            .map(|t| t.name.clone())
            .or_else(|| self.conn.as_ref().and_then(|c| c.metadata.tables.first().map(|t| t.name.clone())))
            .unwrap_or_else(|| "collection".into());
        let cur = self.active_tab().map(|t| t.query_text.clone()).unwrap_or_default();
        let text = match which {
            "find" => format!("db.{coll}.find({{}}).limit(100)"),
            stage => {
                let snippet = match stage {
                    "$match" => r#"{"$match": {}}"#,
                    "$group" => r#"{"$group": {"_id": "$field", "count": {"$sum": 1}}}"#,
                    "$sort" => r#"{"$sort": {"field": -1}}"#,
                    "$project" => r#"{"$project": {"field": 1}}"#,
                    _ => r#"{"$limit": 100}"#,
                };
                let trimmed = cur.trim_end();
                match trimmed.strip_suffix("])") {
                    Some(head) if trimmed.contains("aggregate([") => {
                        let sep = if head.trim_end().ends_with('[') { "" } else { ", " };
                        format!("{}{sep}{snippet}])", head.trim_end())
                    }
                    _ => format!("db.{coll}.aggregate([{snippet}])"),
                }
            }
        };
        self.set_query_text(text);
    }

    pub(crate) fn load_into_query_tab(&mut self, sql: String) {
        if self.active_tab().is_some_and(|t| t.kind == Kind::Query) {
            self.set_query_text(sql);
        } else {
            self.new_query_tab(sql);
        }
        ui(&self.w, |st| st.set_drawer_open(false));
    }

    pub(crate) fn save_query_submit(&mut self, name: String, folder: usize) {
        let sql = self.active_tab().map(|t| t.query_text.clone()).unwrap_or_default();
        if sql.trim().is_empty() {
            return self.toast("Nothing to save");
        }
        self.saved.push(SavedQuery { id: config::new_id(), name, folder: FOLDERS[folder.min(FOLDERS.len() - 1)].to_string(), sql });
        self.saved.sort_by(|a, b| a.folder.cmp(&b.folder).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        let _ = self.store.save_saved_queries(&self.saved);
        self.push_saved_and_history();
        ui(&self.w, |st| st.set_saveq_open(false));
        self.toast("Query saved");
    }
}

