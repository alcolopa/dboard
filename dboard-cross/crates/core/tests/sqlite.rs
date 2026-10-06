//! SQLite needs no server, so these always run.

use dboard_core::dump::{DumpOptions, ImportOptions};
use dboard_core::model::*;
use dboard_core::Conn;

fn cfg(path: &std::path::Path) -> ConnectionConfig {
    ConnectionConfig { id: "sqlite".into(), db_type: DbType::Sqlite, database: path.to_string_lossy().into_owned(), ..Default::default() }
}

fn temp(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("dboard-sqlite-{}-{name}.db", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

#[tokio::test(flavor = "current_thread")]
async fn sqlite_end_to_end() {
    let path = temp("main");
    // an empty file is a valid SQLite database
    std::fs::write(&path, b"").unwrap();
    let mut d = Conn::connect(cfg(&path), "").await.unwrap();
    assert!(d.server_version.starts_with("SQLite"));

    d.execute_query("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER)").await.unwrap();
    d.execute_query("CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER REFERENCES users(id), total REAL)").await.unwrap();
    d.execute_query("CREATE INDEX orders_user ON orders(user_id)").await.unwrap();
    d.execute_query("INSERT INTO users (name, age) VALUES ('ann', 30), ('bob', NULL)").await.unwrap();
    d.execute_query("INSERT INTO orders (user_id, total) VALUES (1, 9.5), (2, 12)").await.unwrap();
    d.execute_query("CREATE VIEW big AS SELECT * FROM orders WHERE total > 10").await.unwrap();
    d.refresh_metadata().await.unwrap();

    // metadata: columns, primary key, foreign key, index, view
    let users = d.table("main", "users").unwrap().clone();
    assert_eq!(users.columns.len(), 3);
    assert!(users.columns[0].is_primary_key && !users.columns[1].nullable && users.columns[2].nullable);
    let orders = d.table("main", "orders").unwrap().clone();
    assert_eq!(orders.columns[1].fk.as_deref(), Some("main.users(id)"));
    assert_eq!(orders.indexes.len(), 1);
    assert_eq!(d.table("main", "big").unwrap().kind, TableKind::View);
    assert!(users.is_editable() && !d.table("main", "big").unwrap().is_editable());

    // browse with sort, filter and the total
    let page = Page { limit: 10, offset: 0, sort_column: Some("name".into()), sort_ascending: false, filter: None };
    let r = d.fetch_page("main", "users", &page).await.unwrap();
    assert_eq!(r.rows[0][1].as_deref(), Some("bob"));
    assert_eq!(r.rows[0][2], None);
    assert_eq!(r.total_estimate, Some(2));
    let f = d.fetch_page("main", "users", &Page { filter: Some("age > 20".into()), ..page.clone() }).await.unwrap();
    assert_eq!(f.rows.len(), 1);

    // edit, insert, delete and undo
    d.edit_cell("main", "users", &r.rows[0], "age", Some("41".into())).await.unwrap();
    let after = d.execute_query("SELECT age FROM users WHERE name = 'bob'").await.unwrap();
    assert_eq!(after.rows[0][0].as_deref(), Some("41"));
    d.undo().await.unwrap();
    let back = d.execute_query("SELECT age FROM users WHERE name = 'bob'").await.unwrap();
    assert_eq!(back.rows[0][0], None);
    d.insert_row("main", "users", &[("name".into(), "cy's".into())]).await.unwrap();
    assert!(d.insert_row("main", "users", &[("age".into(), "5".into())]).await.is_err(), "NOT NULL is enforced");
    let cy = d.execute_query("SELECT * FROM users WHERE name = 'cy''s'").await.unwrap();
    d.delete_row("main", "users", &cy.rows[0]).await.unwrap();
    assert_eq!(d.execute_query("SELECT COUNT(*) FROM users").await.unwrap().rows[0][0].as_deref(), Some("2"));
    // foreign keys are enforced
    assert!(d.execute_query("INSERT INTO orders (user_id, total) VALUES (99, 1)").await.is_err());

    // ddl and plan
    let ddl = d.ddl("main", "orders").await.unwrap();
    assert!(ddl.contains("CREATE TABLE orders") && ddl.contains("CREATE INDEX orders_user"));
    let plan = d.explain("SELECT * FROM orders WHERE user_id = 1", false).await.unwrap();
    assert!(plan.rows.iter().any(|r| r[0].as_deref().is_some_and(|s| s.contains("orders"))));

    // transactions roll back
    d.begin().await.unwrap();
    d.execute_query("DELETE FROM orders").await.unwrap();
    d.rollback().await.unwrap();
    assert_eq!(d.execute_query("SELECT COUNT(*) FROM orders").await.unwrap().rows[0][0].as_deref(), Some("2"));

    // dump -> import into a fresh file reproduces the data
    let dump = temp("dump.sql");
    let st = d.export_database(&dump, &DumpOptions::default(), &mut |_| {}).await.unwrap();
    assert_eq!((st.tables, st.rows), (2, 4));
    let other = temp("copy");
    std::fs::write(&other, b"").unwrap();
    let mut c = Conn::connect(cfg(&other), "").await.unwrap();
    let imp = c.import_database(&dump, &ImportOptions::default(), &mut |_| {}).await.unwrap();
    assert!(imp.errors.is_empty(), "{:?}", imp.errors);
    assert_eq!(c.execute_query("SELECT COUNT(*) FROM orders").await.unwrap().rows[0][0].as_deref(), Some("2"));
    assert!(c.table("main", "big").is_some());

    // truncate and drop
    d.truncate("main", "orders").await.unwrap();
    d.drop_table("main", "orders").await.unwrap();
    assert!(d.table("main", "orders").is_none());

    for p in [&path, &dump, &other] {
        let _ = std::fs::remove_file(p);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn sqlite_read_only_and_missing_files() {
    let path = temp("ro");
    std::fs::write(&path, b"").unwrap();
    let mut rw = Conn::connect(cfg(&path), "").await.unwrap();
    rw.execute_query("CREATE TABLE t (x INTEGER)").await.unwrap();
    drop(rw);
    let mut ro = Conn::connect(ConnectionConfig { read_only: true, ..cfg(&path) }, "").await.unwrap();
    assert!(ro.execute_query("SELECT * FROM t").await.is_ok());
    assert!(ro.execute_query("INSERT INTO t VALUES (1)").await.is_err());
    let err = Conn::connect(cfg(std::path::Path::new("/definitely/not/here.db")), "").await.err().unwrap().to_string();
    assert!(err.contains("No file"), "{err}");
    let _ = std::fs::remove_file(&path);
}

#[tokio::test(flavor = "current_thread")]
async fn sqlite_force_drop_ignores_foreign_keys() {
    let path = temp("force");
    std::fs::write(&path, b"").unwrap();
    let mut d = Conn::connect(cfg(&path), "").await.unwrap();
    d.execute_query("CREATE TABLE a (id INTEGER PRIMARY KEY)").await.unwrap();
    d.execute_query("CREATE TABLE b (id INTEGER PRIMARY KEY, a_id INTEGER REFERENCES a(id))").await.unwrap();
    d.execute_query("INSERT INTO a VALUES (1)").await.unwrap();
    d.execute_query("INSERT INTO b VALUES (1, 1)").await.unwrap();
    d.refresh_metadata().await.unwrap();
    // emptying the referenced table is refused without force and allowed with it
    assert!(d.truncate("main", "a").await.is_err());
    d.truncate_forced("main", "a", true).await.unwrap();
    let items = vec![("main".to_string(), "a".to_string()), ("main".to_string(), "b".to_string())];
    assert_eq!(d.drop_many(&items, true).await.unwrap(), 2);
    assert!(d.table("main", "a").is_none() && d.table("main", "b").is_none());
    // checks are back on afterwards
    d.execute_query("CREATE TABLE p (id INTEGER PRIMARY KEY)").await.unwrap();
    d.execute_query("CREATE TABLE c (p_id INTEGER REFERENCES p(id))").await.unwrap();
    assert!(d.execute_query("INSERT INTO c VALUES (99)").await.is_err());
}
