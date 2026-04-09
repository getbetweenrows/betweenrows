use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
use uuid::Uuid;

use crate::entity::{data_source, decision_function, policy, policy_assignment};

use super::{
    AdminState, ApiErr,
    admin_audit::{AuditAction, AuditedTxn},
    dto::{
        CreateDecisionFunctionRequest, DecisionFunctionResponse, DecisionFunctionSummary,
        ListDecisionFunctionsQuery, PaginatedResponse, TestDecisionFnRequest,
        TestDecisionFnResponse, UpdateDecisionFunctionRequest, validate_decision_function_fields,
        validate_policy_name,
    },
    jwt::AdminClaims,
};

// ---------- helpers ----------

async fn df_response(
    db: &impl sea_orm::ConnectionTrait,
    df: &decision_function::Model,
) -> Result<DecisionFunctionResponse, ApiErr> {
    let policy_count = policy::Entity::find()
        .filter(policy::Column::DecisionFunctionId.eq(Some(df.id)))
        .count(db)
        .await
        .map_err(ApiErr::internal)? as usize;

    let decision_config: Option<serde_json::Value> = df
        .decision_config
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());

    Ok(DecisionFunctionResponse {
        id: df.id,
        name: df.name.clone(),
        description: df.description.clone(),
        language: df.language.clone(),
        decision_fn: df.decision_fn.clone(),
        decision_config,
        evaluate_context: df.evaluate_context.clone(),
        on_error: df.on_error.clone(),
        log_level: df.log_level.clone(),
        is_enabled: df.is_enabled,
        version: df.version,
        policy_count,
        created_by: df.created_by,
        updated_by: df.updated_by,
        created_at: df.created_at,
        updated_at: df.updated_at,
    })
}

pub fn df_summary(df: &decision_function::Model) -> DecisionFunctionSummary {
    DecisionFunctionSummary {
        id: df.id,
        name: df.name.clone(),
        is_enabled: df.is_enabled,
        evaluate_context: df.evaluate_context.clone(),
    }
}

/// Invalidate caches for all datasources that have policies referencing this decision function.
async fn invalidate_for_decision_function(state: &AdminState, df_id: Uuid) {
    // Find policies referencing this function
    let policies = match policy::Entity::find()
        .filter(policy::Column::DecisionFunctionId.eq(Some(df_id)))
        .all(&state.db)
        .await
    {
        Ok(p) => p,
        Err(_) => return,
    };

    let policy_ids: Vec<Uuid> = policies.iter().map(|p| p.id).collect();
    if policy_ids.is_empty() {
        return;
    }

    // Find assignments for those policies to get datasource IDs
    let assignments = match policy_assignment::Entity::find()
        .filter(policy_assignment::Column::PolicyId.is_in(policy_ids))
        .all(&state.db)
        .await
    {
        Ok(a) => a,
        Err(_) => return,
    };

    let ds_ids: std::collections::HashSet<Uuid> =
        assignments.iter().map(|a| a.data_source_id).collect();
    for ds_id in ds_ids {
        if let Ok(Some(ds)) = data_source::Entity::find_by_id(ds_id).one(&state.db).await {
            if let Some(hook) = &state.policy_hook {
                hook.invalidate_datasource(&ds.name).await;
            }
            if let Some(ph) = &state.proxy_handler {
                ph.rebuild_contexts_for_datasource(&ds.name);
            }
        }
    }
}

// ---------- GET /decision-functions ----------

