use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, ModelTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, Set,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::entity::{
    data_source, data_source_access, policy, policy_assignment, proxy_user, role, role_inheritance,
    role_member,
};
use crate::role_resolver;

use super::{
    AdminState, ApiErr,
    admin_audit::{AuditAction, AuditedTxn},
    jwt::AdminClaims,
};

// ---------- request types ----------

#[derive(Debug, Deserialize)]
pub struct CreateRoleRequest {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRoleRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct AddMembersRequest {
    pub user_ids: Vec<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct AddParentRequest {
    pub parent_role_id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct ListRolesQuery {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub search: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SetRoleAccessRequest {
    pub role_ids: Vec<Uuid>,
}

// ---------- response types ----------

#[derive(Debug, Serialize)]
pub struct RoleListResponse {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub is_active: bool,
    pub direct_member_count: usize,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

#[derive(Debug, Serialize)]
pub struct RoleDetailResponse {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub is_active: bool,
    pub direct_member_count: usize,
    pub effective_member_count: usize,
    pub members: Vec<RoleMemberResponse>,
    pub parent_roles: Vec<RoleRefResponse>,
    pub child_roles: Vec<RoleRefResponse>,
    pub policy_assignments: Vec<RolePolicyAssignmentResponse>,
    pub datasource_access: Vec<RoleDatasourceAccessEntry>,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

#[derive(Debug, Serialize)]
pub struct RoleDatasourceAccessEntry {
    pub datasource_id: Uuid,
    pub datasource_name: String,
    pub source: String,
}

#[derive(Debug, Serialize)]
pub struct RoleMemberResponse {
    pub id: Uuid,
    pub username: String,
}

#[derive(Debug, Serialize)]
pub struct RoleRefResponse {
    pub id: Uuid,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct RolePolicyAssignmentResponse {
    pub policy_name: String,
    pub datasource_name: String,
    pub source: String,
    pub priority: i32,
}

#[derive(Debug, Serialize)]
pub struct ImpactResponse {
    pub affected_users: usize,
    pub affected_assignments: usize,
}

use super::dto::PaginatedResponse;

// ---------- validation ----------

fn validate_role_name(name: &str) -> Result<(), &'static str> {
    if name.len() < 3 || name.len() > 50 {
        return Err("Role name must be between 3 and 50 characters");
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return Err("Role name must start with a letter"),
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')) {
        return Err("Role name may only contain letters, digits, underscores, dots, and hyphens");
    }
    Ok(())
}

// ---------- POST /roles ----------

pub async fn create_role(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Json(body): Json<CreateRoleRequest>,
) -> Result<(StatusCode, Json<RoleListResponse>), ApiErr> {
    validate_role_name(&body.name).map_err(|e| ApiErr::new(StatusCode::UNPROCESSABLE_ENTITY, e))?;

    let now = Utc::now().naive_utc();
    let role_id = Uuid::now_v7();

    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let model = role::ActiveModel {
        id: Set(role_id),
        name: Set(body.name.clone()),
        description: Set(body.description.clone()),
        is_active: Set(true),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&*txn)
    .await
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("UNIQUE") || msg.contains("unique") {
            ApiErr::conflict("Role name already exists")
        } else {
            ApiErr::internal(e)
        }
    })?;

    txn.audit(
        "role",
        role_id,
        AuditAction::Create,
        claims.sub,
        serde_json::json!({ "after": { "name": body.name, "description": body.description, "is_active": true } }),
    );

    txn.commit().await.map_err(ApiErr::internal)?;

    Ok((
        StatusCode::CREATED,
        Json(RoleListResponse {
            id: model.id,
            name: model.name,
            description: model.description,
            is_active: model.is_active,
            direct_member_count: 0,
            created_at: model.created_at,
            updated_at: model.updated_at,
        }),
    ))
}

// ---------- GET /roles ----------

pub async fn list_roles(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Query(params): Query<ListRolesQuery>,
) -> Result<Json<PaginatedResponse<RoleListResponse>>, ApiErr> {
    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(20).min(100);

    let mut query = role::Entity::find();
    if let Some(ref search) = params.search
        && !search.is_empty()
    {
        query = query.filter(role::Column::Name.contains(search.as_str()));
    }

    let paginator = query
        .order_by_asc(role::Column::CreatedAt)
        .paginate(&state.db, page_size);

    let total = paginator.num_items().await.map_err(ApiErr::internal)?;
    let items = paginator
        .fetch_page(page - 1)
        .await
        .map_err(ApiErr::internal)?;

    // Batch query: count direct members per role for this page's roles only
    let role_ids: Vec<Uuid> = items.iter().map(|r| r.id).collect();
    let count_map: HashMap<Uuid, usize> = if !role_ids.is_empty() {
        let member_counts: Vec<(Uuid, i64)> = role_member::Entity::find()
            .select_only()
            .column(role_member::Column::RoleId)
            .column_as(role_member::Column::UserId.count(), "count")
            .filter(role_member::Column::RoleId.is_in(role_ids))
            .group_by(role_member::Column::RoleId)
            .into_tuple()
            .all(&state.db)
            .await
            .map_err(ApiErr::internal)?;
        member_counts
            .into_iter()
            .map(|(id, c)| (id, c as usize))
            .collect()
    } else {
        HashMap::new()
    };

    let data: Vec<RoleListResponse> = items
        .into_iter()
        .map(|r| {
            let direct_count = count_map.get(&r.id).copied().unwrap_or(0);
            RoleListResponse {
                id: r.id,
                name: r.name,
                description: r.description,
                is_active: r.is_active,
                direct_member_count: direct_count,
                created_at: r.created_at,
                updated_at: r.updated_at,
            }
        })
        .collect();

    Ok(Json(PaginatedResponse {
        data,
        total,
        page,
        page_size,
    }))
}

// ---------- GET /roles/{id} ----------

pub async fn get_role(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
) -> Result<Json<RoleDetailResponse>, ApiErr> {
    let r = role::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Role not found"))?;

    // Members
    let members = role_member::Entity::find()
        .filter(role_member::Column::RoleId.eq(id))
        .all(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let user_ids: Vec<Uuid> = members.iter().map(|m| m.user_id).collect();
    let users: HashMap<Uuid, String> = if !user_ids.is_empty() {
        proxy_user::Entity::find()
            .filter(proxy_user::Column::Id.is_in(user_ids))
            .all(&state.db)
            .await
            .map_err(ApiErr::internal)?
            .into_iter()
            .map(|u| (u.id, u.username))
            .collect()
    } else {
        HashMap::new()
    };

    let member_responses: Vec<RoleMemberResponse> = members
        .iter()
        .map(|m| RoleMemberResponse {
            id: m.user_id,
            username: users.get(&m.user_id).cloned().unwrap_or_default(),
        })
        .collect();

    // Parent roles
    let parent_edges = role_inheritance::Entity::find()
        .filter(role_inheritance::Column::ChildRoleId.eq(id))
        .all(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let parent_ids: Vec<Uuid> = parent_edges.iter().map(|e| e.parent_role_id).collect();
    let parent_roles: Vec<RoleRefResponse> = if !parent_ids.is_empty() {
        role::Entity::find()
            .filter(role::Column::Id.is_in(parent_ids.clone()))
            .all(&state.db)
            .await
            .map_err(ApiErr::internal)?
            .into_iter()
            .map(|r| RoleRefResponse {
                id: r.id,
                name: r.name,
            })
            .collect()
    } else {
        vec![]
    };

    // Child roles
    let child_edges = role_inheritance::Entity::find()
        .filter(role_inheritance::Column::ParentRoleId.eq(id))
        .all(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let child_ids: Vec<Uuid> = child_edges.iter().map(|e| e.child_role_id).collect();
    let child_roles = if !child_ids.is_empty() {
        role::Entity::find()
            .filter(role::Column::Id.is_in(child_ids))
            .all(&state.db)
            .await
            .map_err(ApiErr::internal)?
            .into_iter()
            .map(|r| RoleRefResponse {
                id: r.id,
                name: r.name,
            })
            .collect()
    } else {
        vec![]
    };

    // Policy assignments (direct on this role)
    let role_assignments = policy_assignment::Entity::find()
        .filter(policy_assignment::Column::RoleId.eq(id))
        .filter(policy_assignment::Column::AssignmentScope.eq("role"))
        .all(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let policy_ids: Vec<Uuid> = role_assignments.iter().map(|a| a.policy_id).collect();
    let ds_ids: Vec<Uuid> = role_assignments.iter().map(|a| a.data_source_id).collect();

    let policy_names: HashMap<Uuid, String> = if !policy_ids.is_empty() {
        policy::Entity::find()
            .filter(policy::Column::Id.is_in(policy_ids.clone()))
            .all(&state.db)
            .await
            .map_err(ApiErr::internal)?
            .into_iter()
            .map(|p| (p.id, p.name))
            .collect()
    } else {
        HashMap::new()
    };

    let ds_names: HashMap<Uuid, String> = if !ds_ids.is_empty() {
        data_source::Entity::find()
            .filter(data_source::Column::Id.is_in(ds_ids.clone()))
            .all(&state.db)
            .await
            .map_err(ApiErr::internal)?
            .into_iter()
            .map(|d| (d.id, d.name))
            .collect()
    } else {
        HashMap::new()
    };

    let mut policy_responses: Vec<RolePolicyAssignmentResponse> = role_assignments
        .iter()
        .map(|a| RolePolicyAssignmentResponse {
            policy_name: policy_names.get(&a.policy_id).cloned().unwrap_or_default(),
            datasource_name: ds_names.get(&a.data_source_id).cloned().unwrap_or_default(),
            source: "direct".to_string(),
            priority: a.priority,
        })
        .collect();

    // Also load inherited assignments from all ancestor roles (not just direct parents)
    let ancestor_ids = role_resolver::resolve_ancestor_roles(&state.db, id)
        .await
        .map_err(ApiErr::internal)?;

    // Load ancestor role names (used for both policy and datasource inheritance annotation)
    let ancestor_role_names: HashMap<Uuid, String> = if !ancestor_ids.is_empty() {
        role::Entity::find()
            .filter(role::Column::Id.is_in(ancestor_ids.clone()))
            .all(&state.db)
            .await
            .map_err(ApiErr::internal)?
            .into_iter()
            .map(|r| (r.id, r.name))
            .collect()
    } else {
        HashMap::new()
    };

    if !ancestor_ids.is_empty() {
        let inherited = policy_assignment::Entity::find()
            .filter(policy_assignment::Column::RoleId.is_in(ancestor_ids.clone()))
            .filter(policy_assignment::Column::AssignmentScope.eq("role"))
            .all(&state.db)
            .await
            .map_err(ApiErr::internal)?;

        let extra_policy_ids: Vec<Uuid> = inherited.iter().map(|a| a.policy_id).collect();
        let extra_ds_ids: Vec<Uuid> = inherited.iter().map(|a| a.data_source_id).collect();

        let extra_policies: HashMap<Uuid, String> = if !extra_policy_ids.is_empty() {
            policy::Entity::find()
                .filter(policy::Column::Id.is_in(extra_policy_ids))
                .all(&state.db)
                .await
                .map_err(ApiErr::internal)?
                .into_iter()
                .map(|p| (p.id, p.name))
                .collect()
        } else {
            HashMap::new()
        };

        let extra_ds: HashMap<Uuid, String> = if !extra_ds_ids.is_empty() {
            data_source::Entity::find()
                .filter(data_source::Column::Id.is_in(extra_ds_ids))
                .all(&state.db)
                .await
                .map_err(ApiErr::internal)?
                .into_iter()
                .map(|d| (d.id, d.name))
                .collect()
        } else {
            HashMap::new()
        };

        for a in &inherited {
            if let Some(rid) = a.role_id {
                let role_name = ancestor_role_names.get(&rid).cloned().unwrap_or_default();
                policy_responses.push(RolePolicyAssignmentResponse {
                    policy_name: extra_policies
                        .get(&a.policy_id)
                        .or_else(|| policy_names.get(&a.policy_id))
                        .cloned()
                        .unwrap_or_default(),
                    datasource_name: extra_ds
                        .get(&a.data_source_id)
                        .or_else(|| ds_names.get(&a.data_source_id))
                        .cloned()
                        .unwrap_or_default(),
                    source: format!("inherited from '{role_name}'"),
                    priority: a.priority,
                });
            }
        }
    }

    // Deduplicate by (policy_name, datasource_name), preferring "direct" over inherited
    {
        let mut seen: HashMap<(String, String), usize> = HashMap::new();
        let mut deduped: Vec<RolePolicyAssignmentResponse> = Vec::new();
        for pa in policy_responses {
            let key = (pa.policy_name.clone(), pa.datasource_name.clone());
            if let Some(&idx) = seen.get(&key) {
                // Keep direct over inherited
                if pa.source == "direct" {
                    deduped[idx] = pa;
                }
            } else {
                seen.insert(key, deduped.len());
                deduped.push(pa);
            }
        }
        policy_responses = deduped;
    }

    let effective_members = role_resolver::resolve_all_role_members(&state.db, id)
        .await
        .map_err(ApiErr::internal)?;

    // Datasource access — direct
    let direct_ds_access = data_source_access::Entity::find()
        .filter(data_source_access::Column::AssignmentScope.eq("role"))
        .filter(data_source_access::Column::RoleId.eq(id))
        .all(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let direct_ds_ids: Vec<Uuid> = direct_ds_access.iter().map(|a| a.data_source_id).collect();

    let mut ds_access_entries: Vec<RoleDatasourceAccessEntry> = Vec::new();

    if !direct_ds_ids.is_empty() {
        let ds_map: HashMap<Uuid, String> = data_source::Entity::find()
            .filter(data_source::Column::Id.is_in(direct_ds_ids.clone()))
            .all(&state.db)
            .await
            .map_err(ApiErr::internal)?
            .into_iter()
            .map(|d| (d.id, d.name))
            .collect();

        for a in &direct_ds_access {
            ds_access_entries.push(RoleDatasourceAccessEntry {
                datasource_id: a.data_source_id,
                datasource_name: ds_map.get(&a.data_source_id).cloned().unwrap_or_default(),
                source: "direct".to_string(),
            });
        }
    }

    // Datasource access — inherited from ancestors
    if !ancestor_ids.is_empty() {
        let inherited_ds_access = data_source_access::Entity::find()
            .filter(data_source_access::Column::AssignmentScope.eq("role"))
            .filter(data_source_access::Column::RoleId.is_in(ancestor_ids.clone()))
            .all(&state.db)
            .await
            .map_err(ApiErr::internal)?;

        if !inherited_ds_access.is_empty() {
            let inherited_ds_ids: Vec<Uuid> = inherited_ds_access
                .iter()
                .map(|a| a.data_source_id)
                .collect();
            let inherited_ds_map: HashMap<Uuid, String> = data_source::Entity::find()
                .filter(data_source::Column::Id.is_in(inherited_ds_ids))
                .all(&state.db)
                .await
                .map_err(ApiErr::internal)?
                .into_iter()
                .map(|d| (d.id, d.name))
                .collect();

            // Reuse ancestor_role_names already loaded above
            for a in &inherited_ds_access {
                if let Some(rid) = a.role_id {
                    // Skip if already listed as direct
                    if direct_ds_ids.contains(&a.data_source_id) {
                        continue;
                    }
                    let role_name = ancestor_role_names.get(&rid).cloned().unwrap_or_default();
                    ds_access_entries.push(RoleDatasourceAccessEntry {
                        datasource_id: a.data_source_id,
                        datasource_name: inherited_ds_map
                            .get(&a.data_source_id)
                            .cloned()
                            .unwrap_or_default(),
                        source: format!("inherited from '{role_name}'"),
                    });
                }
            }
        }
    }

    Ok(Json(RoleDetailResponse {
        id: r.id,
        name: r.name,
        description: r.description,
        is_active: r.is_active,
        direct_member_count: member_responses.len(),
        effective_member_count: effective_members.len(),
        members: member_responses,
        parent_roles,
        child_roles,
        policy_assignments: policy_responses,
        datasource_access: ds_access_entries,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }))
}

// ---------- GET /roles/{id}/effective-members ----------

pub async fn get_effective_members(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<role_resolver::EffectiveMemberEntry>>, ApiErr> {
    role::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Role not found"))?;

    let members = role_resolver::resolve_effective_members(&state.db, id)
        .await
        .map_err(ApiErr::internal)?;

    Ok(Json(members))
}

// ---------- PUT /roles/{id} ----------

pub async fn update_role(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateRoleRequest>,
) -> Result<Json<RoleListResponse>, ApiErr> {
    let r = role::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Role not found"))?;

    let mut active: role::ActiveModel = r.clone().into();
    let mut changes_before = serde_json::Map::new();
    let mut changes_after = serde_json::Map::new();

    if let Some(ref name) = body.name {
        validate_role_name(name).map_err(|e| ApiErr::new(StatusCode::UNPROCESSABLE_ENTITY, e))?;
        changes_before.insert("name".to_string(), serde_json::json!(r.name));
        changes_after.insert("name".to_string(), serde_json::json!(name));
        active.name = Set(name.clone());
    }
    if let Some(ref desc) = body.description {
        changes_before.insert("description".to_string(), serde_json::json!(r.description));
        changes_after.insert("description".to_string(), serde_json::json!(desc));
        active.description = Set(Some(desc.clone()));
    }

    let mut is_activation_change = false;
    if let Some(is_active) = body.is_active
        && is_active != r.is_active
    {
        is_activation_change = true;
        changes_before.insert("is_active".to_string(), serde_json::json!(r.is_active));
        changes_after.insert("is_active".to_string(), serde_json::json!(is_active));
        active.is_active = Set(is_active);
    }

    active.updated_at = Set(Utc::now().naive_utc());

    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let updated = active.update(&*txn).await.map_err(|e| {
        let msg = e.to_string();
        if msg.contains("UNIQUE") || msg.contains("unique") {
            ApiErr::conflict("Role name already exists")
        } else {
            ApiErr::internal(e)
        }
    })?;

    let action = if is_activation_change && !updated.is_active {
        AuditAction::Deactivate
    } else if is_activation_change && updated.is_active {
        AuditAction::Reactivate
    } else {
        AuditAction::Update
    };

    txn.audit(
        "role",
        id,
        action,
        claims.sub,
        serde_json::json!({ "before": changes_before, "after": changes_after }),
    );

    txn.commit().await.map_err(ApiErr::internal)?;

    if is_activation_change {
        invalidate_role_members(&state, id).await;
    }

    let direct_count = role_member::Entity::find()
        .filter(role_member::Column::RoleId.eq(id))
        .count(&state.db)
        .await
        .map_err(ApiErr::internal)? as usize;

    Ok(Json(RoleListResponse {
        id: updated.id,
        name: updated.name,
        description: updated.description,
        is_active: updated.is_active,
        direct_member_count: direct_count,
        created_at: updated.created_at,
        updated_at: updated.updated_at,
    }))
}

// ---------- DELETE /roles/{id} ----------

pub async fn delete_role(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ImpactResponse>, ApiErr> {
    let r = role::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Role not found"))?;

    let before_json = serde_json::json!({ "name": r.name, "description": r.description, "is_active": r.is_active });

    let affected_users = role_resolver::resolve_all_role_members(&state.db, id)
        .await
        .map_err(ApiErr::internal)?;

    let affected_assignment_count = policy_assignment::Entity::find()
        .filter(policy_assignment::Column::RoleId.eq(id))
        .count(&state.db)
        .await
        .map_err(ApiErr::internal)? as usize;

    // Invalidate before deletion so we still have the data to find affected users
    invalidate_role_members(&state, id).await;

    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    // Audit each cascaded policy assignment before they're deleted
    let cascaded_assignments = policy_assignment::Entity::find()
        .filter(policy_assignment::Column::RoleId.eq(id))
        .all(&*txn)
        .await
        .map_err(ApiErr::internal)?;
    for a in &cascaded_assignments {
        txn.audit(
            "policy",
            a.policy_id,
            AuditAction::Unassign,
            claims.sub,
            serde_json::json!({
                "reason": "role_deleted",
                "role_id": id.to_string(),
                "datasource_id": a.data_source_id.to_string(),
                "assignment_id": a.id.to_string(),
            }),
        );
    }

    // Delete the role (cascades via FK to members, inheritance, assignments, access)
    r.delete(&*txn).await.map_err(ApiErr::internal)?;

    txn.audit(
        "role",
        id,
        AuditAction::Delete,
        claims.sub,
        serde_json::json!({ "before": before_json }),
    );

    txn.commit().await.map_err(ApiErr::internal)?;

    Ok(Json(ImpactResponse {
        affected_users: affected_users.len(),
        affected_assignments: affected_assignment_count,
    }))
}

// ---------- GET /roles/{id}/impact ----------

pub async fn get_role_impact(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ImpactResponse>, ApiErr> {
    role::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Role not found"))?;

    let affected_users = role_resolver::resolve_all_role_members(&state.db, id)
        .await
        .map_err(ApiErr::internal)?;

    let affected_assignments = policy_assignment::Entity::find()
        .filter(policy_assignment::Column::RoleId.eq(id))
        .count(&state.db)
        .await
        .map_err(ApiErr::internal)? as usize;

    Ok(Json(ImpactResponse {
        affected_users: affected_users.len(),
        affected_assignments,
    }))
}

// ---------- POST /roles/{id}/members ----------

pub async fn add_members(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
    Json(body): Json<AddMembersRequest>,
) -> Result<StatusCode, ApiErr> {
    let r = role::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Role not found"))?;

    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;
    let now = Utc::now().naive_utc();

    for user_id in &body.user_ids {
        let user = proxy_user::Entity::find_by_id(*user_id)
            .one(&*txn)
            .await
            .map_err(ApiErr::internal)?
            .ok_or_else(|| ApiErr::not_found(format!("User '{user_id}' not found")))?;

        role_member::ActiveModel {
            id: Set(Uuid::now_v7()),
            role_id: Set(id),
            user_id: Set(*user_id),
            created_at: Set(now),
        }
        .insert(&*txn)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("UNIQUE") || msg.contains("unique") {
                ApiErr::conflict(format!(
                    "User '{}' is already a member of role '{}'",
                    user.username, r.name
                ))
            } else {
                ApiErr::internal(e)
            }
        })?;

        txn.audit(
            "role",
            id,
            AuditAction::AddMember,
            claims.sub,
            serde_json::json!({ "user_id": user_id.to_string(), "username": user.username }),
        );
    }

    txn.commit().await.map_err(ApiErr::internal)?;

    for user_id in &body.user_ids {
        invalidate_user(&state, *user_id).await;
    }

    Ok(StatusCode::NO_CONTENT)
}

// ---------- DELETE /roles/{id}/members/{user_id} ----------

pub async fn remove_member(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Path((id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiErr> {
    role::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Role not found"))?;

    let membership = role_member::Entity::find()
        .filter(role_member::Column::RoleId.eq(id))
        .filter(role_member::Column::UserId.eq(user_id))
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("User is not a member of this role"))?;

    let username = proxy_user::Entity::find_by_id(user_id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .map(|u| u.username)
        .unwrap_or_default();

    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    membership
        .into_active_model()
        .delete(&*txn)
        .await
        .map_err(ApiErr::internal)?;

    txn.audit(
        "role",
        id,
        AuditAction::RemoveMember,
        claims.sub,
        serde_json::json!({ "before": { "user_id": user_id.to_string(), "username": username } }),
    );

    txn.commit().await.map_err(ApiErr::internal)?;

    invalidate_user(&state, user_id).await;

    Ok(StatusCode::NO_CONTENT)
}

// ---------- POST /roles/{id}/parents ----------

pub async fn add_parent(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
    Json(body): Json<AddParentRequest>,
) -> Result<StatusCode, ApiErr> {
    let _child = role::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Role not found"))?;

    let parent = role::Entity::find_by_id(body.parent_role_id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Parent role not found"))?;

    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let has_cycle = role_resolver::detect_cycle(&*txn, body.parent_role_id, id)
        .await
        .map_err(ApiErr::internal)?;

    if has_cycle {
        let path = role_resolver::build_cycle_path(&*txn, body.parent_role_id, id)
            .await
            .map_err(ApiErr::internal)?;
        return Err(ApiErr::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("Cannot add inheritance: would create cycle: {path}"),
        ));
    }

    // Depth check — account for both ancestors above parent and descendants below child
    let depth_above = role_resolver::check_inheritance_depth(&*txn, body.parent_role_id)
        .await
        .map_err(ApiErr::internal)?;
    let depth_below = role_resolver::check_inheritance_depth_down(&*txn, id)
        .await
        .map_err(ApiErr::internal)?;
    let total_depth = depth_above + 1 + depth_below;

    if total_depth > 10 {
        return Err(ApiErr::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!(
                "Cannot add inheritance: maximum depth of 10 would be exceeded (total depth: {total_depth})"
            ),
        ));
    }

    role_inheritance::ActiveModel {
        id: Set(Uuid::now_v7()),
        parent_role_id: Set(body.parent_role_id),
        child_role_id: Set(id),
        created_at: Set(Utc::now().naive_utc()),
    }
    .insert(&*txn)
    .await
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("UNIQUE") || msg.contains("unique") {
            ApiErr::conflict("This inheritance relationship already exists")
        } else {
            ApiErr::internal(e)
        }
    })?;

    txn.audit(
        "role",
        id,
        AuditAction::AddInheritance,
        claims.sub,
        serde_json::json!({
            "parent_role_id": body.parent_role_id.to_string(),
            "parent_role_name": parent.name,
        }),
    );

    txn.commit().await.map_err(ApiErr::internal)?;

    invalidate_role_members(&state, id).await;

    Ok(StatusCode::NO_CONTENT)
}

// ---------- DELETE /roles/{id}/parents/{parent_id} ----------

pub async fn remove_parent(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Path((id, parent_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiErr> {
    role::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Role not found"))?;

    let edge = role_inheritance::Entity::find()
        .filter(role_inheritance::Column::ChildRoleId.eq(id))
        .filter(role_inheritance::Column::ParentRoleId.eq(parent_id))
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Inheritance relationship not found"))?;

    let parent_name = role::Entity::find_by_id(parent_id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .map(|r| r.name)
        .unwrap_or_default();

    // Collect affected user IDs before the edge is removed (while we can still traverse)
    let affected_user_ids = role_resolver::resolve_all_role_members(&state.db, id)
        .await
        .unwrap_or_default();

    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    edge.into_active_model()
        .delete(&*txn)
        .await
        .map_err(ApiErr::internal)?;

    txn.audit(
        "role",
        id,
        AuditAction::RemoveInheritance,
        claims.sub,
        serde_json::json!({
            "before": {
                "parent_role_id": parent_id.to_string(),
                "parent_role_name": parent_name,
            }
        }),
    );

    txn.commit().await.map_err(ApiErr::internal)?;

    // Invalidate AFTER txn commits (#4 fix) using pre-collected user IDs
    for user_id in affected_user_ids {
        invalidate_user(&state, user_id).await;
    }

    Ok(StatusCode::NO_CONTENT)
}

// ---------- GET /datasources/{id}/access/roles ----------

#[derive(Debug, Serialize)]
pub struct DatasourceRoleAccessEntry {
    pub id: Uuid,
    pub name: String,
    pub is_active: bool,
}

pub async fn get_datasource_role_access(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<DatasourceRoleAccessEntry>>, ApiErr> {
    data_source::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Data source not found"))?;

    let entries = data_source_access::Entity::find()
        .filter(data_source_access::Column::DataSourceId.eq(id))
        .filter(data_source_access::Column::AssignmentScope.eq("role"))
        .all(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let role_ids: Vec<Uuid> = entries.iter().filter_map(|e| e.role_id).collect();
    if role_ids.is_empty() {
        return Ok(Json(vec![]));
    }

    let roles: Vec<DatasourceRoleAccessEntry> = role::Entity::find()
        .filter(role::Column::Id.is_in(role_ids))
        .all(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .into_iter()
        .map(|r| DatasourceRoleAccessEntry {
            id: r.id,
            name: r.name,
            is_active: r.is_active,
        })
        .collect();

    Ok(Json(roles))
}

// ---------- PUT /datasources/{id}/access/roles ----------

pub async fn set_datasource_role_access(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
    Json(body): Json<SetRoleAccessRequest>,
) -> Result<StatusCode, ApiErr> {
    data_source::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Data source not found"))?;

    // Validate all roles BEFORE making any mutations (#2 fix)
    for role_id in &body.role_ids {
        let found_role = role::Entity::find_by_id(*role_id)
            .one(&state.db)
            .await
            .map_err(ApiErr::internal)?
            .ok_or_else(|| ApiErr::not_found(format!("Role '{role_id}' not found")))?;

        if !found_role.is_active {
            return Err(ApiErr::new(
                StatusCode::BAD_REQUEST,
                format!("Role '{}' is inactive", found_role.name),
            ));
        }
    }

    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    // Capture old role-scoped entries before deleting
    let old_entries = data_source_access::Entity::find()
        .filter(data_source_access::Column::DataSourceId.eq(id))
        .filter(data_source_access::Column::AssignmentScope.eq("role"))
        .all(&*txn)
        .await
        .map_err(ApiErr::internal)?;
    let old_role_ids: HashSet<Uuid> = old_entries.iter().filter_map(|e| e.role_id).collect();

    data_source_access::Entity::delete_many()
        .filter(data_source_access::Column::DataSourceId.eq(id))
        .filter(data_source_access::Column::AssignmentScope.eq("role"))
        .exec(&*txn)
        .await
        .map_err(ApiErr::internal)?;

    let now = Utc::now().naive_utc();
    for role_id in &body.role_ids {
        data_source_access::ActiveModel {
            id: Set(Uuid::now_v7()),
            user_id: Set(None),
            role_id: Set(Some(*role_id)),
            data_source_id: Set(id),
            assignment_scope: Set("role".to_string()),
            created_at: Set(now),
        }
        .insert(&*txn)
        .await
        .map_err(ApiErr::internal)?;
    }

    let new_role_ids: HashSet<Uuid> = body.role_ids.iter().copied().collect();
    let old_ids_json: Vec<String> = old_role_ids.iter().map(|id| id.to_string()).collect();
    let new_ids_json: Vec<String> = new_role_ids.iter().map(|id| id.to_string()).collect();
    txn.audit(
        "datasource",
        id,
        AuditAction::Update,
        claims.sub,
        serde_json::json!({
            "field": "role_access",
            "before": old_ids_json,
            "after": new_ids_json,
        }),
    );

    txn.commit().await.map_err(ApiErr::internal)?;

    // Invalidate all affected roles (both added and removed)
    let all_affected: HashSet<Uuid> = old_role_ids.union(&new_role_ids).copied().collect();
    for role_id in all_affected {
        let members = role_resolver::resolve_all_role_members(&state.db, role_id)
            .await
            .unwrap_or_default();
        for user_id in members {
            invalidate_user(&state, user_id).await;
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

// ---------- cache invalidation helpers ----------

pub(super) async fn invalidate_user(state: &AdminState, user_id: Uuid) {
    if let Some(policy_hook) = &state.policy_hook {
        policy_hook.invalidate_user(user_id).await;
    }
    if let Some(proxy_handler) = &state.proxy_handler {
        proxy_handler.rebuild_contexts_for_user(user_id);
    }
}

async fn invalidate_role_members(state: &AdminState, role_id: Uuid) {
    let members = role_resolver::resolve_all_role_members(&state.db, role_id)
        .await
        .unwrap_or_default();
    for user_id in members {
        invalidate_user(state, user_id).await;
    }
}
