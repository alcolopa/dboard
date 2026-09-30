use crate::model::*;
use crate::sql::{self, Dialect};
use crate::{Error, Result};
use mysql_async::prelude::*;
use mysql_async::{Conn, OptsBuilder, Params, Row, SslOpts, Value};
use std::time::{Duration, Instant};

const D: Dialect = Dialect::My;

pub struct My {
    conn: Conn,
    is_mariadb: bool,
}

impl From<mysql_async::Error> for Error {
    fn from(e: mysql_async::Error) -> Self {
        match &e {
            mysql_async::Error::Server(s) => Error::Db(humanize(s.code, &s.message)),
            _ => Error::Db(e.to_string()),
        }
    }
}

fn humanize(code: u16, message: &str) -> String {
    match code {
        1062 => "A record with this unique value already exists in the table.".into(),
        1451 | 1452 => "This change violates a foreign key constraint.".into(),
        1048 => "A required (NOT NULL) column cannot be empty.".into(),
        1264 | 1366 | 1292 | 1265 => format!("Invalid value for this column type: {message}"),
        1044 | 1045 | 1142 | 1143 => "Permission denied for this operation.".into(),
        _ => message.to_string(),
    }
}

pub fn value_to_cell(v: &Value) -> Cell {
    Some(match v {
        Value::NULL => return None,
        Value::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Double(f) => f.to_string(),
        Value::Date(y, mo, d, h, mi, s, us) => {
            if *h == 0 && *mi == 0 && *s == 0 && *us == 0 {
                format!("{y:04}-{mo:02}-{d:02}")
            } else if *us == 0 {
                format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}")
            } else {
                format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}.{us:06}")
            }
        }
        Value::Time(neg, days, h, mi, s, _) => {
            format!("{}{:02}:{mi:02}:{s:02}", if *neg { "-" } else { "" }, u32::from(*days) * 24 + u32::from(*h))
        }
    })
}

fn rows_to_cells(rows: &[Row], width: usize) -> Vec<Vec<Cell>> {
    rows.iter().map(|r| (0..width).map(|i| r.as_ref(i).and_then(value_to_cell)).collect()).collect()
}

impl My {
    pub async fn connect(c: &ConnectionConfig, password: &str) -> Result<Self> {
        let base = OptsBuilder::default()
            .ip_or_hostname(c.host.clone())
            .tcp_port(c.port)
            .user(Some(c.username.clone()))
            .pass(Some(password.to_string()))
            .db_name(Some(c.database.clone()).filter(|d| !d.is_empty()));
        let with_ssl = |b: OptsBuilder| match c.ssl {
            SslMode::Disable => b,
            SslMode::Prefer | SslMode::Require => b.ssl_opts(Some(
                SslOpts::default().with_danger_accept_invalid_certs(true).with_danger_skip_domain_validation(true),
            )),
            SslMode::VerifyFull => b.ssl_opts(Some(SslOpts::default())),
        };
        let connect = |b: OptsBuilder| async move {
            tokio::time::timeout(Duration::from_secs(10), Conn::new(b))
                .await
                .map_err(|_| Error::Db("connection timed out".into()))?
                .map_err(Error::from)
        };
        let conn = match c.ssl {
            SslMode::Prefer => match connect(with_ssl(base.clone())).await {
                Ok(c) => c,
                Err(_) => connect(base).await?,
            },
            _ => connect(with_ssl(base)).await?,
        };
        Ok(Self { conn, is_mariadb: false })
    }

    pub async fn version(&mut self) -> Result<String> {
        let v: Option<String> = self.conn.query_first("SELECT VERSION()").await?;
        let v = v.unwrap_or_default();
        self.is_mariadb = v.to_lowercase().contains("mariadb");
        Ok(v)
    }