pub async fn list_decision_functions(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Query(params): Query<ListDecisionFunctionsQuery>,
) -> Result<Json<PaginatedResponse<DecisionFunctionResponse>>, ApiErr> {
    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(20).clamp(1, 100);

    let mut query = decision_function::Entity::find();
    if let Some(ref search) = params.search {
        query = query.filter(decision_function::Column::Name.contains(search));
    }

    let total = query
        .clone()
        .count(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let items = query
        .order_by_asc(decision_function::Column::Name)
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let mut data = Vec::with_capacity(items.len());
    for df in &items {
        data.push(df_response(&state.db, df).await?);
    }

    Ok(Json(PaginatedResponse {
        data,
        total,
        page,
        page_size,
    }))
}

// ---------- POST /decision-functions ----------

pub async fn create_decision_function(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Json(body): Json<CreateDecisionFunctionRequest>,
) -> Result<(StatusCode, Json<DecisionFunctionResponse>), ApiErr> {
    validate_policy_name(&body.name)
        .map_err(|e| ApiErr::new(StatusCode::UNPROCESSABLE_ENTITY, e))?;

    validate_decision_function_fields(
        &body.language,
        &body.decision_fn,
        &body.evaluate_context,
        &body.on_error,
        &body.log_level,
    )
    .map_err(|e| ApiErr::new(StatusCode::UNPROCESSABLE_ENTITY, e))?;

    let now = Utc::now().naive_utc();
    let df_id = Uuid::now_v7();

    // Compile JS → WASM
    let decision_wasm = compile_js(&body.decision_fn).await?;

    let decision_config_json = body
        .decision_config
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(ApiErr::internal)?;

    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let model = decision_function::ActiveModel {
        id: Set(df_id),
        name: Set(body.name.clone()),
        description: Set(body.description.clone()),
        language: Set(body.language.clone()),
        decision_fn: Set(body.decision_fn.clone()),
        decision_wasm: Set(Some(decision_wasm)),
        decision_config: Set(decision_config_json),
        evaluate_context: Set(body.evaluate_context.clone()),
        on_error: Set(body.on_error.clone()),
        log_level: Set(body.log_level.clone()),
        is_enabled: Set(true),
        version: Set(1),
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
            ApiErr::conflict("Decision function name already exists")
        } else {
            ApiErr::internal(e)
        }
    })?;

    txn.audit(
        "decision_function",
        df_id,
        AuditAction::Create,
        claims.sub,
        serde_json::json!({
            "after": {
                "name": model.name,
                "description": model.description,
                "language": model.language,
                "evaluate_context": model.evaluate_context,
                "on_error": model.on_error,
                "log_level": model.log_level,
                "is_enabled": model.is_enabled,
                "decision_config": model.decision_config,
            }
        }),
    );
    txn.commit().await.map_err(ApiErr::internal)?;

    let resp = df_response(&state.db, &model).await?;
    Ok((StatusCode::CREATED, Json(resp)))
}

// ---------- GET /decision-functions/{id} ----------

pub async fn get_decision_function(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
) -> Result<Json<DecisionFunctionResponse>, ApiErr> {
    let df = decision_function::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Decision function not found"))?;

    Ok(Json(df_response(&state.db, &df).await?))
}

// ---------- PUT /decision-functions/{id} ----------

