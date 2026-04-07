use sea_orm::entity::prelude::*;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "proxy_user")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(unique)]
    pub username: String,
    pub password_hash: String,
    pub is_admin: bool,
    pub is_active: bool,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub last_login_at: Option<DateTime>,
    pub created_at: DateTime,
    pub updated_at: DateTime,
    #[sea_orm(default_value = "{}")]
    pub attributes: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::role_member::Entity")]
    RoleMember,
}

impl Related<super::role_member::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::RoleMember.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

/// Parse the JSON attributes column into a HashMap.
/// Values may be strings (scalar types) or arrays of strings (list type).
pub fn parse_attributes(json_str: &str) -> HashMap<String, serde_json::Value> {
    serde_json::from_str(json_str).unwrap_or_default()
}
