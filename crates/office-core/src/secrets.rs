//! Secret names the plugin declares.

/// The names `spec.secrets` declares in plugin.yaml; tests hold the two equal.
/// Handlers report presence (`set`/`unset`) only, never a value (STD-05).
pub const SECRET_NAMES: [&str; 2] = ["meta_api_key", "baseten_token"];
