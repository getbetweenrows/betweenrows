# Governance Dev Mode with Branch-Based Changesets

## Context

The proxy's governance state (catalog, policies, decision functions, assignments) is currently edited via CRUD APIs with immediate effect — no preview, no versioning, no safe rollout path. A bad policy edit can instantly block users or expose data.

We're adding:

- **Branches** — named, persistent, shared changesets that overlay live state. Like git branches for governance.
- **Preview** — branch members' proxy connections see the overlay. Everyone else sees production.
- **Deploy gates** — must rebase to latest production + pass validation before deploying.
- **Version history** — each deploy is recorded for audit and revert.

Key design choices:

- **No YAML, no new authoring format** — existing UI forms and CRUD APIs are the editing interface
- **Changeset = map of full entity snapshots** — not diffs, not deltas. Simple overlay merge.
- **GovernanceStore** — thin data access layer that transparently merges branch overlay on reads
- **Production mode is read-only** — all edits require an active branch
- **Branches are shared** — multiple users can be members, enabling team testing
- **YAML export/import can be layered on later** (Model B) without changing the core architecture

---

## Data Model

### Governance revision (on `data_source`)

| Column                | Type                       | Notes                                          |
| --------------------- | -------------------------- | ---------------------------------------------- |
| `governance_revision` | INTEGER NOT NULL DEFAULT 0 | Monotonic counter, incremented on every deploy |

### Branch (new table)

**`governance_branch`**
| Column | Type | Notes |
|---|---|---|
| `id` | UUID PK (v7) | |
| `datasource_id` | UUID FK → data_source (CASCADE) | |
| `name` | TEXT NOT NULL | e.g., "mask-ssn-rollout", "q2-policy-update" |
| `base_revision` | INTEGER NOT NULL | `governance_revision` at branch creation (updated on rebase) |
| `changeset` | TEXT NOT NULL DEFAULT '{}' | JSON: map of entity changes |
| `created_by` | UUID FK → proxy_user | |
| `created_at` | TIMESTAMP NOT NULL | |
| `updated_at` | TIMESTAMP NOT NULL | |

UNIQUE: `uq_governance_branch_ds_name(datasource_id, name)`

### Branch membership (new table)

**`governance_branch_member`**
| Column | Type | Notes |
|---|---|---|
| `id` | UUID PK (v7) | |
| `branch_id` | UUID FK → governance_branch (CASCADE) | |
| `user_id` | UUID FK → proxy_user (CASCADE) | |
| `created_at` | TIMESTAMP NOT NULL | |

UNIQUE: `uq_branch_member(branch_id, user_id)`

A user can only be on **one branch per datasource** at a time (enforced at application level).

### Version history (new table)

**`governance_history`**
| Column | Type | Notes |
|---|---|---|
| `id` | UUID PK (v7) | |
| `datasource_id` | UUID FK → data_source (CASCADE) | |
| `revision` | INTEGER NOT NULL | Matches `governance_revision` at deploy time |
| `changeset` | TEXT NOT NULL | The deployed changeset (for diff/audit) |
| `deployed_by` | UUID FK → proxy_user | |
| `change_summary` | TEXT NULL | Auto-generated: "updated 2 policies, added 1 table" |
| `created_at` | TIMESTAMP NOT NULL | |

INDEX: `idx_governance_history_ds_revision(datasource_id, revision)`

### Physical catalog (refactored from discovered\_\*)

Remove from `discovered_schema`: `is_selected`, `schema_alias`
Remove from `discovered_table`: `is_selected`
Remove from `discovered_column`: `is_selected`

Keep everything else — physical truth populated by discovery.

---

## Changeset Format

Map keyed by `(entity_type, id)`. Each entry is a full entity snapshot (upsert) or a delete marker.

