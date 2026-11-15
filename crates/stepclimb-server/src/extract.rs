// SPDX-License-Identifier: AGPL-3.0-or-later
//! Extractors that answer problem details when they reject (spec 02 B2).
//!
//! axum's own `Json`, `Path` and `Query` reject with plain text. Handlers take these wrappers
//! instead, which convert the rejection into an [`ApiError`] (`error.rs` has the mapping), so
//! no plain-text error leaves the server.

use axum::{
    Json,
    extract::{FromRequest, FromRequestParts, Path, Query, Request},
    http::request::Parts,
};
use serde::de::DeserializeOwned;

use crate::error::ApiError;

mod maps_only;

use maps_only::MapsOnly;

/// A JSON request body: 415 when it is not JSON, 400 when it does not parse or match `T`.
///
/// Every struct in `T`, at any depth, must be a JSON object: an array in its fields' order is
/// a 400 too ([`maps_only`]), so the order of a body type's fields is not part of the API.
#[derive(Debug, Clone, Copy, Default)]
pub struct ApiJson<T>(pub T);

impl<T, S> FromRequest<S> for ApiJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(MapsOnly(value)) = Json::<MapsOnly<T>>::from_request(req, state).await?;
        Ok(Self(value))
    }
}

/// Path parameters: 400 when one does not parse as `T`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ApiPath<T>(pub T);

impl<T, S> FromRequestParts<S> for ApiPath<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Path(value) = Path::<T>::from_request_parts(parts, state).await?;
        Ok(Self(value))
    }
}

/// The query string: 400 when it does not parse as `T`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ApiQuery<T>(pub T);

impl<T, S> FromRequestParts<S> for ApiQuery<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Query(value) = Query::<T>::from_request_parts(parts, state).await?;
        Ok(Self(value))
    }
}