pub async fn update_decision_function(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateDecisionFunctionRequest>,
) -> Result<Json<DecisionFunctionResponse>, ApiErr> {
    let df = decision_function::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Decision function not found"))?;

    if df.version != body.version {
        return Err(ApiErr::conflict(format!(
            "Version conflict: expected {}, got {}",
            df.version, body.version
        )));
    }

    if let Some(ref name) = body.name {
        validate_policy_name(name).map_err(|e| ApiErr::new(StatusCode::UNPROCESSABLE_ENTITY, e))?;
    }

    // Resolve final values for validation
    let final_language = body.language.as_deref().unwrap_or(&df.language);
    let final_decision_fn = body.decision_fn.as_deref().unwrap_or(&df.decision_fn);
    let final_evaluate_context = body
        .evaluate_context
        .as_deref()
        .unwrap_or(&df.evaluate_context);
    let final_on_error = body.on_error.as_deref().unwrap_or(&df.on_error);
    let final_log_level = body.log_level.as_deref().unwrap_or(&df.log_level);

    validate_decision_function_fields(
        final_language,
        final_decision_fn,
        final_evaluate_context,
        final_on_error,
        final_log_level,
    )
    .map_err(|e| ApiErr::new(StatusCode::UNPROCESSABLE_ENTITY, e))?;

    let now = Utc::now().naive_utc();
    let new_version = df.version + 1;

    let mut active: decision_function::ActiveModel = df.clone().into();

    let mut changes_before = serde_json::Map::new();
    let mut changes_after = serde_json::Map::new();

    // Recompile WASM if decision_fn changes
    let mut needs_recompile = false;
    if let Some(ref name) = body.name {
        changes_before.insert("name".into(), serde_json::json!(df.name));
        changes_after.insert("name".into(), serde_json::json!(name));
        active.name = Set(name.clone());
    }
    if let Some(ref desc) = body.description {
        changes_before.insert("description".into(), serde_json::json!(df.description));
        changes_after.insert("description".into(), serde_json::json!(desc));
        active.description = Set(Some(desc.clone()));
    }
    if let Some(ref lang) = body.language {
        changes_before.insert("language".into(), serde_json::json!(df.language));
        changes_after.insert("language".into(), serde_json::json!(lang));
        active.language = Set(lang.clone());
    }
    if let Some(ref fn_source) = body.decision_fn {
        changes_after.insert("code_changed".into(), serde_json::json!(true));
        active.decision_fn = Set(fn_source.clone());
        needs_recompile = true;
    }
    if let Some(config_val) = body.decision_config {
        changes_after.insert("config_changed".into(), serde_json::json!(true));
        let json = config_val
            .map(|v| serde_json::to_string(&v))
            .transpose()
            .map_err(ApiErr::internal)?;
        active.decision_config = Set(json);
    }
    if let Some(ref ctx) = body.evaluate_context {
        changes_before.insert(
            "evaluate_context".into(),
            serde_json::json!(df.evaluate_context),
        );
        changes_after.insert("evaluate_context".into(), serde_json::json!(ctx));
        active.evaluate_context = Set(ctx.clone());
    }
    if let Some(ref on_err) = body.on_error {
        changes_before.insert("on_error".into(), serde_json::json!(df.on_error));
        changes_after.insert("on_error".into(), serde_json::json!(on_err));
        active.on_error = Set(on_err.clone());
    }
    if let Some(ref ll) = body.log_level {
        changes_before.insert("log_level".into(), serde_json::json!(df.log_level));
        changes_after.insert("log_level".into(), serde_json::json!(ll));
        active.log_level = Set(ll.clone());
    }
    if let Some(enabled) = body.is_enabled {
        changes_before.insert("is_enabled".into(), serde_json::json!(df.is_enabled));
        changes_after.insert("is_enabled".into(), serde_json::json!(enabled));
        active.is_enabled = Set(enabled);
    }

    if needs_recompile {
        let wasm = compile_js(final_decision_fn).await?;
        active.decision_wasm = Set(Some(wasm));
    }

    active.version = Set(new_version);
    active.updated_by = Set(claims.sub);
    active.updated_at = Set(now);

    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let updated = active.update(&*txn).await.map_err(|e| {
        let msg = e.to_string();
        if msg.contains("UNIQUE") || msg.contains("unique") {
            ApiErr::conflict("Decision function name already exists")
        } else {
            ApiErr::internal(e)
        }
    })?;

    changes_before.insert("version".into(), serde_json::json!(df.version));
    changes_after.insert("version".into(), serde_json::json!(new_version));

    txn.audit(
        "decision_function",
        id,
        AuditAction::Update,
        claims.sub,
        serde_json::json!({ "before": changes_before, "after": changes_after }),
    );
    txn.commit().await.map_err(ApiErr::internal)?;

    // Invalidate caches
    invalidate_for_decision_function(&state, id).await;

    Ok(Json(df_response(&state.db, &updated).await?))
}

// ---------- DELETE /decision-functions/{id} ----------

pub async fn delete_decision_function(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiErr> {
    let df = decision_function::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Decision function not found"))?;

    // Block deletion if any policy references it
    let ref_count = policy::Entity::find()
        .filter(policy::Column::DecisionFunctionId.eq(Some(id)))
        .count(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    if ref_count > 0 {
        return Err(ApiErr::conflict(format!(
            "Cannot delete: {} polic{} still reference this decision function. Detach them first.",
            ref_count,
            if ref_count == 1 { "y" } else { "ies" }
        )));
    }

    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    txn.audit(
        "decision_function",
        id,
        AuditAction::Delete,
        claims.sub,
        serde_json::json!({
            "before": {
                "name": df.name,
                "description": df.description,
                "language": df.language,
                "evaluate_context": df.evaluate_context,
                "on_error": df.on_error,
                "log_level": df.log_level,
                "is_enabled": df.is_enabled,
            }
        }),
    );

    let active: decision_function::ActiveModel = df.into();
    active.delete(&*txn).await.map_err(ApiErr::internal)?;

    txn.commit().await.map_err(ApiErr::internal)?;

    Ok(StatusCode::NO_CONTENT)
}

// ---------- POST /decision-functions/test ----------

pub async fn test_decision_fn(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Json(body): Json<TestDecisionFnRequest>,
) -> Result<Json<TestDecisionFnResponse>, ApiErr> {
    use crate::decision::DecisionRuntime;

    let result = state
        .wasm_runtime
        .validate(&body.decision_fn, &body.context, &body.config)
        .await
        .map_err(|e| ApiErr::internal(format!("Decision function test failed: {e}")))?;

    Ok(Json(TestDecisionFnResponse {
        success: result.success,
        result: result.result,
        error: result.error,
    }))
}

// ---------- compile helper ----------

async fn compile_js(js_source: &str) -> Result<Vec<u8>, ApiErr> {
    crate::decision::wasm::compile_with_javy(js_source)
        .await
        .map_err(|e| {
            ApiErr::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("JS compilation failed: {e}"),
            )
        })
}
