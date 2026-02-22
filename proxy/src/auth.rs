use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use chrono::Utc;
use password_hash::SaltString;
use pgwire::error::{PgWireError, PgWireResult};
use rand_core::OsRng;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, Set};
use uuid::Uuid;

use crate::entity::proxy_user;

/// Error type for API-layer authentication (avoids coupling to pgwire).
#[derive(Debug)]
pub enum AuthApiError {
    NotFound,
    InvalidPassword,
    Inactive,
    Db(sea_orm::DbErr),
    Hash(String),
}

impl std::fmt::Display for AuthApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthApiError::NotFound => write!(f, "User not found"),
            AuthApiError::InvalidPassword => write!(f, "Invalid password"),
            AuthApiError::Inactive => write!(f, "User is inactive"),
            AuthApiError::Db(e) => write!(f, "Database error: {e}"),
            AuthApiError::Hash(e) => write!(f, "Hash error: {e}"),
        }
    }
}

impl std::error::Error for AuthApiError {}

pub struct Auth {
    db: DatabaseConnection,
}

impl Auth {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    /// Expose the underlying DB connection for direct SeaORM queries.
    pub fn db(&self) -> &DatabaseConnection {
        &self.db
    }

    /// Authenticate a user for the REST API, returning the model on success.
    /// Does NOT update `last_login_at` (that's the pgwire path's concern).
    pub async fn authenticate_for_api(
        &self,
        username: &str,
        password: &str,
    ) -> Result<proxy_user::Model, AuthApiError> {
        let user = proxy_user::Entity::find()
            .filter(proxy_user::Column::Username.eq(username))
            .one(&self.db)
            .await
            .map_err(AuthApiError::Db)?
            .ok_or(AuthApiError::NotFound)?;

        if !user.is_active {
            return Err(AuthApiError::Inactive);
        }

        let hash = PasswordHash::new(&user.password_hash)
            .map_err(|e| AuthApiError::Hash(e.to_string()))?;

        Argon2::default()
            .verify_password(password.as_bytes(), &hash)
            .map_err(|_| AuthApiError::InvalidPassword)?;

        // Update last_login_at
        let mut active: proxy_user::ActiveModel = user.clone().into();
        active.last_login_at = Set(Some(Utc::now().naive_utc()));
        active.update(&self.db).await.map_err(AuthApiError::Db)?;

        Ok(user)
    }

