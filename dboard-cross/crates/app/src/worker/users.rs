//! Users (part of the worker).

use super::*;

// ---------------------------------------------------------------------------------------------
// Users & access
// ---------------------------------------------------------------------------------------------

impl Worker {
    pub(crate) fn user_scope_text(&self, db: Option<String>) -> String {
        match (self.conn.as_ref().map(|c| c.db_type()), db) {
            (Some(DbType::MySql), None) => "Access levels apply to all databases (pick one in the top bar to limit them to it).".to_string(),
            (Some(DbType::Mongo), None) => "Pick a database in the top bar first: MongoDB access levels apply to one database.".to_string(),
            (_, Some(d)) => format!("Access levels apply to the database “{d}”. Users themselves are server-wide."),
            _ => String::new(),
        }
    }

    pub(crate) fn push_users(&self, select: i32) {
        let rows: Vec<(String, String)> = self
            .users
            .iter()
            .map(|u| (u.display(), u.summary.clone()))
            .collect();
        ui(&self.w, move |st| {
            let v: Vec<UserRow> = rows.into_iter().map(|(n, d)| UserRow { name: n.into(), detail: d.into() }).collect();
            st.set_users(ModelRc::new(VecModel::from(v)));
            st.set_user_sel(select);
        });
    }

    pub(crate) async fn open_users(&mut self) {
        let Some(conn) = self.conn.as_mut() else { return };
        let db = conn.current_database().await.ok().flatten();
        let ty = conn.db_type();
        let scope = self.user_scope_text(db);
        ui(&self.w, move |st| {
            st.set_user_is_mysql(ty == DbType::MySql);
            st.set_user_is_pg(ty == DbType::Postgres);
            st.set_user_scope(scope.into());
            st.set_user_error("".into());
            st.set_user_info("".into());
            st.set_user_grants(strs(Vec::new()));
            st.set_user_create_open(false);
            st.set_users_open(true);
        });
        self.reload_users(None).await;
    }

    /// Re-read the user list; keep `select` (by display name) selected when it still exists.
    pub(crate) async fn reload_users(&mut self, select: Option<String>) {
        let Some(conn) = self.conn.as_mut() else { return };
        match conn.list_users().await {
            Ok(list) => {
                self.users = list;
                let idx = select.and_then(|n| self.users.iter().position(|u| u.display() == n)).map_or(-1, |i| i as i32);
                self.push_users(idx);
                if idx >= 0 {
                    self.user_select(idx as usize).await;
                } else {
                    ui(&self.w, |st| st.set_user_grants(strs(Vec::new())));
                }
                ui(&self.w, |st| st.set_user_error("".into()));
            }
            Err(e) => {
                self.users.clear();
                self.push_users(-1);
                let m = e.to_string();
                ui(&self.w, move |st| st.set_user_error(m.into()));
            }
        }
    }

    pub(crate) async fn user_select(&mut self, i: usize) {
        let Some(u) = self.users.get(i).cloned() else { return };
        ui(&self.w, move |st| st.set_user_sel(i as i32));
        let lines = match self.conn.as_mut() {
            Some(c) => c.user_grants(&u).await.unwrap_or_else(|e| vec![format!("Could not read privileges: {e}")]),
            None => return,
        };
        ui(&self.w, move |st| {
            st.set_user_grants(strs(lines));
            st.set_user_error("".into());
        });
    }

    pub(crate) fn user_busy(&self, busy: bool) {
        ui(&self.w, move |st| st.set_user_busy(busy));
    }

    pub(crate) fn user_result(&self, ok: Option<String>, err: Option<String>) {
        ui(&self.w, move |st| {
            st.set_user_busy(false);
            st.set_user_info(ok.unwrap_or_default().into());
            st.set_user_error(err.unwrap_or_default().into());
        });
    }

    pub(crate) async fn user_create(&mut self, name: String, host: String, password: String, level: i32, admin: bool) {
        let new = NewUser { name: name.trim().to_string(), host: host.trim().to_string(), password, access: AccessLevel::from_index(level), admin };
        self.user_busy(true);
        let res = match self.conn.as_mut() {
            Some(c) => c.create_user(&new).await,
            None => return,
        };
        match res {
            Ok(()) => {
                self.log_activity(None, &format!("CREATE USER {}", new.name));
                let shown = UserInfo { name: new.name.clone(), origin: if self.conn.as_ref().is_some_and(|c| c.db_type() == DbType::MySql) { if new.host.is_empty() { "%".into() } else { new.host.clone() } } else { String::new() }, summary: String::new() }.display();
                ui(&self.w, |st| st.set_user_create_open(false));
                self.reload_users(Some(shown)).await;
                self.user_result(Some(format!("User “{}” created.", new.name)), None);
            }
            Err(e) => self.user_result(None, Some(e.to_string())),
        }
    }

    pub(crate) async fn user_set_level(&mut self, i: usize, level: i32) {
        let Some(u) = self.users.get(i).cloned() else { return };
        self.user_busy(true);
        let res = match self.conn.as_mut() {
            Some(c) => c.set_access(&u, AccessLevel::from_index(level)).await,
            None => return,
        };
        match res {
            Ok(()) => {
                self.log_activity(None, &format!("SET ACCESS {} = {}", u.name, AccessLevel::LABELS[level.clamp(0, 3) as usize]));
                self.user_select(i).await;
                self.user_result(Some(format!("{} now has “{}”.", u.name, AccessLevel::LABELS[level.clamp(0, 3) as usize])), None);
            }
            Err(e) => self.user_result(None, Some(e.to_string())),
        }
    }

    pub(crate) async fn user_password(&mut self, i: usize, pw: String) {
        let Some(u) = self.users.get(i).cloned() else { return };
        self.user_busy(true);
        let res = match self.conn.as_mut() {
            Some(c) => c.set_password(&u, &pw).await,
            None => return,
        };
        match res {
            Ok(()) => {
                self.log_activity(None, &format!("CHANGE PASSWORD {}", u.name));
                self.user_result(Some(format!("Password changed for {}.", u.name)), None);
            }
            Err(e) => self.user_result(None, Some(e.to_string())),
        }
    }

    pub(crate) async fn user_drop(&mut self, i: usize) {
        let Some(u) = self.users.get(i).cloned() else { return };
        self.user_busy(true);
        let res = match self.conn.as_mut() {
            Some(c) => c.drop_user(&u).await,
            None => return,
        };
        match res {
            Ok(()) => {
                self.log_activity(None, &format!("DROP USER {}", u.name));
                self.reload_users(None).await;
                self.user_result(Some(format!("User {} dropped.", u.name)), None);
            }
            Err(e) => self.user_result(None, Some(e.to_string())),
        }
    }
}

