use axum::Router;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::{services::documentation::ApiDoc, state::SharedState};

/// Serve the Swagger UI backed by the generated OpenAPI document.
pub fn router(state: SharedState) -> Router<SharedState> {
    let ui: Router<SharedState> = SwaggerUi::new("/docs")
        .url("/api-doc/openapi.json", ApiDoc::openapi())
        .into();

    ui.with_state(state)
}