```json
{
  "policy": {
    "uuid-1": {
      "action": "upsert",
      "data": {
        "id": "uuid-1",
        "name": "mask_ssn",
        "policy_type": "column_mask",
        "targets": [{"schemas": ["*"], "tables": ["*"], "columns": ["ssn"]}],
        "definition": {"mask_expression": "CONCAT('***', RIGHT(ssn, 4))"},
        "is_enabled": true
      }
    },
    "uuid-2": { "action": "delete" }
  },
  "decision_function": {
    "uuid-3": {
      "action": "upsert",
      "data": {
        "id": "uuid-3",
        "name": "business_hours",
        "decision_fn": "function evaluate(ctx) { ... }",
        "decision_wasm": "<base64>",
        "evaluate_context": "session",
        "on_error": "skip"
      }
    }
  },
  "policy_assignment": {
    "uuid-4": {
      "action": "upsert",
      "data": {
        "id": "uuid-4",
        "policy_id": "uuid-1",
        "datasource_id": "ds-uuid",
        "assignment_scope": "all",
        "priority": 100
      }
    }
  },
  "catalog": {
    "discovered_schema": {
      "schema-uuid": { "action": "upsert", "data": { ... } }
    },
    "discovered_table": {
      "table-uuid": { "action": "upsert", "data": { ... } }
    },
    "discovered_column": {
      "col-uuid": { "action": "upsert", "data": { ... } },
      "col-uuid-2": { "action": "delete" }
    }
  }
}
```

**Map, not list** — automatic dedup. Edit same policy 5 times → one entry with final state.

---

## GovernanceStore — The Overlay Layer

A thin data access layer between handlers and the database. Transparently merges branch changeset on reads. Routes writes to changeset when on a branch.

```rust
pub struct GovernanceStore {
    db: DatabaseConnection,
}

impl GovernanceStore {
    // Read: live tables + optional overlay
    async fn list_policies(&self, ds_id: Uuid, branch: Option<&Branch>) -> Vec<Policy> {
        let live = policy::Entity::find().filter(...).all(&self.db).await?;
        match branch {
            None => live,                                    // production: zero overhead
            Some(b) => b.apply_overlay("policy", live),     // branch: merge overlay
        }
    }

    // Write: append to changeset (never touches live tables)
    async fn upsert_policy(&self, branch: &mut Branch, policy: Policy) {
        branch.set_entry("policy", policy.id, Upsert(policy));
    }

    async fn delete_policy(&self, branch: &mut Branch, id: Uuid) {
        branch.set_entry("policy", id, Delete);
    }
}
```

### Generic overlay merge (~15 lines per entity type)

```rust
fn apply_overlay<T: HasId + Clone>(
    mut live: Vec<T>,
    overlay: &HashMap<Uuid, ChangeAction<T>>,
) -> Vec<T> {
    for (id, action) in overlay {
        match action {
            Upsert(data) => {
                if let Some(pos) = live.iter().position(|e| e.id() == *id) {
                    live[pos] = data.clone();
                } else {
                    live.push(data.clone());
                }
            }
            Delete => {
                live.retain(|e| e.id() != *id);
            }
        }
    }
    live
}
```

---

## Engine Changes

### Proxy connection routing

When a user connects:

1. Resolve user's branch for this datasource (if any)
2. If on a branch → `GovernanceStore` reads with overlay → `CachedCatalog` + policies include branch changes
3. If not on branch → standard read, zero overhead

`CachedCatalog` keyed by `(datasource, Option<branch_id>)`. Production cache is shared. Branch caches are per-branch (shared across branch members).

### PolicyHook

PolicyHook already loads policies from DB and caches. For branch members:

1. `load_session` calls `GovernanceStore.list_policies(ds_id, Some(branch))`
2. Returns live policies + branch overlay
3. Enforcement pipeline unchanged — same 5 policy types, same logic

### Catalog (get_catalog)

Same pattern — `GovernanceStore` provides merged catalog entries. `build_arrow_schema()` receives merged column list. Arrow type resolution from `discovered_column` unchanged.

---

## Deploy Gates

Both must pass before deploy is allowed:

### 1. Branch rebased

`branch.base_revision` must equal `data_source.governance_revision`. If production moved ahead since the branch was created, admin must rebase first.

**Rebase**: update `base_revision` to current `governance_revision`. Review what changed in production since branch was created. The changeset itself doesn't change (it stores full snapshots, not relative diffs).

### 2. Validation passes

Catalog entries in the branch must validate against the physical catalog. No errors allowed (warnings OK).

