//! HTTP client, auth, models, and normalization for the Clikk Profile API.
//!
//! Contract (from `GET {base}/api/v1/openapi.json`):
//! - `GET /api/v1/profiles/{handle}` with `Authorization: Bearer <key>`
//! - 200 -> `PublicProfile`; 404 -> no published profile (uniform for
//!   unpublished and nonexistent); 400 invalid handle; 401 bad key;
//!   503 retry later.
//! - Renamed handles resolve server-side; the response carries the canonical
//!   handle.
//!
//! Observed quirk: a syntactically invalid handle is answered by the web app
//! (HTML) rather than the documented JSON 400, so handles are validated here
//! before any request is sent and non-JSON bodies are never echoed.

use aomi_sdk::{DynToolCallCtx, Secret, resolve_secret_value};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Clone, Default)]
pub(crate) struct ClikkApp;

/// Clikk-issued API key (`ck_...`), sent as a bearer token on every request.
pub(crate) const API_KEY: Secret = Secret::new(
    "CLIKK_API_KEY",
    "Clikk-issued API key (ck_...) sent as `Authorization: Bearer <key>`.",
    true,
);

/// Optional base URL override. Defaults to the current Vercel deployment;
/// set to `https://clikk.pro` once Clikk's PR #215 ships. Same paths, same key.
pub(crate) const API_BASE_URL: Secret = Secret::new(
    "CLIKK_API_BASE_URL",
    "Optional Clikk API base URL override (default: current Vercel deployment; later https://clikk.pro).",
    false,
);

pub(crate) const DEFAULT_API_BASE: &str =
    "https://clikk-v5-1-m82xkabwe-derricksiu-mecoms-projects.vercel.app";

