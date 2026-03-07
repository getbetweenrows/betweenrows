pub use sea_orm_migration::prelude::*;

mod m20260219_000001_create_proxy_users;
mod m20260220_000002_create_data_sources;
mod m20260220_000003_create_user_data_sources;
mod m20260221_000004_create_catalog_tables;
mod m20260301_000005_add_column_is_selected;
mod m20260303_000006_add_schema_alias;
mod m20260306_000007_add_access_mode_to_data_source;
mod m20260307_000008_create_policy;
mod m20260307_000009_create_policy_version;
mod m20260307_000010_create_policy_obligation;
mod m20260307_000011_create_policy_assignment;
mod m20260307_000012_create_query_audit_log;
mod m20260307_000013_idx_policy_version;
mod m20260307_000014_idx_query_audit_log_user_id;
mod m20260307_000015_idx_query_audit_log_data_source_id;
mod m20260307_000016_idx_query_audit_log_created_at;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260219_000001_create_proxy_users::Migration),
            Box::new(m20260220_000002_create_data_sources::Migration),
            Box::new(m20260220_000003_create_user_data_sources::Migration),
            Box::new(m20260221_000004_create_catalog_tables::Migration),
            Box::new(m20260301_000005_add_column_is_selected::Migration),
            Box::new(m20260303_000006_add_schema_alias::Migration),
            Box::new(m20260306_000007_add_access_mode_to_data_source::Migration),
            Box::new(m20260307_000008_create_policy::Migration),
            Box::new(m20260307_000009_create_policy_version::Migration),
            Box::new(m20260307_000010_create_policy_obligation::Migration),
            Box::new(m20260307_000011_create_policy_assignment::Migration),
            Box::new(m20260307_000012_create_query_audit_log::Migration),
            Box::new(m20260307_000013_idx_policy_version::Migration),
            Box::new(m20260307_000014_idx_query_audit_log_user_id::Migration),
            Box::new(m20260307_000015_idx_query_audit_log_data_source_id::Migration),
            Box::new(m20260307_000016_idx_query_audit_log_created_at::Migration),
        ]
    }
}
