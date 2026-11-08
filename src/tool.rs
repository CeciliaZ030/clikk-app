use crate::client::*;
use aomi_sdk::schemars::JsonSchema;
use aomi_sdk::*;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct GetClikkProfileArgs {
    /// The member's exact public Clikk handle, e.g. `zoey`. Case-insensitive,
    /// 3-30 letters, digits, or hyphens. A pasted profile link such as
    /// `https://clikk.pro/zoey` or `@zoey` is also accepted.
    pub(crate) handle: String,
    /// Optional filter on field kinds to return: any of `email`, `phone`,
    /// `url`, `text`, `handle`, `org`. Omit to return every published field.
    #[serde(default)]
    pub(crate) kinds: Option<Vec<String>>,
}

pub(crate) struct GetClikkProfile;

impl DynAomiTool for GetClikkProfile {
    type App = ClikkApp;
    type Args = GetClikkProfileArgs;
    const NAME: &'static str = "get_clikk_profile";
    const DESCRIPTION: &'static str = "Fetch one Clikk member's published public card (display name, photo, contact fields, page and vCard links) by exact handle. Read-only. Returns found:false when no public profile exists at that handle; there is no search, so never guess handles.";

    fn run(_app: &ClikkApp, args: Self::Args, ctx: DynToolCallCtx) -> Result<Value, String> {
        let handle = normalize_handle(&args.handle)?;
        let client = ClikkClient::from_ctx(&ctx)?;
        match client.get_profile(&handle)? {
            Lookup::Found(profile) => Ok(profile_to_json(&handle, &profile, args.kinds.as_deref())),
            Lookup::NotFound => Ok(not_found_json(&handle)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aomi_sdk::testing::{TestCtxBuilder, run_tool};

    const SPEC_EXAMPLE: &str = r#"{
      "handle": "zoey",
      "display_name": "Zoey Tan",
      "photo_url": "https://example.test/i/3f9c0e2a7b.jpg",
      "fields": [
        {"kind": "email", "label": "Email", "value": "zoey@example.com"},
        {"kind": "org", "label": "Company", "value": "Acme Pte Ltd"},
        {"kind": "url", "label": "Website", "value": "https://zoey.example.com"}
      ],
      "links": {"page": "https://example.test/zoey", "vcard": "https://example.test/zoey.vcf"},
      "consent": {"source": "published_card", "description": "Published by the member.", "allow_indexing": false}
    }"#;

    #[test]
    fn handle_normalization_accepts_links_and_rejects_bad_input() {
        assert_eq!(normalize_handle("Zoey").unwrap(), "zoey");
        assert_eq!(normalize_handle("@zoey").unwrap(), "zoey");
        assert_eq!(normalize_handle("https://clikk.pro/zoey").unwrap(), "zoey");
        assert_eq!(normalize_handle("clikk.pro/zoey/").unwrap(), "zoey");
        assert_eq!(
            normalize_handle("https://clikk.pro/zoey.vcf").unwrap(),
            "zoey"
        );
        assert!(normalize_handle("a!").is_err());
        assert!(normalize_handle("ab").is_err());
        assert!(normalize_handle(&"x".repeat(31)).is_err());
        assert!(normalize_handle("zoey tan").is_err());
    }

    #[test]
    fn profile_json_flattens_contact_and_flags_rename() {
        let profile: PublicProfile = serde_json::from_str(SPEC_EXAMPLE).unwrap();
        let out = profile_to_json("zoey-old", &profile, None);
        assert_eq!(out["found"], true);
        assert_eq!(out["handle"], "zoey");
        assert_eq!(out["renamed"], true);
        assert_eq!(out["fields"].as_array().unwrap().len(), 3);
        assert_eq!(out["contact"]["emails"][0], "zoey@example.com");
        assert_eq!(out["contact"]["organizations"][0], "Acme Pte Ltd");
        assert_eq!(out["consent"]["allow_indexing"], false);

        let only_email = profile_to_json("zoey", &profile, Some(&["Email".to_string()]));
        assert_eq!(only_email["renamed"], false);
        assert_eq!(only_email["fields"].as_array().unwrap().len(), 1);
        assert_eq!(only_email["fields"][0]["kind"], "email");
    }

    #[test]
    fn missing_key_is_a_clear_error() {
        // SAFETY-ish: tests in this crate do not otherwise read this var.
        unsafe { std::env::remove_var("CLIKK_API_KEY") };
        let ctx = TestCtxBuilder::new(GetClikkProfile::NAME).build();
        let err =
            run_tool::<GetClikkProfile>(&ClikkApp, serde_json::json!({"handle": "zoey"}), ctx)
                .unwrap_err();
        assert!(err.contains("CLIKK_API_KEY"), "{err}");
    }

    #[test]
    fn invalid_handle_fails_before_any_request() {
        let ctx = TestCtxBuilder::new(GetClikkProfile::NAME)
            .secret("CLIKK_API_KEY", "ck_test")
            .build();
        let err = run_tool::<GetClikkProfile>(&ClikkApp, serde_json::json!({"handle": "a!"}), ctx)
            .unwrap_err();
        assert!(err.contains("not a valid Clikk handle"), "{err}");
    }

    /// Live check against the real API. Run with:
    /// `CLIKK_API_KEY=ck_... cargo test -p clikk -- --ignored`
    #[test]
    #[ignore]
    fn live_lookup_known_and_unknown_handles() {
        let key = std::env::var("CLIKK_API_KEY").expect("CLIKK_API_KEY");
        let ctx = || {
            TestCtxBuilder::new(GetClikkProfile::NAME)
                .secret("CLIKK_API_KEY", key.clone())
                .build()
        };
        let found =
            run_tool::<GetClikkProfile>(&ClikkApp, serde_json::json!({"handle": "derrick"}), ctx())
                .unwrap()
                .into_value();
        assert_eq!(found["found"], true);
        assert_eq!(found["handle"], "derrick");
        assert!(found["links"]["vcard"].as_str().unwrap().ends_with(".vcf"));

        let missing =
            run_tool::<GetClikkProfile>(&ClikkApp, serde_json::json!({"handle": "zoey"}), ctx())
                .unwrap()
                .into_value();
        assert_eq!(missing["found"], false);
    }
}
