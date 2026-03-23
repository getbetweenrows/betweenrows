//! Role resolution logic for RBAC.
//!
//! Pure functions that resolve role hierarchies, detect cycles, check depth limits,
//! and compute effective policy assignments and datasource access.

use sea_orm::{ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter};
use std::collections::{HashMap, HashSet, VecDeque};
use uuid::Uuid;

use crate::entity::{
    data_source_access, policy, policy_assignment, role, role_inheritance, role_member,
};

const MAX_INHERITANCE_DEPTH: usize = 10;

/// Resolve all roles a user belongs to (direct + inherited ancestors), via BFS.
/// Skips inactive roles and their ancestors.
/// Returns the set of all reachable active role IDs.
pub async fn resolve_user_roles<C: ConnectionTrait>(
    db: &C,
    user_id: Uuid,
) -> Result<Vec<Uuid>, DbErr> {
    // Get direct role memberships
    let memberships = role_member::Entity::find()
        .filter(role_member::Column::UserId.eq(user_id))
        .all(db)
        .await?;

    let direct_role_ids: Vec<Uuid> = memberships.iter().map(|m| m.role_id).collect();
    if direct_role_ids.is_empty() {
        return Ok(vec![]);
    }

    // TODO: optimize for large deployments — load only reachable roles instead of all
    let all_roles: HashMap<Uuid, role::Model> = role::Entity::find()
        .all(db)
        .await?
        .into_iter()
        .map(|r| (r.id, r))
        .collect();

    // Load all inheritance edges
    let all_edges = role_inheritance::Entity::find().all(db).await?;
    let mut parent_map: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for edge in &all_edges {
        parent_map
            .entry(edge.child_role_id)
            .or_default()
            .push(edge.parent_role_id);
    }

    // BFS from direct roles upward through inheritance, capped at MAX_INHERITANCE_DEPTH
    let mut visited: HashSet<Uuid> = HashSet::new();
    let mut result: Vec<Uuid> = Vec::new();
    let mut queue: VecDeque<(Uuid, usize)> = VecDeque::new();

    for role_id in &direct_role_ids {
        if let Some(r) = all_roles.get(role_id)
            && r.is_active
            && visited.insert(*role_id)
        {
            result.push(*role_id);
            queue.push_back((*role_id, 1));
        }
    }

    while let Some((current_id, depth)) = queue.pop_front() {
        if depth >= MAX_INHERITANCE_DEPTH {
            continue;
        }
        if let Some(parents) = parent_map.get(&current_id) {
            for &parent_id in parents {
                if let Some(r) = all_roles.get(&parent_id)
                    && r.is_active
                    && visited.insert(parent_id)
                {
                    result.push(parent_id);
                    queue.push_back((parent_id, depth + 1));
                }
            }
        }
    }

    Ok(result)
}

/// Detect if adding an inheritance edge (parent_id ← child_id) would create a cycle.
/// Returns true if a cycle would be created.
pub async fn detect_cycle<C: ConnectionTrait>(
    db: &C,
    parent_id: Uuid,
    child_id: Uuid,
) -> Result<bool, DbErr> {
    // Self-reference is always a cycle
    if parent_id == child_id {
        return Ok(true);
    }

    // BFS from parent upward through existing inheritance edges.
    // If we reach child_id, adding child→parent would create a cycle.
    let all_edges = role_inheritance::Entity::find().all(db).await?;
    let mut parent_map: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for edge in &all_edges {
        parent_map
            .entry(edge.child_role_id)
            .or_default()
            .push(edge.parent_role_id);
    }

    let mut visited: HashSet<Uuid> = HashSet::new();
    let mut queue: VecDeque<Uuid> = VecDeque::new();
    visited.insert(parent_id);
    queue.push_back(parent_id);

    while let Some(current) = queue.pop_front() {
        if let Some(parents) = parent_map.get(&current) {
            for &pid in parents {
                if pid == child_id {
                    return Ok(true);
                }
                if visited.insert(pid) {
                    queue.push_back(pid);
                }
            }
        }
    }

    Ok(false)
}

