//! Synthetic provider contracts; no cloud requests or user credential entries.
use j_jump::{
    config::{Config, Paths, Provider},
    credential, engine, provider,
};
use serde_json::{Value, json};
use std::{cell::Cell, fs};

const ACCOUNT: &str = "00000000000000000000000000000001";
fn fixture() -> (tempfile::TempDir, Paths, Config, Vec<u8>) {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().canonicalize().unwrap();
    let paths = Paths {
        config: home.join("config/config.json"),
        data: home.join("data"),
        cache: home.join("cache"),
        home: home.clone(),
    };
    let dir = home.join("server");
    fs::create_dir(&dir).unwrap();
    let group = engine::Group {
        id: "d1".into(),
        name: "server".into(),
        parent: String::new(),
        usage: "low",
        members: vec![engine::Candidate {
            id: "d1".into(),
            path: dir,
            count: 1,
            last_seen: 1,
            weight: 1.0,
        }],
    };
    let cfg = Config {
        semantic: true,
        provider: Provider::ClefFlash,
        cloudflare_account_id: ACCOUNT.into(),
        ..Config::default()
    };
    let request = provider::request("backend", &[group], &home, &paths, &cfg)
        .unwrap()
        .0;
    (temp, paths, cfg, request)
}
fn result(model: &str) -> Value {
    json!({"model":model,"answers":{"destination":{"type":"choice","choice":"d1","probabilities":{"d1":0.9,"none":0.1},"confidence":0.8}},"usage":{"input_tokens":1,"output_tokens":0}})
}
fn response() -> Value {
    json!({"success":true,"errors":[],"messages":[],"result":result("clef-flash")})
}
fn bytes(v: &Value) -> Vec<u8> {
    serde_json::to_vec(v).unwrap()
}

#[test]
fn official_route_and_request_use_the_same_model() {
    let (_t, _paths, _cfg, request) = fixture();
    assert_eq!(
        serde_json::from_slice::<Value>(&request).unwrap()["model"],
        "clef-flash"
    );
    assert_eq!(
        provider::endpoint(Provider::ClefFlash, ACCOUNT).unwrap(),
        format!(
            "https://api.cloudflare.com/client/v4/accounts/{ACCOUNT}/ai/run/@cf/cloudflare/clef-flash"
        )
    );
    assert_eq!(
        provider::endpoint(Provider::Jev, "").unwrap(),
        provider::ENDPOINT
    );
    for id in [
        "",
        "../other",
        "https://example.test",
        "0000000000000000000000000000000/",
        "not-an-account",
    ] {
        assert!(provider::endpoint(Provider::ClefFlash, id).is_err());
    }
    assert!(provider::endpoint(Provider::Jev, ACCOUNT).is_err());
}

#[test]
fn cloudflare_envelope_is_required_and_decision_is_still_bound() {
    let (_t, _paths, _cfg, request) = fixture();
    assert_eq!(
        provider::validate(&bytes(&response()), &request).unwrap(),
        Some("d1".into())
    );
    assert!(provider::validate(&bytes(&result("clef-flash")), &request).is_err());
    let mut mutations = vec![];
    for (key, value) in [
        ("success", json!(false)),
        ("success", json!("true")),
        ("errors", json!([{"message":"synthetic failure"}])),
        ("errors", Value::Null),
        ("result", Value::Null),
        ("messages", json!("invalid")),
    ] {
        let mut v = response();
        v[key] = value;
        mutations.push(v);
    }
    let mut v = response();
    v["result"]["model"] = json!(provider::MODEL);
    mutations.push(v);
    let mut v = response();
    v["result"]["answers"]["destination"]["choice"] = json!("forged");
    mutations.push(v);
    let mut v = response();
    v["result"]["answers"]["destination"]["probabilities"]["d1"] = json!(1.1);
    mutations.push(v);
    let mut v = response();
    v["result"]["answers"]["destination"]["probabilities"]["none"] = json!(0.5);
    mutations.push(v);
    let mut v = response();
    v["result"]["answers"]["extra"] = json!({});
    mutations.push(v);
    for v in mutations {
        assert!(provider::validate(&bytes(&v), &request).is_err(), "{v}");
    }
    let duplicated = String::from_utf8(bytes(&response()))
        .unwrap()
        .replace("\"success\":true", "\"success\":false,\"success\":true");
    assert!(provider::validate(duplicated.as_bytes(), &request).is_err());
    let mut jev_request: Value = serde_json::from_slice(&request).unwrap();
    jev_request["model"] = json!(provider::MODEL);
    assert!(provider::validate(&bytes(&response()), &bytes(&jev_request)).is_err());
    assert_eq!(
        provider::validate(&bytes(&result(provider::MODEL)), &bytes(&jev_request)).unwrap(),
        Some("d1".into())
    );
}