    pub async fn metadata(&mut self) -> Result<Metadata> {
        const SYS: &str = "('mysql','information_schema','performance_schema','sys')";
        let tables: Vec<Row> = self
            .conn
            .query(format!(
                "SELECT TABLE_SCHEMA, TABLE_NAME, TABLE_TYPE, TABLE_ROWS, COALESCE(DATA_LENGTH,0)+COALESCE(INDEX_LENGTH,0) \
                 FROM information_schema.TABLES WHERE TABLE_SCHEMA NOT IN {SYS} ORDER BY 1, 2"
            ))
            .await?;
        let cols: Vec<Row> = self
            .conn
            .query(format!(
                "SELECT TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_KEY, COLUMN_DEFAULT \
                 FROM information_schema.COLUMNS WHERE TABLE_SCHEMA NOT IN {SYS} ORDER BY TABLE_SCHEMA, TABLE_NAME, ORDINAL_POSITION"
            ))
            .await?;
        let fks: Vec<Row> = self
            .conn
            .query(format!(
                "SELECT TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME, REFERENCED_TABLE_SCHEMA, REFERENCED_TABLE_NAME, REFERENCED_COLUMN_NAME \
                 FROM information_schema.KEY_COLUMN_USAGE WHERE REFERENCED_TABLE_NAME IS NOT NULL AND TABLE_SCHEMA NOT IN {SYS}"
            ))
            .await?;
        let idx: Vec<Row> = self
            .conn
            .query(format!(
                "SELECT TABLE_SCHEMA, TABLE_NAME, INDEX_NAME, NON_UNIQUE, GROUP_CONCAT(COLUMN_NAME ORDER BY SEQ_IN_INDEX) \
                 FROM information_schema.STATISTICS WHERE TABLE_SCHEMA NOT IN {SYS} GROUP BY 1, 2, 3, 4"
            ))
            .await?;
        let routines: Vec<Row> = self
            .conn
            .query(format!(
                "SELECT ROUTINE_SCHEMA, ROUTINE_NAME, ROUTINE_TYPE FROM information_schema.ROUTINES WHERE ROUTINE_SCHEMA NOT IN {SYS} ORDER BY 1, 2"
            ))
            .await?;

        let s = |r: &Row, i: usize| r.as_ref(i).and_then(value_to_cell).unwrap_or_default();
        let mut out: Vec<Table> = tables
            .iter()
            .map(|r| Table {
                schema: s(r, 0),
                name: s(r, 1),
                kind: if s(r, 2).contains("VIEW") { TableKind::View } else { TableKind::Table },
                columns: Vec::new(),
                estimated_rows: s(r, 3).parse().ok(),
                size_bytes: s(r, 4).parse().ok(),
                indexes: Vec::new(),
            })
            .collect();
        let pos: std::collections::HashMap<(String, String), usize> =
            out.iter().enumerate().map(|(i, t)| ((t.schema.clone(), t.name.clone()), i)).collect();
        let fk_map: std::collections::HashMap<(String, String, String), String> = fks
            .iter()
            .map(|r| ((s(r, 0), s(r, 1), s(r, 2)), format!("{}.{}({})", s(r, 3), s(r, 4), s(r, 5))))
            .collect();
        for r in &cols {
            let (sc, t, name) = (s(r, 0), s(r, 1), s(r, 2));
            if let Some(&i) = pos.get(&(sc.clone(), t.clone())) {
                let fk = fk_map.get(&(sc, t, name.clone())).cloned();
                out[i].columns.push(Column {
                    name,
                    type_name: s(r, 3),
                    nullable: s(r, 4) == "YES",
                    is_primary_key: s(r, 5) == "PRI",
                    default: r.as_ref(6).and_then(value_to_cell),
                    fk,
                });
            }
        }
        for r in &idx {
            if let Some(&i) = pos.get(&(s(r, 0), s(r, 1))) {
                let unique = if s(r, 3) == "0" { "UNIQUE " } else { "" };
                out[i].indexes.push(format!("{unique}INDEX {} ({})", s(r, 2), s(r, 4)));
            }
        }
        let objects = routines
            .iter()
            .map(|r| DbObject {
                schema: s(r, 0),
                name: s(r, 1),
                kind: if s(r, 2) == "PROCEDURE" { ObjectKind::Procedure } else { ObjectKind::Function },
            })
            .collect();
        Ok(Metadata { tables: out, objects })
    }

    pub async fn fetch(&mut self, t: &Table, p: &Page) -> Result<Rows> {
        let started = Instant::now();
        let rows: Vec<Row> = self.conn.query(sql::select_page(D, t, p)).await?;
        Ok(Rows {
            columns: t.columns.iter().map(|c| c.name.clone()).collect(),
            rows: rows_to_cells(&rows, t.columns.len()),
            duration_ms: started.elapsed().as_secs_f64() * 1000.0,
            total_estimate: t.estimated_rows,
        })
    }

    pub async fn query(&mut self, sql_text: &str) -> Result<Rows> {
        let started = Instant::now();
        let mut res = self.conn.query_iter(sql_text).await?;
        let columns: Vec<String> =
            res.columns().map(|c| c.iter().map(|c| c.name_str().to_string()).collect()).unwrap_or_default();
        let rows: Vec<Row> = res.collect().await?;
        let affected = res.affected_rows();
        drop(res);
        let duration_ms = started.elapsed().as_secs_f64() * 1000.0;
        if columns.is_empty() {
            return Ok(Rows { columns: vec!["rows affected".into()], rows: vec![vec![Some(affected.to_string())]], duration_ms, total_estimate: None });
        }
        let w = columns.len();
        Ok(Rows { columns, rows: rows_to_cells(&rows, w), duration_ms, total_estimate: None })
    }