/// Check the max depth above a given role. Returns the current max depth.
pub async fn check_inheritance_depth<C: ConnectionTrait>(
    db: &C,
    role_id: Uuid,
) -> Result<usize, DbErr> {
    let all_edges = role_inheritance::Entity::find().all(db).await?;
    let mut parent_map: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for edge in &all_edges {
        parent_map
            .entry(edge.child_role_id)
            .or_default()
            .push(edge.parent_role_id);
    }

    // BFS from role_id upward, tracking max depth
    let mut max_depth: usize = 0;
    let mut queue: VecDeque<(Uuid, usize)> = VecDeque::new();
    let mut visited: HashSet<Uuid> = HashSet::new();
    visited.insert(role_id);
    queue.push_back((role_id, 0));

    while let Some((current, depth)) = queue.pop_front() {
        max_depth = max_depth.max(depth);
        if let Some(parents) = parent_map.get(&current) {
            for &pid in parents {
                if visited.insert(pid) {
                    queue.push_back((pid, depth + 1));
                }
            }
        }
    }

    Ok(max_depth)
}

/// Resolve all effective policy assignments for a user on a given datasource.
/// Returns assignments where scope='all' OR (scope='user' AND user_id=user) OR (scope='role' AND role_id in user's roles).
/// Deduplicates: if the same policy_id appears from multiple sources, keeps the one with lowest priority.
pub async fn resolve_effective_assignments<C: ConnectionTrait>(
    db: &C,
    user_id: Uuid,
    datasource_id: Uuid,
) -> Result<Vec<policy_assignment::Model>, DbErr> {
    let user_roles = resolve_user_roles(db, user_id).await?;

    let all_assignments = policy_assignment::Entity::find()
        .filter(policy_assignment::Column::DataSourceId.eq(datasource_id))
        .all(db)
        .await?;

    let role_set: HashSet<Uuid> = user_roles.into_iter().collect();

    let mut best_by_policy: HashMap<Uuid, policy_assignment::Model> = HashMap::new();

    for a in all_assignments {
        let matches = match a.assignment_scope.as_str() {
            "all" => true,
            "user" => a.user_id == Some(user_id),
            "role" => a.role_id.is_some_and(|rid| role_set.contains(&rid)),
            _ => false,
        };

        if matches {
            best_by_policy
                .entry(a.policy_id)
                .and_modify(|existing| {
                    if a.priority < existing.priority {
                        *existing = a.clone();
                    }
                })
                .or_insert(a);
        }
    }

    Ok(best_by_policy.into_values().collect())
}

