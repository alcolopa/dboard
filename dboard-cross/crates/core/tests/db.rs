//! Integration tests against real servers. Each runs only when its env var is set:
//!   DBOARD_TEST_PG=host:port:user:password:database
//!   DBOARD_TEST_MYSQL=host:port:user:password:database

use dboard_core::model::*;
use dboard_core::Conn;

/// The Postgres tests share one database and run DDL, so they take turns (other engines are separate servers).
static PG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn pg_lock() -> std::sync::MutexGuard<'static, ()> {
    PG_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn cfg(var: &str, ty: DbType) -> Option<(ConnectionConfig, String)> {
    let v = std::env::var(var).ok()?;
    let p: Vec<&str> = v.split(':').collect();
    Some((
        ConnectionConfig {
            id: "t".into(),
            db_type: ty,
            host: p[0].into(),
            port: p[1].parse().ok()?,
            username: p[2].into(),
            database: p[4].into(),
            ssl: SslMode::Prefer,
            ..Default::default()
        },
        p[3].into(),
    ))
}

async fn scenario(c: ConnectionConfig, pw: String) {
    let ty = c.db_type;
    let mut d = Conn::connect(c, &pw).await.unwrap();
    assert!(!d.server_version.is_empty());

    for t in ["dboard_t", "dboard_nopk"] {
        let _ = d.execute_query(&format!("DROP TABLE IF EXISTS {t}")).await;
    }
    let (num, flag, ts) = match ty {
        DbType::Postgres => ("numeric", "boolean", "timestamptz"),
        _ => ("decimal(10,2)", "tinyint(1)", "datetime"),
    };
    d.execute_query(&format!("CREATE TABLE dboard_t (id int primary key, name varchar(20), n {num}, flag {flag}, ts {ts})")).await.unwrap();
    d.execute_query("INSERT INTO dboard_t VALUES (1,'a',1.5,true,'2024-01-02 03:04:05'),(2,NULL,2,false,'2024-01-03 00:00:00')").await.unwrap();
    d.execute_query("CREATE TABLE dboard_nopk (x int)").await.unwrap();
    d.refresh_metadata().await.unwrap();

    let schema = d.metadata.tables.iter().find(|t| t.name == "dboard_t").expect("table listed").schema.clone();
    let t = d.table(&schema, "dboard_t").unwrap().clone();
    assert_eq!(t.columns.len(), 5);
    assert!(t.columns[0].is_primary_key && t.is_editable());
    // Keyless tables are only editable on PostgreSQL (row addressed by ctid).
    assert_eq!(d.table(&schema, "dboard_nopk").unwrap().is_editable(), ty == DbType::Postgres);

    // Browse: sorted desc, NULL preserved.
    let page = Page { limit: 10, offset: 0, sort_column: Some("id".into()), sort_ascending: false, filter: None };
    let r = d.fetch_page(&schema, "dboard_t", &page).await.unwrap();
    assert_eq!(r.rows.len(), 2);
    assert_eq!(r.rows[0][0].as_deref(), Some("2"));
    assert_eq!(r.rows[0][1], None);
    let filtered = d.fetch_page(&schema, "dboard_t", &Page { filter: Some("id = 1".into()), ..page.clone() }).await.unwrap();
    assert_eq!(filtered.rows.len(), 1);

    // Numbers sort as numbers (PostgreSQL casts to text, which must not leak into ORDER BY).
    d.execute_query("INSERT INTO dboard_t (id, name) VALUES (10, 'ten'), (9, 'nine')").await.unwrap();
    let asc = d.fetch_page(&schema, "dboard_t", &Page { sort_ascending: true, ..page.clone() }).await.unwrap();
    assert_eq!(asc.rows.iter().map(|r| r[0].clone().unwrap()).collect::<Vec<_>>(), ["1", "2", "9", "10"]);
    let default_order = d.fetch_page(&schema, "dboard_t", &Page { sort_column: None, ..page.clone() }).await.unwrap();
    assert_eq!(default_order.rows.last().unwrap()[0].as_deref(), Some("10"));
    d.execute_query("DELETE FROM dboard_t WHERE id IN (9, 10)").await.unwrap();

    // Edit + undo.
    let r = d.fetch_page(&schema, "dboard_t", &page).await.unwrap();
    let row = r.rows[1].clone(); // id = 1
    d.edit_cell(&schema, "dboard_t", &row, "name", Some("bob".into())).await.unwrap();
    assert_eq!(d.execute_query("SELECT name FROM dboard_t WHERE id = 1").await.unwrap().rows[0][0].as_deref(), Some("bob"));
    assert_eq!(d.history.len(), 1);
    d.undo().await.unwrap().unwrap();
    assert_eq!(d.execute_query("SELECT name FROM dboard_t WHERE id = 1").await.unwrap().rows[0][0].as_deref(), Some("a"));

    // NULL, numeric cast, bad value.
    d.edit_cell(&schema, "dboard_t", &row, "name", None).await.unwrap();
    d.edit_cell(&schema, "dboard_t", &row, "n", Some("9.25".into())).await.unwrap();
    let err = d.edit_cell(&schema, "dboard_t", &row, "n", Some("abc".into())).await.unwrap_err();
    assert!(err.to_string().contains("Invalid value"), "{err}");

    // No PK: MySQL refuses; PostgreSQL addresses the row by ctid, so the edit works and undoes.
    d.execute_query("INSERT INTO dboard_nopk VALUES (1), (1), (2)").await.unwrap();
    d.refresh_metadata().await.unwrap();
    if ty == DbType::MySql {
        let e = d.edit_cell(&schema, "dboard_nopk", &[Some("1".into())], "x", Some("2".into())).await.unwrap_err();
        assert!(e.to_string().contains("no primary key"), "{e}");
    } else {
        assert!(d.table(&schema, "dboard_nopk").unwrap().is_editable());
        d.edit_cell(&schema, "dboard_nopk", &[Some("1".into())], "x", Some("7".into())).await.unwrap();
        let r = d.execute_query("SELECT x FROM dboard_nopk ORDER BY x").await.unwrap();
        let xs: Vec<_> = r.rows.iter().map(|r| r[0].clone().unwrap()).collect();
        assert_eq!(xs, ["1", "2", "7"], "only one of the two identical rows changes");
        d.undo().await.unwrap().unwrap();
        let r = d.execute_query("SELECT x FROM dboard_nopk ORDER BY x").await.unwrap();
        assert_eq!(r.rows.iter().map(|r| r[0].clone().unwrap()).collect::<Vec<_>>(), ["1", "1", "2"]);
        d.delete_row(&schema, "dboard_nopk", &[Some("2".into())]).await.unwrap();
        d.undo().await.unwrap().unwrap(); // restores the deleted row
        assert_eq!(d.execute_query("SELECT count(*) FROM dboard_nopk").await.unwrap().rows[0][0].as_deref(), Some("3"));
    }
    d.history.clear();
    d.edit_cell(&schema, "dboard_t", &row, "name", Some("x'; DROP TABLE t;--".into())).await.unwrap();
    assert_eq!(d.execute_query("SELECT count(*) FROM dboard_t").await.unwrap().rows[0][0].as_deref(), Some("2"));

    // Insert / delete.
    d.insert_row(&schema, "dboard_t", &[("id".into(), "3".into()), ("name".into(), "carol".into())]).await.unwrap();
    assert_eq!(d.execute_query("SELECT count(*) FROM dboard_t").await.unwrap().rows[0][0].as_deref(), Some("3"));
    let dup = d.insert_row(&schema, "dboard_t", &[("id".into(), "3".into())]).await.unwrap_err();
    assert!(dup.to_string().contains("unique"), "{dup}");
    let carol = vec![Some("3".into()), Some("carol".into()), None, None, None];
    d.delete_row(&schema, "dboard_t", &carol).await.unwrap();
    assert_eq!(d.execute_query("SELECT count(*) FROM dboard_t").await.unwrap().rows[0][0].as_deref(), Some("2"));
    // Undo restores the deleted row, including its NULLs.
    d.undo().await.unwrap().unwrap();
    assert_eq!(d.execute_query("SELECT name FROM dboard_t WHERE id = 3").await.unwrap().rows[0][0].as_deref(), Some("carol"));
    d.delete_row(&schema, "dboard_t", &carol).await.unwrap();
    d.history.clear();
    assert_eq!(d.execute_query("SELECT count(*) FROM dboard_t").await.unwrap().rows[0][0].as_deref(), Some("2"));

    // DDL, EXPLAIN, routine-free query results.
    let ddl = d.ddl(&schema, "dboard_t").await.unwrap();
    assert!(ddl.to_uppercase().contains("CREATE TABLE") && ddl.contains("dboard_t"), "{ddl}");
    let plan = d.explain("SELECT * FROM dboard_t", false).await.unwrap();
    assert!(!plan.rows.is_empty());
    let multi = d.execute_query("SELECT 1 AS a, 'x' AS b").await.unwrap();
    assert_eq!(multi.columns, vec!["a", "b"]);
    assert!(d.execute_query("SELEC nonsense").await.is_err());

    // Truncate + drop.
    d.truncate(&schema, "dboard_t").await.unwrap();
    assert_eq!(d.execute_query("SELECT count(*) FROM dboard_t").await.unwrap().rows[0][0].as_deref(), Some("0"));
    d.drop_table(&schema, "dboard_t").await.unwrap();
    d.drop_table(&schema, "dboard_nopk").await.unwrap();
    assert!(d.table(&schema, "dboard_t").is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn postgres() {
    let _g = pg_lock();
    if let Some((c, pw)) = cfg("DBOARD_TEST_PG", DbType::Postgres) {
        scenario(c, pw).await;
    }
}

async fn transactions(c: ConnectionConfig, pw: String) {
    let mut d = Conn::connect(c, &pw).await.unwrap();
    let _ = d.execute_query("DROP TABLE IF EXISTS dboard_tx").await;
    d.execute_query("CREATE TABLE dboard_tx (id int primary key)").await.unwrap();
    let count = |r: dboard_core::model::Rows| r.rows[0][0].clone().unwrap();

    d.begin().await.unwrap();
    assert!(d.in_transaction());
    d.execute_query("INSERT INTO dboard_tx VALUES (1)").await.unwrap();
    d.rollback().await.unwrap();
    assert!(!d.in_transaction());
    assert_eq!(count(d.execute_query("SELECT count(*) FROM dboard_tx").await.unwrap()), "0");

    d.begin().await.unwrap();
    d.execute_query("INSERT INTO dboard_tx VALUES (2)").await.unwrap();
    d.commit().await.unwrap();
    assert_eq!(count(d.execute_query("SELECT count(*) FROM dboard_tx").await.unwrap()), "1");
    d.begin().await.unwrap();
    assert!(d.switch_database(Some("x"), &pw).await.is_err());
    d.rollback().await.unwrap();
    d.execute_query("DROP TABLE dboard_tx").await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn postgres_cancel_and_timeout() {
    let _g = pg_lock();
    let Some((c, pw)) = cfg("DBOARD_TEST_PG", DbType::Postgres) else { return };
    let mut d = Conn::connect(c, &pw).await.unwrap();
    let canceller = d.canceller();
    let started = std::time::Instant::now();
    let (res, cancelled) = tokio::join!(d.execute_query("SELECT pg_sleep(20)"), async {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        canceller.cancel().await
    });
    assert!(cancelled);
    assert!(res.is_err(), "the sleeping query must be interrupted");
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    // the connection stays usable
    assert!(d.execute_query("SELECT 1").await.is_ok());

    d.set_statement_timeout(300).await.unwrap();
    let started = std::time::Instant::now();
    assert!(d.execute_query("SELECT pg_sleep(20)").await.is_err());
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    d.set_statement_timeout(0).await.unwrap();
    assert!(d.execute_query("SELECT 1").await.is_ok());
}

#[tokio::test(flavor = "current_thread")]
async fn postgres_transactions() {
    let _g = pg_lock();
    if let Some((c, pw)) = cfg("DBOARD_TEST_PG", DbType::Postgres) {
        transactions(c, pw).await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn mysql() {
    if let Some((c, pw)) = cfg("DBOARD_TEST_MYSQL", DbType::MySql) {
        scenario(c, pw).await;
    }
}

/// MongoDB scenario: DBOARD_TEST_MONGO=host:port:user:password:database
#[tokio::test(flavor = "current_thread")]
async fn mongo() {
    let Some((c, pw)) = cfg("DBOARD_TEST_MONGO", DbType::Mongo) else { return };
    let db = c.database.clone();
    let mut d = Conn::connect(c, &pw).await.unwrap();
    let _ = d.execute_query(&format!("db.dboard_c.find({{}})")).await; // collection may not exist yet

    // Create the collection by inserting through the driver requires it to exist in metadata,
    // so seed with the raw client first.
    let client = mongodb::Client::with_uri_str(format!("mongodb://{}:27017", "127.0.0.1")).await.unwrap();
    let coll = client.database(&db).collection::<mongodb::bson::Document>("dboard_c");
    let _ = coll.drop().await;
    coll.insert_many(vec![
        mongodb::bson::doc! {"_id": 1, "name": "a", "n": 1.5, "flag": true, "tags": ["x", "y"]},
        mongodb::bson::doc! {"_id": 2, "name": "b", "n": 2.5, "flag": false},
    ])
    .await
    .unwrap();
    d.refresh_metadata().await.unwrap();

    let t = d.table(&db, "dboard_c").expect("collection listed").clone();
    assert_eq!(t.kind, TableKind::Collection);
    assert!(t.is_editable());

    let page = Page { limit: 10, offset: 0, sort_column: Some("_id".into()), sort_ascending: false, filter: None };
    let r = d.fetch_page(&db, "dboard_c", &page).await.unwrap();
    assert_eq!(r.rows.len(), 2);
    assert_eq!(r.rows[0][0].as_deref(), Some("2"));
    let idx = |n: &str| r.columns.iter().position(|c| c == n).unwrap();
    assert_eq!(r.rows[1][idx("tags")].as_deref(), Some(r#"["x","y"]"#));
    let f = d.fetch_page(&db, "dboard_c", &Page { filter: Some(r#"{"flag": true}"#.into()), ..page.clone() }).await.unwrap();
    assert_eq!(f.rows.len(), 1);

    // Typed edit (double), string edit, NULL, bad value, undo.
    let row = r.rows[1].clone(); // _id = 1
    d.edit_cell(&db, "dboard_c", &row, "n", Some("9.5".into())).await.unwrap();
    d.edit_cell(&db, "dboard_c", &row, "name", Some("bob".into())).await.unwrap();
    assert!(d.edit_cell(&db, "dboard_c", &row, "n", Some("abc".into())).await.is_err());
    let q = d.execute_query(r#"db.dboard_c.find({"_id": 1})"#).await.unwrap();
    let qi = |n: &str| q.columns.iter().position(|c| c == n).unwrap();
    assert_eq!(q.rows[0][qi("name")].as_deref(), Some("bob"));
    assert_eq!(q.rows[0][qi("n")].as_deref(), Some("9.5"));
    d.undo().await.unwrap().unwrap();
    d.edit_cell(&db, "dboard_c", &row, "name", None).await.unwrap();
    let q = d.execute_query(r#"db.dboard_c.find({"_id": 1})"#).await.unwrap();
    assert_eq!(q.rows[0][q.columns.iter().position(|c| c == "name").unwrap()], None);

    // Insert / delete / aggregate / count / raw JSON.
    d.insert_row(&db, "dboard_c", &[("_id".into(), "3".into()), ("name".into(), "carol".into())]).await.unwrap();
    d.delete_row(&db, "dboard_c", &[Some("3".into())]).await.unwrap();
    let agg = d.execute_query(r#"db.dboard_c.aggregate([{"$group": {"_id": null, "total": {"$sum": "$n"}}}])"#).await.unwrap();
    assert!(agg.columns.contains(&"total".to_string()));
    let cnt = d.execute_query(r#"db.dboard_c.countDocuments({})"#).await.unwrap();
    assert_eq!(cnt.rows[0][0].as_deref(), Some("2"));
    let json = d.document_json(&db, "dboard_c", &row).await.unwrap();
    assert!(json.contains("\"n\""));
    d.replace_document(&db, "dboard_c", &row, r#"{"name": "replaced", "n": 1}"#).await.unwrap();
    assert!(d.document_json(&db, "dboard_c", &row).await.unwrap().contains("replaced"));
    assert!(d.ddl(&db, "dboard_c").await.unwrap().contains("dboard_c"));

    d.truncate(&db, "dboard_c").await.unwrap();
    d.drop_table(&db, "dboard_c").await.unwrap();
}

/// TLS: forces `Require` (encrypted, no cert verification) and confirms the session is actually SSL.
#[tokio::test(flavor = "current_thread")]
async fn postgres_tls_required() {
    let _g = pg_lock();
    let Some((mut c, pw)) = cfg("DBOARD_TEST_PG", DbType::Postgres) else { return };
    if std::env::var("DBOARD_TEST_PG_SSL").is_err() {
        return; // server may not have ssl enabled
    }
    c.ssl = SslMode::Require;
    let mut d = Conn::connect(c, &pw).await.unwrap();
    let r = d.execute_query("SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()").await.unwrap();
    assert_eq!(r.rows[0][0].as_deref(), Some("t"));
    // A server that can't do TLS must be refused in Require mode, not silently downgraded.
}

/// Export a database that uses most PostgreSQL features, restore it into a fresh one, compare;
/// then exercise users, database switching and the object listing.
#[tokio::test(flavor = "current_thread")]
async fn postgres_dump_import_users() {
    let _g = pg_lock();
    use dboard_core::dump::{DumpOptions, ImportOptions};
    let Some((base, pw)) = cfg("DBOARD_TEST_PG", DbType::Postgres) else { return };
    let mut admin = Conn::connect(base.clone(), &pw).await.unwrap();
    for db in ["dboard_src", "dboard_dst"] {
        let _ = admin.execute_query(&format!("DROP DATABASE IF EXISTS {db} WITH (FORCE)")).await;
        admin.execute_query(&format!("CREATE DATABASE {db}")).await.unwrap();
    }
    let _ = admin.execute_query("DROP ROLE IF EXISTS dboard_ro").await;
    let _ = admin.execute_query("DROP ROLE IF EXISTS dboard_rw").await;

    let at = |db: &str| ConnectionConfig { database: db.into(), ..base.clone() };
    let mut src = Conn::connect(at("dboard_src"), &pw).await.unwrap();
    for sql in [
        "CREATE SCHEMA shop",
        "CREATE TYPE shop.mood AS ENUM ('sad', 'ok', 'it''s great')",
        "CREATE SEQUENCE shop.ticket_seq START 100",
        "CREATE TABLE shop.customers (id serial PRIMARY KEY, name text NOT NULL, mood shop.mood, note text, meta jsonb, tags text[], raw bytea, born date, score numeric(8,2) DEFAULT 1.50)",
        "CREATE TABLE shop.orders (id int GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, customer_id int NOT NULL REFERENCES shop.customers(id), total numeric CHECK (total >= 0), ticket bigint DEFAULT nextval('shop.ticket_seq'), doubled numeric GENERATED ALWAYS AS (total * 2) STORED)",
        "CREATE INDEX orders_customer ON shop.orders (customer_id)",
        "CREATE VIEW shop.big_orders AS SELECT * FROM shop.orders WHERE total > 10",
        "CREATE MATERIALIZED VIEW shop.totals AS SELECT customer_id, sum(total) AS s FROM shop.orders GROUP BY 1",
        "CREATE FUNCTION shop.add_one(a int) RETURNS int LANGUAGE plpgsql AS $$ BEGIN RETURN a + 1; -- ; tricky\nEND; $$",
        "CREATE FUNCTION shop.touch() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN NEW.note := coalesce(NEW.note, 'touched'); RETURN NEW; END; $$",
        "CREATE TRIGGER customers_touch BEFORE INSERT ON shop.customers FOR EACH ROW EXECUTE FUNCTION shop.touch()",
        "CREATE TABLE shop.no_key (a int, b text)",
        "INSERT INTO shop.customers (name, mood, note, meta, tags, raw, born) VALUES \
            ('Ann \"A\"', 'it''s great', E'line1\\nline2\\ttab \\\\ back', '{\"k\": [1, 2]}', ARRAY['x','y z'], '\\xdeadbeef', '2020-02-29'), \
            ('Bob; DROP TABLE x;', NULL, NULL, NULL, NULL, NULL, NULL), \
            ('Ünïcode ✓', 'sad', '', '[]', '{}', '', NULL)",
        "INSERT INTO shop.orders (customer_id, total) VALUES (1, 5), (1, 20.5), (2, 0)",
        "INSERT INTO shop.no_key VALUES (1, 'a'), (1, 'a'), (NULL, NULL)",
        "REFRESH MATERIALIZED VIEW shop.totals",
    ] {
        src.execute_query(sql).await.unwrap_or_else(|e| panic!("{sql}: {e}"));
    }
    src.refresh_metadata().await.unwrap();
    // Everything is listed: routines, triggers, types, indexes, sequences.
    let kinds = |o: &Conn, k: ObjectKind| o.metadata.objects.iter().filter(|x| x.kind == k).map(|x| x.name.clone()).collect::<Vec<_>>();
    assert!(kinds(&src, ObjectKind::Function).contains(&"add_one".to_string()));
    assert_eq!(kinds(&src, ObjectKind::Trigger), ["customers_touch"]);
    assert_eq!(kinds(&src, ObjectKind::Type), ["mood"]);
    assert!(kinds(&src, ObjectKind::Index).contains(&"orders_customer".to_string()));
    assert_eq!(kinds(&src, ObjectKind::Sequence), ["customers_id_seq", "ticket_seq"]);
    let f = src.metadata.objects.iter().find(|o| o.name == "add_one").unwrap().clone();
    assert_eq!(f.detail, "a integer");
    assert!(src.object_def(&f).await.unwrap().contains("RETURN a + 1"));
    let tr = src.metadata.objects.iter().find(|o| o.kind == ObjectKind::Trigger).unwrap().clone();
    assert!(src.object_def(&tr).await.unwrap().contains("BEFORE INSERT ON shop.customers"));

    // Export.
    let dir = std::env::temp_dir().join(format!("dboard-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("dump.sql");
    let mut msgs = Vec::new();
    let st = src.export_database(&file, &DumpOptions::default(), &mut |m| msgs.push(m)).await.unwrap();
    assert_eq!(st.tables, 3);
    assert_eq!(st.rows, 3 + 3 + 3);
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("CREATE TABLE \"shop\".\"customers\"") && text.contains("ADD CONSTRAINT") && text.contains("GENERATED ALWAYS AS"));

    // Import into the empty database and compare every table.
    let mut dst = Conn::connect(at("dboard_dst"), &pw).await.unwrap();
    let r = dst.import_database(&file, &ImportOptions::default(), &mut |_| {}).await.unwrap();
    assert!(r.errors.is_empty() && r.statements > 20, "{r:?}");
    for q in [
        "SELECT * FROM shop.customers ORDER BY id",
        "SELECT * FROM shop.orders ORDER BY id",
        "SELECT * FROM shop.no_key ORDER BY a, b",
        "SELECT * FROM shop.big_orders",
        "SELECT * FROM shop.totals ORDER BY 1",
        "SELECT shop.add_one(41)",
        "SELECT conname FROM pg_constraint WHERE connamespace = 'shop'::regnamespace ORDER BY 1",
        "SELECT indexname FROM pg_indexes WHERE schemaname = 'shop' ORDER BY 1",
    ] {
        let (a, b) = (src.execute_query(q).await.unwrap(), dst.execute_query(q).await.unwrap());
        assert_eq!(a.rows, b.rows, "{q}");
        assert!(!a.rows.is_empty(), "{q}");
    }
    // Sequences and identity continue where they left off; the trigger fires on new rows.
    dst.execute_query("INSERT INTO shop.customers (name) VALUES ('new')").await.unwrap();
    assert_eq!(dst.execute_query("SELECT id::text || note FROM shop.customers WHERE name = 'new'").await.unwrap().rows[0][0].as_deref(), Some("4touched"));
    dst.execute_query("INSERT INTO shop.orders (customer_id, total) VALUES (1, 1)").await.unwrap();
    assert_eq!(dst.execute_query("SELECT id::text || ':' || ticket FROM shop.orders ORDER BY id DESC LIMIT 1").await.unwrap().rows[0][0].as_deref(), Some("4:103"));

    // Importing again must fail atomically (duplicate keys) and change nothing.
    let before = dst.execute_query("SELECT count(*) FROM shop.customers").await.unwrap().rows[0][0].clone();
    let err = dst.import_database(&file, &ImportOptions::default(), &mut |_| {}).await.unwrap_err().to_string();
    assert!(err.contains("rolled back"), "{err}");
    assert_eq!(dst.execute_query("SELECT count(*) FROM shop.customers").await.unwrap().rows[0][0], before);

    // pg_dump style COPY blocks import too.
    let copy = dir.join("copy.sql");
    std::fs::write(&copy, "CREATE TABLE public.c (a int, b text);\nCOPY public.c (a, b) FROM stdin;\n1\tx\\ty\n2\t\\N\n\\.\nSELECT 1;\n").unwrap();
    let r = dst.import_database(&copy, &ImportOptions::default(), &mut |_| {}).await.unwrap();
    assert_eq!(r.rows_copied, 2);
    assert_eq!(dst.execute_query("SELECT b FROM public.c WHERE a = 1").await.unwrap().rows[0][0].as_deref(), Some("x\ty"));
    // Continue-on-error mode reports failures instead of stopping.
    std::fs::write(&copy, "SELECT 1;\nSELEC bad;\nCREATE TABLE public.after_error (a int);\n").unwrap();
    let r = dst.import_database(&copy, &ImportOptions { stop_on_error: false }, &mut |_| {}).await.unwrap();
    assert_eq!(r.errors.len(), 1);
    assert!(dst.execute_query("SELECT * FROM public.after_error").await.is_ok());

    // Databases: list and switch.
    let dbs = admin.list_databases().await.unwrap();
    assert!(dbs.contains(&"dboard_src".to_string()) && dbs.contains(&"dboard_dst".to_string()));
    src.switch_database(Some("dboard_dst"), &pw).await.unwrap();
    assert_eq!(src.current_database().await.unwrap().as_deref(), Some("dboard_dst"));
    assert!(src.table("public", "c").is_some());

    // Users: create with levels, verify what they can really do.
    let ro = NewUser { name: "dboard_ro".into(), host: String::new(), password: "p'w\"1".into(), access: AccessLevel::ReadOnly, admin: false };
    let rw = NewUser { name: "dboard_rw".into(), access: AccessLevel::ReadWrite, ..ro.clone() };
    dst.create_user(&ro).await.unwrap();
    dst.create_user(&rw).await.unwrap();
    assert!(dst.create_user(&ro).await.is_err(), "duplicate user");
    let users = dst.list_users().await.unwrap();
    let ro_info = users.iter().find(|u| u.name == "dboard_ro").cloned().unwrap();
    assert!(ro_info.summary.contains("can log in"));
    let grants = dst.user_grants(&ro_info).await.unwrap().join("\n");
    assert!(grants.contains("shop.customers: SELECT") && !grants.contains("INSERT"), "{grants}");
    let as_user = |name: &str| ConnectionConfig { username: name.into(), database: "dboard_dst".into(), ..base.clone() };
    let mut u = Conn::connect(as_user("dboard_ro"), "p'w\"1").await.unwrap();
    assert!(u.execute_query("SELECT count(*) FROM shop.customers").await.is_ok());
    assert!(u.execute_query("INSERT INTO shop.customers (name) VALUES ('x')").await.is_err());
    let mut w = Conn::connect(as_user("dboard_rw"), "p'w\"1").await.unwrap();
    assert!(w.execute_query("INSERT INTO shop.customers (name) VALUES ('from rw')").await.is_ok());
    assert!(w.execute_query("DROP TABLE shop.customers").await.is_err());
    // Raise, lower, change password, drop.
    dst.set_access(&ro_info, AccessLevel::ReadWrite).await.unwrap();
    let mut u = Conn::connect(as_user("dboard_ro"), "p'w\"1").await.unwrap();
    assert!(u.execute_query("INSERT INTO shop.customers (name) VALUES ('now ok')").await.is_ok());
    dst.set_access(&ro_info, AccessLevel::None).await.unwrap();
    let mut u = Conn::connect(as_user("dboard_ro"), "p'w\"1").await.unwrap();
    assert!(u.execute_query("SELECT * FROM shop.customers").await.is_err());
    dst.set_password(&ro_info, "other").await.unwrap();
    dst.drop_user(&ro_info).await.unwrap();
    let rw_info = dst.list_users().await.unwrap().into_iter().find(|u| u.name == "dboard_rw").unwrap();
    dst.drop_user(&rw_info).await.unwrap();
    assert!(!dst.list_users().await.unwrap().iter().any(|u| u.name.starts_with("dboard_r")));

    drop((src, dst, u, w));
    for db in ["dboard_src", "dboard_dst"] {
        admin.execute_query(&format!("DROP DATABASE {db} WITH (FORCE)")).await.unwrap();
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// MySQL / MariaDB: same round trip as PostgreSQL (DBOARD_TEST_MYSQL must point at an admin account).
#[tokio::test(flavor = "current_thread")]
async fn mysql_dump_import_users() {
    use dboard_core::dump::{DumpOptions, ImportOptions};
    let Some((base, pw)) = cfg("DBOARD_TEST_MYSQL", DbType::MySql) else { return };
    let mut admin = Conn::connect(ConnectionConfig { database: String::new(), ..base.clone() }, &pw).await.unwrap();
    for db in ["dboard_src", "dboard_dst"] {
        admin.execute_query(&format!("DROP DATABASE IF EXISTS {db}")).await.unwrap();
        admin.execute_query(&format!("CREATE DATABASE {db}")).await.unwrap();
    }
    for u in ["dboard_ro", "dboard_rw"] {
        let _ = admin.execute_query(&format!("DROP USER '{u}'@'%'")).await;
    }
    let at = |db: &str| ConnectionConfig { database: db.into(), ..base.clone() };
    let mut src = Conn::connect(at("dboard_src"), &pw).await.unwrap();
    for sql in [
        "CREATE TABLE customers (id int AUTO_INCREMENT PRIMARY KEY, name varchar(60) NOT NULL, note text, raw blob, born date, score decimal(8,2) DEFAULT 1.50, ok tinyint(1), mood enum('sad','it''s great'), at datetime(3), big bigint unsigned)",
        "CREATE TABLE orders (id int AUTO_INCREMENT PRIMARY KEY, customer_id int NOT NULL, total double, CONSTRAINT fk_cust FOREIGN KEY (customer_id) REFERENCES customers(id), INDEX orders_total (total))",
        "CREATE VIEW z_last AS SELECT * FROM orders WHERE total > 10",
        "CREATE VIEW a_first AS SELECT * FROM z_last",
        "CREATE FUNCTION add_one(a int) RETURNS int DETERMINISTIC RETURN a + 1",
        "CREATE PROCEDURE bump(IN cid int) BEGIN UPDATE orders SET total = total + 1 WHERE customer_id = cid; SELECT 1; END",
        "CREATE TRIGGER cust_touch BEFORE INSERT ON customers FOR EACH ROW SET NEW.note = COALESCE(NEW.note, 'touched')",
        "INSERT INTO customers (name, note, raw, born, ok, mood, at, big) VALUES \
            ('Ann \"A\" it''s', 'line1\nline2\ttab \\\\ back', 0xDEADBEEF, '2020-02-29', 1, 'it''s great', '2020-01-02 03:04:05.678', 18446744073709551615), \
            ('Bob; DROP TABLE x;', NULL, NULL, NULL, NULL, NULL, NULL, NULL), \
            ('Ünïcode ✓', '', '', NULL, 0, 'sad', NULL, 0)",
        "INSERT INTO orders (customer_id, total) VALUES (1, 5.5), (1, 20.25), (2, 0)",
    ] {
        src.execute_query(sql).await.unwrap_or_else(|e| panic!("{sql}: {e}"));
    }
    src.refresh_metadata().await.unwrap();
    let kinds = |o: &Conn, k: ObjectKind| o.metadata.objects.iter().filter(|x| x.kind == k).map(|x| x.name.clone()).collect::<Vec<_>>();
    assert_eq!(kinds(&src, ObjectKind::Procedure), ["bump"]);
    assert_eq!(kinds(&src, ObjectKind::Function), ["add_one"]);
    assert_eq!(kinds(&src, ObjectKind::Trigger), ["cust_touch"]);
    assert!(kinds(&src, ObjectKind::Index).contains(&"orders_total".to_string()));
    let p = src.metadata.objects.iter().find(|o| o.kind == ObjectKind::Procedure).unwrap().clone();
    let def = src.object_def(&p).await.unwrap();
    assert!(def.contains("PROCEDURE") && !def.contains("DEFINER"), "{def}");
    let ix = src.metadata.objects.iter().find(|o| o.name == "orders_total").unwrap().clone();
    assert_eq!(src.object_def(&ix).await.unwrap(), "CREATE INDEX `orders_total` ON `dboard_src`.`orders` (total);");
    // Selecting a database narrows the listing to it.
    assert!(src.metadata.tables.iter().all(|t| t.schema == "dboard_src"));

    let dir = std::env::temp_dir().join(format!("dboard-test-my-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("dump.sql");
    let st = src.export_database(&file, &DumpOptions::default(), &mut |_| {}).await.unwrap();
    assert_eq!((st.tables, st.rows), (2, 6));

    let mut dst = Conn::connect(at("dboard_dst"), &pw).await.unwrap();
    let r = dst.import_database(&file, &ImportOptions::default(), &mut |_| {}).await.unwrap();
    assert!(r.errors.is_empty(), "{r:?}");
    for q in [
        "SELECT id, name, note, HEX(raw), born, score, ok, mood, at, big FROM customers ORDER BY id",
        "SELECT * FROM orders ORDER BY id",
        "SELECT * FROM a_first",
        "SELECT add_one(41)",
    ] {
        let (a, b) = (src.execute_query(q).await.unwrap(), dst.execute_query(q).await.unwrap());
        assert_eq!(a.rows, b.rows, "{q}");
        assert!(!a.rows.is_empty(), "{q}");
    }
    dst.execute_query("INSERT INTO customers (name) VALUES ('new')").await.unwrap();
    assert_eq!(dst.execute_query("SELECT CONCAT(id, note) FROM customers WHERE name = 'new'").await.unwrap().rows[0][0].as_deref(), Some("4touched"));
    dst.refresh_metadata().await.unwrap();
    assert_eq!(kinds(&dst, ObjectKind::Trigger), ["cust_touch"]);

    // Databases.
    let dbs = admin.list_databases().await.unwrap();
    assert!(dbs.contains(&"dboard_src".to_string()) && !dbs.contains(&"mysql".to_string()));
    admin.switch_database(Some("dboard_dst"), &pw).await.unwrap();
    assert!(admin.metadata.tables.iter().all(|t| t.schema == "dboard_dst") && !admin.metadata.tables.is_empty());
    admin.switch_database(None, &pw).await.unwrap();
    assert!(admin.metadata.tables.iter().any(|t| t.schema == "dboard_src") && admin.metadata.tables.iter().any(|t| t.schema == "dboard_dst"));
    admin.switch_database(Some("dboard_dst"), &pw).await.unwrap();

    // Users.
    let ro = NewUser { name: "dboard_ro".into(), host: String::new(), password: "p'w\"1\\".into(), access: AccessLevel::ReadOnly, admin: false };
    let rw = NewUser { name: "dboard_rw".into(), access: AccessLevel::ReadWrite, ..ro.clone() };
    admin.create_user(&ro).await.unwrap();
    admin.create_user(&rw).await.unwrap();
    assert!(admin.create_user(&ro).await.is_err());
    let users = admin.list_users().await.unwrap();
    let ro_info = users.iter().find(|u| u.name == "dboard_ro").cloned().unwrap();
    assert_eq!(ro_info.origin, "%");
    assert!(admin.user_grants(&ro_info).await.unwrap().join("\n").contains("SELECT"));
    let as_user = |name: &str| ConnectionConfig { username: name.into(), database: "dboard_dst".into(), ..base.clone() };
    let mut u = Conn::connect(as_user("dboard_ro"), "p'w\"1\\").await.unwrap();
    assert!(u.execute_query("SELECT count(*) FROM customers").await.is_ok());
    assert!(u.execute_query("INSERT INTO customers (name) VALUES ('x')").await.is_err());
    let mut w = Conn::connect(as_user("dboard_rw"), "p'w\"1\\").await.unwrap();
    assert!(w.execute_query("INSERT INTO customers (name) VALUES ('from rw')").await.is_ok());
    assert!(w.execute_query("DROP TABLE customers").await.is_err());
    admin.set_access(&ro_info, AccessLevel::ReadWrite).await.unwrap();
    let mut u = Conn::connect(as_user("dboard_ro"), "p'w\"1\\").await.unwrap();
    assert!(u.execute_query("INSERT INTO customers (name) VALUES ('now ok')").await.is_ok());
    admin.set_access(&ro_info, AccessLevel::None).await.unwrap();
    let mut u = Conn::connect(as_user("dboard_ro"), "p'w\"1\\").await;
    assert!(u.is_err() || u.as_mut().unwrap().execute_query("SELECT * FROM customers").await.is_err());
    admin.set_password(&ro_info, "other").await.unwrap();
    // (No rights left on dboard_dst, so connect without selecting it.)
    assert!(Conn::connect(ConnectionConfig { database: String::new(), ..as_user("dboard_ro") }, "other").await.is_ok());
    admin.drop_user(&ro_info).await.unwrap();
    let rw_info = admin.list_users().await.unwrap().into_iter().find(|u| u.name == "dboard_rw").unwrap();
    admin.drop_user(&rw_info).await.unwrap();

    admin.switch_database(None, &pw).await.unwrap();
    for db in ["dboard_src", "dboard_dst"] {
        admin.execute_query(&format!("DROP DATABASE {db}")).await.unwrap();
    }
    let _ = std::fs::remove_dir_all(&dir);
}
