use sea_orm::entity::prelude::*;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "policy_assignment")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub policy_id: Uuid,
    pub data_source_id: Uuid,
    /// NULL means applies to all users of this datasource
    pub user_id: Option<Uuid>,
    /// Role-based assignment target
    pub role_id: Option<Uuid>,
    /// "user", "role", or "all"
    pub assignment_scope: String,
    /// Lower = higher precedence
    pub priority: i32,
    pub created_at: DateTime,
    pub updated_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::policy::Entity",
        from = "Column::PolicyId",
        to = "super::policy::Column::Id",
        on_delete = "Cascade"
    )]
    Policy,
    #[sea_orm(
        belongs_to = "super::data_source::Entity",
        from = "Column::DataSourceId",
        to = "super::data_source::Column::Id",
        on_delete = "Cascade"
    )]
    DataSource,
}

impl Related<super::policy::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Policy.def()
    }
}

impl Related<super::data_source::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::DataSource.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
