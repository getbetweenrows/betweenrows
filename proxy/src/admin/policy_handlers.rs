use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Set,
};
use std::collections::HashMap;
use uuid::Uuid;

use crate::entity::{
    data_source, decision_function, policy, policy_assignment, policy_version, proxy_user, role,
};
use crate::policy_match::PolicyType;
use crate::role_resolver;

use super::{
    AdminState, ApiErr,
    admin_audit::{AuditAction, AuditedTxn},
    decision_function_handlers::df_summary,
    dto::{
        AssignPolicyRequest, CreatePolicyRequest, DecisionFunctionSummary, ListPoliciesQuery,
        PaginatedResponse, PolicyAssignmentResponse, PolicyResponse, UpdatePolicyRequest,
        validate_definition, validate_policy_name, validate_targets,
    },
    jwt::AdminClaims,
};

// ---------- helpers ----------

fn assignment_response(
    m: &policy_assignment::Model,
    policy_names: &HashMap<Uuid, String>,
    ds_names: &HashMap<Uuid, String>,
    user_names: &HashMap<Uuid, String>,
    role_names: &HashMap<Uuid, String>,
) -> PolicyAssignmentResponse {
    PolicyAssignmentResponse {
        id: m.id,
        policy_id: m.policy_id,
        policy_name: policy_names.get(&m.policy_id).cloned().unwrap_or_default(),
        data_source_id: m.data_source_id,
        datasource_name: ds_names.get(&m.data_source_id).cloned().unwrap_or_default(),
        user_id: m.user_id,
        username: m.user_id.and_then(|uid| user_names.get(&uid).cloned()),
        role_id: m.role_id,
        role_name: m.role_id.and_then(|rid| role_names.get(&rid).cloned()),
        assignment_scope: m.assignment_scope.clone(),
        priority: m.priority,
        created_at: m.created_at,
        updated_at: m.updated_at,
    }
}

async fn fetch_policy_names<C: sea_orm::ConnectionTrait>(
    db: &C,
    ids: Vec<Uuid>,
) -> Result<HashMap<Uuid, String>, ApiErr> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    Ok(policy::Entity::find()
        .filter(policy::Column::Id.is_in(ids))
        .all(db)
        .await
        .map_err(ApiErr::internal)?
        .into_iter()
        .map(|p| (p.id, p.name))
        .collect())
}

async fn fetch_ds_names<C: sea_orm::ConnectionTrait>(
    db: &C,
    ids: Vec<Uuid>,
) -> Result<HashMap<Uuid, String>, ApiErr> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    Ok(data_source::Entity::find()
        .filter(data_source::Column::Id.is_in(ids))
        .all(db)
        .await
        .map_err(ApiErr::internal)?
        .into_iter()
        .map(|d| (d.id, d.name))
        .collect())
}

async fn fetch_user_names<C: sea_orm::ConnectionTrait>(
    db: &C,
    ids: Vec<Uuid>,
) -> Result<HashMap<Uuid, String>, ApiErr> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    Ok(proxy_user::Entity::find()
        .filter(proxy_user::Column::Id.is_in(ids))
        .all(db)
        .await
        .map_err(ApiErr::internal)?
        .into_iter()
        .map(|u| (u.id, u.username))
        .collect())
}

async fn fetch_role_names<C: sea_orm::ConnectionTrait>(
    db: &C,
    ids: Vec<Uuid>,
) -> Result<HashMap<Uuid, String>, ApiErr> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    Ok(role::Entity::find()
        .filter(role::Column::Id.is_in(ids))
        .all(db)
        .await
        .map_err(ApiErr::internal)?
        .into_iter()
        .map(|r| (r.id, r.name))
        .collect())
}

async fn load_decision_function_summaries(
    db: &impl sea_orm::ConnectionTrait,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, DecisionFunctionSummary>, ApiErr> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let dfs = decision_function::Entity::find()
        .filter(decision_function::Column::Id.is_in(ids.to_vec()))
        .all(db)
        .await
        .map_err(ApiErr::internal)?;
    Ok(dfs.iter().map(|df| (df.id, df_summary(df))).collect())
}

fn policy_response_basic(
    p: &policy::Model,
    assignment_count: usize,
    df_summary: Option<DecisionFunctionSummary>,
) -> PolicyResponse {
    let targets: serde_json::Value =
        serde_json::from_str(&p.targets).unwrap_or(serde_json::Value::Array(vec![]));
    let definition: Option<serde_json::Value> = p
        .definition
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());
    PolicyResponse {
        id: p.id,
        name: p.name.clone(),
        description: p.description.clone(),
        policy_type: p.policy_type.clone(),
        targets,
        definition,
        is_enabled: p.is_enabled,
        version: p.version,
        decision_function_id: p.decision_function_id,
        decision_function: df_summary,
        assignment_count,
        created_by: p.created_by,
        updated_by: p.updated_by,
        created_at: p.created_at,
        updated_at: p.updated_at,
        assignments: None,
    }
}

/// Create a policy_version snapshot in the same transaction.
async fn create_snapshot<C: sea_orm::ConnectionTrait>(
    txn: &C,
    policy_id: Uuid,
    version: i32,
    changed_by: Uuid,
    change_type: &str,
    p: &policy::Model,
    assignments: &[policy_assignment::Model],
) -> Result<(), ApiErr> {
    let targets: serde_json::Value =
        serde_json::from_str(&p.targets).unwrap_or(serde_json::Value::Array(vec![]));
    let definition: serde_json::Value = p
        .definition
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(serde_json::Value::Null);

    let snapshot = serde_json::json!({
        "name": p.name,
        "policy_type": p.policy_type,
        "targets": targets,
        "definition": definition,
        "decision_function_id": p.decision_function_id.map(|id| id.to_string()),
        "assignments": assignments.iter().map(|a| {
            serde_json::json!({
                "id": a.id.to_string(),
                "data_source_id": a.data_source_id.to_string(),
                "user_id": a.user_id.map(|u| u.to_string()),
                "role_id": a.role_id.map(|r| r.to_string()),
                "assignment_scope": &a.assignment_scope,
                "priority": a.priority,
            })
        }).collect::<Vec<_>>(),
    });

    policy_version::ActiveModel {
        id: Set(Uuid::now_v7()),
        policy_id: Set(policy_id),
        version: Set(version),
        snapshot: Set(snapshot.to_string()),
        change_type: Set(change_type.to_string()),
        changed_by: Set(changed_by),
        created_at: Set(Utc::now().naive_utc()),
    }
    .insert(txn)
    .await
    .map_err(ApiErr::internal)?;

    Ok(())
}

