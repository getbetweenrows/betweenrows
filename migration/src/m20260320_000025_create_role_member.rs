use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(RoleMember::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(RoleMember::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(RoleMember::RoleId).uuid().not_null())
                    .col(ColumnDef::new(RoleMember::UserId).uuid().not_null())
                    .col(
                        ColumnDef::new(RoleMember::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(RoleMember::Table, RoleMember::RoleId)
                            .to(Role::Table, Role::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(RoleMember::Table, RoleMember::UserId)
                            .to(ProxyUser::Table, ProxyUser::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(RoleMember::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum RoleMember {
    Table,
    Id,
    RoleId,
    UserId,
    CreatedAt,
}

#[derive(Iden)]
enum Role {
    Table,
    Id,
}

#[derive(Iden)]
enum ProxyUser {
    Table,
    Id,
}
