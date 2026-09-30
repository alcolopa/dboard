//! Integration test; runs only when DBOARD_TEST_PG=host:port:user:password:db is set.

use dboard_core::model::{ConnectionConfig, Page};
use dboard_core::postgres::PgDriver;

fn cfg() -> Option<(ConnectionConfig, String)> {
    let v = std::env::var("DBOARD_TEST_PG").ok()?;
    let p: Vec<&str> = v.split(':').collect();
    Some((
        ConnectionConfig {
            host: p[0].into(),
            port: p[1].parse().ok()?,
            username: p[2].into(),
            database: p[4].into(),
            ..Default::default()
        },
        p[3].into(),
    ))
}

#[tokio::test(flavor = "current_thread")]
async fn browse_edit_undo() {
    let Some((c, pw)) = cfg() else { return };
    let mut d = PgDriver::connect(c, &pw).await.unwrap();
    d.execute_query("DROP TABLE IF EXISTS dboard_t").await.unwrap();
    d.execute_query("CREATE TABLE dboard_t (id int primary key, name varchar(20), n numeric, ts timestamptz)").await.unwrap();
    d.execute_query("INSERT INTO dboard_t VALUES (1,'a',1.5,now()),(2,NULL,2,now())").await.unwrap();
    d.execute_query("CREATE TABLE IF NOT EXISTS dboard_nopk (x int)").await.unwrap();
    d.refresh_metadata().await.unwrap();

    let page = Page { limit: 10, offset: 0, sort_column: Some("id".into()), sort_ascending: false, filter: None };
    let r = d.fetch_page("public", "dboard_t", &page).await.unwrap();
    assert_eq!(r.rows.len(), 2);
    assert_eq!(r.rows[0][0].as_deref(), Some("2"));
    assert_eq!(r.rows[0][1], None);

    // Edit + undo
    let row = r.rows[1].clone(); // id=1
    d.edit_cell("public", "dboard_t", &row, "name", Some("bob".into())).await.unwrap();
    let r2 = d.execute_query("SELECT name FROM dboard_t WHERE id = 1").await.unwrap();
    assert_eq!(r2.rows[0][0].as_deref(), Some("bob"));
    d.undo().await.unwrap().unwrap();
    let r3 = d.execute_query("SELECT name FROM dboard_t WHERE id = 1").await.unwrap();
    assert_eq!(r3.rows[0][0].as_deref(), Some("a"));

    // NULL, numeric cast, bad value error
    d.edit_cell("public", "dboard_t", &row, "name", None).await.unwrap();
    d.edit_cell("public", "dboard_t", &row, "n", Some("9.25".into())).await.unwrap();
    let err = d.edit_cell("public", "dboard_t", &row, "n", Some("abc".into())).await.unwrap_err();
    assert!(err.to_string().contains("Invalid value"), "{err}");

    // No PK => refused
    let e = d.edit_cell("public", "dboard_nopk", &[Some("1".into())], "x", Some("2".into())).await.unwrap_err();
    assert!(e.to_string().contains("no primary key"));

    // Injection attempt in a value is just data
    d.edit_cell("public", "dboard_t", &row, "name", Some("x'; DROP TABLE t;--".into())).await.unwrap();
    assert_eq!(d.execute_query("SELECT count(*) FROM dboard_t").await.unwrap().rows[0][0].as_deref(), Some("2"));

    d.execute_query("DROP TABLE dboard_t").await.unwrap();
    d.execute_query("DROP TABLE dboard_nopk").await.unwrap();
}
