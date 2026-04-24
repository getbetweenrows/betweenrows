use sea_orm::entity::prelude::*;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "column_anchor")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub data_source_id: Uuid,
    pub child_table_id: Uuid,
    pub resolved_column_name: String,
    /// FK walk shape. Exactly one of `relationship_id` or `actual_column_name`
    /// is set per row — XOR enforced at the API layer (see
    /// `create_column_anchor` in `admin/relationship_handlers.rs`).
    pub relationship_id: Option<Uuid>,
    /// Same-table alias shape: when set, the row-filter rewriter substitutes
    /// references to `resolved_column_name` with `actual_column_name` in the
    /// filter expression — no join is built. Exists because one broad
    /// row-filter policy can legitimately cover tables whose tenant-isolation
    /// column is spelled differently (e.g. `tenant_id` vs `org_id`).
    pub actual_column_name: Option<String>,
    pub designated_at: DateTime,
    pub designated_by: Uuid,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::data_source::Entity",
        from = "Column::DataSourceId",
        to = "super::data_source::Column::Id",
        on_delete = "Cascade"
    )]
    DataSource,
    #[sea_orm(
        belongs_to = "super::discovered_table::Entity",
        from = "Column::ChildTableId",
        to = "super::discovered_table::Column::Id",
        on_delete = "Cascade"
    )]
    ChildTable,
    #[sea_orm(
        belongs_to = "super::table_relationship::Entity",
        from = "Column::RelationshipId",
        to = "super::table_relationship::Column::Id",
        on_delete = "Restrict"
    )]
    Relationship,
}

impl Related<super::table_relationship::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Relationship.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
