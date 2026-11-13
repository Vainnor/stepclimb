// SPDX-License-Identifier: AGPL-3.0-or-later
//! Print the OpenAPI document. `just openapi` writes it to `openapi/stepclimb.json`, and
//! `just openapi-diff` compares it with the committed copy.

use std::io::Write;

fn main() -> std::io::Result<()> {
    std::io::stdout().write_all(stepclimb_server::openapi::document_json().as_bytes())
}