/// Handle grammar from the spec: `^[a-zA-Z0-9-]{3,30}$`, case-insensitive.
pub(crate) fn normalize_handle(raw: &str) -> Result<String, String> {
    let mut handle = raw.trim();
    // Accept pasted profile links such as `https://clikk.pro/zoey`, `clikk.pro/zoey`
    // or `@zoey`; the last path segment is the handle.
    if let Some(stripped) = handle.strip_prefix('@') {
        handle = stripped;
    }
    if handle.contains('/') {
        handle = handle
            .trim_end_matches('/')
            .trim_end_matches(".vcf")
            .rsplit('/')
            .next()
            .unwrap_or("");
    }
    let handle = handle.trim_end_matches(".vcf");
    let valid_len = (3..=30).contains(&handle.len());
    let valid_chars = handle
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-');
    if !valid_len || !valid_chars {
        return Err(format!(
            "`{raw}` is not a valid Clikk handle. Handles are 3-30 characters of letters, digits, or hyphens."
        ));
    }
    Ok(handle.to_ascii_lowercase())
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PublicProfile {
    pub handle: String,
    pub display_name: String,
    #[serde(default)]
    pub photo_url: Option<String>,
    #[serde(default)]
    pub fields: Vec<ProfileField>,
    pub links: ProfileLinks,
    pub consent: Consent,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ProfileField {
    pub kind: String,
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ProfileLinks {
    pub page: String,
    pub vcard: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Consent {
    pub source: String,
    #[serde(default)]
    pub description: String,
    pub allow_indexing: bool,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    error: ApiErrorBody,
}

#[derive(Debug, Deserialize)]
struct ApiErrorBody {
    code: String,
    message: String,
}

/// Outcome of a profile lookup. `NotFound` is a normal result, not an error:
/// the API deliberately does not distinguish unpublished from nonexistent.
pub(crate) enum Lookup {
    Found(PublicProfile),
    NotFound,
}

pub(crate) struct ClikkClient {
    http: reqwest::blocking::Client,
    base_url: String,
    api_key: String,
}

impl ClikkClient {
    pub(crate) fn from_ctx(ctx: &DynToolCallCtx) -> Result<Self, String> {
        let api_key = resolve_secret_value(
            ctx,
            None,
            API_KEY.name,
            "CLIKK_API_KEY is not set; the Clikk Profile API requires a bearer API key.",
        )?;
        let base_url = resolve_secret_value(ctx, None, API_BASE_URL.name, "")
            .unwrap_or_else(|_| DEFAULT_API_BASE.to_string());
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| format!("failed to build HTTP client: {e}"))?;
        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
        })
    }

    pub(crate) fn get_profile(&self, handle: &str) -> Result<Lookup, String> {
        let url = format!("{}/api/v1/profiles/{handle}", self.base_url);
        let response = self
            .http
            .get(&url)
            .bearer_auth(&self.api_key)
            .header("Accept", "application/json")
            .send()
            .map_err(|e| format!("Clikk request failed: {e}"))?;

        let status = response.status();
        let body = response.text().unwrap_or_default();

        if status.is_success() {
            return serde_json::from_str::<PublicProfile>(&body)
                .map(Lookup::Found)
                .map_err(|e| format!("Clikk returned an unexpected profile shape: {e}"));
        }

        let api_error = serde_json::from_str::<ApiError>(&body).ok();
        match (status.as_u16(), api_error) {
            (404, _) => Ok(Lookup::NotFound),
            (401, _) => Err("Clikk rejected the API key (401). Check CLIKK_API_KEY.".to_string()),
            (400, Some(e)) => Err(format!("Clikk rejected the handle: {}", e.error.message)),
            (400, None) => Err("Clikk rejected the handle as invalid (400).".to_string()),
            (503, _) => Err(
                "Clikk's profile store is temporarily unavailable (503). Retry shortly."
                    .to_string(),
            ),
            (code, Some(e)) => Err(format!(
                "Clikk error {code} ({}): {}",
                e.error.code, e.error.message
            )),
            (code, None) => Err(format!("Clikk error {code} with a non-JSON response.")),
        }
    }
}

/// Flatten the published fields into a compact JSON result for the model.
pub(crate) fn profile_to_json(
    requested_handle: &str,
    profile: &PublicProfile,
    kinds: Option<&[String]>,
) -> Value {
    let wanted = kinds.map(|k| {
        k.iter()
            .map(|s| s.trim().to_ascii_lowercase())
            .collect::<Vec<_>>()
    });
    let fields: Vec<&ProfileField> = profile
        .fields
        .iter()
        .filter(|f| match &wanted {
            Some(w) if !w.is_empty() => w.iter().any(|k| k == &f.kind),
            _ => true,
        })
        .collect();

    let values_of = |kind: &str| -> Vec<&str> {
        profile
            .fields
            .iter()
            .filter(|f| f.kind == kind)
            .map(|f| f.value.as_str())
            .collect()
    };

    json!({
        "found": true,
        "handle": profile.handle,
        "requested_handle": requested_handle,
        "renamed": !profile.handle.eq_ignore_ascii_case(requested_handle),
        "display_name": profile.display_name,
        "photo_url": profile.photo_url,
        "fields": fields.iter().map(|f| json!({
            "kind": f.kind,
            "label": f.label,
            "value": f.value,
        })).collect::<Vec<_>>(),
        "contact": {
            "emails": values_of("email"),
            "phones": values_of("phone"),
            "websites": values_of("url"),
            "social_handles": values_of("handle"),
            "organizations": values_of("org"),
        },
        "links": {
            "page": profile.links.page,
            "vcard": profile.links.vcard,
        },
        "consent": {
            "source": profile.consent.source,
            "allow_indexing": profile.consent.allow_indexing,
            "note": profile.consent.description,
        },
    })
}

pub(crate) fn not_found_json(requested_handle: &str) -> Value {
    json!({
        "found": false,
        "requested_handle": requested_handle,
        "reason": "No published Clikk profile at this handle. Unpublished and nonexistent handles are indistinguishable by design; this does not mean the person does not exist.",
    })
}
