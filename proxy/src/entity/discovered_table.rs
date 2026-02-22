use sea_orm::entity::prelude::*;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "discovered_table")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub discovered_schema_id: Uuid,
    pub table_name: String,
    pub table_type: String,
    pub is_selected: bool,
    pub discovered_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::discovered_schema::Entity",
        from = "Column::DiscoveredSchemaId",
        to = "super::discovered_schema::Column::Id",
        on_delete = "Cascade"
    )]
    DiscoveredSchema,
    #[sea_orm(has_many = "super::discovered_column::Entity")]
    DiscoveredColumn,
}

impl Related<super::discovered_schema::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::DiscoveredSchema.def()
    }
}

impl Related<super::discovered_column::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::DiscoveredColumn.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
