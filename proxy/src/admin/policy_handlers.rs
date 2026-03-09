use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Set,
    TransactionTrait,
};
use std::collections::HashMap;
use uuid::Uuid;

use crate::entity::{
    data_source, policy, policy_assignment, policy_obligation, policy_version, proxy_user,
};

use super::{
    AdminState, ApiErr,
    dto::{
        AssignPolicyRequest, CreatePolicyRequest, ListPoliciesQuery, ObligationRequest,
        ObligationResponse, PaginatedResponse, PolicyAssignmentResponse, PolicyResponse,
        UpdatePolicyRequest, validate_policy_name,
    },
    jwt::AdminClaims,
};

// ---------- helpers ----------

/// Returns an error message if a deny-effect policy has any column_mask obligations.
/// column_mask on a deny policy is silently ignored at query time, so we reject it early.
fn validate_no_deny_column_mask(
    effect: &str,
    obligations: &[ObligationRequest],
) -> Option<&'static str> {
    if effect == "deny"
        && obligations
            .iter()
            .any(|o| o.obligation_type == "column_mask")
    {
        Some("column_mask obligations are not supported on deny-effect policies")
    } else {
        None
    }
}

fn obligation_response(m: &policy_obligation::Model) -> Result<ObligationResponse, ApiErr> {
    let definition: serde_json::Value =
        serde_json::from_str(&m.definition).map_err(ApiErr::internal)?;
    Ok(ObligationResponse {
        id: m.id,
        obligation_type: m.obligation_type.clone(),
        definition,
        created_at: m.created_at,
        updated_at: m.updated_at,
    })
}

