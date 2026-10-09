//! Tree (part of the worker).

use super::*;

// ---------------------------------------------------------------------------------------------
// Sidebar tree
// ---------------------------------------------------------------------------------------------

impl Worker {
    pub(crate) fn rebuild_tree(&mut self) {
        if self.conn.is_none() {
            return;
        }
        // MySQL / MongoDB with "All databases" picked: list nothing until a database is chosen.
        if self.db_all_entry && self.db_idx == 0 && !self.databases.is_empty() {
            self.tree.clear();
            self.tree_sel.clear();
            ui(&self.w, |st| {
                st.set_tree(ModelRc::new(VecModel::from(Vec::<TreeItem>::new())));
                st.set_tree_sel_count(0);
                st.set_db_needed(true);
            });
            return;
        }
        let Some(conn) = &self.conn else { return };
        let filter = self.tree_filter.to_lowercase();
        let filtering = !filter.is_empty();
        let matches = |n: &str| !filtering || n.to_lowercase().contains(&filter);
        let mut entries: Vec<(TreeEntry, String, i32, bool)> = Vec::new(); // (entry, label, level, expanded)

        let mut schemas: std::collections::BTreeMap<&str, (Vec<&Table>, Vec<&DbObject>)> = Default::default();
        for t in &conn.metadata.tables {
            if matches(&t.name) { schemas.entry(&t.schema).or_default().0.push(t); }
        }
        for o in &conn.metadata.objects {
            if matches(&o.name) { schemas.entry(&o.schema).or_default().1.push(o); }
        }
        for (schema, (tables, objects)) in schemas {
            let s = schema.to_string();
            let skey = format!("s:{s}");
            let open = filtering || !self.collapsed.contains(&skey);
            let label = if s.is_empty() { "database".to_string() } else { s.clone() };
            entries.push((TreeEntry { kind: 0, schema: s.clone(), name: String::new(), detail: String::new(), key: skey }, label, 0, open));
            if !open {
                continue;
            }
            // (label, badge, name, detail)
            type Item = (String, i32, String, String);
            let of_kind = |k: ObjectKind, badge: i32| -> Vec<Item> {
                objects
                    .iter()
                    .filter(|o| o.kind == k)
                    .map(|o| {
                        let label = match k {
                            ObjectKind::Function | ObjectKind::Procedure if !o.detail.is_empty() => format!("{}({})", o.name, o.detail),
                            ObjectKind::Trigger | ObjectKind::Index | ObjectKind::Extension if !o.detail.is_empty() => format!("{}  ({})", o.name, o.detail),
                            _ => o.name.clone(),
                        };
                        (label, badge, o.name.clone(), o.detail.clone())
                    })
                    .collect()
            };
            let table_items = |f: &dyn Fn(&Table) -> Option<i32>| -> Vec<Item> {
                tables.iter().filter_map(|t| f(t).map(|b| (t.name.clone(), b, t.name.clone(), String::new()))).collect()
            };
            let mut routines = of_kind(ObjectKind::Function, 6);
            routines.extend(of_kind(ObjectKind::Procedure, 7));
            routines.sort_by(|a, b| a.0.cmp(&b.0));
            // (title, items, open by default). Bulky lists start collapsed so the tree stays readable.
            let sections: Vec<(&str, Vec<Item>, bool)> = vec![
                ("Tables", table_items(&|t| (t.kind == TableKind::Table).then_some(2)), true),
                ("Views", table_items(&|t| match t.kind { TableKind::View => Some(3), TableKind::MaterializedView => Some(4), _ => None }), true),
                ("Collections", table_items(&|t| (t.kind == TableKind::Collection).then_some(5)), true),
                ("Routines", routines, true),
                ("Sequences", of_kind(ObjectKind::Sequence, 8), true),
                ("Triggers", of_kind(ObjectKind::Trigger, 9), false),
                ("Types", of_kind(ObjectKind::Type, 10), false),
                ("Indexes", of_kind(ObjectKind::Index, 11), false),
                ("Events", of_kind(ObjectKind::Event, 12), false),
                ("Extensions", of_kind(ObjectKind::Extension, 13), false),
            ];
            for (title, items, default_open) in sections {
                if items.is_empty() {
                    continue;
                }
                let key = format!("s:{s}/{title}");
                // `collapsed` holds sections the user flipped away from their default.
                let sopen = filtering || (default_open != self.collapsed.contains(&key));
                entries.push((TreeEntry { kind: 1, schema: s.clone(), name: title.into(), detail: String::new(), key: key.clone() }, format!("{title} ({})", items.len()), 1, sopen));
                if sopen {
                    for (label, kind, name, detail) in items {
                        entries.push((TreeEntry { kind, schema: s.clone(), key: format!("{key}/{label}"), name, detail }, label, 2, false));
                    }
                }
            }
        }
        self.tree = entries.iter().map(|e| e.0.clone()).collect();
        // Selected tables that are no longer in the tree (dropped, filtered out) fall out of the selection.
        let keys: HashSet<&String> = self.tree.iter().map(|e| &e.key).collect();
        self.tree_sel.retain(|k| keys.contains(k));
        let sel = self.tree_sel.clone();
        let sel_count = sel.len() as i32;
        let items: Vec<(String, i32, i32, bool, String, bool)> = entries.into_iter().map(|(e, label, level, open)| (label, level, e.kind, open, e.key.clone(), sel.contains(&e.key))).collect();
        ui(&self.w, move |st| {
            let v: Vec<TreeItem> = items
                .into_iter()
                .map(|(label, level, kind, expanded, key, selected)| TreeItem { label: label.into(), level, kind, expanded, key: key.into(), selected })
                .collect();
            st.set_tree(ModelRc::new(VecModel::from(v)));
            st.set_tree_sel_count(sel_count);
            st.set_db_needed(false);
        });
    }

    /// Selection changes do not change the tree's shape or require rebuilding metadata.
    pub(crate) fn push_tree_selection(&self) {
        let selected: Vec<bool> = self.tree.iter().map(|e| self.tree_sel.contains(&e.key)).collect();
        let count = self.tree_sel.len() as i32;
        ui(&self.w, move |st| {
            let model = st.get_tree();
            for (i, selected) in selected.into_iter().enumerate() {
                if let Some(mut item) = slint::Model::row_data(&model, i) {
                    if item.selected != selected {
                        item.selected = selected;
                        slint::Model::set_row_data(&model, i, item);
                    }
                }
            }
            st.set_tree_sel_count(count);
        });
    }

    pub(crate) fn find_object(&self, e: &TreeEntry) -> Option<DbObject> {
        let want = match e.kind {
            6 => ObjectKind::Function,
            7 => ObjectKind::Procedure,
            8 => ObjectKind::Sequence,
            9 => ObjectKind::Trigger,
            10 => ObjectKind::Type,
            11 => ObjectKind::Index,
            12 => ObjectKind::Event,
            13 => ObjectKind::Extension,
            _ => return None,
        };
        self.conn
            .as_ref()?
            .metadata
            .objects
            .iter()
            .find(|o| o.schema == e.schema && o.name == e.name && o.kind == want && o.detail == e.detail)
            .cloned()
    }
}