    /// Verify username/password against the admin store.
    /// Returns the user model on success, or `InvalidPassword` on failure.
    pub async fn authenticate(&self, username: &str, password: &str) -> PgWireResult<proxy_user::Model> {
        let user = proxy_user::Entity::find()
            .filter(proxy_user::Column::Username.eq(username))
            .one(&self.db)
            .await
            .map_err(|e| PgWireError::ApiError(Box::new(e)))?
            .ok_or_else(|| PgWireError::InvalidPassword(username.to_owned()))?;

        if !user.is_active {
            return Err(PgWireError::InvalidPassword(username.to_owned()));
        }

        let hash = PasswordHash::new(&user.password_hash).map_err(|e| {
            PgWireError::ApiError(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            )))
        })?;

        Argon2::default()
            .verify_password(password.as_bytes(), &hash)
            .map_err(|_| PgWireError::InvalidPassword(username.to_owned()))?;

        // Update last_login_at on successful auth
        let mut active: proxy_user::ActiveModel = user.clone().into();
        active.last_login_at = Set(Some(Utc::now().naive_utc()));
        active
            .update(&self.db)
            .await
            .map_err(|e| PgWireError::ApiError(Box::new(e)))?;

        Ok(user)
    }

    /// Create a new proxy user with an Argon2-hashed password.
    pub async fn create_user(
        &self,
        username: &str,
        password: &str,
        tenant: &str,
        is_admin: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let password_hash = Self::hash_password(password)?;
        let now = Utc::now().naive_utc();
        proxy_user::ActiveModel {
            id: Set(Uuid::now_v7()),
            username: Set(username.to_owned()),
            password_hash: Set(password_hash),
            tenant: Set(tenant.to_owned()),
            is_admin: Set(is_admin),
            is_active: Set(true),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&self.db)
        .await?;
        Ok(())
    }

    /// Return the total number of users in the admin store.
    pub async fn count_users(&self) -> Result<u64, Box<dyn std::error::Error>> {
        let count = proxy_user::Entity::find().count(&self.db).await?;
        Ok(count)
    }

    /// Hash a plaintext password with Argon2id + a random salt.
    pub fn hash_password(password: &str) -> Result<String, Box<dyn std::error::Error>> {
        let salt = SaltString::generate(&mut OsRng);
        let hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
            })?
            .to_string();
        Ok(hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use migration::{Migrator, MigratorTrait};
    use sea_orm::{Database, EntityTrait};

    async fn setup() -> Auth {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        Migrator::up(&db, None).await.unwrap();
        Auth::new(db)
    }

    // --- hash_password ---

    #[tokio::test]
    async fn test_hash_produces_argon2_format() {
        let hash = Auth::hash_password("hunter2").unwrap();
        assert!(hash.starts_with("$argon2"), "Expected Argon2 PHC string, got: {}", hash);
    }

    #[tokio::test]
    async fn test_hash_unique_per_call() {
        // Two hashes of the same password must differ (random salt)
        let h1 = Auth::hash_password("same").unwrap();
        let h2 = Auth::hash_password("same").unwrap();
        assert_ne!(h1, h2, "Same password hashed twice should produce different hashes");
    }

    #[tokio::test]
    async fn test_hash_verifies_correctly() {
        let hash = Auth::hash_password("correct horse battery staple").unwrap();
        let parsed = PasswordHash::new(&hash).unwrap();
        Argon2::default()
            .verify_password(b"correct horse battery staple", &parsed)
            .expect("Should verify successfully");
    }

    // --- count_users / create_user ---

    #[tokio::test]
    async fn test_empty_store_count_is_zero() {
        let auth = setup().await;
        assert_eq!(auth.count_users().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_create_user_increments_count() {
        let auth = setup().await;
        auth.create_user("alice", "pw1", "acme", false).await.unwrap();
        assert_eq!(auth.count_users().await.unwrap(), 1);
        auth.create_user("bob", "pw2", "acme", true).await.unwrap();
        assert_eq!(auth.count_users().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn test_create_user_stores_hash_not_plaintext() {
        let auth = setup().await;
        auth.create_user("alice", "supersecret", "acme", false).await.unwrap();

        let row = proxy_user::Entity::find()
            .filter(proxy_user::Column::Username.eq("alice"))
            .one(&auth.db)
            .await
            .unwrap()
            .unwrap();

        assert_ne!(row.password_hash, "supersecret", "Plaintext must never be stored");
        assert!(row.password_hash.starts_with("$argon2"), "Must be Argon2 PHC string");
    }

    #[tokio::test]
    async fn test_create_user_stores_correct_fields() {
        let auth = setup().await;
        auth.create_user("charlie", "pw", "widgets-inc", true).await.unwrap();

        let row = proxy_user::Entity::find()
            .filter(proxy_user::Column::Username.eq("charlie"))
            .one(&auth.db)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(row.username, "charlie");
        assert_eq!(row.tenant, "widgets-inc");
        assert!(row.is_admin);
        assert!(row.is_active);
        assert!(row.email.is_none());
        assert!(row.last_login_at.is_none());
    }

    #[tokio::test]
    async fn test_create_user_duplicate_username_errors() {
        let auth = setup().await;
        auth.create_user("alice", "pw", "acme", false).await.unwrap();
        let result = auth.create_user("alice", "other", "other", false).await;
        assert!(result.is_err(), "Duplicate username must fail");
    }

    // --- authenticate ---

    #[tokio::test]
    async fn test_authenticate_success_returns_model() {
        let auth = setup().await;
        auth.create_user("alice", "correct", "acme", false).await.unwrap();

        let user = auth.authenticate("alice", "correct").await.unwrap();
        assert_eq!(user.username, "alice");
        assert_eq!(user.tenant, "acme");
        assert!(!user.is_admin);
    }

    #[tokio::test]
    async fn test_authenticate_updates_last_login_at() {
        let auth = setup().await;
        auth.create_user("alice", "pw", "acme", false).await.unwrap();

        // Before auth: last_login_at is None
        let before = proxy_user::Entity::find()
            .filter(proxy_user::Column::Username.eq("alice"))
            .one(&auth.db)
            .await
            .unwrap()
            .unwrap();
        assert!(before.last_login_at.is_none());

        auth.authenticate("alice", "pw").await.unwrap();

        // After auth: last_login_at is set
        let after = proxy_user::Entity::find()
            .filter(proxy_user::Column::Username.eq("alice"))
            .one(&auth.db)
            .await
            .unwrap()
            .unwrap();
        assert!(after.last_login_at.is_some(), "last_login_at should be set after successful auth");
    }

    #[tokio::test]
    async fn test_authenticate_wrong_password_rejected() {
        let auth = setup().await;
        auth.create_user("alice", "correct", "acme", false).await.unwrap();

        let err = auth.authenticate("alice", "wrong").await.unwrap_err();
        assert!(
            matches!(err, PgWireError::InvalidPassword(ref u) if u == "alice"),
            "Expected InvalidPassword(alice), got {:?}", err
        );
    }

    #[tokio::test]
    async fn test_authenticate_unknown_user_rejected() {
        let auth = setup().await;

        let err = auth.authenticate("nobody", "pw").await.unwrap_err();
        assert!(matches!(err, PgWireError::InvalidPassword(_)));
    }

    #[tokio::test]
    async fn test_authenticate_inactive_user_rejected() {
        let auth = setup().await;
        auth.create_user("alice", "pw", "acme", false).await.unwrap();

        // Deactivate the user
        let row = proxy_user::Entity::find()
            .filter(proxy_user::Column::Username.eq("alice"))
            .one(&auth.db)
            .await
            .unwrap()
            .unwrap();
        let mut active: proxy_user::ActiveModel = row.into();
        active.is_active = Set(false);
        active.update(&auth.db).await.unwrap();

        let err = auth.authenticate("alice", "pw").await.unwrap_err();
        assert!(matches!(err, PgWireError::InvalidPassword(_)));
    }

    #[tokio::test]
    async fn test_authenticate_admin_flag_preserved() {
        let auth = setup().await;
        auth.create_user("root", "pw", "sys", true).await.unwrap();

        let user = auth.authenticate("root", "pw").await.unwrap();
        assert!(user.is_admin);
    }
}