// ---------- POST /policies/validate-expression ----------

pub async fn validate_expression_handler(
    AdminClaims(_): AdminClaims,
    Json(body): Json<super::dto::ValidateExpressionRequest>,
) -> Json<super::dto::ValidateExpressionResponse> {
    match crate::hooks::policy::validate_expression(&body.expression, body.is_mask) {
        Ok(()) => Json(super::dto::ValidateExpressionResponse {
            valid: true,
            error: None,
        }),
        Err(e) => Json(super::dto::ValidateExpressionResponse {
            valid: false,
            error: Some(e),
        }),
    }
}

// ---------- GET /policies ----------

pub async fn list_policies(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Query(params): Query<ListPoliciesQuery>,
) -> Result<Json<PaginatedResponse<PolicyResponse>>, ApiErr> {
    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(20).min(100);

    let mut query = policy::Entity::find();
    if let Some(ref search) = params.search
        && !search.is_empty()
    {
        query = query.filter(policy::Column::Name.contains(search.as_str()));
    }

    let paginator = query
        .order_by_asc(policy::Column::CreatedAt)
        .paginate(&state.db, page_size);

    let total = paginator.num_items().await.map_err(ApiErr::internal)?;
    let items = paginator
        .fetch_page(page - 1)
        .await
        .map_err(ApiErr::internal)?;

    let ids: Vec<Uuid> = items.iter().map(|p| p.id).collect();
    let all_assignments = policy_assignment::Entity::find()
        .filter(policy_assignment::Column::PolicyId.is_in(ids))
        .all(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let mut asgn_counts: HashMap<Uuid, usize> = HashMap::new();
    for a in &all_assignments {
        *asgn_counts.entry(a.policy_id).or_insert(0) += 1;
    }

    // Load decision functions referenced by these policies
    let df_ids: Vec<Uuid> = items
        .iter()
        .filter_map(|p| p.decision_function_id)
        .collect();
    let df_map = load_decision_function_summaries(&state.db, &df_ids).await?;

    let data = items
        .iter()
        .map(|p| {
            let df = p
                .decision_function_id
                .and_then(|id| df_map.get(&id).cloned());
            policy_response_basic(p, *asgn_counts.get(&p.id).unwrap_or(&0), df)
        })
        .collect();

    Ok(Json(PaginatedResponse {
        data,
        total,
        page,
        page_size,
    }))
}

// ---------- POST /policies ----------

pub async fn create_policy(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Json(body): Json<CreatePolicyRequest>,
) -> Result<(StatusCode, Json<PolicyResponse>), ApiErr> {
    validate_policy_name(&body.name)
        .map_err(|e| ApiErr::new(StatusCode::UNPROCESSABLE_ENTITY, e))?;

    validate_targets(body.policy_type, &body.targets)
        .map_err(|e| ApiErr::new(StatusCode::UNPROCESSABLE_ENTITY, e))?;

    validate_definition(body.policy_type, &body.definition)
        .map_err(|e| ApiErr::new(StatusCode::UNPROCESSABLE_ENTITY, e))?;

    // Validate decision_function_id if provided
    let df = if let Some(df_id) = body.decision_function_id {
        Some(
            decision_function::Entity::find_by_id(df_id)
                .one(&state.db)
                .await
                .map_err(ApiErr::internal)?
                .ok_or_else(|| {
                    ApiErr::new(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "decision_function_id references a non-existent decision function",
                    )
                })?,
        )
    } else {
        None
    };

    let now = Utc::now().naive_utc();
    let policy_id = Uuid::now_v7();

    let targets_json = serde_json::to_string(&body.targets).map_err(ApiErr::internal)?;
    let definition_json = body
        .definition
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(ApiErr::internal)?;

    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let policy_model = policy::ActiveModel {
        id: Set(policy_id),
        name: Set(body.name.clone()),
        description: Set(body.description.clone()),
        policy_type: Set(body.policy_type.to_string()),
        targets: Set(targets_json),
        definition: Set(definition_json),
        is_enabled: Set(body.is_enabled),
        version: Set(1),
        decision_function_id: Set(body.decision_function_id),
        created_by: Set(claims.sub),
        updated_by: Set(claims.sub),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&*txn)
    .await
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("UNIQUE") || msg.contains("unique") {
            ApiErr::conflict("Policy name already exists")
        } else {
            ApiErr::internal(e)
        }
    })?;

    create_snapshot(
        &*txn,
        policy_id,
        1,
        claims.sub,
        "create",
        &policy_model,
        &[],
    )
    .await?;

    txn.audit(
        "policy",
        policy_id,
        AuditAction::Create,
        claims.sub,
        serde_json::json!({
            "after": {
                "name": &body.name,
                "description": &body.description,
                "policy_type": body.policy_type.to_string(),
                "targets": &body.targets,
                "is_enabled": body.is_enabled,
                "decision_function_id": body.decision_function_id.map(|id| id.to_string()),
            }
        }),
    );

    txn.commit().await.map_err(ApiErr::internal)?;

    let df_sum = df.as_ref().map(df_summary);
    Ok((
        StatusCode::CREATED,
        Json(policy_response_basic(&policy_model, 0, df_sum)),
    ))
}

// ---------- GET /policies/{id} ----------