#[test]
fn none_and_tie_never_choose_a_directory() {
    let (_t, _paths, _cfg, request) = fixture();
    for (choice, d1, none) in [("none", 0.1, 0.9), ("d1", 0.5, 0.5)] {
        let mut v = response();
        let a = &mut v["result"]["answers"]["destination"];
        a["choice"] = json!(choice);
        a["probabilities"] = json!({"d1":d1,"none":none});
        assert_eq!(provider::validate(&bytes(&v), &request).unwrap(), None);
    }
}

#[test]
fn cache_reuse_is_separate_for_providers_and_accounts() {
    let (_t, paths, mut cfg, request) = fixture();
    let state = provider::ProviderState::open(&paths).unwrap();
    let calls = Cell::new(0);
    let send = || {
        calls.set(calls.get() + 1);
        Ok(bytes(&response()))
    };
    state
        .decide(&request, "synthetic-binding", &cfg, send)
        .unwrap();
    state
        .decide(&request, "synthetic-binding", &cfg, || {
            panic!("same account should use cache")
        })
        .unwrap();
    cfg.cloudflare_account_id = "00000000000000000000000000000002".into();
    state
        .decide(&request, "synthetic-binding", &cfg, send)
        .unwrap();
    assert_eq!(calls.get(), 2);
    cfg.provider = Provider::Jev;
    assert!(
        state
            .decide(&request, "synthetic-binding", &cfg, || panic!(
                "mismatched provider must not send"
            ))
            .is_err()
    );
    let mut jev: Value = serde_json::from_slice(&request).unwrap();
    jev["model"] = json!(provider::MODEL);
    state
        .decide(&bytes(&jev), "synthetic-binding", &cfg, || {
            calls.set(calls.get() + 1);
            Ok(bytes(&result(provider::MODEL)))
        })
        .unwrap();
    assert_eq!(calls.get(), 3);
}

#[test]
fn existing_schema_three_profiles_default_to_jev() {
    let mut v = serde_json::to_value(Config::default()).unwrap();
    for key in ["provider", "cloudflare_account_id", "cloudflare_credential"] {
        v.as_object_mut().unwrap().remove(key);
    }
    let cfg: Config = serde_json::from_value(v).unwrap();
    cfg.validate().unwrap();
    assert_eq!(cfg.provider, Provider::Jev);
    let mut invalid = cfg;
    invalid.cloudflare_account_id = "../another-account".into();
    assert!(invalid.validate().is_err());
}

#[test]
fn os_credential_entries_are_independent_in_an_isolated_backend() {
    use keyring::credential::{Credential, CredentialApi, CredentialBuilderApi};
    use std::{
        any::Any,
        collections::HashMap,
        sync::{Arc, Mutex},
    };
    type Secret = Arc<Mutex<Option<zeroize::Zeroizing<Vec<u8>>>>>;
    #[derive(Default)]
    struct Backend(Mutex<HashMap<String, Secret>>);
    struct Entry(Secret);
    impl CredentialApi for Entry {
        fn set_secret(&self, value: &[u8]) -> keyring::Result<()> {
            *self.0.lock().unwrap() = Some(zeroize::Zeroizing::new(value.to_vec()));
            Ok(())
        }
        fn get_secret(&self) -> keyring::Result<Vec<u8>> {
            self.0
                .lock()
                .unwrap()
                .as_ref()
                .map(|v| v.to_vec())
                .ok_or(keyring::Error::NoEntry)
        }
        fn delete_credential(&self) -> keyring::Result<()> {
            *self.0.lock().unwrap() = None;
            Ok(())
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }
    impl CredentialBuilderApi for Backend {
        fn build(
            &self,
            _: Option<&str>,
            service: &str,
            account: &str,
        ) -> keyring::Result<Box<Credential>> {
            let entry = self
                .0
                .lock()
                .unwrap()
                .entry(format!("{service}:{account}"))
                .or_default()
                .clone();
            Ok(Box::new(Entry(entry)))
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }
    keyring::set_default_credential_builder(Box::<Backend>::default());
    credential::set_for(Provider::Jev, "synthetic-jev").unwrap();
    credential::set_for(Provider::ClefFlash, "synthetic-cloudflare").unwrap();
    assert_eq!(
        &**credential::system_for(Provider::Jev).unwrap().unwrap(),
        "synthetic-jev"
    );
    assert_eq!(
        &**credential::system_for(Provider::ClefFlash)
            .unwrap()
            .unwrap(),
        "synthetic-cloudflare"
    );
    credential::delete_for(Provider::ClefFlash).unwrap();
    assert!(
        credential::system_for(Provider::ClefFlash)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        &**credential::system_for(Provider::Jev).unwrap().unwrap(),
        "synthetic-jev"
    );
}
