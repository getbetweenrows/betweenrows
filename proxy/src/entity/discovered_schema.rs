use sea_orm::entity::prelude::*;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "discovered_schema")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub data_source_id: Uuid,
    pub schema_name: String,
    pub is_selected: bool,
    pub discovered_at: DateTime,
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
    #[sea_orm(has_many = "super::discovered_table::Entity")]
    DiscoveredTable,
}

impl Related<super::data_source::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::DataSource.def()
    }
}

impl Related<super::discovered_table::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::DiscoveredTable.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