- Catalog column exists in `discovered_column` → OK
- Catalog column missing upstream → Error, blocks deploy
- Type drift (logical vs physical) → Warning, doesn't block
- New upstream column not in catalog → Warning, doesn't block

```
Deploy preflight:
    ✓ Branch rebased (rev 14)
    ✓ Validation: 42 OK, 1 warning, 0 errors

    Changes:
      + policy "mask_ssn" (column_mask)
      ~ policy "tenant_filter" (updated expression)
      + catalog: public.orders (3 columns)

    [Deploy to Production]
```

---

## User Workflow

### Creating a branch and making changes

1. Admin clicks "New Branch" on datasource page → names it "mask-ssn-rollout"
2. System creates `governance_branch` row with `base_revision = current`
3. Admin is auto-added as branch member
4. UI switches to branch view (banner: "Branch: mask-ssn-rollout")
5. All tabs (Catalog, Policies, Decision Functions) now show live + overlay, editable
6. Admin creates/edits/deletes policies via existing forms → writes go to changeset
7. Admin saves → changeset persisted to `governance_branch.changeset`

### Shared testing

1. Admin adds testers (Bob, Charlie) to the branch via "Manage Members"
2. Bob connects to proxy → sees live state + branch overlay
3. Bob runs queries → policies from the branch are enforced for him
4. Charlie connects → same branch overlay
5. Dave (not on branch) connects → sees production only

### Deploying

1. Admin clicks "Deploy"
2. System checks gates:
   - Branch rebased? If not → "Production moved ahead. Please rebase first."
   - Validation passes? If not → "1 error: public.legacy_events missing upstream. Fix before deploy."
3. Both pass → confirmation dialog with changeset summary + diff
4. Admin confirms → changeset replayed to live tables in transaction
5. `governance_revision++`, history recorded, branch deleted
6. All connections rebuilt

### Rebasing

1. Admin sees warning: "Production is 3 revisions ahead of your branch"
2. Clicks "Rebase" → reviews what changed in production (from `governance_history`)
3. Confirms → `base_revision` updated to current
4. Branch overlay re-validated
5. If same entity modified in both branch and production → conflict highlighted for manual resolution

### Reverting

1. Admin views version history: rev 14 (current), rev 13, rev 12, ...
2. Clicks "Revert to rev 12"
3. System creates a new branch with an auto-generated changeset that undoes revs 13-14
4. Admin reviews, tests, deploys through normal branch flow (not a direct revert — goes through the same gates)

---

## API Endpoints

### Branches

| Method   | Path                                            | Description                             |
| -------- | ----------------------------------------------- | --------------------------------------- |
| `POST`   | `/datasources/{id}/branches`                    | Create branch                           |
| `GET`    | `/datasources/{id}/branches`                    | List branches                           |
| `GET`    | `/datasources/{id}/branches/{branch_id}`        | Get branch (changeset, members, status) |
| `DELETE` | `/datasources/{id}/branches/{branch_id}`        | Discard branch                          |
| `POST`   | `/datasources/{id}/branches/{branch_id}/rebase` | Rebase to current revision              |
| `POST`   | `/datasources/{id}/branches/{branch_id}/deploy` | Deploy (with gates)                     |

### Branch membership

| Method   | Path                                                       | Description   |
| -------- | ---------------------------------------------------------- | ------------- |
| `POST`   | `/datasources/{id}/branches/{branch_id}/members`           | Add member    |
| `DELETE` | `/datasources/{id}/branches/{branch_id}/members/{user_id}` | Remove member |

### Version history

| Method | Path                                                     | Description           |
| ------ | -------------------------------------------------------- | --------------------- |
| `GET`  | `/datasources/{id}/governance/history`                   | List deploy history   |
| `GET`  | `/datasources/{id}/governance/history/{revision}`        | Get specific revision |
| `POST` | `/datasources/{id}/governance/history/{revision}/revert` | Create revert branch  |

### Validation

| Method | Path                                              | Description                      |
| ------ | ------------------------------------------------- | -------------------------------- |
| `POST` | `/datasources/{id}/validate`                      | Validate production state        |
| `POST` | `/datasources/{id}/branches/{branch_id}/validate` | Validate branch (live + overlay) |

### Existing CRUD APIs (modified behavior)

