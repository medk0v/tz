use sqlx::PgPool;
use uuid::Uuid;

#[sqlx::test(migrations = false)]
#[ignore = "requires PostgreSQL with CREATEDB"]
async fn team_menu_upgrade_preserves_order_and_start_page(db: PgPool) {
    // Test the actual upgrade boundary, before later migrations add more modules.
    for migration in sqlx::migrate!("./migrations")
        .iter()
        .filter(|migration| migration.version < 101)
    {
        sqlx::raw_sql(&migration.sql).execute(&db).await.unwrap();
    }
    let tenant = Uuid::now_v7();
    let project = Uuid::now_v7();
    sqlx::query("INSERT INTO tenants (id,name) VALUES ($1,'Team navigation')")
        .bind(tenant)
        .execute(&db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO projects (id,tenant_id,name,slug) VALUES ($1,$2,'Menu test','menu-test')",
    )
    .bind(project)
    .bind(tenant)
    .execute(&db)
    .await
    .unwrap();
    for (name, menu) in [
        ("With AI", vec!["channels", "ai", "contacts"]),
        ("Without AI", vec!["channels", "contacts"]),
    ] {
        sqlx::query("INSERT INTO departments (id,tenant_id,project_id,name,sidebar_items,default_page) VALUES ($1,$2,$3,$4,$5,'channels')")
            .bind(Uuid::now_v7()).bind(tenant).bind(project).bind(name).bind(menu)
            .execute(&db).await.unwrap();
    }
    sqlx::raw_sql(include_str!("../migrations/0101_team_navigation.sql"))
        .execute(&db)
        .await
        .unwrap();
    let rows: Vec<(String, Vec<String>, String)> = sqlx::query_as(
        "SELECT name,sidebar_items,default_page FROM departments WHERE project_id=$1 ORDER BY name",
    )
    .bind(project)
    .fetch_all(&db)
    .await
    .unwrap();
    assert_eq!(rows[0].1, ["channels", "team", "ai", "contacts"]);
    assert_eq!(rows[1].1, ["channels", "contacts", "team"]);
    assert!(rows.iter().all(|row| row.2 == "channels"));
    // Re-applying the menu update must not duplicate the newly enabled entry.
    sqlx::raw_sql(include_str!("../migrations/0101_team_navigation.sql"))
        .execute(&db)
        .await
        .unwrap();
    let menus: Vec<Vec<String>> = sqlx::query_scalar(
        "SELECT sidebar_items FROM departments WHERE project_id=$1 ORDER BY name",
    )
    .bind(project)
    .fetch_all(&db)
    .await
    .unwrap();
    assert_eq!(
        menus,
        rows.iter().map(|row| row.1.clone()).collect::<Vec<_>>()
    );
    // Team can also be the selected landing page after the migration.
    sqlx::query("UPDATE departments SET default_page='team' WHERE project_id=$1")
        .bind(project)
        .execute(&db)
        .await
        .unwrap();
    assert!(sqlx::query("UPDATE departments SET sidebar_items=ARRAY['team','unknown_module'] WHERE project_id=$1")
        .bind(project).execute(&db).await.is_err());
}
