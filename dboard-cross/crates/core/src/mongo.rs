//! MongoDB driver. Collections are presented as tables whose columns are the union of
//! top-level keys seen in the fetched documents; `_id` is the key.

use crate::admin;
use crate::dump::{DumpOptions, DumpStats, ImportOptions, ImportStats};
use crate::model::*;
use crate::{Error, Result};
use mongodb::bson::{doc, oid::ObjectId, Bson, Document};
use mongodb::options::ClientOptions;
use mongodb::Client;
use std::io::{BufRead, Write};
use std::time::{Duration, Instant};

impl From<mongodb::error::Error> for Error {
    fn from(e: mongodb::error::Error) -> Self {
        Error::Db(e.to_string())
    }
}

pub struct Mongo {
    client: Client,
    default_db: Option<String>,
}

// ---------------------------------------------------------------------------------------
// Connection strings (passwords are kept out of saved URIs)
// ---------------------------------------------------------------------------------------

fn pct_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `mongodb://user:pw@host/db` -> (`mongodb://user@host/db`, Some(pw)). The password is meant
/// for the OS keyring, never for the config file.
pub fn split_uri_password(uri: &str) -> (String, Option<String>) {
    let Some(scheme_end) = uri.find("://") else { return (uri.to_string(), None) };
    let (scheme, rest) = uri.split_at(scheme_end + 3);
    let authority_end = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    let Some(at) = authority.rfind('@') else { return (uri.to_string(), None) };
    let (userinfo, host) = authority.split_at(at);
    match userinfo.split_once(':') {
        Some((user, pw)) => (format!("{scheme}{user}{host}{tail}"), Some(pct_decode(pw))),
        None => (uri.to_string(), None),
    }
}

/// Inverse of [`split_uri_password`].
pub fn join_uri_password(uri: &str, password: &str) -> String {
    if password.is_empty() {
        return uri.to_string();
    }
    let Some(scheme_end) = uri.find("://") else { return uri.to_string() };
    let (scheme, rest) = uri.split_at(scheme_end + 3);
    let authority_end = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    match authority.rfind('@') {
        Some(at) if !authority[..at].contains(':') => {
            format!("{scheme}{}:{}{}{tail}", &authority[..at], pct_encode(password), &authority[at..])
        }
        _ => uri.to_string(),
    }
}

fn build_uri(c: &ConnectionConfig, password: &str) -> String {
    if !c.mongo_uri.trim().is_empty() {
        return join_uri_password(c.mongo_uri.trim(), password);
    }
    let mut uri = String::from("mongodb://");
    if !c.username.is_empty() {
        uri.push_str(&pct_encode(&c.username));
        if !password.is_empty() {
            uri.push(':');
            uri.push_str(&pct_encode(password));
        }
        uri.push('@');
    }
    uri.push_str(&format!("{}:{}/", c.host, c.port));
    if !c.database.is_empty() {
        uri.push_str(&pct_encode(&c.database));
    }
    let mut opts = Vec::new();
    if matches!(c.ssl, SslMode::Require | SslMode::VerifyFull) {
        opts.push("tls=true");
        if c.ssl == SslMode::Require {
            opts.push("tlsAllowInvalidCertificates=true");
        }
    }
    if !c.username.is_empty() {
        opts.push("authSource=admin");
    }
    if !opts.is_empty() {
        uri.push('?');
        uri.push_str(&opts.join("&"));
    }
    uri
}

// ---------------------------------------------------------------------------------------
// BSON <-> text
// ---------------------------------------------------------------------------------------

pub fn bson_type_name(b: &Bson) -> &'static str {
    match b {
        Bson::String(_) => "string",
        Bson::Int32(_) => "int32",
        Bson::Int64(_) => "int64",
        Bson::Double(_) => "double",
        Bson::Boolean(_) => "bool",
        Bson::DateTime(_) => "date",
        Bson::ObjectId(_) => "objectId",
        Bson::Document(_) => "object",
        Bson::Array(_) => "array",
        Bson::Null => "null",
        _ => "other",
    }
}

pub fn bson_to_cell(b: &Bson) -> Cell {
    Some(match b {
        Bson::Null => return None,
        Bson::String(s) => s.clone(),
        Bson::ObjectId(o) => o.to_hex(),
        Bson::Int32(i) => i.to_string(),
        Bson::Int64(i) => i.to_string(),
        Bson::Double(f) => f.to_string(),
        Bson::Boolean(v) => v.to_string(),
        Bson::DateTime(d) => d.try_to_rfc3339_string().unwrap_or_else(|_| d.to_string()),
        other => other.clone().into_relaxed_extjson().to_string(),
    })
}

