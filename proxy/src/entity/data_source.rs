use sea_orm::entity::prelude::*;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "data_source")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(unique)]
    pub name: String,
    pub ds_type: String,
    /// JSON text: non-secret connection params (host, port, database, username, sslmode, ...)
    pub config: String,
    /// AES-256-GCM encrypted base64: secret params (password, api keys, ...)
    pub secure_config: String,
    pub is_active: bool,
    pub last_sync_at: Option<DateTime>,
    pub last_sync_result: Option<String>,
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
