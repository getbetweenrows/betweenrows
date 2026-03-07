use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(PolicyObligation::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PolicyObligation::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(PolicyObligation::PolicyId).uuid().not_null())
                    .col(
                        ColumnDef::new(PolicyObligation::ObligationType)
                            .string()
                            .not_null(),
                    ) // "row_filter" | "column_mask" | "column_access"
                    .col(
                        ColumnDef::new(PolicyObligation::Definition)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(PolicyObligation::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(PolicyObligation::UpdatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(PolicyObligation::Table, PolicyObligation::PolicyId)
                            .to(Policy::Table, Policy::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(PolicyObligation::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum Policy {
    Table,
    Id,
}

#[derive(Iden)]
enum PolicyObligation {
    Table,
    Id,
    PolicyId,
    ObligationType,
    Definition,
    CreatedAt,
    UpdatedAt,
}
