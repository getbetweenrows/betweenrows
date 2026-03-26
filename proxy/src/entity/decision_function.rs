use sea_orm::entity::prelude::*;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "decision_function")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(unique)]
    pub name: String,
    pub description: Option<String>,
    pub language: String,
    pub decision_fn: String,
    #[sea_orm(column_type = "VarBinary(StringLen::None)")]
    pub decision_wasm: Option<Vec<u8>>,
    pub decision_config: Option<String>,
    pub evaluate_context: String,
    pub on_error: String,
    pub log_level: String,
    pub is_enabled: bool,
    pub version: i32,
    pub created_by: Uuid,
    pub updated_by: Uuid,
    pub created_at: DateTime,
    pub updated_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::policy::Entity")]
    Policy,
}

impl Related<super::policy::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Policy.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
