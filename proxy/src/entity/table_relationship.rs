use sea_orm::entity::prelude::*;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "table_relationship")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub data_source_id: Uuid,
    pub child_table_id: Uuid,
    pub child_column_name: String,
    pub parent_table_id: Uuid,
    pub parent_column_name: String,
    pub created_at: DateTime,
    pub created_by: Option<Uuid>,
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
        belongs_to = "super::discovered_table::Entity",
        from = "Column::ParentTableId",
        to = "super::discovered_table::Column::Id",
        on_delete = "Cascade"
    )]
    ParentTable,
}

impl Related<super::data_source::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::DataSource.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
