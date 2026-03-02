use sea_orm::entity::prelude::*;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "discovered_column")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub discovered_table_id: Uuid,
    pub column_name: String,
    pub ordinal_position: i32,
    pub data_type: String,
    pub is_nullable: bool,
    pub column_default: Option<String>,
    /// Mapped DataFusion/Arrow type string (e.g. "Utf8", "Int32"). None = unsupported type.
    pub arrow_type: Option<String>,
    pub is_selected: bool,
    pub discovered_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::discovered_table::Entity",
        from = "Column::DiscoveredTableId",
        to = "super::discovered_table::Column::Id",
        on_delete = "Cascade"
    )]
    DiscoveredTable,
}

impl Related<super::discovered_table::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::DiscoveredTable.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
