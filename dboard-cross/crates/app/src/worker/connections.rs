//! Connections (part of the worker).

use super::*;

// ---------------------------------------------------------------------------------------------
// Connection manager
// ---------------------------------------------------------------------------------------------

impl Worker {
    pub(crate) fn cfg_from_form(&self, f: &ConnForm) -> (ConnectionConfig, String) {
        let db_type = DbType::ALL[(f.type_index.max(0) as usize).min(3)];
        let mut uri = f.uri.trim().to_string();
        let mut password = f.password.to_string();
        if db_type == DbType::Mongo && !uri.is_empty() {
            // A password typed into the URI belongs in the keyring, not in the config file.
            let (clean, pw) = mongo::split_uri_password(&uri);
            if let Some(p) = pw {
                uri = clean;
                if password.is_empty() {
                    password = p;
                }
            }
        }
        let port_text = f.port.trim();
        let port = if port_text.is_empty() { db_type.default_port() } else { port_text.parse().unwrap_or(0) };
        let existing_last_used = self.connections.iter().find(|c| c.id == f.id.as_str()).map_or(0, |c| c.last_used);
        let cfg = ConnectionConfig {
            id: if f.id.is_empty() { config::new_id() } else { f.id.to_string() },
            name: f.name.trim().to_string(),
            db_type,
            host: f.host.trim().to_string(),
            port,
            database: f.database.trim().to_string(),
            username: f.user.trim().to_string(),
            environment: Environment::ALL[(f.env_index.max(0) as usize).min(3)],
            ssl: SslMode::ALL[(f.ssl_index.max(0) as usize).min(3)],
            mongo_uri: uri,
            remember_password: f.remember,
            ssh_host: f.ssh_host.trim().to_string(),
            ssh_port: f.ssh_port.trim().parse().unwrap_or(22),
            ssh_user: f.ssh_user.trim().to_string(),
            ssh_key: f.ssh_key.trim().to_string(),
            read_only: f.read_only,
            ssl_ca: f.ssl_ca.trim().to_string(),
            ssl_cert: f.ssl_cert.trim().to_string(),
            ssl_key: f.ssl_key.trim().to_string(),
            last_used: existing_last_used,
        };
        (cfg, password)
    }

    pub(crate) fn show_form(&mut self, c: &ConnectionConfig, password: String, is_new: bool, info: String, error: String) {
        self.form_id = c.id.clone();
        let c = c.clone();
        ui(&self.w, move |st| {
            st.set_form(ConnForm {
                id: c.id.into(),
                name: c.name.into(),
                type_index: c.db_type.index() as i32,
                host: c.host.into(),
                port: c.port.to_string().into(),
                database: c.database.into(),
                user: c.username.into(),
                password: password.into(),
                env_index: c.environment.index() as i32,
                ssl_index: c.ssl.index() as i32,
                uri: c.mongo_uri.into(),
                remember: c.remember_password,
                ssh_port: if c.ssh_host.is_empty() { String::new() } else { c.ssh_port.to_string() }.into(),
                ssh_host: c.ssh_host.into(),
                ssh_user: c.ssh_user.into(),
                ssh_key: c.ssh_key.into(),
                read_only: c.read_only,
                ssl_ca: c.ssl_ca.into(),
                ssl_cert: c.ssl_cert.into(),
                ssl_key: c.ssl_key.into(),
            });
            st.set_form_is_new(is_new);
            st.set_form_info(info.into());
            st.set_form_error(error.into());
            st.set_form_busy(false);
        });
        self.push_connections();
    }

    /// Fill the form from a pasted `postgres://` / `mysql://` string.
    pub(crate) fn parse_conn_url(&mut self, text: &str) {
        let Some(p) = dboard_core::url::parse(text) else {
            return self.form_error("Not a postgres:// or mysql:// connection string.");
        };
        let mut c = ConnectionConfig::new_blank();
        if let Some(existing) = self.connections.iter().find(|c| c.id == self.form_id) {
            c.id = existing.id.clone();
        } else if !self.form_id.is_empty() {
            c.id = self.form_id.clone();
        }
        c.db_type = p.db_type;
        c.host = p.host.clone();
        c.port = p.port.unwrap_or(p.db_type.default_port());
        c.username = p.user;
        c.database = p.database;
        c.name = p.host;
        if let Some(s) = p.ssl {
            c.ssl = s;
        }
        self.show_form(&c, p.password, true, "Filled from connection string. Pick an environment, then Save or Connect.".into(), String::new());
    }

