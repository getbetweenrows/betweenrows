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
    /// "row_filter" | "column_mask" | "column_allow" | "column_deny" | "table_deny"
    pub policy_type: String,
    /// JSON array of TargetEntry (schemas, tables, columns?)
    pub targets: String,
    /// JSON definition (filter_expression or mask_expression). Null for non-expression types.
    pub definition: Option<String>,
    pub is_enabled: bool,
    pub version: i32,
    pub decision_function_id: Option<Uuid>,
    pub created_by: Uuid,
    pub updated_by: Uuid,
    pub created_at: DateTime,
    pub updated_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::policy_version::Entity")]
    PolicyVersion,
    #[sea_orm(has_many = "super::policy_assignment::Entity")]
    PolicyAssignment,
    #[sea_orm(
        belongs_to = "super::decision_function::Entity",
        from = "Column::DecisionFunctionId",
        to = "super::decision_function::Column::Id"
    )]
    DecisionFunction,
}

impl Related<super::policy_version::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PolicyVersion.def()
    }
}

impl Related<super::policy_assignment::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PolicyAssignment.def()
    }
}

impl Related<super::decision_function::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::DecisionFunction.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
