use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(PolicyAssignment::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PolicyAssignment::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(PolicyAssignment::PolicyId).uuid().not_null())
                    .col(
                        ColumnDef::new(PolicyAssignment::DataSourceId)
                            .uuid()
                            .not_null(),
                    )
                    .col(ColumnDef::new(PolicyAssignment::UserId).uuid().null())
                    .col(
                        ColumnDef::new(PolicyAssignment::Priority)
                            .integer()
                            .not_null()
                            .default(100),
                    )
                    .col(
                        ColumnDef::new(PolicyAssignment::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(PolicyAssignment::UpdatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(PolicyAssignment::Table, PolicyAssignment::PolicyId)
                            .to(Policy::Table, Policy::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(PolicyAssignment::Table, PolicyAssignment::DataSourceId)
                            .to(DataSource::Table, DataSource::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_policy_assignment_unique")
                    .table(PolicyAssignment::Table)
                    .col(PolicyAssignment::PolicyId)
                    .col(PolicyAssignment::DataSourceId)
                    .col(PolicyAssignment::UserId)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(PolicyAssignment::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum Policy {
    Table,
    Id,
}

#[derive(Iden)]
enum DataSource {
    Table,
    Id,
}

#[derive(Iden)]
enum PolicyAssignment {
    Table,
    Id,
    PolicyId,
    DataSourceId,
    UserId,
    Priority,
    CreatedAt,
    UpdatedAt,
}
