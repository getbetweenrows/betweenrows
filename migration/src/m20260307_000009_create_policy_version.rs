use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(PolicyVersion::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PolicyVersion::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(PolicyVersion::PolicyId).uuid().not_null())
                    .col(ColumnDef::new(PolicyVersion::Version).integer().not_null())
                    .col(ColumnDef::new(PolicyVersion::Snapshot).text().not_null())
                    .col(
                        ColumnDef::new(PolicyVersion::ChangeType)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(PolicyVersion::ChangedBy).uuid().not_null())
                    .col(
                        ColumnDef::new(PolicyVersion::CreatedAt)
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
            .drop_table(Table::drop().table(PolicyVersion::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum PolicyVersion {
    Table,
    Id,
    PolicyId,
    Version,
    Snapshot,
    ChangeType,
    ChangedBy,
    CreatedAt,
}