All governance CRUD endpoints (`/policies`, `/decision-functions`, `/datasources/{id}/policies`, catalog save) now:

- **No branch context** → read-only (403 for writes)
- **Branch context** (via header `X-Branch-Id` or query param) → reads merge overlay, writes go to changeset

### Discovery (modified)

| Method | Path                                | Change                                                             |
| ------ | ----------------------------------- | ------------------------------------------------------------------ |
| `POST` | `/datasources/{id}/discover` (save) | If on branch → catalog changes go to changeset. If not → blocked.  |
| `POST` | `/datasources/{id}/discover` (sync) | Always updates physical catalog (not governance). Runs validation. |

---

## Migration Strategy

Next migration: 055. No backfill needed (no production deployments).

| #   | Description                                                                           |
| --- | ------------------------------------------------------------------------------------- |
| 055 | ALTER TABLE `data_source` ADD COLUMN `governance_revision` INTEGER NOT NULL DEFAULT 0 |
| 056 | CREATE TABLE `governance_branch` + unique index on (datasource_id, name)              |
| 057 | CREATE TABLE `governance_branch_member` + unique index on (branch_id, user_id)        |
| 058 | CREATE TABLE `governance_history` + index on (datasource_id, revision)                |
| 059 | ALTER TABLE `discovered_schema` DROP COLUMN `is_selected`                             |
| 060 | ALTER TABLE `discovered_schema` DROP COLUMN `schema_alias`                            |
| 061 | ALTER TABLE `discovered_table` DROP COLUMN `is_selected`                              |
| 062 | ALTER TABLE `discovered_column` DROP COLUMN `is_selected`                             |

---

## New Files

| File                                            | Purpose                                                                      |
| ----------------------------------------------- | ---------------------------------------------------------------------------- |
| `proxy/src/governance/mod.rs`                   | Module root — `GovernanceStore`, `Branch`, `ChangeAction`, `Changeset` types |
| `proxy/src/governance/overlay.rs`               | `apply_overlay()` generic merge function                                     |
| `proxy/src/governance/deploy.rs`                | Deploy logic — replay changeset, gate checks, revision bump                  |
| `proxy/src/governance/validation.rs`            | `ValidationReport`, `validate_governance()`                                  |
| `proxy/src/governance/history.rs`               | Version history, revert branch generation, change summary                    |
| `proxy/src/entity/governance_branch.rs`         | SeaORM entity                                                                |
| `proxy/src/entity/governance_branch_member.rs`  | SeaORM entity                                                                |
| `proxy/src/entity/governance_history.rs`        | SeaORM entity                                                                |
| `proxy/src/admin/governance_handlers.rs`        | Branch CRUD, deploy, rebase, history endpoints                               |
| `admin-ui/src/components/BranchBanner.tsx`      | Persistent banner across governance tabs                                     |
| `admin-ui/src/components/BranchManager.tsx`     | Create, list, manage members                                                 |
| `admin-ui/src/components/DeployDialog.tsx`      | Deploy preflight checks, diff, confirmation                                  |
| `admin-ui/src/components/GovernanceHistory.tsx` | Version history, diff view                                                   |
| `admin-ui/src/components/ValidationReport.tsx`  | Drift/validation report renderer                                             |

## Modified Files

| File                                                   | Changes                                                                     |
| ------------------------------------------------------ | --------------------------------------------------------------------------- |
| `proxy/src/engine/mod.rs`                              | `get_catalog()` + `build_user_context()` — branch-aware via GovernanceStore |
| `proxy/src/hooks/policy.rs`                            | `load_session` calls GovernanceStore with branch context                    |
| `proxy/src/admin/policy_handlers.rs`                   | Route writes to branch changeset, block writes without branch               |
| `proxy/src/admin/decision_function_handlers.rs`        | Same — route writes to changeset                                            |
| `proxy/src/admin/catalog_handlers.rs`                  | Discovery save → changeset. Sync → physical only.                           |
| `proxy/src/admin/mod.rs`                               | New routes, branch context middleware                                       |
| `proxy/src/entity/mod.rs`                              | Add governance_branch, governance_branch_member, governance_history         |
| `proxy/src/entity/data_source.rs`                      | Add governance_revision field                                               |
| `proxy/src/entity/discovered_{schema,table,column}.rs` | Remove dropped fields                                                       |
| `migration/src/lib.rs`                                 | Register migrations 055-062                                                 |
| `admin-ui/src/types/governance.ts`                     | Branch, changeset, history, validation types                                |
| `admin-ui/src/contexts/BranchContext.tsx`              | React context for active branch state across tabs                           |
| `admin-ui/src/pages/DataSourceCatalogPage.tsx`         | Read-only without branch, editable with branch                              |
| `admin-ui/src/pages/RulesListPage.tsx`                 | Same — read-only / branch-aware                                             |