pub async fn get_policy(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
) -> Result<Json<PolicyResponse>, ApiErr> {
    let p = policy::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Policy not found"))?;

    let assignments = policy_assignment::Entity::find()
        .filter(policy_assignment::Column::PolicyId.eq(id))
        .all(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let ds_ids: Vec<Uuid> = assignments.iter().map(|a| a.data_source_id).collect();
    let user_ids: Vec<Uuid> = assignments.iter().filter_map(|a| a.user_id).collect();
    let role_ids: Vec<Uuid> = assignments.iter().filter_map(|a| a.role_id).collect();
    let policy_names: HashMap<Uuid, String> = [(p.id, p.name.clone())].into_iter().collect();
    let ds_names = fetch_ds_names(&state.db, ds_ids).await?;
    let user_names = fetch_user_names(&state.db, user_ids).await?;
    let role_names = fetch_role_names(&state.db, role_ids).await?;

    // Load decision function summary if attached
    let df_sum = if let Some(df_id) = p.decision_function_id {
        decision_function::Entity::find_by_id(df_id)
            .one(&state.db)
            .await
            .map_err(ApiErr::internal)?
            .map(|d| df_summary(&d))
    } else {
        None
    };

    let asgn_count = assignments.len();
    let mut resp = policy_response_basic(&p, asgn_count, df_sum);
    resp.assignments = Some(
        assignments
            .iter()
            .map(|a| assignment_response(a, &policy_names, &ds_names, &user_names, &role_names))
            .collect(),
    );

    Ok(Json(resp))
}

// ---------- PUT /policies/{id} ----------

pub async fn update_policy(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdatePolicyRequest>,
) -> Result<Json<PolicyResponse>, ApiErr> {
    let p = policy::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Policy not found"))?;

    if let Some(ref name) = body.name {
        validate_policy_name(name).map_err(|e| ApiErr::new(StatusCode::UNPROCESSABLE_ENTITY, e))?;
    }

    if p.version != body.version {
        return Err(ApiErr::conflict(format!(
            "Policy version conflict: expected {}, got {}",
            p.version, body.version
        )));
    }

    let final_policy_type = body.policy_type.unwrap_or_else(|| {
        p.policy_type
            .parse::<PolicyType>()
            .unwrap_or(PolicyType::RowFilter)
    });

    // Determine final targets for validation
    let final_targets = match &body.targets {
        Some(r) => r.clone(),
        None => serde_json::from_str(&p.targets).unwrap_or_default(),
    };

    validate_targets(final_policy_type, &final_targets)
        .map_err(|e| ApiErr::new(StatusCode::UNPROCESSABLE_ENTITY, e))?;

    // For validation, use the incoming definition if provided, else the existing DB definition.
    let final_definition: Option<serde_json::Value> = if body.definition.is_some() {
        body.definition.clone()
    } else {
        p.definition
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
    };
    validate_definition(final_policy_type, &final_definition)
        .map_err(|e| ApiErr::new(StatusCode::UNPROCESSABLE_ENTITY, e))?;

    // Validate decision_function_id if changing
    if let Some(Some(df_id)) = body.decision_function_id {
        decision_function::Entity::find_by_id(df_id)
            .one(&state.db)
            .await
            .map_err(ApiErr::internal)?
            .ok_or_else(|| {
                ApiErr::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "decision_function_id references a non-existent decision function",
                )
            })?;
    }

    let now = Utc::now().naive_utc();
    let new_version = p.version + 1;

    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let mut changes_before = serde_json::Map::new();
    let mut changes_after = serde_json::Map::new();

    let mut active: policy::ActiveModel = p.clone().into();
    if let Some(ref name) = body.name {
        changes_before.insert("name".into(), serde_json::json!(p.name));
        changes_after.insert("name".into(), serde_json::json!(name));
        active.name = Set(name.clone());
    }
    if let Some(pt) = body.policy_type {
        changes_before.insert("policy_type".into(), serde_json::json!(p.policy_type));
        changes_after.insert("policy_type".into(), serde_json::json!(pt.to_string()));
        active.policy_type = Set(pt.to_string());
    }
    if let Some(ref desc) = body.description {
        changes_before.insert("description".into(), serde_json::json!(p.description));
        changes_after.insert("description".into(), serde_json::json!(desc));
        active.description = Set(Some(desc.clone()));
    }
    if let Some(enabled) = body.is_enabled {
        changes_before.insert("is_enabled".into(), serde_json::json!(p.is_enabled));
        changes_after.insert("is_enabled".into(), serde_json::json!(enabled));
        active.is_enabled = Set(enabled);
    }
    if let Some(ref targets) = body.targets {
        changes_before.insert("targets".into(), serde_json::json!(p.targets));
        changes_after.insert("targets".into(), serde_json::json!(targets));
        let json = serde_json::to_string(targets).map_err(ApiErr::internal)?;
        active.targets = Set(json);
    }
    // Definition is type-driven: clear it for types that don't use one so that a type
    // change never leaves a stale filter_expression / mask_expression in the DB.
    match final_policy_type {
        PolicyType::RowFilter | PolicyType::ColumnMask => {
            if let Some(ref definition) = body.definition {
                changes_after.insert("definition_changed".into(), serde_json::json!(true));
                let json = serde_json::to_string(definition).map_err(ApiErr::internal)?;
                active.definition = Set(Some(json));
            }
        }
        PolicyType::ColumnAllow | PolicyType::ColumnDeny | PolicyType::TableDeny => {
            active.definition = Set(None);
        }
    }
    // Decision function FK: 3-state — absent=no change, null=detach, uuid=attach
    if let Some(df_id_val) = body.decision_function_id {
        changes_before.insert(
            "decision_function_id".into(),
            serde_json::json!(p.decision_function_id.map(|id| id.to_string())),
        );
        changes_after.insert(
            "decision_function_id".into(),
            serde_json::json!(df_id_val.map(|id| id.to_string())),
        );
        active.decision_function_id = Set(df_id_val);
    }
    changes_before.insert("version".into(), serde_json::json!(p.version));
    changes_after.insert("version".into(), serde_json::json!(new_version));
    active.version = Set(new_version);
    active.updated_by = Set(claims.sub);
    active.updated_at = Set(now);

    let updated = active.update(&*txn).await.map_err(|e| {
        let msg = e.to_string();
        if msg.contains("UNIQUE") || msg.contains("unique") {
            ApiErr::conflict("Policy name already exists")
        } else {
            ApiErr::internal(e)
        }
    })?;

    let assignments = policy_assignment::Entity::find()
        .filter(policy_assignment::Column::PolicyId.eq(id))
        .all(&*txn)
        .await
        .map_err(ApiErr::internal)?;

    create_snapshot(
        &*txn,
        id,
        new_version,
        claims.sub,
        "update",
        &updated,
        &assignments,
    )
    .await?;

    txn.audit(
        "policy",
        id,
        AuditAction::Update,
        claims.sub,
        serde_json::json!({ "before": changes_before, "after": changes_after }),
    );

    txn.commit().await.map_err(ApiErr::internal)?;

    // Invalidate policy cache for all datasources this policy is assigned to
    if let Some(hook) = &state.policy_hook {
        for a in &assignments {
            if let Ok(Some(ds)) = data_source::Entity::find_by_id(a.data_source_id)
                .one(&state.db)
                .await
            {
                hook.invalidate_datasource(&ds.name).await;
                if let Some(ph) = &state.proxy_handler {
                    ph.rebuild_contexts_for_datasource(&ds.name);
                }
            }
        }
    }

    let ds_ids: Vec<Uuid> = assignments.iter().map(|a| a.data_source_id).collect();
    let user_ids: Vec<Uuid> = assignments.iter().filter_map(|a| a.user_id).collect();
    let upd_role_ids: Vec<Uuid> = assignments.iter().filter_map(|a| a.role_id).collect();
    let policy_names: HashMap<Uuid, String> =
        [(updated.id, updated.name.clone())].into_iter().collect();
    let ds_names = fetch_ds_names(&state.db, ds_ids).await?;
    let user_names = fetch_user_names(&state.db, user_ids).await?;
    let upd_role_names = fetch_role_names(&state.db, upd_role_ids).await?;

    // Load decision function summary if attached
    let df_sum = if let Some(df_id) = updated.decision_function_id {
        decision_function::Entity::find_by_id(df_id)
            .one(&state.db)
            .await
            .map_err(ApiErr::internal)?
            .map(|d| df_summary(&d))
    } else {
        None
    };

    let asgn_count = assignments.len();
    let mut resp = policy_response_basic(&updated, asgn_count, df_sum);
    resp.assignments = Some(
        assignments
            .iter()
            .map(|a| assignment_response(a, &policy_names, &ds_names, &user_names, &upd_role_names))
            .collect(),
    );

    Ok(Json(resp))
}

