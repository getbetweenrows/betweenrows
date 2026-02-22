use sea_orm::entity::prelude::*;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "proxy_user")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(unique)]
    pub username: String,
    pub password_hash: String,
    pub tenant: String,
    pub is_admin: bool,
    pub is_active: bool,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub last_login_at: Option<DateTime>,
    pub created_at: DateTime,
    pub updated_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::user_data_source::Entity")]
    UserDataSource,
}

impl Related<super::user_data_source::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::UserDataSource.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