---

## Implementation Phases

### Phase 1: Data model + GovernanceStore foundation

- Migration files 055-062
- New entities: `governance_branch`, `governance_branch_member`, `governance_history`
- Update `data_source.rs` entity (add `governance_revision`)
- Update `discovered_*.rs` entities (remove dropped columns)
- Create `proxy/src/governance/` module:
  - `Changeset` type with serialization
  - `ChangeAction<T>` enum (Upsert/Delete)
  - `apply_overlay<T>()` generic merge
  - `GovernanceStore` with read methods (list_policies, list_decision_functions, etc.)

### Phase 2: Branch CRUD + membership

- Branch create/list/get/delete endpoints
- Membership add/remove endpoints
- Branch context resolution from request (header or query param)
- Constraint: one branch per user per datasource

### Phase 3: Write routing — CRUD goes to changeset

- Modify policy handlers: detect branch context → write to changeset
- Modify decision function handlers: same
- Modify catalog handlers: discovery save → changeset
- Block all governance writes without branch context (403)
- Production mode = read-only for governance entities

### Phase 4: Engine branch-aware reads

- `get_catalog()` calls GovernanceStore with branch context
- `PolicyHook.load_session` calls GovernanceStore with branch context
- `compute_user_visibility()` uses merged catalog
- `build_user_context()` resolves user's branch at connection time
- Branch cache keyed by `(datasource, branch_id)`

### Phase 5: Deploy + rebase + gates

- Deploy endpoint: check gates (rebased + validation), replay changeset, bump revision, record history
- Rebase endpoint: update base_revision, re-validate
- Validation system: catalog vs physical, policy reference checks
- `ValidationReport` with standardized schema
- Deploy blocked on validation errors

### Phase 6: Version history + revert

- History list endpoint with change summaries
- Auto-generate summaries from changeset (count upserts/deletes per entity type)
- Revert: generate inverse changeset → create branch → admin deploys through normal flow

### Phase 7: Admin UI

- `BranchContext` React context (wraps datasource tabs)
- Branch banner: persistent across Catalog/Policies/Functions tabs
- Branch manager: create, list, manage members, delete
- All governance forms: read-only without branch, editable with branch
- Deploy dialog: preflight checks, changeset diff, confirmation
- Rebase flow: show production changes, confirm
- Validation report component
- Version history panel with diff view

---

## Verification

1. **Existing tests pass**: `cargo test` — no regression
2. **Branch creation**: Create branch → verify changeset empty, base_revision matches current
3. **Write routing**: On branch, create policy → verify changeset updated, live table unchanged
4. **Overlay merge**: On branch, read policies → verify live + changeset merged correctly
5. **Proxy preview**: Branch member connects → sees branch policies enforced. Non-member → sees production only
6. **Shared branch**: Add second user to branch → both see same overlay
7. **Deploy gates — rebase**: Make production change → try deploy branch → verify 409 (must rebase)
8. **Deploy gates — validation**: Add catalog entry with no upstream match → try deploy → verify blocked
9. **Deploy success**: Rebase + validate → deploy → verify live tables updated, revision bumped, branch deleted
10. **Version history**: Deploy 3 times → history shows 3 entries → verify changeset recorded
11. **Revert**: Revert to earlier revision → verify revert branch created with inverse changeset
12. **Concurrent branches**: Two branches on same datasource → each member sees their own overlay
13. **Read-only production**: Without branch, attempt policy create → verify 403
14. **Crash safety**: Create branch, make changes, restart server → verify branch persists, live tables untouched
