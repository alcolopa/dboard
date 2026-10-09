//! Users (part of the worker).

use super::*;

// ---------------------------------------------------------------------------------------------
// Users & access
// ---------------------------------------------------------------------------------------------

impl Worker {
    pub(crate) fn user_scope_text(&self) -> String {
        if self.conn.as_ref().is_some_and(|c| c.db_type() == DbType::Postgres) {
            "Choose a database and access level. PostgreSQL public privileges and inherited roles can also provide access to other databases.".into()
        } else {
            "Choose a database and access level. Existing users keep permissions they already have on other databases.".into()
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
        let mut databases = conn.list_databases().await.unwrap_or_default();
        if let Some(d) = &db {
            if !databases.contains(d) { databases.push(d.clone()); }
        }
        databases.sort();
        databases.dedup();
        let selected = db.as_ref().and_then(|d| databases.iter().position(|x| x == d)).map_or(-1, |i| i as i32);
        let scope = self.user_scope_text();
        ui(&self.w, move |st| {
            st.set_user_is_mysql(ty == DbType::MySql);
            st.set_user_is_pg(ty == DbType::Postgres);
            st.set_user_scope(scope.into());
            st.set_user_databases(strs(databases));
            st.set_user_db_index(selected);
            st.set_user_error("".into());
            st.set_user_info("".into());
            st.set_user_grants(strs(Vec::new()));
            st.set_user_can_manage(false);
            st.set_user_can_password(false);
            st.set_user_level(-1);
            st.set_user_tables(strs(Vec::new()));
            st.set_user_create_open(false);
            st.set_users_open(true);
        });
        self.reload_users(None).await;
        if !self.users.is_empty() { self.user_select(0, db.as_deref().unwrap_or("")).await; }
    }

    /// Re-read the user list; keep `select` (by display name) selected when it still exists.
    pub(crate) async fn reload_users(&mut self, select: Option<String>) {
        let Some(conn) = self.conn.as_mut() else { return };
        match conn.list_users().await {
            Ok(list) => {
                self.users = list;
                let idx = select.and_then(|n| self.users.iter().position(|u| u.display() == n)).map_or(-1, |i| i as i32);
                self.push_users(idx);
                ui(&self.w, |st| st.set_user_grants(strs(Vec::new())));
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

    pub(crate) async fn user_select(&mut self, i: usize, database: &str) {
        let Some(u) = self.users.get(i).cloned() else { return };
        ui(&self.w, move |st| st.set_user_sel(i as i32));
        self.user_busy(true);
        let result = match self.user_access_connection(database).await {
            Ok(mut c) => match c.user_access(&u).await {
                Ok(access) => c.user_grants(&u).await.map(|lines| (access, lines)),
                Err(e) => Err(e),
            },
            Err(e) => Err(e),
        };
        let read_only = self.conn.as_ref().is_some_and(|c| c.config.read_only);
        ui(&self.w, move |st| {
            st.set_user_busy(false);
            st.set_user_info("".into());
            st.set_user_table_index(-1);
            st.set_user_table_level(-1);
            match result {
                Ok((access, lines)) => {
                    st.set_user_level(access.level);
                    st.set_user_can_manage(access.manage && !read_only);
                    st.set_user_can_password(access.password && !read_only);
                    let names = access.tables.iter().map(|(s,n,_)|format!("{s}.{n}")).collect();
                    st.set_user_tables(strs(names));
                    st.set_user_table_levels(ModelRc::new(VecModel::from(access.tables.iter().map(|t|t.2).collect::<Vec<_>>())));
                    st.set_user_grants(strs(lines));
                    st.set_user_error("".into());
                }
                Err(e) => {
                    st.set_user_level(-1);
                    st.set_user_can_manage(false);
                    st.set_user_can_password(false);
                    st.set_user_tables(strs(Vec::new()));
                    st.set_user_grants(strs(Vec::new()));
                    st.set_user_error(e.to_string().into());
                }
            }
        });
    }

    pub(crate) async fn user_set_table(&mut self, i: usize, level: i32, database: String, table: usize) {
        if self.refuse_if_read_only() || !(0..=3).contains(&level) { return; }
        let Some(u) = self.users.get(i).cloned() else { return };
        self.user_busy(true);
        let result = async {
            let mut c = self.user_access_connection(&database).await?;
            let access = c.user_access(&u).await?;
            if !access.manage { return Err(dboard_core::Error::Db("You cannot manage this user's access.".into())); }
            let Some((schema, name, _)) = access.tables.get(table) else { return Err(dboard_core::Error::Db("Select a table first.".into())); };
            c.set_table_access(&u, schema, name, AccessLevel::from_index(level)).await
        }.await;
        match result {
            Ok(()) => { self.user_select(i, &database).await; self.user_result(Some("Table access saved. Database and inherited grants may still provide broader access.".into()), None); }
            Err(e) => self.user_result(None, Some(e.to_string())),
        }
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

    /// Use an independent connection so access management never switches the editor's database
    /// or disturbs its transaction or undo history.
    async fn user_access_connection(&self, database: &str) -> dboard_core::Result<Conn> {
        if database.is_empty() {
            return Err(dboard_core::Error::Db("Choose a database first.".into()));
        }
        let config = self.conn.as_ref().ok_or_else(|| dboard_core::Error::Db("Connect to a server first.".into()))?.config.clone();
        Conn::connect_for_access(config, &self.session_pw, database).await
    }

    pub(crate) async fn user_create(&mut self, name: String, host: String, password: String, level: i32, admin: bool, database: String) {
        if self.refuse_if_read_only() { return; }
        let new = NewUser { name: name.trim().to_string(), host: host.trim().to_string(), password, access: AccessLevel::from_index(level), admin };
        self.user_busy(true);
        let res = match self.user_access_connection(&database).await {
            Ok(mut c) => c.create_user(&new).await,
            Err(e) => Err(e),
        };
        match res {
            Ok(()) => {
                self.log_activity(None, &format!("CREATE USER {}", new.name));
                let shown = UserInfo { name: new.name.clone(), origin: if self.conn.as_ref().is_some_and(|c| c.db_type() == DbType::MySql) { if new.host.is_empty() { "%".into() } else { new.host.clone() } } else if self.conn.as_ref().is_some_and(|c| c.db_type() == DbType::Mongo) { database.clone() } else { String::new() }, summary: String::new() }.display();
                ui(&self.w, |st| st.set_user_create_open(false));
                self.reload_users(Some(shown.clone())).await;
                if let Some(i) = self.users.iter().position(|u| u.display() == shown) {
                    self.user_select(i, &database).await;
                }
                self.user_result(Some(format!("User “{}” created with {} access to “{}”.", new.name, AccessLevel::LABELS[level.clamp(0, 3) as usize], database)), None);
            }
            Err(e) => self.user_result(None, Some(e.to_string())),
        }
    }

    pub(crate) async fn user_set_level(&mut self, i: usize, level: i32, database: String) {
        if self.refuse_if_read_only() { return; }
        let Some(u) = self.users.get(i).cloned() else { return };
        self.user_busy(true);
        let res = match self.user_access_connection(&database).await {
            Ok(mut c) => match c.user_access(&u).await {
                Ok(a) if a.manage && (0..=3).contains(&level) => c.set_access(&u, AccessLevel::from_index(level)).await,
                Ok(_) => Err(dboard_core::Error::Db("You cannot manage this user's access.".into())),
                Err(e) => Err(e),
            },
            Err(e) => Err(e),
        };
        match res {
            Ok(()) => {
                self.log_activity(None, &format!("SET ACCESS {} = {}", u.name, AccessLevel::LABELS[level.clamp(0, 3) as usize]));
                self.user_select(i, &database).await;
                self.user_result(Some(format!("{} now has “{}” on “{}”.", u.name, AccessLevel::LABELS[level.clamp(0, 3) as usize], database)), None);
            }
            Err(e) => self.user_result(None, Some(e.to_string())),
        }
    }

    pub(crate) async fn user_password(&mut self, i: usize, pw: String) {
        if self.refuse_if_read_only() { return; }
        let Some(u) = self.users.get(i).cloned() else { return };
        self.user_busy(true);
        let res = match self.conn.as_mut() {
            Some(c) => match c.user_access(&u).await {
                Ok(a) if a.password => c.set_password(&u, &pw).await,
                Ok(_) => Err(dboard_core::Error::Db("You cannot change this user's password.".into())),
                Err(e) => Err(e),
            },
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
        if self.refuse_if_read_only() { return; }
        let Some(u) = self.users.get(i).cloned() else { return };
        self.user_busy(true);
        let res = match self.conn.as_mut() {
            Some(c) => match c.user_access(&u).await {
                Ok(a) if a.manage => c.drop_user(&u).await,
                Ok(_) => Err(dboard_core::Error::Db("You cannot delete this user.".into())),
                Err(e) => Err(e),
            },
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

