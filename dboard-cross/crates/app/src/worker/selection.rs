//! Selection (part of the worker).

use super::*;

// ---------------------------------------------------------------------------------------------
// Selection, clipboard, whole-row editing
// ---------------------------------------------------------------------------------------------

impl Worker {
    /// A selection rectangle (any corner order) clamped to what is loaded: (row0, col0, row1, col1).
    pub(crate) fn norm_rect(&self, r0: usize, c0: usize, r1: usize, c1: usize) -> Option<(usize, usize, usize, usize)> {
        let t = self.active_tab()?;
        if t.rows.is_empty() || t.cols.is_empty() {
            return None;
        }
        let (rmax, cmax) = (t.rows.len() - 1, t.cols.len() - 1);
        Some((r0.min(r1).min(rmax), c0.min(c1).min(cmax), r0.max(r1).min(rmax), c0.max(c1).min(cmax)))
    }

    pub(crate) fn selection_data(&self, rect: (usize, usize, usize, usize)) -> Option<(Vec<ExportCol>, Vec<Vec<Cell>>)> {
        let t = self.active_tab()?;
        let (r0, c0, r1, c1) = rect;
        let cols = t.cols.iter().skip(c0).take(c1 - c0 + 1).map(|c| ExportCol { name: c.name.clone(), type_name: c.type_name.clone() }).collect();
        let rows = t.rows.iter().skip(r0).take(r1 - r0 + 1).map(|r| r.iter().skip(c0).take(c1 - c0 + 1).cloned().collect()).collect();
        Some((cols, rows))
    }

    pub(crate) fn export_table_name(&self, t: &Tab) -> String {
        let d = self.export_dialect;
        if t.name.is_empty() {
            "results".to_string()
        } else if t.schema.is_empty() {
            d.quote(&t.name)
        } else {
            format!("{}.{}", d.quote(&t.schema), d.quote(&t.name))
        }
    }

    /// Copy the selected cells. Modes: 0 plain, 1 with header row, 2 CSV, 3 JSON, 4 SQL INSERT, 5 column names.
    pub(crate) fn copy_selection(&mut self, r0: usize, c0: usize, r1: usize, c1: usize, mode: i32) {
        let Some(rect) = self.norm_rect(r0, c0, r1, c1) else { return self.toast("Nothing selected") };
        let Some((cols, rows)) = self.selection_data(rect) else { return };
        self.export_dialect = self.dialect();
        let table = self.active_tab().map(|t| self.export_table_name(t)).unwrap_or_default();
        let text = match mode {
            1 => export::tsv(&cols, &rows, true),
            2 => export::csv(&cols, &rows, true),
            3 => export::json(&cols, &rows),
            4 => export::sql_inserts(&cols, &rows, &table, self.export_dialect),
            5 => cols.iter().map(|c| c.name.clone()).collect::<Vec<_>>().join("\t"),
            _ => export::tsv(&cols, &rows, false),
        };
        let what = if mode == 5 {
            format!("{} column name(s)", cols.len())
        } else if cols.len() == 1 && rows.len() > 1 {
            format!("column “{}” ({} values)", cols[0].name, rows.len())
        } else if rows.len() == 1 && cols.len() > 1 {
            format!("row ({} values)", cols.len())
        } else if rows.len() == 1 {
            "value".to_string()
        } else {
            format!("{} rows × {} columns", rows.len(), cols.len())
        };
        match crate::clipboard::set(&text) {
            Ok(()) => self.toast(format!("Copied {what}")),
            Err(e) => self.toast(format!("Clipboard unavailable: {e}")),
        }
    }

    pub(crate) fn paste_selection(&mut self, r0: usize, c0: usize, r1: usize, c1: usize) {
        let Some(t) = self.active_tab() else { return };
        if t.kind != Kind::Table || !t.editable {
            return self.toast("This table is read-only, so nothing can be pasted into it");
        }
        let Some((rr0, cc0, rr1, cc1)) = self.norm_rect(r0, c0, r1, c1) else { return };
        let text = match crate::clipboard::get() {
            Ok(t) => t,
            Err(e) => return self.toast(format!("Clipboard unavailable: {e}")),
        };
        let mut grid = export::parse_tsv(&text);
        if grid.is_empty() {
            return self.toast("The clipboard is empty");
        }
        let (sel_rows, sel_cols) = (rr1 - rr0 + 1, cc1 - cc0 + 1);
        if grid.len() == 1 && grid[0].len() == 1 && sel_rows * sel_cols > 1 {
            // One value over a selection fills the whole selection, like a spreadsheet.
            grid = vec![vec![grid[0][0].clone(); sel_cols]; sel_rows];
        }
        let (rows, cols) = (t.rows.len(), t.cols.len());
        grid.truncate(rows - rr0);
        for r in grid.iter_mut() {
            r.truncate(cols - cc0);
        }
        let n: usize = grid.iter().map(Vec::len).sum();
        if n == 0 {
            return;
        }
        if n == 1 {
            let v = grid[0][0].clone();
            let tx = self.tx.clone();
            let _ = tx.send(Cmd::EditCell(rr0, cc0, v, false));
            return;
        }
        let name = if t.name.is_empty() { t.title.clone() } else { t.name.clone() };
        self.ask(Pending::Paste(rr0, cc0, grid), format!("Paste {n} cells"), format!("Write {n} pasted values into {name}, starting at row {} of this page? Every change can be undone from History.", rr0 + 1));
    }

