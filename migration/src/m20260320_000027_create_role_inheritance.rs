use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(RoleInheritance::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(RoleInheritance::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(RoleInheritance::ParentRoleId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(RoleInheritance::ChildRoleId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(RoleInheritance::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(RoleInheritance::Table, RoleInheritance::ParentRoleId)
                            .to(Role::Table, Role::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(RoleInheritance::Table, RoleInheritance::ChildRoleId)
                            .to(Role::Table, Role::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(RoleInheritance::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum RoleInheritance {
    Table,
    Id,
    ParentRoleId,
    ChildRoleId,
    CreatedAt,
}

#[derive(Iden)]
enum Role {
    Table,
    Id,
}
