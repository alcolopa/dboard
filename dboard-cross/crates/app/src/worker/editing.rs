//! Editing (part of the worker).

use super::*;

// ---------------------------------------------------------------------------------------------
// Editing rows
// ---------------------------------------------------------------------------------------------

impl Worker {
    pub(crate) fn schedule_clear(&self) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(1200)).await;
            let _ = tx.send(Cmd::ClearFlash);
        });
    }

    /// Write one cell. On success the grid shows the new value; on failure it is left unchanged.
    pub(crate) async fn try_edit_cell(&mut self, r: usize, c: usize, text: String, null: bool) -> Result<bool, String> {
        let Some(tab) = self.active_tab().cloned() else { return Ok(false) };
        if self.read_only() {
            return Err("This connection is read-only.".into());
        }
        if tab.kind != Kind::Table || !tab.editable {
            return Err("This table cannot be edited.".into());
        }
        if !(tab.stage && !self.is_mongo()) && !self.hook_gate(&format!("UPDATE {}.{} SET {}", tab.schema, tab.name, tab.cols.get(c).map(|c| c.name.as_str()).unwrap_or(""))) {
            return Ok(false);
        }
        let (Some(col), Some(row)) = (tab.cols.get(c).cloned(), tab.rows.get(r).cloned()) else { return Ok(false) };
        let new: Cell = if null { None } else { Some(text) };
        if row.get(c) == Some(&new) {
            return Ok(false); // unchanged
        }
        if tab.stage && !self.is_mongo() {
            self.stage_edit(r, c, new);
            return Ok(true);
        }
        if let Some(t) = self.active_mut() {
            t.rows[r][c] = new.clone();
        }
        self.set_cell(r, c, new.clone(), 1);
        self.flashed.push((r, c));
        let res = match self.conn.as_mut() {
            Some(conn) => conn.edit_cell(&tab.schema, &tab.name, &row, &col.name, new.clone()).await,
            None => return Ok(false),
        };
        match res {
            Ok(()) => {
                self.set_cell(r, c, new, 2);
                self.log_activity(None, &format!("UPDATE {}.{} SET {}", tab.schema, tab.name, col.name));
                self.push_inspector();
                self.schedule_clear();
                Ok(true)
            }
            Err(e) => {
                if let Some(t) = self.active_mut() {
                    t.rows[r][c] = row[c].clone();
                }
                self.set_cell(r, c, row[c].clone(), 3);
                self.log_activity(None, &format!("ERROR {e}"));
                self.schedule_clear();
                Err(e.to_string())
            }
        }
    }

    pub(crate) async fn edit_cell(&mut self, r: usize, c: usize, text: String, null: bool) {
        if let Err(e) = self.try_edit_cell(r, c, text, null).await {
            self.set_banner(&e, true);
        }
    }

    /// After Tab / Shift+Tab in a cell editor: open the neighbouring editable cell.
    pub(crate) fn edit_next(&mut self, r: usize, c: usize, dir: i32) {
        let Some(t) = self.active_tab().filter(|t| t.kind == Kind::Table && t.editable) else { return };
        let (rows, cols) = (t.rows.len() as i64, t.cols.len() as i64);
        if rows == 0 || cols == 0 {
            return;
        }
        let flat = r as i64 * cols + c as i64 + dir as i64;
        if flat < 0 || flat >= rows * cols {
            return;
        }
        let (nr, nc) = ((flat / cols) as i32, (flat % cols) as i32);
        ui(&self.w, move |st| {
            st.invoke_request_edit(nr, nc);
            st.set_selected_row(nr);
            st.set_sel_kind(1);
            st.set_sel_r0(nr);
            st.set_sel_r1(nr);
            st.set_sel_c0(nc);
            st.set_sel_c1(nc);
        });
    }

    pub(crate) async fn toggle_bool(&mut self, r: usize, c: usize) {
        let Some(tab) = self.active_tab() else { return };
        let (Some(col), Some(row)) = (tab.cols.get(c), tab.rows.get(r)) else { return };
        let on = matches!(row.get(c).cloned().flatten().as_deref(), Some("true" | "t" | "1"));
        let numeric = col.type_name.contains("tinyint");
        let new = match (on, numeric) {
            (true, true) => "0",
            (false, true) => "1",
            (true, false) => "false",
            (false, false) => "true",
        };
        self.edit_cell(r, c, new.to_string(), false).await;
    }

    pub(crate) fn cell_text(&self, r: usize, c: usize) -> Option<Cell> {
        self.active_tab()?.rows.get(r)?.get(c).cloned()
    }

    /// Follow a foreign key: open the referenced table filtered to the row this cell points at.
    pub(crate) async fn goto_fk(&mut self, r: usize, c: usize) {
        let Some(fk) = self.active_tab().and_then(|t| t.cols.get(c)).map(|c| c.fk.clone()) else { return };
        let Some(Some(value)) = self.cell_text(r, c) else { return self.toast("This cell is NULL, nothing to follow.") };
        let Some((table_part, col)) = fk.strip_suffix(')').and_then(|s| s.rsplit_once('(')) else { return };
        let (schema, table) = table_part.split_once('.').unwrap_or(("", table_part));
        let d = self.dialect();
        let filter = format!("{} = {}", d.quote(col), dboard_core::sql::literal(d, &value));
        self.open_table_in(schema, table, true).await;
        if let Some(t) = self.active_mut() {
            t.page.filter = Some(filter.clone());
            t.page.offset = 0;
            t.filter_text = filter;
        }
        self.load_active().await;
    }

    /// Narrow the open table to rows where this cell's column equals (or differs from) its value.
    pub(crate) async fn filter_by_cell(&mut self, r: usize, c: usize, equal: bool) {
        let Some(col) = self.active_tab().and_then(|t| t.cols.get(c)).map(|c| c.name.clone()) else { return };
        let d = self.dialect();
        let q = d.quote(&col);
        let clause = match self.cell_text(r, c) {
            Some(Some(v)) => format!("{q} {} {}", if equal { "=" } else { "<>" }, dboard_core::sql::literal(d, &v)),
            _ => format!("{q} IS {}NULL", if equal { "" } else { "NOT " }),
        };
        if let Some(t) = self.active_mut().filter(|t| t.kind == Kind::Table) {
            t.filter_text = if t.filter_text.trim().is_empty() { clause } else { format!("({}) AND {clause}", t.filter_text.trim()) };
            t.page.offset = 0;
            self.apply_effective_filter();
            self.load_active().await;
        }
    }

    pub(crate) fn open_json_cell(&mut self, r: usize, c: usize) {
        let Some(Some(v)) = self.cell_text(r, c).map(|v| v.or(Some(String::new()))) else { return };
        let col = self.active_tab().and_then(|t| t.cols.get(c)).cloned().unwrap_or_default();
        let pretty = serde_json::from_str::<serde_json::Value>(&v).ok().and_then(|j| serde_json::to_string_pretty(&j).ok()).unwrap_or(v);
        self.json_target = Some(JsonTarget::Cell(r, c));
        let title = format!("Edit {} · {}", if col.is_json { "JSON" } else { "text" }, col.name);
        ui(&self.w, move |st| {
            st.set_json_title(title.into());
            st.set_json_text(pretty.into());
            st.set_json_error("".into());
            st.set_json_open(true);
        });
    }

    pub(crate) async fn json_save(&mut self, text: String) {
        let Some(target) = self.json_target.take() else { return };
        let fail = |w: &Weak<App>, msg: String| ui(w, move |st| st.set_json_error(msg.into()));
        match target {
            JsonTarget::Cell(r, c) => {
                let is_json = self.active_tab().and_then(|t| t.cols.get(c)).is_some_and(|c| c.is_json);
                let value = if is_json {
                    match serde_json::from_str::<serde_json::Value>(&text) {
                        Ok(v) => serde_json::to_string(&v).unwrap_or(text),
                        Err(e) => {
                            self.json_target = Some(JsonTarget::Cell(r, c));
                            return fail(&self.w, format!("Invalid JSON: {e}"));
                        }
                    }
                } else {
                    text
                };
                ui(&self.w, |st| st.set_json_open(false));
                self.edit_cell(r, c, value, false).await;
            }
            JsonTarget::Doc(row) => {
                let Some(tab) = self.active_tab().cloned() else { return };
                let res = match self.conn.as_mut() {
                    Some(conn) => conn.replace_document(&tab.schema, &tab.name, &row, &text).await,
                    None => return,
                };
                match res {
                    Ok(()) => {
                        ui(&self.w, |st| st.set_json_open(false));
                        self.toast("Document saved");
                        self.log_activity(None, &format!("REPLACE document in {}", tab.name));
                        self.load_active().await;
                    }
                    Err(e) => {
                        self.json_target = Some(JsonTarget::Doc(row));
                        fail(&self.w, e.to_string());
                    }
                }
            }
            JsonTarget::NewDoc => {
                let Some(tab) = self.active_tab().cloned() else { return };
                let res = match self.conn.as_mut() {
                    Some(conn) => conn.insert_document(&tab.schema, &tab.name, &text).await,
                    None => return,
                };
                match res {
                    Ok(()) => {
                        ui(&self.w, |st| st.set_json_open(false));
                        self.toast("Document inserted");
                        self.log_activity(None, &format!("INSERT document into {}", tab.name));
                        self.load_active().await;
                    }
                    Err(e) => {
                        self.json_target = Some(JsonTarget::NewDoc);
                        fail(&self.w, e.to_string());
                    }
                }
            }
        }
    }

    pub(crate) async fn open_doc(&mut self, r: usize) {
        if !self.is_mongo() {
            return;
        }
        let (Some(tab), Some(row)) = (self.active_tab().cloned(), self.active_tab().and_then(|t| t.rows.get(r).cloned())) else { return };
        let res = match self.conn.as_mut() {
            Some(conn) => conn.document_json(&tab.schema, &tab.name, &row).await,
            None => return,
        };
        match res {
            Ok(json) => {
                self.json_target = Some(JsonTarget::Doc(row));
                let title = format!("Edit document · {}", tab.name);
                ui(&self.w, move |st| {
                    st.set_json_title(title.into());
                    st.set_json_text(json.into());
                    st.set_json_error("".into());
                    st.set_json_open(true);
                });
            }
            Err(e) => self.toast(e.to_string()),
        }
    }

    pub(crate) fn open_insert(&mut self) {
        if self.refuse_if_read_only() {
            return;
        }
        let Some(tab) = self.active_tab().cloned() else { return };
        if tab.kind != Kind::Table {
            return;
        }
        if self.is_mongo() {
            self.json_target = Some(JsonTarget::NewDoc);
            let title = format!("Insert document · {}", tab.name);
            ui(&self.w, move |st| {
                st.set_json_title(title.into());
                st.set_json_text("{\n  \n}".into());
                st.set_json_error("".into());
                st.set_json_open(true);
            });
            return;
        }
        let Some(table) = self.conn.as_ref().and_then(|c| c.table(&tab.schema, &tab.name)).cloned() else { return };
        if !matches!(table.kind, TableKind::Table) {
            return self.toast("Rows can only be inserted into tables");
        }
        self.insert_vals = vec![String::new(); table.columns.len()];
        // The new row is typed straight into the grid: a hint per column says what an empty cell becomes.
        let hints: Vec<String> = table
            .columns
            .iter()
            .map(|c| match (&c.default, c.nullable) {
                (Some(d), _) => format!("default: {d}"),
                (None, true) => "NULL".to_string(),
                (None, false) => "required".to_string(),
            })
            .collect();
        let blank = vec![String::new(); hints.len()];
        ui(&self.w, move |st| {
            st.set_draft_open(false);
            st.set_draft_hints(strs(hints));
            st.set_draft(strs(blank));
            st.set_draft_open(true);
        });
    }

    /// Jump to the last page. SQL engines count the rows exactly; MongoDB uses its estimate.
    pub(crate) async fn last_page(&mut self) {
        let Some(tab) = self.active_tab().filter(|t| t.kind == Kind::Table).cloned() else { return };
        let dialect = self.dialect();
        let limit = tab.page.limit.max(1);
        let total: Option<i64> = if self.is_mongo() {
            self.conn.as_ref().and_then(|c| c.table(&tab.schema, &tab.name)).and_then(|t| t.estimated_rows)
        } else {
            let Some(table) = self.conn.as_ref().and_then(|c| c.table(&tab.schema, &tab.name)).cloned() else { return };
            let mut sql = format!("SELECT COUNT(*) FROM {}", dialect.qualified(&table));
            if let Some(f) = tab.page.filter.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
                sql.push_str(&format!(" WHERE {f}"));
            }
            match self.conn.as_mut() {
                Some(c) => match c.execute_query(&sql).await {
                    Ok(r) => r.rows.first().and_then(|row| row.first()).and_then(|v| v.as_deref()).and_then(|v| v.trim().parse().ok()),
                    Err(e) => return self.toast(format!("Could not count rows: {e}")),
                },
                None => return,
            }
        };
        let Some(total) = total.filter(|n| *n > 0) else { return };
        let offset = ((total - 1) / limit) * limit;
        if let Some(t) = self.active_mut() {
            t.page.offset = offset;
        }
        self.load_active().await;
    }

    pub(crate) async fn insert_submit(&mut self) {
        if self.refuse_if_read_only() {
            return;
        }
        let Some(tab) = self.active_tab().cloned() else { return };
        let Some(table) = self.conn.as_ref().and_then(|c| c.table(&tab.schema, &tab.name)).cloned() else { return };
        let vals: Vec<(String, String)> = table
            .columns
            .iter()
            .zip(&self.insert_vals)
            .filter(|(_, v)| !v.is_empty())
            .map(|(c, v)| (c.name.clone(), v.clone()))
            .collect();
        let res = match self.conn.as_mut() {
            Some(conn) => conn.insert_row(&tab.schema, &tab.name, &vals).await,
            None => return,
        };
        match res {
            Ok(()) => {
                ui(&self.w, |st| st.set_insert_open(false));
                self.toast("Row inserted");
                self.log_activity(None, &format!("INSERT INTO {}.{}", tab.schema, tab.name));
                self.load_active().await;
            }
            Err(e) => {
                let m = e.to_string();
                self.set_banner(&m, true);
                ui(&self.w, move |st| st.set_insert_error(m.into()));
            }
        }
    }

    pub(crate) fn ask_delete_row(&mut self, r: usize) {
        if self.refuse_if_read_only() {
            return;
        }
        let Some(tab) = self.active_tab() else { return };
        if tab.kind != Kind::Table || !tab.editable || r >= tab.rows.len() {
            return;
        }
        let title = format!("Delete row {}", tab.page.offset + r as i64 + 1);
        let text = format!("Delete the selected row from {}.{}? You can bring it back with Undo (top bar or History) until you disconnect.", tab.schema, tab.name);
        self.ask(Pending::DeleteRow(r), title, text);
    }

    pub(crate) async fn confirm_run(&mut self) {
        ui(&self.w, |st| st.set_confirm_open(false));
        match self.pending.take() {
            Some(Pending::Query(sql)) => self.run_sql(sql).await,
            Some(Pending::Explain(sql, a)) => self.run_explain(sql, a).await,
            Some(Pending::DeleteRow(r)) => {
                let (Some(tab), Some(row)) = (self.active_tab().cloned(), self.active_tab().and_then(|t| t.rows.get(r).cloned())) else { return };
                if !self.hook_gate(&format!("DELETE FROM {}.{}", tab.schema, tab.name)) {
                    return;
                }
                let res = match self.conn.as_mut() {
                    Some(conn) => conn.delete_row(&tab.schema, &tab.name, &row).await,
                    None => return,
                };
                match res {
                    Ok(()) => {
                        self.toast("Row deleted. Undo is in History.");
                        self.log_activity(None, &format!("DELETE FROM {}.{}", tab.schema, tab.name));
                        self.push_inspector();
                        self.load_active().await;
                    }
                    Err(e) => self.set_banner(&e.to_string(), true),
                }
            }
            Some(Pending::Truncate(s, n)) => {
                let res = match self.conn.as_mut() {
                    Some(conn) => conn.truncate_forced(&s, &n, self.force).await,
                    None => return,
                };
                match res {
                    Ok(()) => {
                        self.toast(format!("Truncated {n}"));
                        self.log_activity(None, &format!("TRUNCATE {s}.{n}"));
                        if self.active_tab().is_some_and(|t| t.kind == Kind::Table && t.schema == s && t.name == n) {
                            self.load_active().await;
                        }
                    }
                    Err(e) => self.toast(e.to_string()),
                }
            }
            Some(Pending::Drop(s, n)) => {
                let res = match self.conn.as_mut() {
                    Some(conn) => if self.force { conn.drop_many(&[(s.clone(), n.clone())], true).await.map(|_| ()) } else { conn.drop_table(&s, &n).await },
                    None => return,
                };
                match res {
                    Ok(()) => {
                        self.tabs.retain(|t| !(matches!(t.kind, Kind::Table | Kind::Structure) && t.schema == s && t.name == n));
                        self.active = if self.tabs.is_empty() { None } else { Some(self.tabs.len() - 1) };
                        self.rebuild_tree();
                        self.show_active();
                        self.toast(format!("Dropped {n}"));
                        self.log_activity(None, &format!("DROP {s}.{n}"));
                    }
                    Err(e) => self.toast(e.to_string()),
                }
            }
            Some(Pending::DropMany(items)) => self.drop_many(items).await,
            Some(Pending::DropUser(i)) => self.user_drop(i).await,
            Some(Pending::Paste(r, c, grid)) => self.apply_paste(r, c, grid).await,
            Some(Pending::ImportDatabase(path, stop)) => self.run_import_db(path, stop).await,
            Some(Pending::ImportRows(path, header)) => self.run_import_rows(path, header).await,
            Some(Pending::GenerateRows(n)) => self.generate_rows(n, true).await,
            Some(Pending::DownloadUpdate(url, name)) => {
                self.toast(format!("Downloading {name}…"));
                let tx = self.tx.clone();
                std::thread::spawn(move || {
                    let r = crate::update::download(&url, &name, &downloads_dir());
                    let _ = tx.send(Cmd::UpdateDownloaded(r));
                });
            }
            None => {}
        }
    }
}