    pub(crate) async fn apply_paste(&mut self, r0: usize, c0: usize, grid: Vec<Vec<String>>) {
        let (mut done, mut failed) = (0usize, None);
        'outer: for (dr, row) in grid.iter().enumerate() {
            for (dc, v) in row.iter().enumerate() {
                match self.try_edit_cell(r0 + dr, c0 + dc, v.clone(), false).await {
                    Ok(true) => done += 1,
                    Ok(false) => {}
                    Err(e) => {
                        failed = Some(e);
                        break 'outer;
                    }
                }
            }
        }
        match failed {
            None => self.toast(format!("Pasted {done} value(s)")),
            Some(e) => {
                self.set_banner(&format!("Paste stopped after {done} value(s): {e}"), true);
            }
        }
    }

    // ---- edit a whole row -------------------------------------------------------------------

    pub(crate) fn push_edit_row(&self) {
        let (Some(er), Some(t)) = (&self.edit_row, self.active_tab()) else { return };
        let fields: Vec<(String, String, String, String, bool)> = t
            .cols
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let v = er.values.get(i).cloned().flatten();
                (c.name.clone(), format!("{}{}", c.type_name, if c.pk { " · PK" } else { "" }), if c.pk { "primary key" } else { "" }.to_string(), v.clone().unwrap_or_default(), v.is_none())
            })
            .collect();
        ui(&self.w, move |st| {
            let v: Vec<FieldItem> = fields.into_iter().map(|(n, t, h, val, nul)| FieldItem { name: n.into(), type_name: t.into(), hint: h.into(), value: val.into(), is_null: nul }).collect();
            st.set_editrow_fields(ModelRc::new(VecModel::from(v)));
        });
    }

    pub(crate) fn open_edit_row(&mut self, r: usize) {
        if self.refuse_if_read_only() {
            return;
        }
        let Some(t) = self.active_tab() else { return };
        if t.kind != Kind::Table || !t.editable {
            return self.toast("This table is read-only");
        }
        let Some(row) = t.rows.get(r).cloned() else { return };
        let title = format!("Edit row {} · {}.{}", t.page.offset + r as i64 + 1, t.schema, t.name);
        self.edit_row = Some(EditRowState { row: r, original: row.clone(), values: row });
        self.push_edit_row();
        ui(&self.w, move |st| {
            st.set_editrow_title(title.into());
            st.set_editrow_error("".into());
            st.set_editrow_open(true);
        });
    }

    pub(crate) async fn editrow_submit(&mut self) {
        let Some(er) = self.edit_row.as_ref() else { return };
        let (row, original, values) = (er.row, er.original.clone(), er.values.clone());
        let names: Vec<String> = self.active_tab().map(|t| t.cols.iter().map(|c| c.name.clone()).collect()).unwrap_or_default();
        let mut saved = 0;
        for c in 0..values.len() {
            if values[c] == original[c] {
                continue;
            }
            let (text, null) = (values[c].clone().unwrap_or_default(), values[c].is_none());
            match self.try_edit_cell(row, c, text, null).await {
                Ok(_) => {
                    saved += 1;
                    if let Some(er) = self.edit_row.as_mut() {
                        er.original[c] = values[c].clone();
                    }
                }
                Err(e) => {
                    let msg = format!("Could not save “{}”: {e}", names.get(c).cloned().unwrap_or_default());
                    ui(&self.w, move |st| st.set_editrow_error(msg.into()));
                    return;
                }
            }
        }
        self.edit_row = None;
        ui(&self.w, |st| st.set_editrow_open(false));
        self.toast(if saved == 0 { "No changes".to_string() } else { format!("Saved {saved} change(s)") });
    }
}

