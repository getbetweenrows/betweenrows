use sea_orm::entity::prelude::*;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "policy_version")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub policy_id: Uuid,
    pub version: i32,
    /// Full JSON snapshot: {name, effect, obligations: [...], assignments: [...]}
    pub snapshot: String,
    /// "create" | "update" | "delete" | "obligation_change" | "assignment_change"
    pub change_type: String,
    pub changed_by: Uuid,
    pub created_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::policy::Entity",
        from = "Column::PolicyId",
        to = "super::policy::Column::Id"
    )]
    Policy,
}

impl Related<super::policy::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Policy.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
