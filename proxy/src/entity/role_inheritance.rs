use sea_orm::entity::prelude::*;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "role_inheritance")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub parent_role_id: Uuid,
    pub child_role_id: Uuid,
    pub created_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::role::Entity",
        from = "Column::ParentRoleId",
        to = "super::role::Column::Id",
        on_delete = "Cascade"
    )]
    ParentRole,
    #[sea_orm(
        belongs_to = "super::role::Entity",
        from = "Column::ChildRoleId",
        to = "super::role::Column::Id",
        on_delete = "Cascade"
    )]
    ChildRole,
}

impl ActiveModelBehavior for ActiveModel {}
