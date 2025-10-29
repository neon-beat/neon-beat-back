//! Binary that generates the OpenAPI 3.1 specification from the Rust code.
//!
//! This tool uses `utoipa` to extract API documentation from the codebase and
//! outputs a JSON representation of the OpenAPI specification to stdout.
//!
//! # Usage
//!
//! ```bash
//! cargo build --bin openapi-generator
//! ./target/debug/openapi-generator > docs/openapi.json
//! ```

use neon_beat_back::services::documentation::ApiDoc;
use utoipa::OpenApi;

fn main() {
    let doc = ApiDoc::openapi();
    println!("{}", doc.to_pretty_json().unwrap());
}
