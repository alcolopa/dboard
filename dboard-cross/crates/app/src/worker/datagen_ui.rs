//! Fill the open table with generated sample rows (part of the worker).

use super::*;
use crate::datagen::{value_for, Rng};

impl Worker {
    pub(crate) async fn generate_rows(&mut self, count: usize, confirmed: bool) {
        if self.is_mongo() {
            return self.toast("Sample data is for SQL tables.");
        }
        if self.refuse_if_read_only() {
            return;
        }
        let Some(tab) = self.active_tab().filter(|t| t.kind == Kind::Table).cloned() else {
            return self.toast("Open a table first, then generate rows into it.");
        };
        if !confirmed && self.protected() {
            let env = self.env.label();
            return self.ask(Pending::GenerateRows(count), format!("Generate rows in {env}"), format!("Insert {count} made-up rows into {}?", tab.name));
        }
        let Some(table) = self.conn.as_ref().and_then(|c| c.table(&tab.schema, &tab.name)).cloned() else { return };
        if !matches!(table.kind, TableKind::Table) {
            return self.toast("Rows can only be generated into tables.");
        }
        if !self.hook_gate(&format!("GENERATE {count} rows into {}.{}", tab.schema, tab.name)) {
            return;
        }
        let d = self.dialect();
        // Pools of real parent keys for foreign-key columns.
        let mut pools: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
        for c in table.columns.iter().filter(|c| c.default.is_none()) {
            let Some(fk) = &c.fk else { continue };
            let Some((target, col)) = fk.strip_suffix(')').and_then(|s| s.rsplit_once('(')) else { continue };
            let (schema, name) = target.split_once('.').unwrap_or(("", target));
            let from = if schema.is_empty() { d.quote(name) } else { format!("{}.{}", d.quote(schema), d.quote(name)) };
            let sql = format!("SELECT {} FROM {from} LIMIT 200", d.quote(col));
            if let Some(conn) = self.conn.as_mut() {
                if let Ok(r) = conn.execute_query(&sql).await {
                    pools.insert(c.name.clone(), r.rows.into_iter().filter_map(|row| row.into_iter().next().flatten()).collect());
                }
            }
        }
        let mut rng = Rng::new(chrono::Local::now().timestamp_nanos_opt().unwrap_or(1) as u64);
        let own_tx = self.conn.as_ref().is_some_and(|c| !c.in_transaction());
        if own_tx {
            if let Some(conn) = self.conn.as_mut() {
                if let Err(e) = conn.begin().await {
                    return self.toast(e.to_string());
                }
            }
        }
        let mut done = 0usize;
        let mut failure: Option<String> = None;
        'rows: for n in 1..=count {
            let mut vals: Vec<(String, String)> = Vec::new();
            for c in table.columns.iter().filter(|c| c.default.is_none()) {
                let v = match pools.get(&c.name) {
                    Some(pool) if !pool.is_empty() => Some(pool[rng.below(pool.len() as u64) as usize].clone()),
                    _ if c.fk.is_some() && !c.nullable => {
                        failure = Some(format!("“{}” points at a table with no rows yet; generate those first.", c.name));
                        break 'rows;
                    }
                    _ if c.fk.is_some() => None,
                    _ => value_for(c, n, &mut rng),
                };
                if let Some(v) = v {
                    vals.push((c.name.clone(), v));
                }
            }
            let Some(conn) = self.conn.as_mut() else { return };
            if let Err(e) = conn.insert_row(&tab.schema, &tab.name, &vals).await {
                failure = Some(e.to_string());
                break;
            }
            done += 1;
        }
        if let Some(conn) = self.conn.as_mut() {
            if own_tx {
                let _ = if failure.is_some() { conn.rollback().await } else { conn.commit().await };
            }
        }
        match failure {
            Some(msg) => self.toast(format!("Stopped after {} row(s), nothing was kept: {msg}", if own_tx { 0 } else { done })),
            None => {
                self.log_activity(None, &format!("GENERATE {done} rows into {}.{}", tab.schema, tab.name));
                self.toast(format!("Inserted {done} sample row(s) into {}.", tab.name));
            }
        }
        self.load_active().await;
    }
}
