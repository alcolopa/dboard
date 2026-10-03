//! Query (part of the worker).

use super::*;

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
        if !self.is_mongo() && self.protected() && safety::is_destructive(&text) {
            let preview = one_line(&text);
            return self.ask(Pending::Query(text), "Destructive statement".into(), preview);
        }
        self.run_sql(text).await;
    }

    pub(crate) async fn run_sql(&mut self, sql: String) {
        let Some(i) = self.active else { return };
        let res = match self.conn.as_mut() {
            Some(c) => c.execute_query(&sql).await,
            None => return,
        };
        match res {
            Ok(r) => {
                let total = r.rows.len();
                let mut rows = r.rows;
                rows.truncate(RESULT_CAP);
                let cols: Vec<ColMeta> = r.columns.iter().map(|n| ColMeta::plain(n)).collect();
                let t = &mut self.tabs[i];
                t.widths = auto_widths(&cols, &rows);
                t.cols = cols;
                t.rows = rows;
                t.editable = false;
                t.banner = String::new();
                t.banner_err = false;
                t.page.offset = 0;
                t.timing = format!("{:.1} ms", r.duration_ms);
                t.page_info = if total > RESULT_CAP { format!("{total} rows (showing the first {RESULT_CAP})") } else { format!("{total} row(s)") };
                self.record_history(&sql, r.duration_ms, true);
                self.log_activity(Some(r.duration_ms), &sql);
                if changes_schema(&sql) {
                    if let Some(c) = self.conn.as_mut() {
                        let _ = c.refresh_metadata().await;
                    }
                    self.rebuild_tree();
                }
            }
            Err(e) => {
                let t = &mut self.tabs[i];
                t.banner = e.to_string();
                t.banner_err = true;
                self.record_history(&sql, 0.0, false);
                self.log_activity(None, &format!("ERROR {e}"));
            }
        }
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