    pub(crate) fn new_conn(&mut self) {
        let c = ConnectionConfig::new_blank();
        self.show_form(&c, String::new(), true, String::new(), String::new());
    }

    pub(crate) fn select_conn(&mut self, id: &str) {
        let Some(c) = self.connections.iter().find(|c| c.id == id).cloned() else { return self.new_conn() };
        let pw = if c.remember_password { secrets::get(&c).unwrap_or_default() } else { String::new() };
        self.show_form(&c, pw, false, String::new(), String::new());
    }

    pub(crate) fn form_error(&self, msg: impl Into<String>) {
        let m = msg.into();
        ui(&self.w, move |st| {
            st.set_form_error(m.into());
            st.set_form_info("".into());
            st.set_form_busy(false);
            st.set_busy(false);
        });
    }

    /// Validate + persist the form. Returns the saved config and the password typed for this session.
    pub(crate) fn save_conn(&mut self, f: &ConnForm, quiet: bool) -> Option<(ConnectionConfig, String)> {
        let (cfg, pw) = self.cfg_from_form(f);
        if let Some(msg) = cfg.validate() {
            self.form_error(msg);
            return None;
        }
        match self.connections.iter_mut().find(|c| c.id == cfg.id) {
            Some(slot) => *slot = cfg.clone(),
            None => self.connections.push(cfg.clone()),
        }
        if let Err(e) = self.store.save_connections(&self.connections) {
            self.form_error(format!("Could not save connections: {e}"));
            return None;
        }
        let mut info = if quiet { String::new() } else { "Saved.".to_string() };
        if cfg.remember_password && !pw.is_empty() {
            if let Err(e) = secrets::set(&cfg, &pw) {
                info = format!("Saved, but the password could not be stored ({e}). You'll be asked for it next time.");
            }
        } else if !cfg.remember_password || pw.is_empty() {
            secrets::delete(&cfg);
        }
        self.show_form(&cfg, pw.clone(), false, info, String::new());
        Some((cfg, pw))
    }

    pub(crate) async fn test_conn(&mut self, f: &ConnForm) {
        let (cfg, pw) = self.cfg_from_form(f);
        if let Some(msg) = cfg.validate() {
            return self.form_error(msg);
        }
        ui(&self.w, |st| {
            st.set_form_busy(true);
            st.set_form_error("".into());
            st.set_form_info("Connecting…".into());
        });
        match dboard_core::driver::test_connection(cfg, &pw).await {
            Ok(v) => ui(&self.w, move |st| {
                st.set_form_busy(false);
                st.set_form_info(format!("Connection successful · server {v}").into());
            }),
            Err(e) => self.form_error(e.to_string()),
        }
    }

    pub(crate) async fn connect_conn(&mut self, f: &ConnForm) {
        let Some((cfg, pw)) = self.save_conn(f, true) else { return };
        ui(&self.w, |st| {
            st.set_busy(true);
            st.set_form_error("".into());
        });
        match Conn::connect(cfg.clone(), &pw).await {
            Ok(conn) => self.on_connected(cfg, conn, pw).await,
            Err(e) => self.form_error(e.to_string()),
        }
    }

    pub(crate) async fn on_connected(&mut self, cfg: ConnectionConfig, conn: Conn, pw: String) {
        // Park the connection we were using, if any; the new one gets its own tab.
        if self.conn.is_some() {
            let old = self.take_session();
            if self.cur < self.parked.len() {
                self.parked[self.cur] = Some(old);
            }
            self.cur = self.sess_meta.len();
        } else {
            self.sess_meta.clear();
            self.sess_ids.clear();
            self.parked.clear();
            self.cur = 0;
        }
        self.sess_meta.push((cfg.display_name(), cfg.environment.color()));
        self.sess_ids.push(cfg.id.clone());
        self.parked.push(None);
        self.env = cfg.environment;
        self.pending = None;
        self.edit_row = None;
        if let Some(c) = self.connections.iter_mut().find(|c| c.id == cfg.id) {
            c.last_used = config::now_secs();
        }
        let _ = self.store.save_connections(&self.connections);
        self.settings.last_connection_id = cfg.id.clone();
        self.persist_settings();
        self.session_pw = pw;
        let mut conn = conn;
        let secs = self.settings.statement_timeout_secs;
        if secs > 0 {
            let _ = conn.set_statement_timeout(secs as u64 * 1000).await;
        }
        self.conn = Some(conn);
        self.hooks_background("on_connect", "");
        self.load_databases().await;
        self.show_session();
    }

