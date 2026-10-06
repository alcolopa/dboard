//! Bulk table actions and new databases (part of the worker): pick several tables in the sidebar,
//! drop the selection or everything, create a database.

use super::*;

impl Worker {
    /// Ctrl/Cmd-click (mode 1) toggles one table; Shift-click (mode 2) adds the range from the last pick.
    pub(crate) fn tree_select(&mut self, i: usize, mode: i32) {
        let Some(e) = self.tree.get(i).cloned() else { return };
        if !(2..=5).contains(&e.kind) {
            return;
        }
        if mode == 2 {
            let from = self.tree_anchor.unwrap_or(i);
            let (a, b) = (from.min(i), from.max(i));
            let keys: Vec<String> = self.tree[a..=b.min(self.tree.len() - 1)].iter().filter(|t| (2..=5).contains(&t.kind)).map(|t| t.key.clone()).collect();
            self.tree_sel.extend(keys);
        } else {
            if !self.tree_sel.remove(&e.key) {
                self.tree_sel.insert(e.key.clone());
            }
            self.tree_anchor = Some(i);
        }
        self.rebuild_tree();
    }

    pub(crate) fn tree_clear_selection(&mut self) {
        self.tree_sel.clear();
        self.tree_anchor = None;
        self.rebuild_tree();
    }

    fn selected_tables(&self) -> Vec<(String, String)> {
        self.tree.iter().filter(|e| (2..=5).contains(&e.kind) && self.tree_sel.contains(&e.key)).map(|e| (e.schema.clone(), e.name.clone())).collect()
    }

    fn preview(items: &[(String, String)]) -> String {
        let mut names: Vec<String> = items.iter().take(8).map(|(s, n)| if s.is_empty() { n.clone() } else { format!("{s}.{n}") }).collect();
        if items.len() > 8 {
            names.push(format!("… and {} more", items.len() - 8));
        }
        names.join(", ")
    }

    pub(crate) fn ask_drop_selected(&mut self) {
        if self.refuse_if_read_only() {
            return;
        }
        let items = self.selected_tables();
        if items.is_empty() {
            return self.toast("Select tables first (Ctrl/Cmd-click or Shift-click).");
        }
        let text = format!("Drop {} object(s)? This cannot be undone.\n{}", items.len(), Self::preview(&items));
        let typing = self.protected();
        self.ask_with(Pending::DropMany(items.clone()), format!("Drop {} object(s)", items.len()), text, typing);
    }

    /// Everything the sidebar lists as a table, view or collection. Always asks for typed CONFIRM.
    pub(crate) fn ask_drop_all(&mut self) {
        if self.refuse_if_read_only() {
            return;
        }
        let Some(conn) = &self.conn else { return };
        let items: Vec<(String, String)> = conn.metadata.tables.iter().map(|t| (t.schema.clone(), t.name.clone())).collect();
        if items.is_empty() {
            return self.toast("Nothing to drop.");
        }
        let db = if conn.config.database.is_empty() { conn.config.display_name() } else { conn.config.database.clone() };
        let text = format!("Drop ALL {} table(s), view(s) and collection(s) in {db}? The data in them is lost and this cannot be undone.\n{}", items.len(), Self::preview(&items));
        self.ask_with(Pending::DropMany(items), format!("Drop everything in {db}"), text, true);
    }

    pub(crate) async fn drop_many(&mut self, items: Vec<(String, String)>) {
        let desc = format!("DROP {} object(s): {}", items.len(), Self::preview(&items));
        if !self.hook_gate(&desc) {
            return;
        }
        let force = self.force;
        let res = match self.conn.as_mut() {
            Some(conn) => conn.drop_many(&items, force).await,
            None => return,
        };
        self.tree_sel.clear();
        self.tree_anchor = None;
        match res {
            Ok(n) => {
                self.toast(format!("Dropped {n} object(s){}", if force { " (forced)" } else { "" }));
                self.log_activity(None, &format!("{desc}{}", if force { " [force]" } else { "" }));
            }
            Err(e) => self.toast(format!("{e}{}", if force { "" } else { " — tick “Force” to ignore foreign keys." })),
        }
        // Whatever was dropped (even before an error) is gone from the metadata: close its tabs.
        let remaining: HashSet<(String, String)> = self.conn.as_ref().map(|c| c.metadata.tables.iter().map(|t| (t.schema.clone(), t.name.clone())).collect()).unwrap_or_default();
        self.tabs.retain(|t| !matches!(t.kind, Kind::Table | Kind::Structure) || remaining.contains(&(t.schema.clone(), t.name.clone())));
        self.active = if self.tabs.is_empty() { None } else { Some(self.tabs.len() - 1) };
        self.rebuild_tree();
        self.show_active();
    }

    pub(crate) async fn create_database(&mut self, name: String) {
        let name = name.trim().to_string();
        if name.is_empty() || self.refuse_if_read_only() {
            return;
        }
        let res = match self.conn.as_mut() {
            Some(c) => c.create_database(&name).await,
            None => return,
        };
        if let Err(e) = res {
            return self.toast(e.to_string());
        }
        ui(&self.w, |st| st.set_new_db_open(false));
        self.log_activity(None, &format!("CREATE DATABASE {name}"));
        self.load_databases().await;
        if !self.databases.contains(&name) {
            // MongoDB lists a database only once it holds data.
            self.databases.push(name.clone());
            self.db_entries.push(name.clone());
            self.push_databases();
        }
        let idx = self.db_entries.iter().position(|e| *e == name).unwrap_or(0);
        self.switch_database(idx).await;
    }
}
