use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ProxyUser::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ProxyUser::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(ProxyUser::Username)
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(ProxyUser::PasswordHash).string().not_null())
                    .col(ColumnDef::new(ProxyUser::Tenant).string().not_null())
                    .col(
                        ColumnDef::new(ProxyUser::IsAdmin)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(ProxyUser::IsActive)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(ColumnDef::new(ProxyUser::Email).string().null())
                    .col(ColumnDef::new(ProxyUser::DisplayName).string().null())
                    .col(ColumnDef::new(ProxyUser::LastLoginAt).timestamp().null())
                    .col(
                        ColumnDef::new(ProxyUser::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(ProxyUser::UpdatedAt)
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
            .drop_table(Table::drop().table(ProxyUser::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum ProxyUser {
    Table,
    Id,
    Username,
    PasswordHash,
    Tenant,
    IsAdmin,
    IsActive,
    Email,
    DisplayName,
    LastLoginAt,
    CreatedAt,
    UpdatedAt,
}
