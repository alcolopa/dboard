//! Context (part of the worker).

use super::*;

// ---------------------------------------------------------------------------------------------
// Context menus
// ---------------------------------------------------------------------------------------------

pub(crate) fn item(label: &str, action: &str) -> (String, String, bool, bool) {
    (label.to_string(), action.to_string(), false, false)
}
pub(crate) fn danger(label: &str, action: &str) -> (String, String, bool, bool) {
    (label.to_string(), action.to_string(), false, true)
}
pub(crate) fn sep() -> (String, String, bool, bool) {
    (String::new(), String::new(), true, false)
}

impl Worker {
    pub(crate) fn open_ctx(&mut self, kind: &str, i: usize, j: usize, x: f32, y: f32) {
        let editable = self.active_tab().is_some_and(|t| t.kind == Kind::Table && t.editable);
        let is_table_tab = self.active_tab().is_some_and(|t| t.kind == Kind::Table);
        let relational = !self.is_mongo();
        let (target, items) = match kind {
            "tree" => {
                let Some(e) = self.tree.get(i) else { return };
                let items = match e.kind {
                    2 | 5 => vec![item("Open data", "open"), item("Open in new tab", "open-new"), item("Inspect structure", "structure"), item("Query…", "query"), item("Insert row…", "insert"), item("Import rows…", "import"), item("Export data…", "export"), sep(), item("Copy definition", "copy-def"), item("Copy name", "copy"), sep(), danger("Truncate…", "truncate"), danger("Drop…", "drop")],
                    3 | 4 => vec![item("Open data", "open"), item("Open in new tab", "open-new"), item("Inspect structure", "structure"), item("Query…", "query"), item("Export data…", "export"), sep(), item("Copy definition", "copy-def"), item("Copy name", "copy"), sep(), danger("Drop…", "drop")],
                    6 | 7 => vec![item("View definition", "definition"), item("Run in query tab", "run"), item("Copy definition", "copy-def"), item("Copy name", "copy")],
                    _ => vec![item("View definition", "definition"), item("Copy definition", "copy-def"), item("Copy name", "copy")],
                };
                (CtxTarget::Tree(i), items)
            }
            "session" => {
                if i >= self.sess_meta.len() {
                    return;
                }
                let mut v = vec![item("Close", "close"), item("Close other connections", "close-others"), sep()];
                if i > 0 {
                    v.push(item("Move left", "left"));
                }
                if i + 1 < self.sess_meta.len() {
                    v.push(item("Move right", "right"));
                }
                (CtxTarget::Session(i), v)
            }
            "tab" => {
                let Some(t) = self.tabs.get(i) else { return };
                (CtxTarget::Tab(i), vec![item(if t.pinned { "Unpin tab" } else { "Pin tab" }, "pin"), item("Duplicate tab", "duplicate"), sep(), item("Close tab", "close"), item("Close other tabs", "close-others")])
            }
            "cell" => {
                let mut v = vec![item("Copy", "copy"), item("Copy with column header", "copy-h")];
                if editable {
                    v.push(item("Paste", "paste"));
                }
                let has_fk = self.active_tab().and_then(|t| t.cols.get(j)).is_some_and(|c| !c.fk.is_empty());
                if has_fk && relational {
                    v.push(sep());
                    v.push(item("Go to referenced row", "goto-fk"));
                }
                v.push(sep());
                v.push(item("Select whole row", "sel-row"));
                v.push(item("Select whole column", "sel-col"));
                if editable {
                    v.push(sep());
                    if relational {
                        v.push(item("Edit row…", "edit-row"));
                    }
                    v.push(item("Set to NULL", "null"));
                    v.push(item("Edit as JSON / text…", "json"));
                    v.push(sep());
                    v.push(danger("Delete row…", "delete"));
                }
                (CtxTarget::Cell(i, j), v)
            }
            "header" => {
                let mut v = vec![item("Copy column", "copy"), item("Copy column with header", "copy-h"), item("Copy column name", "copy-names"), item("Copy as CSV", "copy-csv"), item("Copy as JSON", "copy-json")];
                if editable {
                    v.push(item("Paste into column", "paste"));
                }
                if is_table_tab {
                    v.push(sep());
                    v.push(item("Sort ascending", "sort-asc"));
                    v.push(item("Sort descending", "sort-desc"));
                }
                (CtxTarget::Header(j), v)
            }
            "row" => {
                let mut v = vec![item("Copy row", "copy"), item("Copy row with header", "copy-h"), item("Copy as CSV", "copy-csv"), item("Copy as JSON", "copy-json")];
                if relational {
                    v.push(item("Copy as SQL INSERT", "copy-sql"));
                }
                if editable {
                    v.push(sep());
                    if relational {
                        v.push(item("Edit row…", "edit-row"));
                    }
                    v.push(danger("Delete row…", "delete"));
                }
                (CtxTarget::Row(i), v)
            }
            "dbmenu" => (
                CtxTarget::DbMenu,
                vec![item("Users & access…", "users"), sep(), item("Export database…", "export-db"), item("Import database…", "import-db"), item("Import rows into open table…", "import-rows"), sep(), item("Refresh metadata", "refresh")],
            ),
            _ => return,
        };
        self.ctx_target = Some(target);
        ui(&self.w, move |st| {
            let v: Vec<CtxItem> = items.into_iter().map(|(l, a, s, d)| CtxItem { label: l.into(), action: a.into(), sep: s, danger: d }).collect();
            st.set_ctx_items(ModelRc::new(VecModel::from(v)));
            st.set_ctx_x(x);
            st.set_ctx_y(y);
            st.set_ctx_open(true);
        });
    }

