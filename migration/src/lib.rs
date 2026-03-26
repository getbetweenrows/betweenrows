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
mod m20260315_000017_add_status_to_query_audit_log;
mod m20260315_000018_add_error_message_to_query_audit_log;
mod m20260316_000019_drop_policy_obligation;
mod m20260316_000020_drop_policy_tables;
mod m20260316_000021_create_policy_v2;
mod m20260316_000022_create_policy_version_v2;
mod m20260316_000023_create_policy_assignment_v2;
mod m20260320_000024_create_role;
mod m20260320_000025_create_role_member;
mod m20260320_000026_idx_role_member_unique;
mod m20260320_000027_create_role_inheritance;
mod m20260320_000028_idx_role_inheritance_unique;
mod m20260320_000029_create_admin_audit_log;
mod m20260320_000030_idx_admin_audit_resource;
mod m20260320_000031_idx_admin_audit_created_at;
mod m20260320_000032_add_scope_to_policy_assignment;
mod m20260320_000033_backfill_policy_assignment_scope;
mod m20260320_000034_add_role_id_to_policy_assignment;
mod m20260320_000035_create_data_source_access;
mod m20260320_000036_migrate_user_data_source;
mod m20260320_000037_drop_user_data_source;
mod m20260320_000038_idx_data_source_access_unique;
mod m20260320_000039_fk_data_source_access_user;
mod m20260320_000040_fk_data_source_access_role;
mod m20260320_000041_fk_role_inheritance_child;
mod m20260325_000048_create_decision_function;
mod m20260325_000049_idx_decision_function_name;
mod m20260325_000050_add_decision_function_id_to_policy;
mod m20260325_000051_clear_static_wasm;

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
            Box::new(m20260315_000017_add_status_to_query_audit_log::Migration),
            Box::new(m20260315_000018_add_error_message_to_query_audit_log::Migration),
            Box::new(m20260316_000019_drop_policy_obligation::Migration),
            Box::new(m20260316_000020_drop_policy_tables::Migration),
            Box::new(m20260316_000021_create_policy_v2::Migration),
            Box::new(m20260316_000022_create_policy_version_v2::Migration),
            Box::new(m20260316_000023_create_policy_assignment_v2::Migration),
            Box::new(m20260320_000024_create_role::Migration),
            Box::new(m20260320_000025_create_role_member::Migration),
            Box::new(m20260320_000026_idx_role_member_unique::Migration),
            Box::new(m20260320_000027_create_role_inheritance::Migration),
            Box::new(m20260320_000028_idx_role_inheritance_unique::Migration),
            Box::new(m20260320_000029_create_admin_audit_log::Migration),
            Box::new(m20260320_000030_idx_admin_audit_resource::Migration),
            Box::new(m20260320_000031_idx_admin_audit_created_at::Migration),
            Box::new(m20260320_000032_add_scope_to_policy_assignment::Migration),
            Box::new(m20260320_000033_backfill_policy_assignment_scope::Migration),
            Box::new(m20260320_000034_add_role_id_to_policy_assignment::Migration),
            Box::new(m20260320_000035_create_data_source_access::Migration),
            Box::new(m20260320_000036_migrate_user_data_source::Migration),
            Box::new(m20260320_000037_drop_user_data_source::Migration),
            Box::new(m20260320_000038_idx_data_source_access_unique::Migration),
            Box::new(m20260320_000039_fk_data_source_access_user::Migration),
            Box::new(m20260320_000040_fk_data_source_access_role::Migration),
            Box::new(m20260320_000041_fk_role_inheritance_child::Migration),
            Box::new(m20260325_000048_create_decision_function::Migration),
            Box::new(m20260325_000049_idx_decision_function_name::Migration),
            Box::new(m20260325_000050_add_decision_function_id_to_policy::Migration),
            Box::new(m20260325_000051_clear_static_wasm::Migration),
        ]
    }
}
