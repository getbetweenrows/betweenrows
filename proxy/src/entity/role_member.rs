use sea_orm::entity::prelude::*;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "role_member")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub role_id: Uuid,
    pub user_id: Uuid,
    pub created_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::role::Entity",
        from = "Column::RoleId",
        to = "super::role::Column::Id",
        on_delete = "Cascade"
    )]
    Role,
    #[sea_orm(
        belongs_to = "super::proxy_user::Entity",
        from = "Column::UserId",
        to = "super::proxy_user::Column::Id",
        on_delete = "Cascade"
    )]
    ProxyUser,
}

impl Related<super::role::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Role.def()
    }
}

impl Related<super::proxy_user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ProxyUser.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
