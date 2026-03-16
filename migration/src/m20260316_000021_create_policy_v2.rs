use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Policy::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Policy::Id).uuid().not_null().primary_key())
                    .col(
                        ColumnDef::new(Policy::Name)
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(Policy::Description).text().null())
                    .col(ColumnDef::new(Policy::PolicyType).string().not_null())
                    .col(ColumnDef::new(Policy::Targets).text().not_null())
                    .col(ColumnDef::new(Policy::Definition).text().null())
                    .col(
                        ColumnDef::new(Policy::IsEnabled)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(
                        ColumnDef::new(Policy::Version)
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    .col(ColumnDef::new(Policy::CreatedBy).uuid().not_null())
                    .col(ColumnDef::new(Policy::UpdatedBy).uuid().not_null())
                    .col(
                        ColumnDef::new(Policy::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Policy::UpdatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Policy::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum Policy {
    Table,
    Id,
    Name,
    Description,
    #[allow(clippy::enum_variant_names)]
    PolicyType,
    Targets,
    Definition,
    IsEnabled,
    Version,
    CreatedBy,
    UpdatedBy,
    CreatedAt,
    UpdatedAt,
}