fn assignment_response(
    m: &policy_assignment::Model,
    policy_names: &HashMap<Uuid, String>,
    ds_names: &HashMap<Uuid, String>,
    user_names: &HashMap<Uuid, String>,
) -> PolicyAssignmentResponse {
    PolicyAssignmentResponse {
        id: m.id,
        policy_id: m.policy_id,
        policy_name: policy_names.get(&m.policy_id).cloned().unwrap_or_default(),
        data_source_id: m.data_source_id,
        datasource_name: ds_names.get(&m.data_source_id).cloned().unwrap_or_default(),
        user_id: m.user_id,
        username: m.user_id.and_then(|uid| user_names.get(&uid).cloned()),
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

fn policy_response_basic(
    p: &policy::Model,
    obligation_count: usize,
    assignment_count: usize,
) -> PolicyResponse {
    PolicyResponse {
        id: p.id,
        name: p.name.clone(),
        description: p.description.clone(),
        effect: p.effect.clone(),
        is_enabled: p.is_enabled,
        version: p.version,
        obligation_count,
        assignment_count,
        created_by: p.created_by,
        updated_by: p.updated_by,
        created_at: p.created_at,
        updated_at: p.updated_at,
        obligations: None,
        assignments: None,
    }
}

/// Create a policy_version snapshot in the same transaction.
#[allow(clippy::too_many_arguments)]
async fn create_snapshot<C: sea_orm::ConnectionTrait>(
    txn: &C,
    policy_id: Uuid,
    version: i32,
    changed_by: Uuid,
    change_type: &str,
    name: &str,
    effect: &str,
    obligations: &[policy_obligation::Model],
    assignments: &[policy_assignment::Model],
) -> Result<(), ApiErr> {
    let snapshot = serde_json::json!({
        "name": name,
        "effect": effect,
        "obligations": obligations.iter().map(|o| {
            serde_json::json!({
                "id": o.id.to_string(),
                "obligation_type": o.obligation_type,
                "definition": serde_json::from_str::<serde_json::Value>(&o.definition).unwrap_or(serde_json::Value::Null),
            })
        }).collect::<Vec<_>>(),
        "assignments": assignments.iter().map(|a| {
            serde_json::json!({
                "id": a.id.to_string(),
                "data_source_id": a.data_source_id.to_string(),
                "user_id": a.user_id.map(|u| u.to_string()),
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

    // Count obligations and assignments for this page of policies
    let ids: Vec<Uuid> = items.iter().map(|p| p.id).collect();
    let all_obligations = policy_obligation::Entity::find()
        .filter(policy_obligation::Column::PolicyId.is_in(ids.clone()))
        .all(&state.db)
        .await
        .map_err(ApiErr::internal)?;
    let all_assignments = policy_assignment::Entity::find()
        .filter(policy_assignment::Column::PolicyId.is_in(ids))
        .all(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let mut obl_counts: HashMap<Uuid, usize> = HashMap::new();
    for o in &all_obligations {
        *obl_counts.entry(o.policy_id).or_insert(0) += 1;
    }
    let mut asgn_counts: HashMap<Uuid, usize> = HashMap::new();
    for a in &all_assignments {
        *asgn_counts.entry(a.policy_id).or_insert(0) += 1;
    }

    let data = items
        .iter()
        .map(|p| {
            policy_response_basic(
                p,
                *obl_counts.get(&p.id).unwrap_or(&0),
                *asgn_counts.get(&p.id).unwrap_or(&0),
            )
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
    // Validate name
    validate_policy_name(&body.name)
        .map_err(|e| ApiErr::new(StatusCode::UNPROCESSABLE_ENTITY, e))?;

    // Validate effect
    if body.effect != "permit" && body.effect != "deny" {
        return Err(ApiErr::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "effect must be 'permit' or 'deny'",
        ));
    }

    // Validate deny + column_mask combination
    if let Some(err) = validate_no_deny_column_mask(&body.effect, &body.obligations) {
        return Err(ApiErr::new(StatusCode::UNPROCESSABLE_ENTITY, err));
    }

    let now = Utc::now().naive_utc();
    let policy_id = Uuid::now_v7();
    let txn = state.db.begin().await.map_err(ApiErr::internal)?;

    let policy_model = policy::ActiveModel {
        id: Set(policy_id),
        name: Set(body.name.clone()),
        description: Set(body.description.clone()),
        effect: Set(body.effect.clone()),
        is_enabled: Set(body.is_enabled),
        version: Set(1),
        created_by: Set(claims.sub),
        updated_by: Set(claims.sub),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&txn)
    .await
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("UNIQUE") || msg.contains("unique") {
            ApiErr::conflict("Policy name already exists")
        } else {
            ApiErr::internal(e)
        }
    })?;

    // Insert obligations
    let mut obligation_models = Vec::new();
    for obl in &body.obligations {
        let def_json = serde_json::to_string(&obl.definition).map_err(ApiErr::internal)?;
        let m = policy_obligation::ActiveModel {
            id: Set(Uuid::now_v7()),
            policy_id: Set(policy_id),
            obligation_type: Set(obl.obligation_type.clone()),
            definition: Set(def_json),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&txn)
        .await
        .map_err(ApiErr::internal)?;
        obligation_models.push(m);
    }

    // Create version snapshot
    create_snapshot(
        &txn,
        policy_id,
        1,
        claims.sub,
        "create",
        &body.name,
        &body.effect,
        &obligation_models,
        &[],
    )
    .await?;

    txn.commit().await.map_err(ApiErr::internal)?;

    let obl_responses: Vec<ObligationResponse> = obligation_models
        .iter()
        .map(obligation_response)
        .collect::<Result<_, _>>()?;

    let obl_count = obl_responses.len();
    Ok((
        StatusCode::CREATED,
        Json(PolicyResponse {
            id: policy_model.id,
            name: policy_model.name,
            description: policy_model.description,
            effect: policy_model.effect,
            is_enabled: policy_model.is_enabled,
            version: policy_model.version,
            obligation_count: obl_count,
            assignment_count: 0,
            created_by: policy_model.created_by,
            updated_by: policy_model.updated_by,
            created_at: policy_model.created_at,
            updated_at: policy_model.updated_at,
            obligations: Some(obl_responses),
            assignments: Some(vec![]),
        }),
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

    let obligations = policy_obligation::Entity::find()
        .filter(policy_obligation::Column::PolicyId.eq(id))
        .all(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let assignments = policy_assignment::Entity::find()
        .filter(policy_assignment::Column::PolicyId.eq(id))
        .all(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let obl_responses: Vec<ObligationResponse> = obligations
        .iter()
        .map(obligation_response)
        .collect::<Result<_, _>>()?;

    let ds_ids: Vec<Uuid> = assignments.iter().map(|a| a.data_source_id).collect();
    let user_ids: Vec<Uuid> = assignments.iter().filter_map(|a| a.user_id).collect();
    let policy_names: HashMap<Uuid, String> = [(p.id, p.name.clone())].into_iter().collect();
    let ds_names = fetch_ds_names(&state.db, ds_ids).await?;
    let user_names = fetch_user_names(&state.db, user_ids).await?;

    Ok(Json(PolicyResponse {
        obligations: Some(obl_responses),
        assignments: Some(
            assignments
                .iter()
                .map(|a| assignment_response(a, &policy_names, &ds_names, &user_names))
                .collect(),
        ),
        ..policy_response_basic(&p, obligations.len(), assignments.len())
    }))
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
    if let Some(ref effect) = body.effect
        && (effect != "permit" && effect != "deny")
    {
        return Err(ApiErr::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "effect must be 'permit' or 'deny'",
        ));
    }

    // Optimistic concurrency check
    if p.version != body.version {
        return Err(ApiErr::conflict(format!(
            "Policy version conflict: expected {}, got {}",
            p.version, body.version
        )));
    }

    // Validate deny + column_mask combination
    let final_effect = body.effect.as_deref().unwrap_or(&p.effect);
    if let Some(ref new_obls) = body.obligations {
        if let Some(err) = validate_no_deny_column_mask(final_effect, new_obls) {
            return Err(ApiErr::new(StatusCode::UNPROCESSABLE_ENTITY, err));
        }
    } else if final_effect == "deny" {
        // No new obligations provided but effect is (or stays) deny — check existing ones
        let current_obls = policy_obligation::Entity::find()
            .filter(policy_obligation::Column::PolicyId.eq(id))
            .all(&state.db)
            .await
            .map_err(ApiErr::internal)?;
        if current_obls
            .iter()
            .any(|m| m.obligation_type == "column_mask")
        {
            return Err(ApiErr::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "column_mask obligations are not supported on deny-effect policies",
            ));
        }
    }

    let now = Utc::now().naive_utc();
    let new_version = p.version + 1;

    let txn = state.db.begin().await.map_err(ApiErr::internal)?;

    // Update policy using raw SQL for version check (optimistic lock)
    let mut active: policy::ActiveModel = p.clone().into();
    if let Some(name) = body.name {
        active.name = Set(name);
    }
    if let Some(effect) = body.effect {
        active.effect = Set(effect);
    }
    if let Some(desc) = body.description {
        active.description = Set(Some(desc));
    }
    if let Some(enabled) = body.is_enabled {
        active.is_enabled = Set(enabled);
    }
    active.version = Set(new_version);
    active.updated_by = Set(claims.sub);
    active.updated_at = Set(now);

    let updated = active.update(&txn).await.map_err(|e| {
        let msg = e.to_string();
        if msg.contains("UNIQUE") || msg.contains("unique") {
            ApiErr::conflict("Policy name already exists")
        } else {
            ApiErr::internal(e)
        }
    })?;

    // Replace obligations if provided
    let obligation_models = if let Some(new_obligations) = body.obligations {
        // Delete existing obligations
        policy_obligation::Entity::delete_many()
            .filter(policy_obligation::Column::PolicyId.eq(id))
            .exec(&txn)
            .await
            .map_err(ApiErr::internal)?;

        // Insert new ones
        let mut models = Vec::new();
        for obl in &new_obligations {
            let def_json = serde_json::to_string(&obl.definition).map_err(ApiErr::internal)?;
            let m = policy_obligation::ActiveModel {
                id: Set(Uuid::now_v7()),
                policy_id: Set(id),
                obligation_type: Set(obl.obligation_type.clone()),
                definition: Set(def_json),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&txn)
            .await
            .map_err(ApiErr::internal)?;
            models.push(m);
        }
        models
    } else {
        policy_obligation::Entity::find()
            .filter(policy_obligation::Column::PolicyId.eq(id))
            .all(&txn)
            .await
            .map_err(ApiErr::internal)?
    };

    let assignments = policy_assignment::Entity::find()
        .filter(policy_assignment::Column::PolicyId.eq(id))
        .all(&txn)
        .await
        .map_err(ApiErr::internal)?;

    // Create snapshot
    create_snapshot(
        &txn,
        id,
        new_version,
        claims.sub,
        "update",
        &updated.name,
        &updated.effect,
        &obligation_models,
        &assignments,
    )
    .await?;

    txn.commit().await.map_err(ApiErr::internal)?;

    // Invalidate policy cache for all datasources this policy is assigned to
    if let Some(hook) = &state.policy_hook {
        for a in &assignments {
            // Look up datasource name
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

    let obl_responses: Vec<ObligationResponse> = obligation_models
        .iter()
        .map(obligation_response)
        .collect::<Result<_, _>>()?;

    let ds_ids: Vec<Uuid> = assignments.iter().map(|a| a.data_source_id).collect();
    let user_ids: Vec<Uuid> = assignments.iter().filter_map(|a| a.user_id).collect();
    let policy_names: HashMap<Uuid, String> =
        [(updated.id, updated.name.clone())].into_iter().collect();
    let ds_names = fetch_ds_names(&state.db, ds_ids).await?;
    let user_names = fetch_user_names(&state.db, user_ids).await?;

    Ok(Json(PolicyResponse {
        obligations: Some(obl_responses),
        assignments: Some(
            assignments
                .iter()
                .map(|a| assignment_response(a, &policy_names, &ds_names, &user_names))
                .collect(),
        ),
        ..policy_response_basic(&updated, obligation_models.len(), assignments.len())
    }))
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

    // Capture assignments before deletion for cache invalidation
    let assignments = policy_assignment::Entity::find()
        .filter(policy_assignment::Column::PolicyId.eq(id))
        .all(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let txn = state.db.begin().await.map_err(ApiErr::internal)?;

    // Snapshot the deletion
    let obligations = policy_obligation::Entity::find()
        .filter(policy_obligation::Column::PolicyId.eq(id))
        .all(&txn)
        .await
        .map_err(ApiErr::internal)?;

    create_snapshot(
        &txn,
        id,
        p.version + 1,
        claims.sub,
        "delete",
        &p.name,
        &p.effect,
        &obligations,
        &assignments,
    )
    .await?;

    let active: policy::ActiveModel = p.into();
    active.delete(&txn).await.map_err(ApiErr::internal)?;

    txn.commit().await.map_err(ApiErr::internal)?;

    // Invalidate cache
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
    // Confirm datasource exists
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
    let policy_names = fetch_policy_names(&state.db, policy_ids).await?;
    let ds_names: HashMap<Uuid, String> = [(ds_id, ds.name.clone())].into_iter().collect();
    let user_names = fetch_user_names(&state.db, user_ids).await?;

    Ok(Json(
        assignments
            .iter()
            .map(|a| assignment_response(a, &policy_names, &ds_names, &user_names))
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
    // Confirm datasource exists
    let ds = data_source::Entity::find_by_id(ds_id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Data source not found"))?;

    // Confirm policy exists
    let p = policy::Entity::find_by_id(body.policy_id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Policy not found"))?;

    let now = Utc::now().naive_utc();
    let txn = state.db.begin().await.map_err(ApiErr::internal)?;

    let assignment = policy_assignment::ActiveModel {
        id: Set(Uuid::now_v7()),
        policy_id: Set(body.policy_id),
        data_source_id: Set(ds_id),
        user_id: Set(body.user_id),
        priority: Set(body.priority),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&txn)
    .await
    .map_err(ApiErr::internal)?;

    // Bump policy version + snapshot on assignment change
    let new_version = p.version + 1;
    let mut active: policy::ActiveModel = p.clone().into();
    active.version = Set(new_version);
    active.updated_by = Set(claims.sub);
    active.updated_at = Set(now);
    active.update(&txn).await.map_err(ApiErr::internal)?;

    let obligations = policy_obligation::Entity::find()
        .filter(policy_obligation::Column::PolicyId.eq(p.id))
        .all(&txn)
        .await
        .map_err(ApiErr::internal)?;

    let all_assignments = policy_assignment::Entity::find()
        .filter(policy_assignment::Column::PolicyId.eq(p.id))
        .all(&txn)
        .await
        .map_err(ApiErr::internal)?;

    create_snapshot(
        &txn,
        p.id,
        new_version,
        claims.sub,
        "assignment_change",
        &p.name,
        &p.effect,
        &obligations,
        &all_assignments,
    )
    .await?;

    txn.commit().await.map_err(ApiErr::internal)?;

    // Invalidate cache and rebuild active connection contexts
    if let Some(hook) = &state.policy_hook {
        hook.invalidate_datasource(&ds.name).await;
    }
    if let Some(ph) = &state.proxy_handler {
        ph.rebuild_contexts_for_datasource(&ds.name);
    }

    let policy_names: HashMap<Uuid, String> = [(p.id, p.name.clone())].into_iter().collect();
    let ds_names: HashMap<Uuid, String> = [(ds_id, ds.name.clone())].into_iter().collect();
    let user_ids: Vec<Uuid> = body.user_id.into_iter().collect();
    let user_names = fetch_user_names(&state.db, user_ids).await?;

    Ok((
        StatusCode::CREATED,
        Json(assignment_response(
            &assignment,
            &policy_names,
            &ds_names,
            &user_names,
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

    let now = Utc::now().naive_utc();
    let txn = state.db.begin().await.map_err(ApiErr::internal)?;

    let active: policy_assignment::ActiveModel = assignment.into();
    active.delete(&txn).await.map_err(ApiErr::internal)?;

    let new_version = p.version + 1;
    let mut policy_active: policy::ActiveModel = p.clone().into();
    policy_active.version = Set(new_version);
    policy_active.updated_by = Set(claims.sub);
    policy_active.updated_at = Set(now);
    policy_active.update(&txn).await.map_err(ApiErr::internal)?;

    let obligations = policy_obligation::Entity::find()
        .filter(policy_obligation::Column::PolicyId.eq(p.id))
        .all(&txn)
        .await
        .map_err(ApiErr::internal)?;

    let remaining_assignments = policy_assignment::Entity::find()
        .filter(policy_assignment::Column::PolicyId.eq(p.id))
        .all(&txn)
        .await
        .map_err(ApiErr::internal)?;

    create_snapshot(
        &txn,
        p.id,
        new_version,
        claims.sub,
        "assignment_change",
        &p.name,
        &p.effect,
        &obligations,
        &remaining_assignments,
    )
    .await?;

    txn.commit().await.map_err(ApiErr::internal)?;

    if let Some(hook) = &state.policy_hook {
        hook.invalidate_datasource(&ds.name).await;
    }
    if let Some(ph) = &state.proxy_handler {
        ph.rebuild_contexts_for_datasource(&ds.name);
    }

    Ok(StatusCode::NO_CONTENT)
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
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use tower::ServiceExt;
    use uuid::Uuid;

    const JWT_SECRET: &str = "test-jwt-secret-key-32-chars-pad";

    async fn setup_db() -> DatabaseConnection {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        migration::Migrator::up(&db, None).await.unwrap();
        db
    }

    fn make_state(db: DatabaseConnection) -> AdminState {
        let engine_cache = EngineCache::new(db.clone(), [0u8; 32]);
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
            tenant: Set("default".to_string()),
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
                    .body(json_body(serde_json::json!({
                        "name": "row-filter",
                        "effect": "permit",
                        "is_enabled": true,
                        "obligations": []
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::CREATED);
        let body = body_json(res).await;
        assert_eq!(body["name"], "row-filter");
        assert_eq!(body["effect"], "permit");
        assert_eq!(body["version"], 1);
    }

    #[tokio::test]
    async fn create_policy_with_obligations() {
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
                        "name": "with-obligations",
                        "effect": "permit",
                        "is_enabled": true,
                        "obligations": [
                            {
                                "obligation_type": "row_filter",
                                "definition": {"filter": "tenant_id = '{user.tenant}'"}
                            }
                        ]
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::CREATED);
        let body = body_json(res).await;
        let obligations = body["obligations"].as_array().unwrap();
        assert_eq!(obligations.len(), 1);
        assert_eq!(obligations[0]["obligation_type"], "row_filter");
    }

    #[tokio::test]
    async fn create_policy_duplicate_name_409() {
        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;

        let token = admin_token(admin_id);
        let payload = serde_json::json!({
            "name": "my-policy",
            "effect": "permit",
            "is_enabled": true,
            "obligations": []
        });

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
    async fn create_policy_invalid_effect_422() {
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
                        "name": "bad-effect",
                        "effect": "allow",
                        "is_enabled": true,
                        "obligations": []
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn create_deny_column_mask_rejected_422() {
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
                        "name": "bad-deny-mask",
                        "effect": "deny",
                        "is_enabled": true,
                        "obligations": [
                            {
                                "obligation_type": "column_mask",
                                "definition": {"schema": "*", "table": "*", "column": "ssn", "mask_expression": "'***'"}
                            }
                        ]
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn create_deny_row_filter_ok() {
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
                        "name": "deny-row-filter",
                        "effect": "deny",
                        "is_enabled": true,
                        "obligations": [
                            {
                                "obligation_type": "row_filter",
                                "definition": {"schema": "*", "table": "*", "filter_expression": "1=0"}
                            }
                        ]
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn create_deny_column_access_ok() {
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
                        "name": "deny-col-access",
                        "effect": "deny",
                        "is_enabled": true,
                        "obligations": [
                            {
                                "obligation_type": "column_access",
                                "definition": {"schema": "*", "table": "*", "columns": ["ssn"], "action": "deny"}
                            }
                        ]
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn create_permit_column_mask_ok() {
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
                        "name": "permit-mask",
                        "effect": "permit",
                        "is_enabled": true,
                        "obligations": [
                            {
                                "obligation_type": "column_mask",
                                "definition": {"schema": "*", "table": "*", "column": "ssn", "mask_expression": "'***'"}
                            }
                        ]
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn update_effect_to_deny_with_existing_column_mask_rejected_422() {
        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;

        let token = admin_token(admin_id);
        // Create a permit policy with column_mask obligation
        let create_res = make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/policies")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "name": "permit-with-mask",
                        "effect": "permit",
                        "is_enabled": true,
                        "obligations": [
                            {
                                "obligation_type": "column_mask",
                                "definition": {"schema": "*", "table": "*", "column": "ssn", "mask_expression": "'***'"}
                            }
                        ]
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();
        let policy_body = body_json(create_res).await;
        let policy_id = policy_body["id"].as_str().unwrap().to_string();

        // Now try to change effect to deny (without providing new obligations)
        let res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/policies/{policy_id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "version": 1,
                        "effect": "deny"
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn update_obligations_column_mask_on_deny_policy_rejected_422() {
        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;

        let token = admin_token(admin_id);
        // Create a deny policy with no obligations
        let create_res = make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/policies")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "name": "deny-no-obls",
                        "effect": "deny",
                        "is_enabled": true,
                        "obligations": []
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();
        let policy_body = body_json(create_res).await;
        let policy_id = policy_body["id"].as_str().unwrap().to_string();

        // Try to add a column_mask obligation to a deny policy
        let res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/policies/{policy_id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "version": 1,
                        "obligations": [
                            {
                                "obligation_type": "column_mask",
                                "definition": {"schema": "*", "table": "*", "column": "ssn", "mask_expression": "'***'"}
                            }
                        ]
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn get_policy_not_found_404() {
        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;

        let token = admin_token(admin_id);
        let random_id = Uuid::now_v7();
        let res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("/policies/{random_id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_policy_returns_enriched_assignments() {
        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        let user_id = Uuid::now_v7();
        let ds_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;
        insert_user(&db, user_id, "alice").await;
        insert_datasource(&db, ds_id, "prod-db").await;

        let token = admin_token(admin_id);

        let create_res = make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/policies")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "name": "row-filter",
                        "effect": "permit",
                        "is_enabled": true,
                        "obligations": []
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_res.status(), StatusCode::CREATED);
        let policy_body = body_json(create_res).await;
        let policy_id = policy_body["id"].as_str().unwrap().to_string();

        let assign_res = make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/datasources/{ds_id}/policies"))
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "policy_id": policy_id,
                        "user_id": user_id.to_string(),
                        "priority": 100
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(assign_res.status(), StatusCode::CREATED);

        // GET /policies/{id} — assignments must be enriched with names
        let get_res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("/policies/{policy_id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_res.status(), StatusCode::OK);
        let body = body_json(get_res).await;
        let assignments = body["assignments"].as_array().unwrap();
        assert_eq!(assignments.len(), 1);
        assert_eq!(assignments[0]["policy_name"], "row-filter");
        assert_eq!(assignments[0]["datasource_name"], "prod-db");
        assert_eq!(assignments[0]["username"], "alice");
    }

    #[tokio::test]
    async fn get_policy_assignment_all_users() {
        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        let ds_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;
        insert_datasource(&db, ds_id, "analytics-db").await;

        let token = admin_token(admin_id);

        let create_res = make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/policies")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "name": "all-users-policy",
                        "effect": "permit",
                        "is_enabled": true,
                        "obligations": []
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();
        let policy_body = body_json(create_res).await;
        let policy_id = policy_body["id"].as_str().unwrap().to_string();

        make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/datasources/{ds_id}/policies"))
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "policy_id": policy_id,
                        "user_id": null,
                        "priority": 50
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        let get_res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("/policies/{policy_id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = body_json(get_res).await;
        let assignments = body["assignments"].as_array().unwrap();
        assert_eq!(assignments.len(), 1);
        assert!(assignments[0]["username"].is_null());
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
                    .body(json_body(serde_json::json!({
                        "name": "conflict-policy",
                        "effect": "permit",
                        "is_enabled": true,
                        "obligations": []
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();
        let policy_body = body_json(create_res).await;
        let policy_id = policy_body["id"].as_str().unwrap().to_string();

        // version 0 is wrong (policy is at version 1)
        let res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/policies/{policy_id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "version": 0,
                        "name": "conflict-policy-updated"
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn update_policy_bumps_version() {
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
                    .body(json_body(serde_json::json!({
                        "name": "bump-policy",
                        "effect": "permit",
                        "is_enabled": true,
                        "obligations": []
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();
        let policy_body = body_json(create_res).await;
        let policy_id = policy_body["id"].as_str().unwrap().to_string();

        let res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/policies/{policy_id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "version": 1,
                        "name": "bump-policy-v2"
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let body = body_json(res).await;
        assert_eq!(body["version"], 2);
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
                    .body(json_body(serde_json::json!({
                        "name": "delete-me",
                        "effect": "deny",
                        "is_enabled": true,
                        "obligations": []
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();
        let policy_body = body_json(create_res).await;
        let policy_id = policy_body["id"].as_str().unwrap().to_string();

        let res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/policies/{policy_id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::NO_CONTENT);
    }

    // ===== Assignment tests =====

    #[tokio::test]
    async fn list_datasource_policies_enriched() {
        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        let ds_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;
        insert_datasource(&db, ds_id, "warehouse").await;

        let token = admin_token(admin_id);

        let create_res = make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/policies")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "name": "warehouse-policy",
                        "effect": "permit",
                        "is_enabled": true,
                        "obligations": []
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();
        let policy_body = body_json(create_res).await;
        let policy_id = policy_body["id"].as_str().unwrap().to_string();

        make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/datasources/{ds_id}/policies"))
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "policy_id": policy_id,
                        "user_id": null,
                        "priority": 100
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

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
        let body = body_json(list_res).await;
        let items = body.as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["policy_name"], "warehouse-policy");
        assert_eq!(items[0]["datasource_name"], "warehouse");
    }

    #[tokio::test]
    async fn assign_policy_returns_enriched() {
        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        let ds_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;
        insert_datasource(&db, ds_id, "staging").await;

        let token = admin_token(admin_id);

        let create_res = make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/policies")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "name": "staging-policy",
                        "effect": "permit",
                        "is_enabled": true,
                        "obligations": []
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();
        let policy_body = body_json(create_res).await;
        let policy_id = policy_body["id"].as_str().unwrap().to_string();

        let assign_res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/datasources/{ds_id}/policies"))
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "policy_id": policy_id,
                        "user_id": null,
                        "priority": 100
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(assign_res.status(), StatusCode::CREATED);
        let body = body_json(assign_res).await;
        assert_eq!(body["policy_name"], "staging-policy");
        assert_eq!(body["datasource_name"], "staging");
        assert!(body["username"].is_null());
    }

    #[tokio::test]
    async fn assign_policy_nonexistent_ds_404() {
        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;

        let token = admin_token(admin_id);
        let random_ds = Uuid::now_v7();
        let random_policy = Uuid::now_v7();

        let res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/datasources/{random_ds}/policies"))
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "policy_id": random_policy.to_string(),
                        "user_id": null,
                        "priority": 100
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn remove_assignment_returns_204() {
        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        let ds_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;
        insert_datasource(&db, ds_id, "remove-test-db").await;

        let token = admin_token(admin_id);

        let create_res = make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/policies")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "name": "remove-policy",
                        "effect": "permit",
                        "is_enabled": true,
                        "obligations": []
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();
        let policy_body = body_json(create_res).await;
        let policy_id = policy_body["id"].as_str().unwrap().to_string();

        let assign_res = make_router(make_state(db.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/datasources/{ds_id}/policies"))
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "policy_id": policy_id,
                        "user_id": null,
                        "priority": 100
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();
        let assign_body = body_json(assign_res).await;
        let assignment_id = assign_body["id"].as_str().unwrap().to_string();

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

    #[tokio::test]
    async fn list_policies_pagination() {
        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin").await;

        let token = admin_token(admin_id);

        for name in ["alpha", "beta", "gamma"] {
            make_router(make_state(db.clone()))
                .oneshot(
                    Request::builder()
                        .method(Method::POST)
                        .uri("/policies")
                        .header("Authorization", format!("Bearer {token}"))
                        .header("Content-Type", "application/json")
                        .body(json_body(serde_json::json!({
                            "name": name,
                            "effect": "permit",
                            "is_enabled": true,
                            "obligations": []
                        })))
                        .unwrap(),
                )
                .await
                .unwrap();
        }

        let res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/policies?page_size=2")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let body = body_json(res).await;
        assert_eq!(body["total"], 3);
        assert_eq!(body["data"].as_array().unwrap().len(), 2);
    }
}