    pub(crate) fn take_session(&mut self) -> Session {
        Session {
            conn: self.conn.take(),
            env: self.env,
            tabs: std::mem::take(&mut self.tabs),
            active: self.active.take(),
            closed: std::mem::take(&mut self.closed),
            query_counter: std::mem::take(&mut self.query_counter),
            tree_filter: std::mem::take(&mut self.tree_filter),
            collapsed: std::mem::take(&mut self.collapsed),
            tree: std::mem::take(&mut self.tree),
            activity: std::mem::take(&mut self.activity),
            session_pw: std::mem::take(&mut self.session_pw),
            databases: std::mem::take(&mut self.databases),
            db_all_entry: std::mem::take(&mut self.db_all_entry),
            db_entries: std::mem::take(&mut self.db_entries),
            db_idx: std::mem::replace(&mut self.db_idx, -1),
            users: std::mem::take(&mut self.users),
        }
    }

    pub(crate) fn put_session(&mut self, s: Session) {
        self.conn = s.conn;
        self.env = s.env;
        self.tabs = s.tabs;
        self.active = s.active;
        self.closed = s.closed;
        self.query_counter = s.query_counter;
        self.tree_filter = s.tree_filter;
        self.collapsed = s.collapsed;
        self.tree = s.tree;
        self.activity = s.activity;
        self.session_pw = s.session_pw;
        self.databases = s.databases;
        self.db_all_entry = s.db_all_entry;
        self.db_entries = s.db_entries;
        self.db_idx = s.db_idx;
        self.users = s.users;
        self.pending = None;
        self.edit_row = None;
    }

    pub(crate) fn push_sessions(&self) {
        let v: Vec<(String, u32, bool)> = self.sess_meta.iter().enumerate().map(|(i, (n, c))| (n.clone(), *c, i == self.cur)).collect();
        ui(&self.w, move |st| {
            let items: Vec<SessionTab> = v
                .into_iter()
                .map(|(n, c, active)| SessionTab { name: n.into(), color: slint::Color::from_rgb_u8((c >> 16) as u8, (c >> 8) as u8, c as u8), active })
                .collect();
            st.set_sessions(ModelRc::new(VecModel::from(items)));
        });
    }

    pub(crate) fn push_databases(&self) {
        let (entries, idx) = (self.db_entries.clone(), self.db_idx);
        ui(&self.w, move |st| {
            st.set_databases(strs(entries));
            st.set_database_index(idx);
        });
    }

    pub(crate) fn close_dialogs(&self) {
        ui(&self.w, |st| {
            st.set_busy(false);
            st.set_confirm_open(false);
            st.set_insert_open(false);
            st.set_json_open(false);
            st.set_palette_open(false);
            st.set_export_open(false);
            st.set_editrow_open(false);
            st.set_users_open(false);
            st.set_xfer_open(false);
            st.set_ctx_open(false);
            st.set_draft_open(false);
        });
    }

    /// Show the active session: header, databases, tree, tabs, grid and inspector.
    pub(crate) fn show_session(&mut self) {
        let Some(conn) = &self.conn else { return };
        let (name, color) = self.sess_meta.get(self.cur).cloned().unwrap_or_default();
        let _ = color;
        let (env, protected, db) = (self.env, self.protected(), conn.db_type().index() as i32);
        let status = self.status_line();
        let act = self.activity.clone();
        let in_tx = self.conn.as_ref().is_some_and(|c| c.in_transaction());
        let read_only = self.read_only();
        self.push_sessions();
        self.save_open_sessions();
        self.push_databases();
        self.rebuild_tree();
        self.push_saved_and_history();
        self.show_active();
        self.push_inspector();
        ui(&self.w, move |st| {
            st.set_conn_name(name.into());
            st.set_env_label(env.label().into());
            let c = env.color();
            st.set_env_color(slint::Color::from_rgb_u8((c >> 16) as u8, (c >> 8) as u8, c as u8));
            st.set_status(status.into());
            st.set_db_type(db);
            st.set_is_protected(protected);
            st.set_in_tx(in_tx);
            st.set_read_only(read_only);
            st.set_tree_filter("".into());
            st.set_activity(strs(act));
            st.set_busy(false);
            st.set_connected(true);
        });
    }