    pub(crate) async fn ctx_pick(&mut self, action: &str, rect: [i32; 4]) {
        ui(&self.w, |st| st.set_ctx_open(false));
        let Some(target) = self.ctx_target.take() else { return };
        let [r0, c0, r1, c1] = rect.map(|v| v.max(0) as usize);
        match target {
            CtxTarget::Tree(i) => self.tree_action(i, action).await,
            CtxTarget::Session(i) => match action {
                "close" => self.close_session(i as i32),
                "close-others" => {
                    self.switch_session(i);
                    for j in (0..self.sess_meta.len()).rev() {
                        if j != self.cur {
                            self.close_session(j as i32);
                        }
                    }
                }
                "left" | "right" => {
                    let j = if action == "left" { i.wrapping_sub(1) } else { i + 1 };
                    if j < self.sess_meta.len() {
                        self.sess_meta.swap(i, j);
                        self.parked.swap(i, j);
                        if self.cur == i {
                            self.cur = j;
                        } else if self.cur == j {
                            self.cur = i;
                        }
                        self.push_sessions();
                    }
                }
                _ => {}
            },
            CtxTarget::Tab(i) => match action {
                "close" => self.close_tab(i as i32),
                other => self.tab_action(i, other).await,
            },
            CtxTarget::DbMenu => match action {
                "users" => self.open_users().await,
                "export-db" => self.open_transfer(0),
                "import-db" => self.open_transfer(1),
                "import-rows" => self.open_transfer(2),
                "refresh" => self.refresh().await,
                _ => {}
            },
            CtxTarget::Cell(r, c) => match action {
                "copy" => self.copy_selection(r0, c0, r1, c1, 0),
                "copy-h" => self.copy_selection(r0, c0, r1, c1, 1),
                "paste" => self.paste_selection(r0, c0, r1, c1),
                "sel-row" => ui(&self.w, move |st| st.invoke_select_row(r as i32, false)),
                "sel-col" => ui(&self.w, move |st| st.invoke_select_column(c as i32, false)),
                "edit-row" => self.open_edit_row(r),
                "goto-fk" => self.goto_fk(r, c).await,
                "null" => self.edit_cell(r, c, String::new(), true).await,
                "json" => self.open_json_cell(r, c),
                "delete" => self.ask_delete_row(r),
                _ => {}
            },
            CtxTarget::Header(c) => match action {
                "copy" => self.copy_selection(r0, c0, r1, c1, 0),
                "copy-h" => self.copy_selection(r0, c0, r1, c1, 1),
                "copy-names" => self.copy_selection(r0, c0, r1, c1, 5),
                "copy-csv" => self.copy_selection(r0, c0, r1, c1, 2),
                "copy-json" => self.copy_selection(r0, c0, r1, c1, 3),
                "paste" => self.paste_selection(r0, c0, r1, c1),
                "sort-asc" | "sort-desc" => {
                    let asc = action == "sort-asc";
                    let name = self.active_tab().and_then(|t| t.cols.get(c)).map(|c| c.name.clone());
                    if let (Some(name), Some(t)) = (name, self.active_mut().filter(|t| t.kind == Kind::Table)) {
                        t.page.sort_column = Some(name);
                        t.page.sort_ascending = asc;
                        t.page.offset = 0;
                        self.load_active().await;
                    }
                }
                _ => {}
            },
            CtxTarget::Row(r) => match action {
                "copy" => self.copy_selection(r0, c0, r1, c1, 0),
                "copy-h" => self.copy_selection(r0, c0, r1, c1, 1),
                "copy-csv" => self.copy_selection(r0, c0, r1, c1, 2),
                "copy-json" => self.copy_selection(r0, c0, r1, c1, 3),
                "copy-sql" => self.copy_selection(r0, c0, r1, c1, 4),
                "edit-row" => self.open_edit_row(r),
                "delete" => self.ask_delete_row(r),
                _ => {}
            },
        }
    }
}

