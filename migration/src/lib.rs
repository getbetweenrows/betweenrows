pub use sea_orm_migration::prelude::*;

mod m20260219_000001_create_proxy_users;
mod m20260220_000002_create_data_sources;
mod m20260220_000003_create_user_data_sources;
mod m20260221_000004_create_catalog_tables;
mod m20260301_000005_add_column_is_selected;

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
        ]
    }
}
