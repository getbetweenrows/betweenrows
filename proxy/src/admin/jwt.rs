use axum::{
    extract::{FromRef, FromRequestParts},
    http::{StatusCode, request::Parts},
};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::AdminState;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    /// User id (UUID, stored as string in JWT)
    pub sub: Uuid,
    pub username: String,
    pub is_admin: bool,
    /// Unix timestamp expiry
    pub exp: u64,
}

pub fn encode_jwt(claims: &Claims, secret: &str) -> Result<String, jsonwebtoken::errors::Error> {
    encode(
        &Header::default(),
        claims,
        &EncodingKey::from_secret(secret.as_ref()),
    )
}

pub fn decode_jwt(token: &str, secret: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_ref()),
        &Validation::new(Algorithm::HS256),
    )?;
    Ok(data.claims)
}

fn extract_bearer(parts: &Parts) -> Option<&str> {
    parts
        .headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
}

/// Extractor: validates Bearer token, requires is_admin == true.
pub struct AdminClaims(pub Claims);

impl<S> FromRequestParts<S> for AdminClaims
where
    S: Send + Sync,
    AdminState: FromRef<S>,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let state = AdminState::from_ref(state);

        let token = extract_bearer(parts).ok_or((
            StatusCode::UNAUTHORIZED,
            "Missing or invalid Authorization header",
        ))?;

        let claims = decode_jwt(token, &state.jwt_secret)
            .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or expired token"))?;

        if !claims.is_admin {
            return Err((StatusCode::FORBIDDEN, "Admin access required"));
        }

        Ok(AdminClaims(claims))
    }
}

/// Extractor: validates Bearer token (any authenticated user).
pub struct AuthClaims(pub Claims);

impl<S> FromRequestParts<S> for AuthClaims
where
    S: Send + Sync,
    AdminState: FromRef<S>,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let state = AdminState::from_ref(state);

        let token = extract_bearer(parts).ok_or((
            StatusCode::UNAUTHORIZED,
            "Missing or invalid Authorization header",
        ))?;

        let claims = decode_jwt(token, &state.jwt_secret)
            .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or expired token"))?;

        Ok(AuthClaims(claims))
    }
}
