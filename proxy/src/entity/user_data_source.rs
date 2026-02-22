use sea_orm::entity::prelude::*;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "user_data_source")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub user_id: Uuid,
    pub data_source_id: Uuid,
    pub created_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::proxy_user::Entity",
        from = "Column::UserId",
        to = "super::proxy_user::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    ProxyUser,
    #[sea_orm(
        belongs_to = "super::data_source::Entity",
        from = "Column::DataSourceId",
        to = "super::data_source::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    DataSource,
}

impl Related<super::proxy_user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ProxyUser.def()
    }
}

impl Related<super::data_source::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::DataSource.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