// ---------- DELETE /policies/{id} ----------

pub async fn delete_policy(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiErr> {
    let p = policy::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Policy not found"))?;

    let assignments = policy_assignment::Entity::find()
        .filter(policy_assignment::Column::PolicyId.eq(id))
        .all(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    create_snapshot(
        &*txn,
        id,
        p.version + 1,
        claims.sub,
        "delete",
        &p,
        &assignments,
    )
    .await?;

    txn.audit(
        "policy",
        id,
        AuditAction::Delete,
        claims.sub,
        serde_json::json!({
            "before": {
                "name": &p.name,
                "description": &p.description,
                "policy_type": &p.policy_type,
                "is_enabled": p.is_enabled,
                "version": p.version,
                "decision_function_id": p.decision_function_id.map(|id| id.to_string()),
            }
        }),
    );

    let active: policy::ActiveModel = p.into();
    active.delete(&*txn).await.map_err(ApiErr::internal)?;

    txn.commit().await.map_err(ApiErr::internal)?;

    if let Some(hook) = &state.policy_hook {
        for a in &assignments {
            if let Ok(Some(ds)) = data_source::Entity::find_by_id(a.data_source_id)
                .one(&state.db)
                .await
            {
                hook.invalidate_datasource(&ds.name).await;
                if let Some(ph) = &state.proxy_handler {
                    ph.rebuild_contexts_for_datasource(&ds.name);
                }
            }
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

// ---------- GET /datasources/{id}/policies ----------

pub async fn list_datasource_policies(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Path(ds_id): Path<Uuid>,
) -> Result<Json<Vec<PolicyAssignmentResponse>>, ApiErr> {
    let ds = data_source::Entity::find_by_id(ds_id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Data source not found"))?;

    let assignments = policy_assignment::Entity::find()
        .filter(policy_assignment::Column::DataSourceId.eq(ds_id))
        .order_by_asc(policy_assignment::Column::Priority)
        .all(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let policy_ids: Vec<Uuid> = assignments.iter().map(|a| a.policy_id).collect();
    let user_ids: Vec<Uuid> = assignments.iter().filter_map(|a| a.user_id).collect();
    let list_role_ids: Vec<Uuid> = assignments.iter().filter_map(|a| a.role_id).collect();
    let policy_names = fetch_policy_names(&state.db, policy_ids).await?;
    let ds_names: HashMap<Uuid, String> = [(ds_id, ds.name.clone())].into_iter().collect();
    let user_names = fetch_user_names(&state.db, user_ids).await?;
    let list_role_names = fetch_role_names(&state.db, list_role_ids).await?;

    Ok(Json(
        assignments
            .iter()
            .map(|a| {
                assignment_response(a, &policy_names, &ds_names, &user_names, &list_role_names)
            })
            .collect(),
    ))
}

// ---------- POST /datasources/{id}/policies ----------

pub async fn assign_policy(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Path(ds_id): Path<Uuid>,
    Json(body): Json<AssignPolicyRequest>,
) -> Result<(StatusCode, Json<PolicyAssignmentResponse>), ApiErr> {
    let ds = data_source::Entity::find_by_id(ds_id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Data source not found"))?;

    let p = policy::Entity::find_by_id(body.policy_id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Policy not found"))?;

    // Infer scope if not provided
    let scope = match body.scope.as_deref() {
        Some(s) => s.to_string(),
        None => {
            if body.role_id.is_some() {
                "role".to_string()
            } else if body.user_id.is_some() {
                "user".to_string()
            } else {
                "all".to_string()
            }
        }
    };

    // Validate scope/field constraints
    match scope.as_str() {
        "user" => {
            if body.user_id.is_none() {
                return Err(ApiErr::new(
                    StatusCode::BAD_REQUEST,
                    "scope 'user' requires user_id",
                ));
            }
            if body.role_id.is_some() {
                return Err(ApiErr::new(
                    StatusCode::BAD_REQUEST,
                    "scope 'user' must not have role_id",
                ));
            }
        }
        "role" => {
            if body.role_id.is_none() {
                return Err(ApiErr::new(
                    StatusCode::BAD_REQUEST,
                    "scope 'role' requires role_id",
                ));
            }
            if body.user_id.is_some() {
                return Err(ApiErr::new(
                    StatusCode::BAD_REQUEST,
                    "scope 'role' must not have user_id",
                ));
            }
            // Validate role exists
            role::Entity::find_by_id(body.role_id.unwrap())
                .one(&state.db)
                .await
                .map_err(ApiErr::internal)?
                .ok_or_else(|| ApiErr::not_found("Role not found"))?;
        }
        "all" => {
            if body.user_id.is_some() || body.role_id.is_some() {
                return Err(ApiErr::new(
                    StatusCode::BAD_REQUEST,
                    "scope 'all' must not have user_id or role_id",
                ));
            }
        }
        _ => {
            return Err(ApiErr::new(
                StatusCode::BAD_REQUEST,
                "scope must be 'user', 'role', or 'all'",
            ));
        }
    }

    // Duplicate check for scope='all' (SQLite NULL != NULL in unique indexes)
    if scope == "all" {
        let existing = policy_assignment::Entity::find()
            .filter(policy_assignment::Column::PolicyId.eq(body.policy_id))
            .filter(policy_assignment::Column::DataSourceId.eq(ds_id))
            .filter(policy_assignment::Column::AssignmentScope.eq("all"))
            .one(&state.db)
            .await
            .map_err(ApiErr::internal)?;
        if existing.is_some() {
            return Err(ApiErr::conflict(
                "This policy is already assigned to this datasource for all users",
            ));
        }
    }

    let now = Utc::now().naive_utc();
    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let assignment = policy_assignment::ActiveModel {
        id: Set(Uuid::now_v7()),
        policy_id: Set(body.policy_id),
        data_source_id: Set(ds_id),
        user_id: Set(body.user_id),
        role_id: Set(body.role_id),
        assignment_scope: Set(scope.clone()),
        priority: Set(body.priority),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&*txn)
    .await
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("UNIQUE") || msg.contains("unique") {
            ApiErr::conflict("This policy assignment already exists")
        } else {
            ApiErr::internal(e)
        }
    })?;

    let new_version = p.version + 1;
    let mut active: policy::ActiveModel = p.clone().into();
    active.version = Set(new_version);
    active.updated_by = Set(claims.sub);
    active.updated_at = Set(now);
    let updated_p = active.update(&*txn).await.map_err(ApiErr::internal)?;

    let all_assignments = policy_assignment::Entity::find()
        .filter(policy_assignment::Column::PolicyId.eq(p.id))
        .all(&*txn)
        .await
        .map_err(ApiErr::internal)?;

    create_snapshot(
        &*txn,
        p.id,
        new_version,
        claims.sub,
        "assignment_change",
        &updated_p,
        &all_assignments,
    )
    .await?;

    txn.audit(
        "policy",
        p.id,
        AuditAction::Assign,
        claims.sub,
        serde_json::json!({
            "assignment_id": assignment.id.to_string(),
            "datasource_id": ds_id.to_string(),
            "scope": &scope,
            "user_id": body.user_id.map(|id| id.to_string()),
            "role_id": body.role_id.map(|id| id.to_string()),
            "priority": body.priority,
        }),
    );

    txn.commit().await.map_err(ApiErr::internal)?;

    if let Some(hook) = &state.policy_hook {
        hook.invalidate_datasource(&ds.name).await;
    }
    if let Some(ph) = &state.proxy_handler {
        ph.rebuild_contexts_for_datasource(&ds.name);
    }

    // If role-scoped, also invalidate all role members
    if scope == "role"
        && let Some(role_id) = body.role_id
    {
        let members = crate::role_resolver::resolve_all_role_members(&state.db, role_id)
            .await
            .unwrap_or_default();
        for uid in members {
            if let Some(hook) = &state.policy_hook {
                hook.invalidate_user(uid).await;
            }
            if let Some(ph) = &state.proxy_handler {
                ph.rebuild_contexts_for_user(uid);
            }
        }
    }

    let policy_names: HashMap<Uuid, String> = [(p.id, p.name.clone())].into_iter().collect();
    let ds_names: HashMap<Uuid, String> = [(ds_id, ds.name.clone())].into_iter().collect();
    let assign_user_ids: Vec<Uuid> = body.user_id.into_iter().collect();
    let assign_role_ids: Vec<Uuid> = body.role_id.into_iter().collect();
    let user_names = fetch_user_names(&state.db, assign_user_ids).await?;
    let assign_role_names = fetch_role_names(&state.db, assign_role_ids).await?;

    Ok((
        StatusCode::CREATED,
        Json(assignment_response(
            &assignment,
            &policy_names,
            &ds_names,
            &user_names,
            &assign_role_names,
        )),
    ))
}

// ---------- DELETE /datasources/{id}/policies/{assignment_id} ----------

pub async fn remove_assignment(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Path((ds_id, assignment_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiErr> {
    let ds = data_source::Entity::find_by_id(ds_id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Data source not found"))?;

    let assignment = policy_assignment::Entity::find_by_id(assignment_id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Assignment not found"))?;

    let p = policy::Entity::find_by_id(assignment.policy_id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Policy not found"))?;

    // Capture assignment fields before .into() consumes it
    let removed_id = assignment.id;
    let removed_scope = assignment.assignment_scope.clone();
    let removed_user_id = assignment.user_id;
    let removed_role_id = assignment.role_id;
    let removed_ds_id = assignment.data_source_id;

    let now = Utc::now().naive_utc();
    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let active: policy_assignment::ActiveModel = assignment.into();
    active.delete(&*txn).await.map_err(ApiErr::internal)?;

    let new_version = p.version + 1;
    let mut policy_active: policy::ActiveModel = p.clone().into();
    policy_active.version = Set(new_version);
    policy_active.updated_by = Set(claims.sub);
    policy_active.updated_at = Set(now);
    let updated_p = policy_active
        .update(&*txn)
        .await
        .map_err(ApiErr::internal)?;

    let remaining_assignments = policy_assignment::Entity::find()
        .filter(policy_assignment::Column::PolicyId.eq(p.id))
        .all(&*txn)
        .await
        .map_err(ApiErr::internal)?;

    create_snapshot(
        &*txn,
        p.id,
        new_version,
        claims.sub,
        "assignment_change",
        &updated_p,
        &remaining_assignments,
    )
    .await?;

    txn.audit(
        "policy",
        p.id,
        AuditAction::Unassign,
        claims.sub,
        serde_json::json!({
            "assignment_id": removed_id.to_string(),
            "datasource_id": removed_ds_id.to_string(),
            "scope": &removed_scope,
            "user_id": removed_user_id.map(|id| id.to_string()),
            "role_id": removed_role_id.map(|id| id.to_string()),
        }),
    );

    txn.commit().await.map_err(ApiErr::internal)?;

    if let Some(hook) = &state.policy_hook {
        hook.invalidate_datasource(&ds.name).await;
    }
    if let Some(ph) = &state.proxy_handler {
        ph.rebuild_contexts_for_datasource(&ds.name);
    }

    Ok(StatusCode::NO_CONTENT)
}

// ---------- GET /users/{id}/effective-policies ----------

#[derive(Debug, serde::Deserialize)]
pub struct EffectivePoliciesQuery {
    pub datasource_id: Uuid,
}

pub async fn get_effective_policies(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Path(user_id): Path<Uuid>,
    Query(params): Query<EffectivePoliciesQuery>,
) -> Result<Json<Vec<role_resolver::EffectivePolicyEntry>>, ApiErr> {
    proxy_user::Entity::find_by_id(user_id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("User not found"))?;

    let ds = data_source::Entity::find_by_id(params.datasource_id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Data source not found"))?;

    let entries = role_resolver::resolve_effective_policies(
        &state.db,
        user_id,
        params.datasource_id,
        &ds.name,
    )
    .await
    .map_err(ApiErr::internal)?;

    Ok(Json(entries))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        admin::{discovery_job, jwt},
        auth::Auth,
        engine::EngineCache,
        entity::{data_source, proxy_user},
    };
    use axum::{
        Router,
        body::Body,
        http::{Method, Request, StatusCode},
        routing::{delete, get},
    };
    use chrono::Utc;
    use migration::MigratorTrait as _;
    use sea_orm::{ActiveModelTrait, Database, DatabaseConnection, Set};
    use std::sync::{Arc, OnceLock};
    use tokio::sync::Mutex;
    use tower::ServiceExt;
    use uuid::Uuid;

    const JWT_SECRET: &str = "test-jwt-secret-key-32-chars-pad";

    async fn setup_db() -> DatabaseConnection {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        migration::Migrator::up(&db, None).await.unwrap();
        db
    }

    fn shared_wasm_runtime() -> Arc<crate::decision::wasm::WasmDecisionRuntime> {
        static RUNTIME: OnceLock<Arc<crate::decision::wasm::WasmDecisionRuntime>> = OnceLock::new();
        RUNTIME
            .get_or_init(|| Arc::new(crate::decision::wasm::WasmDecisionRuntime::new().unwrap()))
            .clone()
    }

    fn make_state(db: DatabaseConnection) -> AdminState {
        let wasm_runtime = shared_wasm_runtime();
        let engine_cache = EngineCache::new(db.clone(), [0u8; 32], wasm_runtime.clone());
        AdminState {
            auth: Arc::new(Auth::new(db.clone())),
            db,
            jwt_secret: JWT_SECRET.to_string(),
            jwt_expiry_hours: 1,
            engine_cache,
            master_key: [0u8; 32],
            job_store: Arc::new(Mutex::new(discovery_job::JobStore::new())),
            policy_hook: None,
            proxy_handler: None,
            wasm_runtime,
        }
    }

    fn admin_token(id: Uuid) -> String {
        let claims = jwt::Claims {
            sub: id,
            username: "admin".to_string(),
            is_admin: true,
            exp: (Utc::now().timestamp() as u64) + 3600,
        };
        jwt::encode_jwt(&claims, JWT_SECRET).unwrap()
    }

    async fn insert_user(db: &DatabaseConnection, id: Uuid, username: &str) {
        let now = Utc::now().naive_utc();
        proxy_user::ActiveModel {
            id: Set(id),
            username: Set(username.to_string()),
            password_hash: Set("hash".to_string()),
            is_admin: Set(false),
            is_active: Set(true),
            email: Set(None),
            display_name: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }

    async fn insert_datasource(db: &DatabaseConnection, id: Uuid, name: &str) {
        let now = Utc::now().naive_utc();
        data_source::ActiveModel {
            id: Set(id),
            name: Set(name.to_string()),
            ds_type: Set("postgres".to_string()),
            config: Set("{}".to_string()),
            secure_config: Set(String::new()),
            is_active: Set(true),
            access_mode: Set("open".to_string()),
            last_sync_at: Set(None),
            last_sync_result: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(db)
        .await
        .unwrap();
    }

    fn make_router(state: AdminState) -> Router {
        Router::new()
            .route("/policies", get(list_policies).post(create_policy))
            .route(
                "/policies/{id}",
                get(get_policy).put(update_policy).delete(delete_policy),
            )
            .route(
                "/datasources/{id}/policies",
                get(list_datasource_policies).post(assign_policy),
            )
            .route(
                "/datasources/{id}/policies/{assignment_id}",
                delete(remove_assignment),
            )
            .with_state(state)
    }

    fn json_body(value: serde_json::Value) -> Body {
        Body::from(serde_json::to_string(&value).unwrap())
    }

    async fn body_json(res: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn row_filter_payload(name: &str) -> serde_json::Value {
        serde_json::json!({
            "name": name,
            "policy_type": "row_filter",
            "is_enabled": true,
            "targets": [{"schemas": ["public"], "tables": ["orders"]}],
            "definition": {"filter_expression": "tenant_id = {user.tenant}"}
        })
    }

    // ===== Policy CRUD =====

    #[tokio::test]
    async fn create_policy_returns_201() {
        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;

        let token = admin_token(admin_id);
        let res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/policies")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(row_filter_payload("row-filter")))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::CREATED);
        let body = body_json(res).await;
        assert_eq!(body["name"], "row-filter");
        assert_eq!(body["policy_type"], "row_filter");
        assert_eq!(body["version"], 1);
    }

    #[tokio::test]
    async fn create_policy_duplicate_name_409() {
        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;

        let token = admin_token(admin_id);
        let payload = row_filter_payload("my-policy");

        make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/policies")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(payload.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/policies")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(payload))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn create_policy_invalid_policy_type_422() {
        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;

        let token = admin_token(admin_id);
        let res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/policies")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "name": "bad-type",
                        "policy_type": "invalid_type",
                        "targets": [{"schemas": ["public"], "tables": ["t"]}],
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn create_row_filter_missing_definition_422() {
        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;

        let token = admin_token(admin_id);
        let res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/policies")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "name": "missing-def",
                        "policy_type": "row_filter",
                        "targets": [{"schemas": ["public"], "tables": ["orders"]}],
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn create_column_mask_requires_columns_422() {
        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;

        let token = admin_token(admin_id);
        let res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/policies")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "name": "mask-no-col",
                        "policy_type": "column_mask",
                        // Missing columns in resource entry
                        "targets": [{"schemas": ["public"], "tables": ["customers"]}],
                        "definition": {"mask_expression": "'***'"}
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn create_column_deny_ok() {
        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;

        let token = admin_token(admin_id);
        let res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/policies")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "name": "deny-ssn",
                        "policy_type": "column_deny",
                        "targets": [{"schemas": ["public"], "tables": ["customers"], "columns": ["ssn"]}],
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::CREATED);
        let body = body_json(res).await;
        assert_eq!(body["policy_type"], "column_deny");
    }

    #[tokio::test]
    async fn create_table_deny_columns_rejected_422() {
        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;

        let token = admin_token(admin_id);
        let res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/policies")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "name": "table-deny-with-cols",
                        "policy_type": "table_deny",
                        // table_deny must not have columns
                        "targets": [{"schemas": ["public"], "tables": ["secret"], "columns": ["id"]}],
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn get_policy_returns_200() {
        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;
        let token = admin_token(admin_id);

        // Create policy
        let create_res = make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/policies")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(row_filter_payload("get-me")))
                    .unwrap(),
            )
            .await
            .unwrap();
        let created = body_json(create_res).await;
        let id = created["id"].as_str().unwrap().to_string();

        // Get policy
        let res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("/policies/{id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let body = body_json(res).await;
        assert_eq!(body["id"], id);
        assert_eq!(body["policy_type"], "row_filter");
    }

    #[tokio::test]
    async fn update_policy_version_conflict_409() {
        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;
        let token = admin_token(admin_id);

        let create_res = make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/policies")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(row_filter_payload("conflict-test")))
                    .unwrap(),
            )
            .await
            .unwrap();
        let created = body_json(create_res).await;
        let id = created["id"].as_str().unwrap().to_string();

        let res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/policies/{id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "name": "updated",
                        "version": 999
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn delete_policy_returns_204() {
        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;
        let token = admin_token(admin_id);

        let create_res = make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/policies")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(row_filter_payload("delete-me")))
                    .unwrap(),
            )
            .await
            .unwrap();
        let created = body_json(create_res).await;
        let id = created["id"].as_str().unwrap().to_string();

        let res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/policies/{id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn assign_and_list_datasource_policies() {
        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        let ds_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;
        insert_datasource(&db, ds_id, "my-ds").await;
        let token = admin_token(admin_id);

        let create_res = make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/policies")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(row_filter_payload("assignable")))
                    .unwrap(),
            )
            .await
            .unwrap();
        let policy_id = body_json(create_res).await["id"]
            .as_str()
            .unwrap()
            .to_string();

        let assign_res = make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/datasources/{ds_id}/policies"))
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "policy_id": policy_id,
                        "priority": 50
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(assign_res.status(), StatusCode::CREATED);

        let list_res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("/datasources/{ds_id}/policies"))
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(list_res.status(), StatusCode::OK);
        let list = body_json(list_res).await;
        assert_eq!(list.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn remove_assignment_returns_204() {
        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        let ds_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;
        insert_datasource(&db, ds_id, "ds-remove").await;
        let token = admin_token(admin_id);

        let create_res = make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/policies")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(row_filter_payload("removable")))
                    .unwrap(),
            )
            .await
            .unwrap();
        let policy_id = body_json(create_res).await["id"]
            .as_str()
            .unwrap()
            .to_string();

        let assign_res = make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/datasources/{ds_id}/policies"))
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({"policy_id": policy_id})))
                    .unwrap(),
            )
            .await
            .unwrap();
        let assignment_id = body_json(assign_res).await["id"]
            .as_str()
            .unwrap()
            .to_string();

        let del_res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/datasources/{ds_id}/policies/{assignment_id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(del_res.status(), StatusCode::NO_CONTENT);
    }

    // When creating a non-expression policy (column_allow, column_deny, table_deny) with
    // `"definition": null` in the request body, the DB row must store SQL NULL — not the
    // string "null".  Previously, serde deserialised JSON null as Some(Value::Null) and
    // serde_json::to_string produced the 4-char string "null" which was stored verbatim.
    #[tokio::test]
    async fn create_column_allow_with_null_definition_stores_sql_null() {
        use sea_orm::EntityTrait;

        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;

        let token = admin_token(admin_id);
        let res = make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/policies")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "name": "col-allow-null-def",
                        "policy_type": "column_allow",
                        "is_enabled": true,
                        "targets": [{"schemas": ["public"], "tables": ["t"], "columns": ["id"]}],
                        "definition": null
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::CREATED);
        let policy_id: Uuid = body_json(res).await["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();

        let p = crate::entity::policy::Entity::find_by_id(policy_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert!(
            p.definition.is_none(),
            "column_allow definition must be SQL NULL, got: {:?}",
            p.definition
        );
    }

    // When updating a row_filter policy to column_deny and sending `"definition": null`,
    // the DB row must be updated to SQL NULL — not the string "null".
    #[tokio::test]
    async fn update_policy_type_change_to_column_deny_clears_definition() {
        use sea_orm::EntityTrait;

        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;

        let token = admin_token(admin_id);

        // Create a row_filter policy (stores a real definition).
        let create_res = make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/policies")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(row_filter_payload("rf-to-col-deny")))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_res.status(), StatusCode::CREATED);
        let policy_id: Uuid = body_json(create_res).await["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();

        // Change type to column_deny, explicitly sending definition: null.
        let update_res = make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/policies/{policy_id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "policy_type": "column_deny",
                        "targets": [{"schemas": ["public"], "tables": ["t"], "columns": ["secret"]}],
                        "definition": null,
                        "version": 1
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(update_res.status(), StatusCode::OK);

        let p = crate::entity::policy::Entity::find_by_id(policy_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert!(
            p.definition.is_none(),
            "After type change to column_deny, definition must be SQL NULL, got: {:?}",
            p.definition
        );
    }

    // ===== Audit logging tests =====

    use crate::entity::admin_audit_log;

    async fn get_audit_entries(
        db: &DatabaseConnection,
        resource_type: &str,
    ) -> Vec<admin_audit_log::Model> {
        use sea_orm::{EntityTrait, QueryFilter};
        admin_audit_log::Entity::find()
            .filter(admin_audit_log::Column::ResourceType.eq(resource_type))
            .all(db)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn audit_create_policy() {
        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;
        let token = admin_token(admin_id);

        let res = make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/policies")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(row_filter_payload("audit-create")))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        let policy_id: Uuid = body_json(res).await["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();

        let entries = get_audit_entries(&db, "policy").await;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].resource_id, policy_id);
        assert_eq!(entries[0].action, "create");
        assert_eq!(entries[0].actor_id, admin_id);

        let changes: serde_json::Value =
            serde_json::from_str(entries[0].changes.as_deref().unwrap()).unwrap();
        assert_eq!(changes["after"]["name"], "audit-create");
        assert_eq!(changes["after"]["policy_type"], "row_filter");
        assert_eq!(changes["after"]["is_enabled"], true);
    }

    #[tokio::test]
    async fn audit_update_policy() {
        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;
        let token = admin_token(admin_id);

        let create_res = make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/policies")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(row_filter_payload("audit-update")))
                    .unwrap(),
            )
            .await
            .unwrap();
        let id = body_json(create_res).await["id"]
            .as_str()
            .unwrap()
            .to_string();

        let _update_res = make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/policies/{id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "name": "renamed",
                        "is_enabled": false,
                        "version": 1
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        let entries = get_audit_entries(&db, "policy").await;
        let update_entry = entries.iter().find(|e| e.action == "update").unwrap();
        let changes: serde_json::Value =
            serde_json::from_str(update_entry.changes.as_deref().unwrap()).unwrap();
        assert_eq!(changes["before"]["name"], "audit-update");
        assert_eq!(changes["after"]["name"], "renamed");
        assert_eq!(changes["before"]["is_enabled"], true);
        assert_eq!(changes["after"]["is_enabled"], false);
        // version always tracked
        assert_eq!(changes["before"]["version"], 1);
        assert_eq!(changes["after"]["version"], 2);
    }

    #[tokio::test]
    async fn audit_delete_policy() {
        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;
        let token = admin_token(admin_id);

        let create_res = make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/policies")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(row_filter_payload("audit-delete")))
                    .unwrap(),
            )
            .await
            .unwrap();
        let id = body_json(create_res).await["id"]
            .as_str()
            .unwrap()
            .to_string();

        let _del_res = make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/policies/{id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let entries = get_audit_entries(&db, "policy").await;
        let del_entry = entries.iter().find(|e| e.action == "delete").unwrap();
        let changes: serde_json::Value =
            serde_json::from_str(del_entry.changes.as_deref().unwrap()).unwrap();
        assert_eq!(changes["before"]["name"], "audit-delete");
        assert_eq!(changes["before"]["policy_type"], "row_filter");
    }

    #[tokio::test]
    async fn audit_assign_and_unassign_policy() {
        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        let ds_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;
        insert_datasource(&db, ds_id, "audit-ds").await;
        let token = admin_token(admin_id);

        let create_res = make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/policies")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(row_filter_payload("audit-assign")))
                    .unwrap(),
            )
            .await
            .unwrap();
        let policy_id = body_json(create_res).await["id"]
            .as_str()
            .unwrap()
            .to_string();

        let assign_res = make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/datasources/{ds_id}/policies"))
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "policy_id": policy_id,
                        "priority": 10
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();
        let assignment_id = body_json(assign_res).await["id"]
            .as_str()
            .unwrap()
            .to_string();

        // Verify assign audit
        let entries = get_audit_entries(&db, "policy").await;
        let assign_entry = entries.iter().find(|e| e.action == "assign").unwrap();
        let changes: serde_json::Value =
            serde_json::from_str(assign_entry.changes.as_deref().unwrap()).unwrap();
        assert_eq!(changes["datasource_id"], ds_id.to_string());
        assert_eq!(changes["scope"], "all");
        assert_eq!(changes["priority"], 10);

        // Remove assignment
        make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/datasources/{ds_id}/policies/{assignment_id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let entries = get_audit_entries(&db, "policy").await;
        let unassign_entry = entries.iter().find(|e| e.action == "unassign").unwrap();
        let changes: serde_json::Value =
            serde_json::from_str(unassign_entry.changes.as_deref().unwrap()).unwrap();
        assert_eq!(changes["datasource_id"], ds_id.to_string());
        assert_eq!(changes["scope"], "all");
    }
}
