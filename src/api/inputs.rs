//! Status API for input sources.

use crate::inputs::inputs_status;
use rocket::get;
use rocket::serde::json::Json;

/// Report the configured input sources, their bound devices and last keypress.
///
/// This is the "is my remote detected?" endpoint.
#[get("/")]
pub fn get_inputs_status() -> Json<serde_json::Value> {
    Json(inputs_status())
}