    async fn exec(&mut self, stmt: &str, params: Vec<Value>) -> Result<u64> {
        self.conn.exec_drop(stmt, Params::Positional(params)).await?;
        Ok(self.conn.affected_rows())
    }

    pub async fn update(&mut self, t: &Table, column: &str, key: &[String], new: Option<&str>) -> Result<()> {
        let stmt = sql::update_cell(D, t, column, new.is_none()).ok_or_else(|| Error::Db(format!("unknown column {column}")))?;
        let mut params: Vec<Value> = Vec::new();
        if let Some(v) = new {
            params.push(Value::from(v));
        }
        params.extend(key.iter().map(|k| Value::from(k.as_str())));
        // MySQL reports 0 affected rows when the value is unchanged, so 0 is not an error here.
        self.exec(&stmt, params).await?;
        Ok(())
    }

    pub async fn insert(&mut self, t: &Table, vals: &[(String, String)]) -> Result<()> {
        let cols: Vec<&str> = vals.iter().map(|v| v.0.as_str()).collect();
        let stmt = sql::insert_row(D, t, &cols).ok_or_else(|| Error::Db("unknown column".into()))?;
        self.exec(&stmt, vals.iter().map(|v| Value::from(v.1.as_str())).collect()).await?;
        Ok(())
    }

    pub async fn delete(&mut self, t: &Table, key: &[String]) -> Result<()> {
        let stmt = sql::delete_row(D, t).ok_or_else(|| Error::Unsafe("This table has no primary key.".into()))?;
        if self.exec(&stmt, key.iter().map(|k| Value::from(k.as_str())).collect()).await? == 0 {
            return Err(Error::Db("No row matched; it may have been changed or deleted.".into()));
        }
        Ok(())
    }

    pub async fn truncate(&mut self, t: &Table) -> Result<()> {
        self.conn.query_drop(sql::truncate(D, t)).await?;
        Ok(())
    }

    pub async fn drop_table(&mut self, t: &Table) -> Result<()> {
        self.conn.query_drop(sql::drop_table(D, t)).await?;
        Ok(())
    }

    pub async fn ddl(&mut self, t: &Table) -> Result<String> {
        let what = if t.kind == TableKind::View { "VIEW" } else { "TABLE" };
        let rows: Vec<Row> = self.conn.query(format!("SHOW CREATE {what} {}", D.qualified(t))).await?;
        Ok(rows.first().and_then(|r| r.as_ref(1)).and_then(value_to_cell).unwrap_or_default())
    }

    pub async fn explain(&mut self, sql_text: &str, analyze: bool) -> Result<Rows> {
        let body = sql_text.trim().trim_end_matches(';');
        let stmt = match (analyze, self.is_mariadb) {
            (false, _) => format!("EXPLAIN {body}"),
            (true, false) => format!("EXPLAIN ANALYZE {body}"),
            (true, true) => format!("ANALYZE {body}"),
        };
        self.query(&stmt).await
    }

    pub async fn object_def(&mut self, o: &DbObject) -> Result<String> {
        let kind = if o.kind == ObjectKind::Procedure { "PROCEDURE" } else { "FUNCTION" };
        let rows: Vec<Row> = self.conn.query(format!("SHOW CREATE {kind} {}.{}", D.quote(&o.schema), D.quote(&o.name))).await?;
        Ok(rows.first().and_then(|r| r.as_ref(2)).and_then(value_to_cell).unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_values() {
        assert_eq!(value_to_cell(&Value::NULL), None);
        assert_eq!(value_to_cell(&Value::Int(-4)).as_deref(), Some("-4"));
        assert_eq!(value_to_cell(&Value::Bytes(b"hi".to_vec())).as_deref(), Some("hi"));
        assert_eq!(value_to_cell(&Value::Date(2024, 1, 2, 0, 0, 0, 0)).as_deref(), Some("2024-01-02"));
        assert_eq!(value_to_cell(&Value::Date(2024, 1, 2, 3, 4, 5, 0)).as_deref(), Some("2024-01-02 03:04:05"));
        assert_eq!(value_to_cell(&Value::Time(true, 1, 2, 3, 4, 0)).as_deref(), Some("-26:03:04"));
    }

    #[test]
    fn humanizes_errors() {
        assert!(humanize(1062, "x").contains("unique"));
        assert_eq!(humanize(9999, "raw"), "raw");
    }
}
