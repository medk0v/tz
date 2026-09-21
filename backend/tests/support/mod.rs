use sqlx::PgPool;
use uuid::Uuid;

pub async fn add_project_admin(db: &PgPool, tenant_id: Uuid, project_id: Uuid, user_id: Uuid) {
    sqlx::query(
        r#"
        INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, is_system, permissions)
        VALUES ($1, $2, 'admin', 'Admin', 'admin', true, ARRAY[
            'projects:read', 'projects:manage', 'conversations:read', 'conversations:reply',
            'conversations:close', 'contacts:read', 'contacts:manage', 'channels:read',
            'channels:manage', 'access_tokens:manage', 'visitor_network:read', 'quality:read',
            'quality:read_all', 'reviews:read', 'routing:manage', 'teams:manage', 'ai:manage',
            'knowledge:manage', 'notes:read', 'notes:write', 'reply_templates:manage', 'processes:read', 'processes:edit',
            'processes:approve', 'tasks:own', 'tasks:manage', 'tasks:configure', 'integrations:manage', 'roles:manage', 'system:read'
        ])
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .execute(db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO memberships (id, tenant_id, project_id, user_id, role) VALUES ($1, $2, $3, $4, 'admin')",
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(project_id)
    .bind(user_id)
    .execute(db)
    .await
    .unwrap();
}
