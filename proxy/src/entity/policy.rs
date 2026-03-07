use sea_orm::entity::prelude::*;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "policy")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(unique)]
    pub name: String,
    pub description: Option<String>,
    /// "permit" or "deny"
    pub effect: String,
    pub is_enabled: bool,
    pub version: i32,
    pub created_by: Uuid,
    pub updated_by: Uuid,
    pub created_at: DateTime,
    pub updated_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::policy_version::Entity")]
    PolicyVersion,
    #[sea_orm(has_many = "super::policy_obligation::Entity")]
    PolicyObligation,
    #[sea_orm(has_many = "super::policy_assignment::Entity")]
    PolicyAssignment,
}

impl Related<super::policy_version::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PolicyVersion.def()
    }
}

impl Related<super::policy_obligation::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PolicyObligation.def()
    }
}

impl Related<super::policy_assignment::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PolicyAssignment.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