/// Parse user text into a BSON value of the column's observed type.
pub fn parse_typed(text: &str, ty: &str) -> std::result::Result<Bson, String> {
    let bad = |what: &str| format!("'{text}' is not a valid {what}");
    match ty {
        "string" => Ok(Bson::String(text.to_string())),
        "int32" => text.trim().parse().map(Bson::Int32).map_err(|_| bad("32-bit integer")),
        "int64" => text.trim().parse().map(Bson::Int64).map_err(|_| bad("64-bit integer")),
        "double" => text.trim().parse().map(Bson::Double).map_err(|_| bad("number")),
        "bool" => match text.trim().to_lowercase().as_str() {
            "true" | "t" | "1" => Ok(Bson::Boolean(true)),
            "false" | "f" | "0" => Ok(Bson::Boolean(false)),
            _ => Err(bad("boolean")),
        },
        "objectId" => ObjectId::parse_str(text.trim()).map(Bson::ObjectId).map_err(|_| bad("ObjectId (24 hex characters)")),
        _ => {
            // object / array / date / null / unknown: JSON if it parses, otherwise a plain string.
            match serde_json::from_str::<serde_json::Value>(text) {
                Ok(v) => Bson::try_from(v).map_err(|e| e.to_string()),
                Err(_) if matches!(ty, "object" | "array") => Err(bad("JSON value")),
                Err(_) => Ok(Bson::String(text.to_string())),
            }
        }
    }
}

fn docs_to_rows(docs: &[Document], first_cols: &[String]) -> (Vec<String>, Vec<Vec<Cell>>, Vec<Column>) {
    let mut cols: Vec<String> = first_cols.to_vec();
    if !cols.iter().any(|c| c == "_id") && docs.iter().any(|d| d.contains_key("_id")) {
        cols.insert(0, "_id".into());
    }
    let mut types: std::collections::HashMap<String, &'static str> = std::collections::HashMap::new();
    for d in docs {
        for (k, v) in d {
            if !cols.contains(k) {
                cols.push(k.clone());
            }
            let e = types.entry(k.clone()).or_insert("null");
            if *e == "null" {
                *e = bson_type_name(v);
            }
        }
    }
    let rows = docs.iter().map(|d| cols.iter().map(|c| d.get(c).and_then(bson_to_cell)).collect()).collect();
    let columns = cols
        .iter()
        .map(|c| Column {
            name: c.clone(),
            type_name: types.get(c).copied().unwrap_or("string").to_string(),
            nullable: c != "_id",
            is_primary_key: c == "_id",
            default: None,
            fk: None,
        })
        .collect();
    (cols, rows, columns)
}

// ---------------------------------------------------------------------------------------
// Shell-style query parsing:  db.users.find({...}, {...}).sort({...}).limit(10)
// ---------------------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
pub struct ParsedQuery {
    pub collection: String,
    pub method: String,
    pub args: Vec<String>,
    pub limit: Option<i64>,
    pub sort: Option<String>,
}

