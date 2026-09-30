//! Integration tests against real servers. Each runs only when its env var is set:
//!   DBOARD_TEST_PG=host:port:user:password:database
//!   DBOARD_TEST_MYSQL=host:port:user:password:database

use dboard_core::model::*;
use dboard_core::Conn;

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
    assert!(!d.table(&schema, "dboard_nopk").unwrap().is_editable());

    // Browse: sorted desc, NULL preserved.
    let page = Page { limit: 10, offset: 0, sort_column: Some("id".into()), sort_ascending: false, filter: None };
    let r = d.fetch_page(&schema, "dboard_t", &page).await.unwrap();
    assert_eq!(r.rows.len(), 2);
    assert_eq!(r.rows[0][0].as_deref(), Some("2"));
    assert_eq!(r.rows[0][1], None);
    let filtered = d.fetch_page(&schema, "dboard_t", &Page { filter: Some("id = 1".into()), ..page.clone() }).await.unwrap();
    assert_eq!(filtered.rows.len(), 1);

    // Edit + undo.
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

    // No PK => refused; injection text is just data.
    let e = d.edit_cell(&schema, "dboard_nopk", &[Some("1".into())], "x", Some("2".into())).await.unwrap_err();
    assert!(e.to_string().contains("no primary key"), "{e}");
    d.edit_cell(&schema, "dboard_t", &row, "name", Some("x'; DROP TABLE t;--".into())).await.unwrap();
    assert_eq!(d.execute_query("SELECT count(*) FROM dboard_t").await.unwrap().rows[0][0].as_deref(), Some("2"));

    // Insert / delete.
    d.insert_row(&schema, "dboard_t", &[("id".into(), "3".into()), ("name".into(), "carol".into())]).await.unwrap();
    assert_eq!(d.execute_query("SELECT count(*) FROM dboard_t").await.unwrap().rows[0][0].as_deref(), Some("3"));
    let dup = d.insert_row(&schema, "dboard_t", &[("id".into(), "3".into())]).await.unwrap_err();
    assert!(dup.to_string().contains("unique"), "{dup}");
    d.delete_row(&schema, "dboard_t", &[Some("3".into())]).await.unwrap();
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
    if let Some((c, pw)) = cfg("DBOARD_TEST_PG", DbType::Postgres) {
        scenario(c, pw).await;
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