    pub(crate) fn switch_session(&mut self, i: usize) {
        if i >= self.sess_meta.len() {
            return;
        }
        if i == self.cur && self.conn.is_some() {
            ui(&self.w, |st| st.set_connected(true));
            return;
        }
        let old = self.take_session();
        if self.cur < self.parked.len() {
            self.parked[self.cur] = Some(old);
        }
        if let Some(s) = self.parked[i].take() {
            self.put_session(s);
        }
        self.cur = i;
        self.close_dialogs();
        self.show_session();
    }

    /// Close one open connection (`-1` = the current one). The last one returns to the connection list.
    pub(crate) fn close_session(&mut self, idx: i32) {
        let i = if idx < 0 { self.cur } else { idx as usize };
        if i >= self.sess_meta.len() {
            return;
        }
        if i == self.cur {
            self.hooks_background("on_disconnect", "");
        }
        self.sess_meta.remove(i);
        self.sess_ids.remove(i);
        if i == self.cur {
            drop(self.take_session());
            self.parked.remove(i);
            self.close_dialogs();
            if self.sess_meta.is_empty() {
                self.cur = 0;
                self.push_sessions();
        self.save_open_sessions();
                self.push_connections();
                ui(&self.w, |st| {
                    st.set_connected(false);
                    st.set_databases(strs(Vec::new()));
                });
            } else {
                self.cur = i.min(self.sess_meta.len() - 1);
                if let Some(s) = self.parked[self.cur].take() {
                    self.put_session(s);
                }
                self.show_session();
            }
        } else {
            self.parked.remove(i);
            if i < self.cur {
                self.cur -= 1;
            }
            self.push_sessions();
        self.save_open_sessions();
        }
    }

    /// "PostgreSQL 16.4 · user@host:5432 / database", for the top bar.
    pub(crate) fn status_line(&self) -> String {
        let Some(conn) = &self.conn else { return String::new() };
        let cfg = &conn.config;
        if cfg.db_type == DbType::Sqlite {
            // the server version already reads "SQLite 3.x"
            return format!("{} · {}", conn.server_version, cfg.database);
        }
        let who = if cfg.db_type == DbType::Mongo && !cfg.mongo_uri.is_empty() {
            cfg.mongo_uri.clone()
        } else {
            format!(
                "{}{}:{}{}",
                if cfg.username.is_empty() { String::new() } else { format!("{}@", cfg.username) },
                cfg.host,
                cfg.port,
                if cfg.database.is_empty() { String::new() } else { format!(" / {}", cfg.database) }
            )
        };
        format!("{} {} · {who}", cfg.db_type.label(), conn.server_version)
    }

    /// Fill the database drop-down with every database on the server.
    pub(crate) async fn load_databases(&mut self) {
        let Some(conn) = self.conn.as_mut() else { return };
        let list = conn.list_databases().await.unwrap_or_default();
        let current = conn.current_database().await.ok().flatten();
        self.db_all_entry = conn.db_type() != DbType::Postgres;
        let mut entries = Vec::new();
        if self.db_all_entry && !list.is_empty() {
            entries.push("All databases".to_string());
        }
        entries.extend(list.iter().cloned());
        let idx = match &current {
            Some(c) => entries.iter().position(|e| e == c).unwrap_or(0),
            None => 0,
        };
        self.databases = list;
        self.db_entries = entries;
        self.db_idx = idx as i32;
        self.push_databases();
        self.rebuild_tree();
    }

    pub(crate) async fn switch_database(&mut self, i: usize) {
        let pick: Option<String> = if self.db_all_entry {
            if i == 0 { None } else { self.databases.get(i - 1).cloned() }
        } else {
            self.databases.get(i).cloned()
        };
        let pw = self.session_pw.clone();
        let Some(conn) = self.conn.as_mut() else { return };
        ui(&self.w, |st| st.set_busy(true));
        let res = conn.switch_database(pick.as_deref(), &pw).await;
        match res {
            Ok(()) => {
                self.tabs.clear();
                self.closed.clear();
                self.active = None;
                self.collapsed.clear();
                self.tree_filter.clear();
                let name = pick.clone().unwrap_or_else(|| "all databases".into());
                self.log_activity(None, &format!("USE {name}"));
                let status = self.status_line();
                self.rebuild_tree();
                self.show_active();
                self.push_inspector();
                ui(&self.w, move |st| {
                    st.set_status(status.into());
                    st.set_tree_filter("".into());
                    st.set_busy(false);
                });
                self.toast(format!("Now using {name}"));
            }
            Err(e) => {
                // Put the drop-down back on the database that is still in use.
                self.load_databases().await;
                ui(&self.w, |st| st.set_busy(false));
                self.toast(format!("Could not switch database: {e}"));
            }
        }
    }