/// Check if a user has access to a datasource (direct, role-based, or all-scoped).
pub async fn resolve_datasource_access<C: ConnectionTrait>(
    db: &C,
    user_id: Uuid,
    datasource_id: Uuid,
) -> Result<bool, DbErr> {
    let user_roles = resolve_user_roles(db, user_id).await?;

    let accesses = data_source_access::Entity::find()
        .filter(data_source_access::Column::DataSourceId.eq(datasource_id))
        .all(db)
        .await?;

    let role_set: HashSet<Uuid> = user_roles.into_iter().collect();

    for a in &accesses {
        let matches = match a.assignment_scope.as_str() {
            "all" => true,
            "user" => a.user_id == Some(user_id),
            "role" => a.role_id.is_some_and(|rid| role_set.contains(&rid)),
            _ => false,
        };
        if matches {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Resolve all user IDs that are members of a role (direct + inherited via child subtree).
/// Used for cache invalidation when a role changes.
pub async fn resolve_all_role_members<C: ConnectionTrait>(
    db: &C,
    role_id: Uuid,
) -> Result<Vec<Uuid>, DbErr> {
    // BFS downward from role_id through inheritance to find all child roles
    let all_edges = role_inheritance::Entity::find().all(db).await?;
    let mut child_map: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for edge in &all_edges {
        child_map
            .entry(edge.parent_role_id)
            .or_default()
            .push(edge.child_role_id);
    }

    let mut all_role_ids: HashSet<Uuid> = HashSet::new();
    let mut queue: VecDeque<Uuid> = VecDeque::new();
    all_role_ids.insert(role_id);
    queue.push_back(role_id);

    while let Some(current) = queue.pop_front() {
        if let Some(children) = child_map.get(&current) {
            for &child_id in children {
                if all_role_ids.insert(child_id) {
                    queue.push_back(child_id);
                }
            }
        }
    }

    // Get all members of all collected roles
    let all_members = role_member::Entity::find()
        .filter(role_member::Column::RoleId.is_in(all_role_ids.into_iter().collect::<Vec<_>>()))
        .all(db)
        .await?;

    let user_ids: HashSet<Uuid> = all_members.iter().map(|m| m.user_id).collect();
    Ok(user_ids.into_iter().collect())
}

/// Effective policy report entry with source annotation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EffectivePolicyEntry {
    pub policy_id: Uuid,
    pub policy_name: String,
    pub policy_type: String,
    pub datasource_name: String,
    pub priority: i32,
    pub source: String, // "direct", "role:<name>", "inherited from '<name>'"
    pub is_enabled: bool,
}

/// Resolve effective policies for a user on a datasource with source annotations.
pub async fn resolve_effective_policies<C: ConnectionTrait>(
    db: &C,
    user_id: Uuid,
    datasource_id: Uuid,
    datasource_name: &str,
) -> Result<Vec<EffectivePolicyEntry>, DbErr> {
    let user_roles = resolve_user_roles(db, user_id).await?;

    let all_assignments = policy_assignment::Entity::find()
        .filter(policy_assignment::Column::DataSourceId.eq(datasource_id))
        .all(db)
        .await?;

    // Load role names
    let role_ids: Vec<Uuid> = user_roles.clone();
    let roles: HashMap<Uuid, String> = if !role_ids.is_empty() {
        role::Entity::find()
            .filter(role::Column::Id.is_in(role_ids.clone()))
            .all(db)
            .await?
            .into_iter()
            .map(|r| (r.id, r.name))
            .collect()
    } else {
        HashMap::new()
    };

    // Load direct role memberships to distinguish direct vs inherited
    let direct_memberships = role_member::Entity::find()
        .filter(role_member::Column::UserId.eq(user_id))
        .all(db)
        .await?;
    let direct_role_ids: HashSet<Uuid> = direct_memberships.iter().map(|m| m.role_id).collect();

    let role_set: HashSet<Uuid> = user_roles.into_iter().collect();

    // Collect assignments with source
    let mut entries: Vec<(policy_assignment::Model, String)> = Vec::new();

    for a in all_assignments {
        let source = match a.assignment_scope.as_str() {
            "all" => Some("all users".to_string()),
            "user" if a.user_id == Some(user_id) => Some("direct".to_string()),
            "role" if a.role_id.is_some_and(|rid| role_set.contains(&rid)) => {
                let rid = a.role_id.unwrap();
                let role_name = roles.get(&rid).cloned().unwrap_or_default();
                if direct_role_ids.contains(&rid) {
                    Some(format!("role '{role_name}'"))
                } else {
                    Some(format!("inherited from '{role_name}'"))
                }
            }
            _ => None,
        };
        if let Some(src) = source {
            entries.push((a, src));
        }
    }

    // Load policy details
    let policy_ids: Vec<Uuid> = entries.iter().map(|(a, _)| a.policy_id).collect();
    let policies: HashMap<Uuid, policy::Model> = if !policy_ids.is_empty() {
        policy::Entity::find()
            .filter(policy::Column::Id.is_in(policy_ids))
            .all(db)
            .await?
            .into_iter()
            .map(|p| (p.id, p))
            .collect()
    } else {
        HashMap::new()
    };

    let result = entries
        .into_iter()
        .filter_map(|(a, source)| {
            policies.get(&a.policy_id).map(|p| EffectivePolicyEntry {
                policy_id: p.id,
                policy_name: p.name.clone(),
                policy_type: p.policy_type.clone(),
                datasource_name: datasource_name.to_string(),
                priority: a.priority,
                source,
                is_enabled: p.is_enabled,
            })
        })
        .collect();

    Ok(result)
}

/// Effective member entry with source annotation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EffectiveMemberEntry {
    pub user_id: Uuid,
    pub username: String,
    pub source: String, // "direct" or "via role '<name>'"
}

/// Resolve all users who inherit policies from a role (direct + child subtree members).
pub async fn resolve_effective_members<C: ConnectionTrait>(
    db: &C,
    role_id: Uuid,
) -> Result<Vec<EffectiveMemberEntry>, DbErr> {
    // BFS downward to find all child roles
    let all_edges = role_inheritance::Entity::find().all(db).await?;
    let mut child_map: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for edge in &all_edges {
        child_map
            .entry(edge.parent_role_id)
            .or_default()
            .push(edge.child_role_id);
    }

    // Collect role_id -> role_name
    let all_roles: HashMap<Uuid, String> = role::Entity::find()
        .all(db)
        .await?
        .into_iter()
        .map(|r| (r.id, r.name))
        .collect();

    // BFS: collect (child_role_id, via_role_name_or_direct)
    let mut role_sources: Vec<(Uuid, String)> = vec![(role_id, "direct".to_string())];
    let mut queue: VecDeque<Uuid> = VecDeque::new();
    let mut visited: HashSet<Uuid> = HashSet::new();
    visited.insert(role_id);

    if let Some(children) = child_map.get(&role_id) {
        for &child_id in children {
            if visited.insert(child_id) {
                queue.push_back(child_id);
                let name = all_roles.get(&child_id).cloned().unwrap_or_default();
                role_sources.push((child_id, format!("via role '{name}'")));
            }
        }
    }

    while let Some(current) = queue.pop_front() {
        if let Some(children) = child_map.get(&current) {
            for &child_id in children {
                if visited.insert(child_id) {
                    queue.push_back(child_id);
                    let name = all_roles.get(&child_id).cloned().unwrap_or_default();
                    role_sources.push((child_id, format!("via role '{name}'")));
                }
            }
        }
    }

    // Get members of each collected role
    let all_role_ids: Vec<Uuid> = role_sources.iter().map(|(id, _)| *id).collect();
    let members = role_member::Entity::find()
        .filter(role_member::Column::RoleId.is_in(all_role_ids))
        .all(db)
        .await?;

    // Build role_id → source mapping
    let role_source_map: HashMap<Uuid, String> = role_sources.into_iter().collect();

    // Load user details
    let user_ids: Vec<Uuid> = members
        .iter()
        .map(|m| m.user_id)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let users: HashMap<Uuid, String> = if !user_ids.is_empty() {
        crate::entity::proxy_user::Entity::find()
            .filter(crate::entity::proxy_user::Column::Id.is_in(user_ids))
            .all(db)
            .await?
            .into_iter()
            .map(|u| (u.id, u.username))
            .collect()
    } else {
        HashMap::new()
    };

    // Deduplicate: if user appears via multiple paths, prefer "direct"
    let mut user_entries: HashMap<Uuid, EffectiveMemberEntry> = HashMap::new();
    for m in &members {
        let source = role_source_map.get(&m.role_id).cloned().unwrap_or_default();
        let username = users.get(&m.user_id).cloned().unwrap_or_default();

        user_entries
            .entry(m.user_id)
            .and_modify(|existing| {
                // Prefer "direct" over "via role ..."
                if source == "direct" {
                    existing.source = "direct".to_string();
                }
            })
            .or_insert(EffectiveMemberEntry {
                user_id: m.user_id,
                username,
                source,
            });
    }

    Ok(user_entries.into_values().collect())
}

/// Check the max depth below a given role (BFS downward through child roles).
/// Used together with `check_inheritance_depth` to compute total chain depth
/// when adding an inheritance edge.
pub async fn check_inheritance_depth_down<C: ConnectionTrait>(
    db: &C,
    role_id: Uuid,
) -> Result<usize, DbErr> {
    let all_edges = role_inheritance::Entity::find().all(db).await?;
    let mut child_map: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for edge in &all_edges {
        child_map
            .entry(edge.parent_role_id)
            .or_default()
            .push(edge.child_role_id);
    }

    let mut max_depth: usize = 0;
    let mut queue: VecDeque<(Uuid, usize)> = VecDeque::new();
    let mut visited: HashSet<Uuid> = HashSet::new();
    visited.insert(role_id);
    queue.push_back((role_id, 0));

    while let Some((current, depth)) = queue.pop_front() {
        max_depth = max_depth.max(depth);
        if let Some(children) = child_map.get(&current) {
            for &cid in children {
                if visited.insert(cid) {
                    queue.push_back((cid, depth + 1));
                }
            }
        }
    }

    Ok(max_depth)
}

/// Resolve all ancestor role IDs of a given role (BFS upward through parent_map).
/// Returns all role IDs reachable by walking up the inheritance DAG (not including
/// the starting role itself).
pub async fn resolve_ancestor_roles<C: ConnectionTrait>(
    db: &C,
    role_id: Uuid,
) -> Result<Vec<Uuid>, DbErr> {
    let all_edges = role_inheritance::Entity::find().all(db).await?;
    let mut parent_map: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for edge in &all_edges {
        parent_map
            .entry(edge.child_role_id)
            .or_default()
            .push(edge.parent_role_id);
    }

    let mut visited: HashSet<Uuid> = HashSet::new();
    let mut result: Vec<Uuid> = Vec::new();
    let mut queue: VecDeque<Uuid> = VecDeque::new();

    // Seed with direct parents of role_id
    if let Some(parents) = parent_map.get(&role_id) {
        for &pid in parents {
            if visited.insert(pid) {
                result.push(pid);
                queue.push_back(pid);
            }
        }
    }

    while let Some(current) = queue.pop_front() {
        if let Some(parents) = parent_map.get(&current) {
            for &pid in parents {
                if visited.insert(pid) {
                    result.push(pid);
                    queue.push_back(pid);
                }
            }
        }
    }

    Ok(result)
}

/// Build a path string showing the cycle for error messages.
pub async fn build_cycle_path<C: ConnectionTrait>(
    db: &C,
    parent_id: Uuid,
    child_id: Uuid,
) -> Result<String, DbErr> {
    let all_edges = role_inheritance::Entity::find().all(db).await?;
    let all_roles: HashMap<Uuid, String> = role::Entity::find()
        .all(db)
        .await?
        .into_iter()
        .map(|r| (r.id, r.name))
        .collect();

    let mut parent_map: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for edge in &all_edges {
        parent_map
            .entry(edge.child_role_id)
            .or_default()
            .push(edge.parent_role_id);
    }

    // BFS from parent_id upward to find path to child_id
    let mut visited: HashMap<Uuid, Uuid> = HashMap::new(); // node -> came_from
    let mut queue: VecDeque<Uuid> = VecDeque::new();
    queue.push_back(parent_id);

    while let Some(current) = queue.pop_front() {
        if let Some(parents) = parent_map.get(&current) {
            for &pid in parents {
                if pid == child_id {
                    // Reconstruct path
                    let mut path = vec![child_id, current];
                    let mut node = current;
                    while node != parent_id {
                        if let Some(&prev) = visited.get(&node) {
                            path.push(prev);
                            node = prev;
                        } else {
                            break;
                        }
                    }
                    // Add child_id again to show the cycle
                    path.push(child_id);
                    path.reverse();
                    let names: Vec<String> = path
                        .iter()
                        .map(|id| all_roles.get(id).cloned().unwrap_or_else(|| id.to_string()))
                        .collect();
                    return Ok(names.join(" → "));
                }
                if let std::collections::hash_map::Entry::Vacant(e) = visited.entry(pid) {
                    e.insert(current);
                    queue.push_back(pid);
                }
            }
        }
    }

    let child_name = all_roles
        .get(&child_id)
        .cloned()
        .unwrap_or_else(|| child_id.to_string());
    let parent_name = all_roles
        .get(&parent_id)
        .cloned()
        .unwrap_or_else(|| parent_id.to_string());
    Ok(format!("{child_name} → {parent_name} → {child_name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use migration::{Migrator, MigratorTrait};
    use sea_orm::{ActiveModelTrait, Database, Set};

    async fn setup() -> sea_orm::DatabaseConnection {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        Migrator::up(&db, None).await.unwrap();
        db
    }

    async fn create_role(
        db: &sea_orm::DatabaseConnection,
        name: &str,
        active: bool,
    ) -> role::Model {
        let now = Utc::now().naive_utc();
        role::ActiveModel {
            id: Set(Uuid::now_v7()),
            name: Set(name.to_string()),
            description: Set(None),
            is_active: Set(active),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn add_member(db: &sea_orm::DatabaseConnection, role_id: Uuid, user_id: Uuid) {
        role_member::ActiveModel {
            id: Set(Uuid::now_v7()),
            role_id: Set(role_id),
            user_id: Set(user_id),
            created_at: Set(Utc::now().naive_utc()),
        }
        .insert(db)
        .await
        .unwrap();
    }

    async fn add_inheritance(db: &sea_orm::DatabaseConnection, parent_id: Uuid, child_id: Uuid) {
        role_inheritance::ActiveModel {
            id: Set(Uuid::now_v7()),
            parent_role_id: Set(parent_id),
            child_role_id: Set(child_id),
            created_at: Set(Utc::now().naive_utc()),
        }
        .insert(db)
        .await
        .unwrap();
    }

    async fn create_user(
        db: &sea_orm::DatabaseConnection,
        username: &str,
    ) -> crate::entity::proxy_user::Model {
        let now = Utc::now().naive_utc();
        crate::entity::proxy_user::ActiveModel {
            id: Set(Uuid::now_v7()),
            username: Set(username.to_string()),
            password_hash: Set("$argon2id$v=19$m=19456,t=2,p=1$fake".to_string()),
            tenant: Set("default".to_string()),
            is_admin: Set(false),
            is_active: Set(true),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    // U1: BFS traversal: linear chain A→B→C returns [A, B, C]
    #[tokio::test]
    async fn u1_bfs_linear_chain() {
        let db = setup().await;
        let user = create_user(&db, "alice").await;
        let a = create_role(&db, "A", true).await;
        let b = create_role(&db, "B", true).await;
        let c = create_role(&db, "C", true).await;

        add_member(&db, a.id, user.id).await;
        add_inheritance(&db, b.id, a.id).await; // A's parent is B
        add_inheritance(&db, c.id, b.id).await; // B's parent is C

        let roles = resolve_user_roles(&db, user.id).await.unwrap();
        assert_eq!(roles.len(), 3);
        let role_set: HashSet<Uuid> = roles.into_iter().collect();
        assert!(role_set.contains(&a.id));
        assert!(role_set.contains(&b.id));
        assert!(role_set.contains(&c.id));
    }

    // U2: BFS traversal: diamond A→B, A→C, B→D, C→D returns [A, B, C, D] (deduped)
    #[tokio::test]
    async fn u2_bfs_diamond() {
        let db = setup().await;
        let user = create_user(&db, "alice").await;
        let a = create_role(&db, "A", true).await;
        let b = create_role(&db, "B", true).await;
        let c = create_role(&db, "C", true).await;
        let d = create_role(&db, "D", true).await;

        add_member(&db, a.id, user.id).await;
        add_inheritance(&db, b.id, a.id).await;
        add_inheritance(&db, c.id, a.id).await;
        add_inheritance(&db, d.id, b.id).await;
        add_inheritance(&db, d.id, c.id).await;

        let roles = resolve_user_roles(&db, user.id).await.unwrap();
        assert_eq!(roles.len(), 4, "Diamond should produce 4 unique roles");
    }

    // U3: BFS traversal: disconnected subgraphs — only reachable roles returned
    #[tokio::test]
    async fn u3_bfs_disconnected() {
        let db = setup().await;
        let user = create_user(&db, "alice").await;
        let a = create_role(&db, "A", true).await;
        let b = create_role(&db, "B", true).await;
        let _disconnected = create_role(&db, "disconnected", true).await;

        add_member(&db, a.id, user.id).await;
        add_inheritance(&db, b.id, a.id).await;

        let roles = resolve_user_roles(&db, user.id).await.unwrap();
        assert_eq!(roles.len(), 2);
    }

    // U4: BFS traversal: single role with no parents/children
    #[tokio::test]
    async fn u4_bfs_single_role() {
        let db = setup().await;
        let user = create_user(&db, "alice").await;
        let a = create_role(&db, "A", true).await;
        add_member(&db, a.id, user.id).await;

        let roles = resolve_user_roles(&db, user.id).await.unwrap();
        assert_eq!(roles.len(), 1);
        assert_eq!(roles[0], a.id);
    }

    // U5: BFS traversal: empty graph (user has no roles) → empty vec
    #[tokio::test]
    async fn u5_bfs_no_roles() {
        let db = setup().await;
        let user = create_user(&db, "alice").await;
        let roles = resolve_user_roles(&db, user.id).await.unwrap();
        assert!(roles.is_empty());
    }

    // U6: BFS skips inactive roles — chain A→B(inactive)→C returns [A] only
    #[tokio::test]
    async fn u6_bfs_skips_inactive() {
        let db = setup().await;
        let user = create_user(&db, "alice").await;
        let a = create_role(&db, "A", true).await;
        let b = create_role(&db, "B_inactive", false).await;
        let c = create_role(&db, "C", true).await;

        add_member(&db, a.id, user.id).await;
        add_inheritance(&db, b.id, a.id).await;
        add_inheritance(&db, c.id, b.id).await;

        let roles = resolve_user_roles(&db, user.id).await.unwrap();
        assert_eq!(roles.len(), 1, "Only active role A should be returned");
        assert_eq!(roles[0], a.id);
    }

    // U7: Cycle detection: direct cycle A→B→A detected
    #[tokio::test]
    async fn u7_cycle_detection_direct() {
        let db = setup().await;
        let a = create_role(&db, "A", true).await;
        let b = create_role(&db, "B", true).await;
        add_inheritance(&db, b.id, a.id).await; // A's parent is B

        // Try to add B's parent as A — would create A→B→A
        let has_cycle = detect_cycle(&db, a.id, b.id).await.unwrap();
        assert!(has_cycle);
    }

    // U8: Cycle detection: indirect cycle A→B→C→A detected
    #[tokio::test]
    async fn u8_cycle_detection_indirect() {
        let db = setup().await;
        let a = create_role(&db, "A", true).await;
        let b = create_role(&db, "B", true).await;
        let c = create_role(&db, "C", true).await;
        add_inheritance(&db, b.id, a.id).await;
        add_inheritance(&db, c.id, b.id).await;

        // Try to add A as parent of C — would create cycle
        let has_cycle = detect_cycle(&db, a.id, c.id).await.unwrap();
        assert!(has_cycle);
    }

    // U9: Cycle detection: self-reference A→A detected
    #[tokio::test]
    async fn u9_cycle_self_reference() {
        let db = setup().await;
        let a = create_role(&db, "A", true).await;
        let has_cycle = detect_cycle(&db, a.id, a.id).await.unwrap();
        assert!(has_cycle);
    }

    // U10: Cycle detection: no false positive on valid DAG
    #[tokio::test]
    async fn u10_no_false_positive() {
        let db = setup().await;
        let a = create_role(&db, "A", true).await;
        let b = create_role(&db, "B", true).await;
        let c = create_role(&db, "C", true).await;
        add_inheritance(&db, b.id, a.id).await;

        // Adding C as parent of A is valid (no cycle)
        let has_cycle = detect_cycle(&db, c.id, a.id).await.unwrap();
        assert!(!has_cycle);
    }

    // U11: Depth check: chain of 10 → depth=10, accepted
    #[tokio::test]
    async fn u11_depth_check_at_limit() {
        let db = setup().await;
        let mut roles = Vec::new();
        for i in 0..11 {
            roles.push(create_role(&db, &format!("R{i}"), true).await);
        }
        // Chain: R0 → R1 → ... → R10
        for i in 0..10 {
            add_inheritance(&db, roles[i + 1].id, roles[i].id).await;
        }

        let depth = check_inheritance_depth(&db, roles[0].id).await.unwrap();
        assert_eq!(depth, 10);
    }

    // U12: Depth check: chain of 10, adding 11th → rejected
    #[tokio::test]
    async fn u12_depth_check_exceeds_limit() {
        let db = setup().await;
        let mut roles = Vec::new();
        for i in 0..12 {
            roles.push(create_role(&db, &format!("R{i}"), true).await);
        }
        for i in 0..11 {
            add_inheritance(&db, roles[i + 1].id, roles[i].id).await;
        }

        // Check depth from R0 (which already has depth 11)
        let depth = check_inheritance_depth(&db, roles[0].id).await.unwrap();
        assert!(depth > MAX_INHERITANCE_DEPTH);
    }

    // U13: resolve_all_role_members includes direct + inherited via child subtree
    #[tokio::test]
    async fn u13_resolve_all_members() {
        let db = setup().await;
        let user1 = create_user(&db, "alice").await;
        let user2 = create_user(&db, "bob").await;
        let parent = create_role(&db, "parent", true).await;
        let child = create_role(&db, "child", true).await;

        add_member(&db, parent.id, user1.id).await;
        add_member(&db, child.id, user2.id).await;
        add_inheritance(&db, parent.id, child.id).await; // child's parent is parent

        let members = resolve_all_role_members(&db, parent.id).await.unwrap();
        let member_set: HashSet<Uuid> = members.into_iter().collect();
        assert!(
            member_set.contains(&user1.id),
            "Direct member should be included"
        );
        assert!(
            member_set.contains(&user2.id),
            "Child role member should be included"
        );
    }

    // U14: resolve_all_role_members deduplicates users in multiple child roles
    #[tokio::test]
    async fn u14_resolve_all_members_dedup() {
        let db = setup().await;
        let user = create_user(&db, "alice").await;
        let parent = create_role(&db, "parent", true).await;
        let child1 = create_role(&db, "child1", true).await;
        let child2 = create_role(&db, "child2", true).await;

        add_member(&db, child1.id, user.id).await;
        add_member(&db, child2.id, user.id).await;
        add_inheritance(&db, parent.id, child1.id).await;
        add_inheritance(&db, parent.id, child2.id).await;

        let members = resolve_all_role_members(&db, parent.id).await.unwrap();
        assert_eq!(
            members.len(),
            1,
            "User should appear only once despite being in 2 child roles"
        );
    }

    // U15: Performance: 100 roles complex hierarchy resolves quickly
    #[tokio::test]
    async fn u15_performance_100_roles() {
        let db = setup().await;
        let user = create_user(&db, "alice").await;

        let mut roles = Vec::new();
        for i in 0..100 {
            roles.push(create_role(&db, &format!("R{i:03}"), true).await);
        }

        // Create a chain for the first 10, then fan out
        add_member(&db, roles[0].id, user.id).await;
        for i in 0..9 {
            add_inheritance(&db, roles[i + 1].id, roles[i].id).await;
        }
        // Fan out from role 5
        for i in 10..50 {
            add_inheritance(&db, roles[i].id, roles[5].id).await;
        }

        let start = std::time::Instant::now();
        let resolved = resolve_user_roles(&db, user.id).await.unwrap();
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() < 1000,
            "Should resolve in <1s, took {:?}",
            elapsed
        );
        // Chain: 0-9 (10 roles) + fan out: 10-49 (40 roles, but only reachable up to depth 10)
        assert!(!resolved.is_empty());
    }

    // U16: check_inheritance_depth_down — chain A→B→C→D, depth_down(B) = 2
    #[tokio::test]
    async fn u16_depth_down_chain() {
        let db = setup().await;
        let a = create_role(&db, "A", true).await;
        let b = create_role(&db, "B", true).await;
        let c = create_role(&db, "C", true).await;
        let d = create_role(&db, "D", true).await;

        // Chain: D is child of C, C is child of B, B is child of A
        add_inheritance(&db, a.id, b.id).await; // B's parent is A
        add_inheritance(&db, b.id, c.id).await; // C's parent is B
        add_inheritance(&db, c.id, d.id).await; // D's parent is C

        let depth = check_inheritance_depth_down(&db, b.id).await.unwrap();
        assert_eq!(depth, 2, "B has C and D below it → depth_down = 2");

        let depth_a = check_inheritance_depth_down(&db, a.id).await.unwrap();
        assert_eq!(depth_a, 3, "A has B, C, D below it → depth_down = 3");

        let depth_d = check_inheritance_depth_down(&db, d.id).await.unwrap();
        assert_eq!(depth_d, 0, "D is a leaf → depth_down = 0");
    }

    // U17: add_parent should reject when total depth (above + 1 + below) exceeds limit
    #[tokio::test]
    async fn u17_total_depth_check() {
        let db = setup().await;
        // Build chain of 5 above: R0→R1→R2→R3→R4 (parent chain for R0)
        let mut above = Vec::new();
        for i in 0..5 {
            above.push(create_role(&db, &format!("Above{i}"), true).await);
        }
        for i in 0..4 {
            add_inheritance(&db, above[i + 1].id, above[i].id).await;
        }
        // above[0] has depth_up = 4

        // Build chain of 5 below: B0→B1→B2→B3→B4 (child chain from B0)
        let mut below = Vec::new();
        for i in 0..6 {
            below.push(create_role(&db, &format!("Below{i}"), true).await);
        }
        for i in 0..5 {
            add_inheritance(&db, below[i].id, below[i + 1].id).await;
        }
        // below[0] has depth_down = 5

        // Adding above[0] as parent of below[0]: total = 4 + 1 + 5 = 10, OK
        let depth_above = check_inheritance_depth(&db, above[0].id).await.unwrap();
        let depth_below = check_inheritance_depth_down(&db, below[0].id)
            .await
            .unwrap();
        assert_eq!(depth_above, 4);
        assert_eq!(depth_below, 5);
        assert_eq!(
            depth_above + 1 + depth_below,
            10,
            "Total depth = 10, at limit"
        );

        // If we add one more child, total would be 11 → should exceed
        let extra = create_role(&db, "ExtraChild", true).await;
        add_inheritance(&db, below[5].id, extra.id).await;
        let depth_below2 = check_inheritance_depth_down(&db, below[0].id)
            .await
            .unwrap();
        assert_eq!(depth_below2, 6);
        assert!(
            depth_above + 1 + depth_below2 > MAX_INHERITANCE_DEPTH,
            "Total depth 11 > 10"
        );
    }

    // U18: resolve_ancestor_roles — full ancestor chain
    #[tokio::test]
    async fn u18_resolve_ancestors() {
        let db = setup().await;
        let a = create_role(&db, "A", true).await;
        let b = create_role(&db, "B", true).await;
        let c = create_role(&db, "C", true).await;

        add_inheritance(&db, b.id, a.id).await; // A's parent is B
        add_inheritance(&db, c.id, b.id).await; // B's parent is C

        let ancestors = resolve_ancestor_roles(&db, a.id).await.unwrap();
        let ancestor_set: HashSet<Uuid> = ancestors.into_iter().collect();
        assert_eq!(ancestor_set.len(), 2);
        assert!(ancestor_set.contains(&b.id));
        assert!(ancestor_set.contains(&c.id));

        // B's ancestors should be only C
        let b_ancestors = resolve_ancestor_roles(&db, b.id).await.unwrap();
        assert_eq!(b_ancestors.len(), 1);
        assert_eq!(b_ancestors[0], c.id);

        // C has no ancestors
        let c_ancestors = resolve_ancestor_roles(&db, c.id).await.unwrap();
        assert!(c_ancestors.is_empty());
    }
}
