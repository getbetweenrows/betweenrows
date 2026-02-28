use axum::{extract::State, http::StatusCode, response::Json};
use chrono::Utc;
use sea_orm::EntityTrait;

use crate::entity::proxy_user;

use super::{
    AdminState, ApiErr,
    dto::{LoginRequest, LoginResponse, UserResponse},
    jwt::{AuthClaims, Claims, encode_jwt},
};

pub async fn login(
    State(state): State<AdminState>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, ApiErr> {
    let user = state
        .auth
        .authenticate_for_api(&body.username, &body.password)
        .await
        .map_err(|_| ApiErr::new(StatusCode::UNAUTHORIZED, "Invalid credentials"))?;

    if !user.is_admin {
        return Err(ApiErr::new(StatusCode::FORBIDDEN, "Admin access required"));
    }

    let exp = (Utc::now().timestamp() as u64) + state.jwt_expiry_hours * 3600;
    let claims = Claims {
        sub: user.id,
        username: user.username.clone(),
        is_admin: user.is_admin,
        exp,
    };

    let token = encode_jwt(&claims, &state.jwt_secret).map_err(ApiErr::internal)?;

    Ok(Json(LoginResponse {
        token,
        user: UserResponse::from(user),
    }))
}

pub async fn me(
    AuthClaims(claims): AuthClaims,
    State(state): State<AdminState>,
) -> Result<Json<UserResponse>, ApiErr> {
    let user = proxy_user::Entity::find_by_id(claims.sub)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("User not found"))?;

    Ok(Json(UserResponse::from(user)))
}