    pub(crate) fn disconnect(&mut self) {
        self.close_session(-1);
    }

    pub(crate) fn delete_conn(&mut self, id: &str) {
        if let Some(c) = self.connections.iter().find(|c| c.id == id).cloned() {
            secrets::delete(&c);
        }
        self.connections.retain(|c| c.id != id);
        let _ = self.store.save_connections(&self.connections);
        if self.settings.last_connection_id == id {
            self.settings.last_connection_id.clear();
            self.persist_settings();
        }
        match self.connections.first().map(|c| c.id.clone()) {
            Some(next) => self.select_conn(&next),
            None => self.new_conn(),
        }
    }

    pub(crate) fn duplicate_conn(&mut self, id: &str) {
        let Some(orig) = self.connections.iter().find(|c| c.id == id).cloned() else { return };
        let mut copy = orig.clone();
        copy.id = config::new_id();
        copy.name = format!("{} (copy)", orig.display_name());
        copy.last_used = 0;
        let pw = secrets::get(&orig).unwrap_or_default();
        if copy.remember_password && !pw.is_empty() {
            let _ = secrets::set(&copy, &pw);
        }
        self.connections.push(copy.clone());
        let _ = self.store.save_connections(&self.connections);
        self.show_form(&copy, pw, false, "Duplicated. Edit the copy and Save.".into(), String::new());
    }
}


impl Worker {
    /// Remember which connections are open so the next launch can reopen them.
    pub(crate) fn save_open_sessions(&mut self) {
        if self.settings.open_connections != self.sess_ids {
            self.settings.open_connections = self.sess_ids.clone();
            self.persist_settings();
        }
    }

    /// Reopen the connections that were open when the app last quit (only those whose password is
    /// in the keyring, or that need none). Failures are skipped quietly.
    pub async fn restore_sessions(&mut self) {
        let ids = self.settings.open_connections.clone();
        let mut failed = Vec::new();
        for id in ids {
            let Some(cfg) = self.connections.iter().find(|c| c.id == id).cloned() else { continue };
            let pw = if cfg.remember_password { secrets::get(&cfg).unwrap_or_default() } else { String::new() };
            if cfg.remember_password && pw.is_empty() && cfg.db_type != DbType::Mongo {
                failed.push(cfg.display_name());
                continue;
            }
            ui(&self.w, |st| st.set_busy(true));
            match Conn::connect(cfg.clone(), &pw).await {
                Ok(conn) => self.on_connected(cfg, conn, pw).await,
                Err(_) => failed.push(cfg.display_name()),
            }
            ui(&self.w, |st| st.set_busy(false));
        }
        if !failed.is_empty() {
            self.toast(format!("Could not reopen: {}", failed.join(", ")));
        }
    }
}

impl Worker {
    /// 0 = begin, 1 = commit, 2 = roll back the session's transaction.
    pub(crate) async fn tx_action(&mut self, what: u8) {
        let Some(conn) = self.conn.as_mut() else { return };
        let res = match what {
            0 => conn.begin().await,
            1 => conn.commit().await,
            _ => conn.rollback().await,
        };
        let in_tx = conn.in_transaction();
        match res {
            Ok(()) => {
                ui(&self.w, move |st| st.set_in_tx(in_tx));
                self.toast(match what {
                    0 => "Transaction started. Nothing is saved until you Commit.",
                    1 => "Transaction committed.",
                    _ => "Transaction rolled back.",
                });
                if what != 0 {
                    // Every open table may show rows the transaction wrote or discarded.
                    let others: Vec<usize> = (0..self.tabs.len()).filter(|i| Some(*i) != self.active && self.tabs[*i].kind == Kind::Table).collect();
                    for i in others {
                        let tab = self.tabs[i].request_snapshot();
                        self.load_table(i, tab).await;
                    }
                    self.refresh().await;
                }
            }
            Err(e) => self.toast(e.to_string()),
        }
    }
}