/// Split a string on top-level commas (ignoring those inside {}, [], () or strings).
fn split_top_level(s: &str) -> Vec<String> {
    let (mut depth, mut in_str, mut esc) = (0i32, false, false);
    let (mut out, mut cur) = (Vec::new(), String::new());
    for ch in s.chars() {
        if in_str {
            cur.push(ch);
            if esc { esc = false } else if ch == '\\' { esc = true } else if ch == '"' { in_str = false }
            continue;
        }
        match ch {
            '"' => { in_str = true; cur.push(ch) }
            '{' | '[' | '(' => { depth += 1; cur.push(ch) }
            '}' | ']' | ')' => { depth -= 1; cur.push(ch) }
            ',' if depth == 0 => { out.push(cur.trim().to_string()); cur.clear() }
            _ => cur.push(ch),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// Take `name(args)` from the front of `s`, returning (name, args, rest).
fn take_call(s: &str) -> Option<(String, String, &str)> {
    let open = s.find('(')?;
    let name = s[..open].trim().to_string();
    let (mut depth, mut in_str, mut esc) = (0i32, false, false);
    for (i, ch) in s[open..].char_indices() {
        if in_str {
            if esc { esc = false } else if ch == '\\' { esc = true } else if ch == '"' { in_str = false }
            continue;
        }
        match ch {
            '"' => in_str = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some((name, s[open + 1..open + i].to_string(), &s[open + i + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}

pub fn parse_query(text: &str) -> std::result::Result<ParsedQuery, String> {
    let t = text.trim().trim_end_matches(';').trim();
    let rest = t.strip_prefix("db.").ok_or("Queries look like db.collection.find({...}) or db.collection.aggregate([...])")?;
    let dot = rest.find('.').ok_or("Missing method, e.g. db.users.find({})")?;
    let collection = rest[..dot].trim().to_string();
    let mut rest = &rest[dot + 1..];
    let mut q = ParsedQuery { collection, method: String::new(), args: vec![], limit: None, sort: None };
    let mut first = true;
    while !rest.trim().is_empty() {
        let (name, args, tail) = take_call(rest.trim_start_matches('.')).ok_or("Unbalanced parentheses")?;
        if first {
            q.method = name;
            q.args = split_top_level(&args);
            first = false;
        } else {
            match name.as_str() {
                "limit" => q.limit = Some(args.trim().parse().map_err(|_| "limit() needs a number")?),
                "sort" => q.sort = Some(args.trim().to_string()),
                other => return Err(format!("Unsupported chained method .{other}() (use .limit() or .sort())")),
            }
        }
        rest = tail;
    }
    if q.method.is_empty() {
        return Err("Missing method".into());
    }
    Ok(q)
}

fn json_to_doc(s: &str) -> std::result::Result<Document, String> {
    let v: serde_json::Value = serde_json::from_str(s).map_err(|e| format!("Invalid JSON: {e}"))?;
    match Bson::try_from(v).map_err(|e| e.to_string())? {
        Bson::Document(d) => Ok(d),
        _ => Err("Expected a JSON object".into()),
    }
}

fn json_to_docs(s: &str) -> std::result::Result<Vec<Document>, String> {
    let v: serde_json::Value = serde_json::from_str(s).map_err(|e| format!("Invalid JSON: {e}"))?;
    let arr = v.as_array().ok_or("Expected a JSON array (aggregation pipeline)")?;
    arr.iter()
        .map(|x| match Bson::try_from(x.clone()).map_err(|e| e.to_string())? {
            Bson::Document(d) => Ok(d),
            _ => Err("Every pipeline stage must be an object".to_string()),
        })
        .collect()
}

// ---------------------------------------------------------------------------------------

impl Mongo {
    pub async fn connect(c: &ConnectionConfig, password: &str) -> Result<Self> {
        let uri = build_uri(c, password);
        let mut opts = ClientOptions::parse(&uri).await?;
        opts.app_name = Some("dboard".into());
        opts.server_selection_timeout = Some(Duration::from_secs(10));
        opts.connect_timeout = Some(Duration::from_secs(10));
        let default_db = opts.default_database.clone().or_else(|| Some(c.database.clone()).filter(|d| !d.is_empty()));
        let client = Client::with_options(opts)?;
        // Force a round trip so bad credentials / hosts fail here, not on first use.
        client.database("admin").run_command(mongodb::bson::doc! { "ping": 1 }).await?;
        Ok(Self { client, default_db })
    }

    pub async fn version(&mut self) -> Result<String> {
        let d = self.client.database("admin").run_command(mongodb::bson::doc! { "buildInfo": 1 }).await?;
        Ok(d.get_str("version").unwrap_or("").to_string())
    }

    pub async fn metadata(&mut self) -> Result<Metadata> {
        let dbs = match &self.default_db {
            Some(d) => vec![d.clone()],
            None => self
                .client
                .list_database_names()
                .await?
                .into_iter()
                .filter(|n| !matches!(n.as_str(), "admin" | "local" | "config"))
                .collect(),
        };
        let mut tables = Vec::new();
        for db in dbs {
            let d = self.client.database(&db);
            let mut names = d.list_collection_names().await?;
            names.sort();
            for name in names.into_iter().filter(|n| !n.starts_with("system.")) {
                let coll = d.collection::<Document>(&name);
                let mut docs = Vec::new();
                if let Ok(mut cur) = coll.find(Document::new()).limit(50).await {
                    while cur.advance().await.unwrap_or(false) {
                        if let Ok(doc) = cur.deserialize_current() {
                            docs.push(doc);
                        }
                    }
                }
                let (_, _, columns) = docs_to_rows(&docs, &[]);
                let estimated_rows = coll.estimated_document_count().await.ok().map(|n| n as i64);
                tables.push(Table {
                    schema: db.clone(),
                    name,
                    kind: TableKind::Collection,
                    columns,
                    estimated_rows,
                    size_bytes: None,
                    indexes: Vec::new(),
                    keyless_edit: false,
                });
            }
        }
        Ok(Metadata { tables, objects: Vec::new() })
    }

    fn coll(&self, t: &Table) -> mongodb::Collection<Document> {
        self.client.database(&t.schema).collection::<Document>(&t.name)
    }

    async fn collect(cur: &mut mongodb::Cursor<Document>) -> Result<Vec<Document>> {
        let mut docs = Vec::new();
        while cur.advance().await? {
            docs.push(cur.deserialize_current()?);
        }
        Ok(docs)
    }

    /// Returns rows plus the refreshed column list for the collection.
    pub async fn fetch(&mut self, t: &Table, p: &Page) -> Result<(Rows, Vec<Column>)> {
        let started = Instant::now();
        let filter = match p.filter.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
            Some(f) => json_to_doc(f).map_err(Error::Db)?,
            None => Document::new(),
        };
        let coll = self.coll(t);
        let mut find = coll.find(filter).skip(p.offset.max(0) as u64).limit(p.limit.max(1));
        if let Some(c) = &p.sort_column {
            find = find.sort(mongodb::bson::doc! { c.as_str(): if p.sort_ascending { 1 } else { -1 } });
        }
        let mut cur = find.await?;
        let docs = Self::collect(&mut cur).await?;
        let known: Vec<String> = t.columns.iter().map(|c| c.name.clone()).collect();
        let (cols, rows, columns) = docs_to_rows(&docs, &known);
        // Keep previously known type info for columns that are null in this page.
        let columns = columns
            .into_iter()
            .map(|mut c| {
                if let Some(old) = t.column(&c.name) {
                    if c.type_name == "null" {
                        c.type_name = old.type_name.clone();
                    }
                }
                c
            })
            .collect();
        Ok((
            Rows { columns: cols, rows, duration_ms: started.elapsed().as_secs_f64() * 1000.0, total_estimate: t.estimated_rows },
            columns,
        ))
    }

    fn resolve_db(&self, t_schema: &str) -> String {
        if t_schema.is_empty() { self.default_db.clone().unwrap_or_else(|| "test".into()) } else { t_schema.to_string() }
    }

    pub async fn query(&mut self, text: &str) -> Result<Rows> {
        let started = Instant::now();
        let q = parse_query(text).map_err(Error::Db)?;
        let db = self.client.database(&self.resolve_db(""));
        let coll = db.collection::<Document>(&q.collection);
        let docs = match q.method.as_str() {
            "find" => {
                let filter = q.args.first().map(|a| json_to_doc(a)).transpose().map_err(Error::Db)?.unwrap_or_default();
                let mut f = coll.find(filter).limit(q.limit.unwrap_or(500));
                if let Some(Ok(proj)) = q.args.get(1).map(|a| json_to_doc(a)) {
                    f = f.projection(proj);
                }
                if let Some(s) = &q.sort {
                    f = f.sort(json_to_doc(s).map_err(Error::Db)?);
                }
                let mut cur = f.await?;
                Self::collect(&mut cur).await?
            }
            "aggregate" => {
                let pipeline = json_to_docs(q.args.first().map(String::as_str).unwrap_or("[]")).map_err(Error::Db)?;
                let mut cur = coll.aggregate(pipeline).await?;
                let mut docs = Vec::new();
                while cur.advance().await? {
                    docs.push(cur.deserialize_current()?);
                }
                docs
            }
            "countDocuments" | "count" => {
                let filter = q.args.first().map(|a| json_to_doc(a)).transpose().map_err(Error::Db)?.unwrap_or_default();
                let n = coll.count_documents(filter).await?;
                return Ok(Rows {
                    columns: vec!["count".into()],
                    rows: vec![vec![Some(n.to_string())]],
                    duration_ms: started.elapsed().as_secs_f64() * 1000.0,
                    total_estimate: None,
                });
            }
            other => return Err(Error::Db(format!("Unsupported method {other}(). Use find, aggregate or countDocuments."))),
        };
        let (cols, rows, _) = docs_to_rows(&docs, &[]);
        if cols.is_empty() {
            return Ok(Rows { columns: vec!["result".into()], rows: vec![vec![Some("no documents".into())]], duration_ms: started.elapsed().as_secs_f64() * 1000.0, total_estimate: None });
        }
        Ok(Rows { columns: cols, rows, duration_ms: started.elapsed().as_secs_f64() * 1000.0, total_estimate: None })
    }

    fn key_filter(t: &Table, key: &[Cell]) -> Result<Document> {
        let ty = t.column("_id").map(|c| c.type_name.as_str()).unwrap_or("objectId");
        let raw = key.first().cloned().flatten().ok_or_else(|| Error::Unsafe("This document has no _id.".into()))?;
        let id = parse_typed(&raw, ty).map_err(Error::Db)?;
        Ok(mongodb::bson::doc! { "_id": id })
    }

    pub async fn update(&mut self, t: &Table, column: &str, key: &[Cell], new: Option<&str>) -> Result<()> {
        let value = match new {
            None => Bson::Null,
            Some(text) => {
                let ty = t.column(column).map(|c| c.type_name.as_str()).unwrap_or("string");
                parse_typed(text, ty).map_err(Error::Db)?
            }
        };
        let r = self
            .coll(t)
            .update_one(Self::key_filter(t, key)?, mongodb::bson::doc! { "$set": { column: value } })
            .await?;
        if r.matched_count == 0 {
            return Err(Error::Db("No document matched; it may have been changed or deleted.".into()));
        }
        Ok(())
    }

    /// Replace a whole document (raw JSON editor).
    pub async fn replace_document(&mut self, t: &Table, key: &[Cell], json: &str) -> Result<()> {
        let mut doc = json_to_doc(json).map_err(Error::Db)?;
        doc.remove("_id");
        let r = self.coll(t).replace_one(Self::key_filter(t, key)?, doc).await?;
        if r.matched_count == 0 {
            return Err(Error::Db("No document matched; it may have been changed or deleted.".into()));
        }
        Ok(())
    }

    pub async fn get_document_json(&mut self, t: &Table, key: &[Cell]) -> Result<String> {
        let d = self.coll(t).find_one(Self::key_filter(t, key)?).await?.ok_or_else(|| Error::Db("Document not found".into()))?;
        serde_json::to_string_pretty(&Bson::Document(d).into_relaxed_extjson()).map_err(|e| Error::Db(e.to_string()))
    }

    pub async fn insert(&mut self, t: &Table, vals: &[(String, String)]) -> Result<()> {
        let mut doc = Document::new();
        for (k, v) in vals {
            let ty = t.column(k).map(|c| c.type_name.as_str()).unwrap_or("string");
            doc.insert(k.clone(), parse_typed(v, ty).map_err(Error::Db)?);
        }
        self.coll(t).insert_one(doc).await?;
        Ok(())
    }

    pub async fn insert_json(&mut self, t: &Table, json: &str) -> Result<()> {
        self.coll(t).insert_one(json_to_doc(json).map_err(Error::Db)?).await?;
        Ok(())
    }

    pub async fn delete(&mut self, t: &Table, key: &[Cell]) -> Result<()> {
        let r = self.coll(t).delete_one(Self::key_filter(t, key)?).await?;
        if r.deleted_count == 0 {
            return Err(Error::Db("No document matched; it may have been changed or deleted.".into()));
        }
        Ok(())
    }

    pub async fn truncate(&mut self, t: &Table) -> Result<()> {
        self.coll(t).delete_many(Document::new()).await?;
        Ok(())
    }

    pub async fn drop_table(&mut self, t: &Table) -> Result<()> {
        self.coll(t).drop().await?;
        Ok(())
    }

    pub async fn ddl(&mut self, t: &Table) -> Result<String> {
        let mut out = format!("db.createCollection(\"{}\")\n\n// Indexes\n", t.name);
        if let Ok(mut cur) = self.coll(t).list_indexes().await {
            while cur.advance().await.unwrap_or(false) {
                if let Ok(ix) = cur.deserialize_current() {
                    let keys = Bson::Document(ix.keys.clone()).into_relaxed_extjson();
                    let name = ix.options.as_ref().and_then(|o| o.name.clone()).unwrap_or_default();
                    out.push_str(&format!("db.{}.createIndex({keys}, {{ name: \"{name}\" }})\n", t.name));
                }
            }
        }
        Ok(out)
    }

    pub async fn explain(&mut self, text: &str, _analyze: bool) -> Result<Rows> {
        let started = Instant::now();
        let q = parse_query(text).map_err(Error::Db)?;
        let inner = match q.method.as_str() {
            "find" => {
                let filter = q.args.first().map(|a| json_to_doc(a)).transpose().map_err(Error::Db)?.unwrap_or_default();
                mongodb::bson::doc! { "find": &q.collection, "filter": filter }
            }
            "aggregate" => {
                let p = json_to_docs(q.args.first().map(String::as_str).unwrap_or("[]")).map_err(Error::Db)?;
                mongodb::bson::doc! { "aggregate": &q.collection, "pipeline": p, "cursor": {} }
            }
            other => return Err(Error::Db(format!("Cannot explain {other}()"))),
        };
        let db = self.client.database(&self.resolve_db(""));
        let plan = db.run_command(mongodb::bson::doc! { "explain": inner, "verbosity": "executionStats" }).await?;
        let text = serde_json::to_string_pretty(&Bson::Document(plan).into_relaxed_extjson()).unwrap_or_default();
        Ok(Rows {
            columns: vec!["Plan".into()],
            rows: text.lines().map(|l| vec![Some(l.to_string())]).collect(),
            duration_ms: started.elapsed().as_secs_f64() * 1000.0,
            total_estimate: None,
        })
    }
}

/// First line of a collection in a dboard export: everything but the documents, then `"documents":[`.
fn collection_header(db: &str, name: &str, indexes: Vec<serde_json::Value>) -> String {
    let head = serde_json::json!({ "db": db, "name": name, "indexes": indexes }).to_string();
    format!("{},\"documents\":[", &head[..head.len() - 1])
}

/// Inverse of [`collection_header`]; `None` when the line is not a header.
fn parse_collection_header(line: &str) -> Option<std::result::Result<(String, String, Vec<Document>), String>> {
    const TAIL: &str = ",\"documents\":[";
    if !(line.starts_with("{\"db\":") && line.ends_with(TAIL)) {
        return None;
    }
    let json = format!("{}}}", &line[..line.len() - TAIL.len()]);
    Some((|| {
        let v: serde_json::Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
        let indexes = v["indexes"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|i| match Bson::try_from(i) {
                Ok(Bson::Document(d)) => Some(d),
                _ => None,
            })
            .collect();
        Ok((v["db"].as_str().unwrap_or("test").to_string(), v["name"].as_str().unwrap_or("").to_string(), indexes))
    })())
}

/// Database a user lives in / commands run against.
const ADMIN_DB: &str = "admin";

impl Mongo {
    pub async fn list_databases(&mut self) -> Result<Vec<String>> {
        let mut names = self.client.list_database_names().await?;
        names.retain(|n| !matches!(n.as_str(), "admin" | "local" | "config"));
        names.sort();
        Ok(names)
    }

    pub fn current_database(&self) -> Option<String> {
        self.default_db.clone()
    }

    pub fn use_database(&mut self, db: Option<&str>) {
        self.default_db = db.map(str::to_string);
    }

    fn user_db(&self) -> String {
        self.default_db.clone().unwrap_or_else(|| ADMIN_DB.into())
    }

    fn roles_of(d: &Document) -> Vec<(String, String)> {
        d.get_array("roles")
            .map(|a| a.iter().filter_map(|r| r.as_document()).map(|r| (r.get_str("role").unwrap_or("").to_string(), r.get_str("db").unwrap_or("").to_string())).collect())
            .unwrap_or_default()
    }

    pub async fn list_users(&mut self) -> Result<Vec<UserInfo>> {
        let all = self.client.database(ADMIN_DB).run_command(doc! { "usersInfo": { "forAllDBs": true } }).await;
        let reply = match all {
            Ok(r) => r,
            Err(_) => self.client.database(&self.user_db()).run_command(doc! { "usersInfo": 1 }).await?,
        };
        let mut out = Vec::new();
        for u in reply.get_array("users").map(|a| a.to_vec()).unwrap_or_default() {
            let Some(u) = u.as_document() else { continue };
            let roles: Vec<String> = Self::roles_of(u).into_iter().map(|(r, d)| format!("{r}@{d}")).collect();
            out.push(UserInfo { name: u.get_str("user").unwrap_or("").into(), origin: u.get_str("db").unwrap_or("").into(), summary: roles.join(", ") });
        }
        out.sort_by(|a, b| (&a.origin, &a.name).cmp(&(&b.origin, &b.name)));
        Ok(out)
    }

    pub async fn user_grants(&mut self, u: &UserInfo) -> Result<Vec<String>> {
        let r = self
            .client
            .database(&u.origin)
            .run_command(doc! { "usersInfo": { "user": &u.name, "db": &u.origin }, "showPrivileges": true })
            .await?;
        let Some(user) = r.get_array("users").ok().and_then(|a| a.first()).and_then(|x| x.as_document()).cloned() else {
            return Ok(vec!["User not found.".into()]);
        };
        let mut out: Vec<String> = Self::roles_of(&user).into_iter().map(|(r, d)| format!("Role {r} on {d}")).collect();
        if let Ok(p) = user.get_array("inheritedPrivileges") {
            for priv_ in p.iter().filter_map(|x| x.as_document()).take(60) {
                let res = priv_.get_document("resource").map(|r| Bson::Document(r.clone()).into_relaxed_extjson().to_string()).unwrap_or_default();
                let actions = priv_.get_array("actions").map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default();
                out.push(format!("{res}: {actions}"));
            }
        }
        Ok(out)
    }

    pub async fn create_user(&mut self, n: &NewUser) -> Result<()> {
        let (db, roles) = if n.admin {
            (ADMIN_DB.to_string(), vec![doc! { "role": "root", "db": ADMIN_DB }])
        } else {
            let db = self.user_db();
            let roles: Vec<Document> = admin::mongo_role(n.access).map(|r| doc! { "role": r, "db": &db }).into_iter().collect();
            (db, roles)
        };
        self.client.database(&db).run_command(doc! { "createUser": &n.name, "pwd": &n.password, "roles": roles }).await?;
        Ok(())
    }

    pub async fn set_password(&mut self, u: &UserInfo, password: &str) -> Result<()> {
        self.client.database(&u.origin).run_command(doc! { "updateUser": &u.name, "pwd": password }).await?;
        Ok(())
    }

    pub async fn drop_user(&mut self, u: &UserInfo) -> Result<()> {
        self.client.database(&u.origin).run_command(doc! { "dropUser": &u.name }).await?;
        Ok(())
    }

    /// Replace the user's roles on the current database, keeping roles on other databases.
    pub async fn set_access(&mut self, u: &UserInfo, level: AccessLevel) -> Result<()> {
        let target = self.default_db.clone().ok_or_else(|| Error::Db("Select a database first; access levels apply to one database.".into()))?;
        let info = self.client.database(&u.origin).run_command(doc! { "usersInfo": { "user": &u.name, "db": &u.origin } }).await?;
        let current = info.get_array("users").ok().and_then(|a| a.first()).and_then(|x| x.as_document()).map(Self::roles_of).unwrap_or_default();
        let mut roles: Vec<Document> = current.into_iter().filter(|(_, d)| *d != target).map(|(r, d)| doc! { "role": r, "db": d }).collect();
        if let Some(r) = admin::mongo_role(level) {
            roles.push(doc! { "role": r, "db": &target });
        }
        self.client.database(&u.origin).run_command(doc! { "updateUser": &u.name, "roles": roles }).await?;
        Ok(())
    }

    // ---- export / import --------------------------------------------------------------------

    async fn index_specs(&self, db: &str, coll: &str) -> Vec<Document> {
        let Ok(r) = self.client.database(db).run_command(doc! { "listIndexes": coll }).await else { return Vec::new() };
        r.get_document("cursor")
            .and_then(|c| c.get_array("firstBatch"))
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_document().cloned())
                    .filter(|d| d.get_str("name").unwrap_or("") != "_id_")
                    .map(|mut d| {
                        d.remove("v");
                        d.remove("ns");
                        d
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Line-oriented JSON: one document per line, so import can stream the file.
    pub async fn dump(&mut self, opts: &DumpOptions, tables: &[Table], out: &mut dyn Write, progress: &mut dyn FnMut(String)) -> Result<DumpStats> {
        let mut st = DumpStats::default();
        writeln!(out, "{{\"format\":\"dboard-mongo-dump\",\"version\":1,\"collections\":[")?;
        let colls: Vec<&Table> = tables.iter().filter(|t| t.kind == TableKind::Collection).collect();
        for (n, t) in colls.iter().enumerate() {
            let indexes: Vec<serde_json::Value> = if opts.schema {
                self.index_specs(&t.schema, &t.name).await.into_iter().map(|d| Bson::Document(d).into_canonical_extjson()).collect()
            } else {
                Vec::new()
            };
            writeln!(out, "{}", collection_header(&t.schema, &t.name, indexes))?;
            let mut count = 0u64;
            if opts.data {
                let mut cur = self.coll(t).find(Document::new()).await?;
                let mut first = true;
                while cur.advance().await? {
                    let d = cur.deserialize_current()?;
                    if !first {
                        writeln!(out, ",")?;
                    }
                    first = false;
                    write!(out, "{}", Bson::Document(d).into_canonical_extjson())?;
                    count += 1;
                }
                if !first {
                    writeln!(out)?;
                }
            }
            writeln!(out, "]}}{}", if n + 1 < colls.len() { "," } else { "" })?;
            st.tables += 1;
            st.rows += count;
            progress(format!("Exported {} ({count} documents)", t.full_name()));
        }
        writeln!(out, "]}}")?;
        out.flush()?;
        Ok(st)
    }

    async fn flush_docs(&self, db: &str, coll: &str, batch: &mut Vec<Document>, stats: &mut ImportStats) -> Result<()> {
        if batch.is_empty() {
            return Ok(());
        }
        let n = batch.len() as u64;
        self.client.database(db).collection::<Document>(coll).insert_many(std::mem::take(batch)).await?;
        stats.rows_copied += n;
        Ok(())
    }

    /// Restore a dump written by [`Mongo::dump`]. With `target_db`, every collection goes there.
    pub async fn import(&mut self, reader: &mut dyn BufRead, target_db: Option<&str>, opts: &ImportOptions, progress: &mut dyn FnMut(String)) -> Result<ImportStats> {
        let mut stats = ImportStats::default();
        let mut cur: Option<(String, String)> = None; // (db, collection)
        let mut batch: Vec<Document> = Vec::new();
        let mut cur_indexes: Vec<Document> = Vec::new();
        let mut seen_header = false;
        let mut line_no = 0usize;
        let mut raw = Vec::new();
        loop {
            raw.clear();
            if reader.read_until(b'\n', &mut raw)? == 0 {
                break;
            }
            line_no += 1;
            let text = String::from_utf8_lossy(&raw);
            let line = text.trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with("{\"format\"") {
                seen_header = true;
                continue;
            }
            if !seen_header {
                return Err(Error::Db("This file is not a dboard MongoDB export.".into()));
            }
            if let Some(h) = parse_collection_header(line) {
                let (file_db, name, indexes) = h.map_err(|e| Error::Db(format!("Line {line_no}: {e}")))?;
                let db = target_db.map(str::to_string).unwrap_or(file_db);
                cur_indexes = indexes;
                progress(format!("Importing {db}.{name}…"));
                cur = Some((db, name));
                stats.statements += 1;
                continue;
            }
            if line == "]}" || line == "]}," {
                if let Some((db, name)) = cur.take() {
                    self.flush_docs(&db, &name, &mut batch, &mut stats).await?;
                    if !cur_indexes.is_empty() {
                        let r = self.client.database(&db).run_command(doc! { "createIndexes": &name, "indexes": std::mem::take(&mut cur_indexes) }).await;
                        if let Err(e) = r {
                            let msg = format!("Indexes of {db}.{name} could not be created: {e}");
                            if opts.stop_on_error {
                                return Err(Error::Db(msg));
                            }
                            stats.errors.push(msg);
                        }
                    }
                }
                continue;
            }
            let Some((db, name)) = &cur else { continue };
            let json = line.trim_end_matches(',');
            let parsed = serde_json::from_str::<serde_json::Value>(json).map_err(|e| e.to_string()).and_then(|v| match Bson::try_from(v) {
                Ok(Bson::Document(d)) => Ok(d),
                Ok(_) => Err("not a document".to_string()),
                Err(e) => Err(e.to_string()),
            });
            match parsed {
                Ok(d) => batch.push(d),
                Err(e) => {
                    let msg = format!("Line {line_no}: invalid document: {e}");
                    if opts.stop_on_error {
                        return Err(Error::Db(msg));
                    }
                    stats.errors.push(msg);
                }
            }
            if batch.len() >= 500 {
                let (db, name) = (db.clone(), name.clone());
                if let Err(e) = self.flush_docs(&db, &name, &mut batch, &mut stats).await {
                    let msg = format!("Inserting into {db}.{name} failed: {e}");
                    if opts.stop_on_error {
                        return Err(Error::Db(format!("{msg}\nDocuments inserted before this point were kept.")));
                    }
                    batch.clear();
                    stats.errors.push(msg);
                }
            }
        }
        if !seen_header {
            return Err(Error::Db("This file is not a dboard MongoDB export.".into()));
        }
        Ok(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_headers_round_trip() {
        let idx = Bson::Document(doc! { "key": { "a": 1 }, "name": "a_1", "unique": true }).into_canonical_extjson();
        let line = collection_header("shop", "us\"ers", vec![idx]);
        assert!(line.starts_with("{\"db\":\"shop\"") && line.ends_with("\"documents\":["));
        let (db, name, indexes) = parse_collection_header(&line).unwrap().unwrap();
        assert_eq!((db.as_str(), name.as_str()), ("shop", "us\"ers"));
        assert_eq!(indexes[0].get_str("name").unwrap(), "a_1");
        assert!(indexes[0].get_bool("unique").unwrap());
        assert!(parse_collection_header("{\"_id\": 1, \"x\": \"documents\":[").is_none());
        assert!(parse_collection_header("{\"db\":\"x\"}").is_none());
    }

    #[test]
    fn splits_and_joins_uri_password() {
        let (clean, pw) = split_uri_password("mongodb://alice:p%40ss@host:27017/db?x=1");
        assert_eq!(clean, "mongodb://alice@host:27017/db?x=1");
        assert_eq!(pw.as_deref(), Some("p@ss"));
        assert_eq!(join_uri_password(&clean, "p@ss"), "mongodb://alice:p%40ss@host:27017/db?x=1");
        assert_eq!(split_uri_password("mongodb://host/db"), ("mongodb://host/db".into(), None));
        assert_eq!(split_uri_password("mongodb+srv://u@h/db").1, None);
    }

    #[test]
    fn builds_uri_from_fields_without_leaking_defaults() {
        let c = ConnectionConfig { host: "h".into(), port: 27017, username: "u".into(), database: "d".into(), db_type: DbType::Mongo, ..Default::default() };
        assert_eq!(build_uri(&c, "p w"), "mongodb://u:p%20w@h:27017/d?authSource=admin");
    }

    #[test]
    fn parses_shell_queries() {
        let q = parse_query(r#"db.users.find({"a": {"$gt": 1}}, {"name": 1}).sort({"a": -1}).limit(5);"#).unwrap();
        assert_eq!(q.collection, "users");
        assert_eq!(q.method, "find");
        assert_eq!(q.args, vec![r#"{"a": {"$gt": 1}}"#, r#"{"name": 1}"#]);
        assert_eq!(q.limit, Some(5));
        assert_eq!(q.sort.as_deref(), Some(r#"{"a": -1}"#));
        let a = parse_query(r#"db.orders.aggregate([{"$match": {"s": "a,b)"}}])"#).unwrap();
        assert_eq!(a.method, "aggregate");
        assert_eq!(a.args.len(), 1);
        assert!(parse_query("select 1").is_err());
        assert!(parse_query("db.users.find({}).skip(1)").is_err());
    }

    #[test]
    fn typed_parsing_and_cells() {
        assert_eq!(parse_typed("5", "int32").unwrap(), Bson::Int32(5));
        assert!(parse_typed("x", "int32").is_err());
        assert_eq!(parse_typed("true", "bool").unwrap(), Bson::Boolean(true));
        assert!(parse_typed("nothex", "objectId").is_err());
        assert_eq!(parse_typed("hello", "null").unwrap(), Bson::String("hello".into()));
        assert!(parse_typed("{bad", "object").is_err());
        assert_eq!(bson_to_cell(&Bson::Null), None);
        assert_eq!(bson_to_cell(&Bson::Int32(3)).as_deref(), Some("3"));
    }

    #[test]
    fn unions_document_columns() {
        let docs = vec![mongodb::bson::doc! {"_id": 1, "a": "x"}, mongodb::bson::doc! {"_id": 2, "b": true}];
        let (cols, rows, meta) = docs_to_rows(&docs, &[]);
        assert_eq!(cols, vec!["_id", "a", "b"]);
        assert_eq!(rows[1][1], None);
        assert!(meta[0].is_primary_key);
        assert_eq!(meta[2].type_name, "bool");
    }
}
