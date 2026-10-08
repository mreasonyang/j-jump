//! Official Decisions contracts with synthetic data; no cloud or user credentials.
use j_jump::{
    config::{Config, Paths, Provider},
    credential, engine, provider,
};
use serde_json::{Value, json};
use std::{cell::Cell, fs};

fn fixture(count: usize) -> (tempfile::TempDir, Paths, Config, Vec<engine::Group>) {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().canonicalize().unwrap();
    let paths = Paths {
        config: home.join("config/config.json"),
        data: home.join("data"),
        cache: home.join("cache"),
        home: home.clone(),
    };
    let groups = (0..count)
        .map(|i| {
            let path = home.join(format!("server{i}"));
            fs::create_dir(&path).unwrap();
            engine::Group {
                id: format!("d{i}"),
                name: format!("server{i}"),
                parent: "synthetic-parent".into(),
                usage: "low",
                members: vec![engine::Candidate {
                    id: format!("d{i}"),
                    path,
                    count: 1,
                    last_seen: 1,
                    weight: 1.0,
                }],
            }
        })
        .collect();
    let cfg = Config {
        semantic: true,
        provider: Provider::OpenAI,
        ..Config::default()
    };
    (temp, paths, cfg, groups)
}
fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}
fn response() -> Value {
    json!({"model":"gpt-6-luna", "answers":[{
        "type":"choice", "name":"destination", "choice":"d0", "confidence":0.8,
        "probabilities":[{"value":"d0", "probability":0.9}, {"value":"none", "probability":0.1}]
    }], "usage":{"input_tokens":42, "input_tokens_details":{"cached_tokens":0,"cache_write_tokens":0},
        "output_tokens":0, "output_tokens_details":{"reasoning_tokens":0}, "total_tokens":42}})
}

#[test]
fn official_request_and_privacy_policy() {
    let (_t, paths, mut cfg, groups) = fixture(1);
    assert_eq!(Provider::parse("openai").unwrap(), Provider::OpenAI);
    assert_eq!(provider::model(cfg.provider), "gpt-6-luna");
    assert_eq!(
        provider::endpoint(cfg.provider, "").unwrap(),
        "https://api.openai.com/v1/decisions"
    );
    assert!(provider::endpoint(cfg.provider, "https://example.test").is_err());
    let (request, count) = provider::request("后端", &groups, &paths.home, &paths, &cfg).unwrap();
    assert_eq!(count, 1);
    let wire: Value = serde_json::from_slice(&request).unwrap();
    assert_eq!(wire.as_object().unwrap().len(), 3);
    assert_eq!(wire["model"], "gpt-6-luna");
    assert_eq!(
        serde_json::from_str::<Value>(wire["input"].as_str().unwrap()).unwrap(),
        json!({"query":"后端"})
    );
    assert_eq!(wire["questions"].as_array().unwrap().len(), 1);
    let q = &wire["questions"][0];
    assert_eq!(q["type"], "choice");
    assert_eq!(q["name"], "destination");
    assert_eq!(
        q["choices"][0],
        json!({"value":"d0", "description":"{\"name\":\"server0\"}"})
    );
    let text = String::from_utf8(request).unwrap();
    assert!(!text.contains("synthetic-parent"));
    assert!(!text.contains(paths.home.to_str().unwrap()));
    cfg.privacy = "balanced".into();
    let (request, _) = provider::request("backend", &groups, &paths.home, &paths, &cfg).unwrap();
    assert!(
        String::from_utf8(request)
            .unwrap()
            .contains("synthetic-parent")
    );
    cfg.no_send = vec![groups[0].members[0].path.clone()];
    assert!(provider::request("backend", &groups, &paths.home, &paths, &cfg).is_err());
    cfg.no_send = vec![paths.home.clone()];
    assert!(provider::request("backend", &groups, &paths.home, &paths, &cfg).is_err());
}

#[test]
fn payload_budget_retains_the_largest_priority_prefix() {
    let (_t, paths, mut cfg, mut groups) = fixture(254);
    cfg.privacy = "balanced".into();
    for g in &mut groups {
        g.name = "名".repeat(170);
        g.parent = "親".repeat(170);
    }
    let (request, count) =
        provider::request("backend", &groups, &paths.home, &paths, &cfg).unwrap();
    assert!(request.len() <= 65536);
    assert!(count > 0 && count < 254);
    let wire: Value = serde_json::from_slice(&request).unwrap();
    let choices = wire["questions"][0]["choices"].as_array().unwrap();
    assert_eq!(choices.len(), count + 1);
    let sent: Vec<_> = choices
        .iter()
        .map(|c| c["value"].as_str().unwrap())
        .collect();
    for (i, group) in groups.iter().enumerate() {
        assert_eq!(sent.contains(&group.id.as_str()), i < count);
    }
    let mut task = json!({"state":{"query":"backend","cwd":paths.home.file_name().unwrap().to_str().unwrap()},
        "questions":{"destination":{"type":"choice", "instructions":wire["questions"][0]["instructions"], "criteria":{}}}});
    for g in &groups[..count + 1] {
        task["questions"]["destination"]["criteria"][&g.id] =
            json!({"name":g.name,"parent":g.parent,"usage":g.usage});
    }
    task["questions"]["destination"]["criteria"]["none"] =
        json!("No uniquely supported destination, including ambiguity or insufficient evidence.");
    assert!(
        cfg.provider
            .driver()
            .prepare(task, "gpt-6-luna")
            .unwrap()
            .len()
            > 65536
    );
    cfg.privacy = "strict".into();
    for g in &mut groups {
        g.name = "s".into();
    }
    let (request, count) =
        provider::request("backend", &groups, &paths.home, &paths, &cfg).unwrap();
    assert_eq!(count, 254);
    assert_eq!(
        serde_json::from_slice::<Value>(&request).unwrap()["questions"][0]["choices"]
            .as_array()
            .unwrap()
            .len(),
        255
    );
}

#[test]
fn answers_are_bound_to_the_exact_request_and_strictly_decoded() {
    let (_t, paths, cfg, groups) = fixture(1);
    let (request, _) = provider::request("backend", &groups, &paths.home, &paths, &cfg).unwrap();
    assert_eq!(
        provider::validate(&bytes(&response()), &request).unwrap(),
        Some("d0".into())
    );
    let mutations = [
        ("/model", json!("jev-1.13.0")),
        ("/answers/0/name", json!("other")),
        ("/answers/0/name", Value::Null),
        ("/answers/0/type", json!("predicate")),
        ("/answers/0/choice", json!("forged")),
        ("/answers/0/choice", json!(true)),
        ("/answers/0/confidence", json!(1.1)),
        ("/answers/0/probabilities/0/probability", json!(-0.1)),
        ("/answers/0/probabilities/0/probability", json!("0.9")),
        ("/answers/0/probabilities/0/probability", json!(0.2)),
        ("/answers/0/probabilities/0/value", json!(true)),
        ("/answers/0/probabilities/1/value", json!("d0")),
        ("/answers/0/probabilities/1/value", json!("forged")),
        (
            "/answers/0/probabilities",
            json!([{"value":"d0","probability":1.0}]),
        ),
        ("/usage/input_tokens", json!(-1)),
        ("/usage/total_tokens", json!(43)),
        ("/usage/input_tokens_details/cached_tokens", json!(43)),
        ("/usage/output_tokens_details/reasoning_tokens", json!(1)),
        ("/usage", Value::Null),
        ("/answers", json!([])),
    ];
    for (pointer, value) in mutations {
        let mut v = response();
        *v.pointer_mut(pointer).unwrap() = value;
        assert!(
            provider::validate(&bytes(&v), &request).is_err(),
            "{pointer}"
        );
    }
    let mut v = response();
    let answer = v["answers"][0].clone();
    v["answers"].as_array_mut().unwrap().push(answer);
    assert!(provider::validate(&bytes(&v), &request).is_err());
    let duplicated = String::from_utf8(bytes(&response())).unwrap().replace(
        "\"confidence\":0.8",
        "\"confidence\":0.1,\"confidence\":0.8",
    );
    assert!(provider::validate(duplicated.as_bytes(), &request).is_err());
    let mut v = response();
    v["answers"][0]["extra"] = json!(true);
    assert!(provider::validate(&bytes(&v), &request).is_err());
    let mut wrong_request: Value = serde_json::from_slice(&request).unwrap();
    wrong_request["questions"][0]["choices"][0]["value"] = json!("other-snapshot");
    assert!(provider::validate(&bytes(&response()), &bytes(&wrong_request)).is_err());
    wrong_request["questions"][0]["choices"][0]["value"] = json!("none");
    assert!(provider::validate(&bytes(&response()), &bytes(&wrong_request)).is_err());
    let mut v = response();
    v["usage"]["input_tokens_details"]
        .as_object_mut()
        .unwrap()
        .remove("cache_write_tokens");
    assert!(provider::validate(&bytes(&v), &request).is_err());
}

#[test]
fn refusals_none_ties_and_weak_evidence_abstain() {
    let (_t, paths, cfg, groups) = fixture(1);
    let (request, _) = provider::request("backend", &groups, &paths.home, &paths, &cfg).unwrap();
    let mut v = response();
    v["answers"] = json!([{"type":"refusal", "name":"destination"}]);
    assert_eq!(provider::validate(&bytes(&v), &request).unwrap(), None);
    v["answers"][0]["name"] = json!("other");
    assert!(provider::validate(&bytes(&v), &request).is_err());
    for (choice, p) in [("none", 0.1), ("d0", 0.5)] {
        let mut v = response();
        v["answers"][0]["choice"] = json!(choice);
        v["answers"][0]["probabilities"][0]["probability"] = json!(p);
        v["answers"][0]["probabilities"][1]["probability"] = json!(1.0 - p);
        assert_eq!(provider::validate(&bytes(&v), &request).unwrap(), None);
    }
    let (_t, paths, cfg, groups) = fixture(2);
    let (request, _) = provider::request("backend", &groups, &paths.home, &paths, &cfg).unwrap();
    let mut v = response();
    v["answers"][0]["probabilities"] = json!([
        {"value":"d0","probability":0.4}, {"value":"d1","probability":0.35}, {"value":"none","probability":0.25}]);
    assert_eq!(provider::validate(&bytes(&v), &request).unwrap(), None);
    v["answers"][0]["probabilities"][0]["probability"] = json!(0.3);
    v["answers"][0]["probabilities"][1]["probability"] = json!(0.45);
    assert!(provider::validate(&bytes(&v), &request).is_err());
}

#[test]
fn cache_and_credential_configuration_are_isolated() {
    let (_t, paths, mut cfg, groups) = fixture(1);
    assert_eq!(cfg.credential_source(), "environment");
    cfg.set_credential_source("system");
    assert_eq!(cfg.providers["openai"]["credential"], "system");
    assert_eq!(cfg.credential, "environment");
    assert_eq!(cfg.cloudflare_credential, "environment");
    cfg.validate().unwrap();
    let restored: Config = serde_json::from_slice(&bytes(&json!(cfg))).unwrap();
    assert_eq!(restored.credential_source(), "system");
    assert_eq!(cfg.provider.env_key(), "OPENAI_API_KEY");
    assert_eq!(
        credential::entry_identity(cfg.provider),
        ("j-jump.openai", "openai-api-key")
    );
    let (request, _) = provider::request("backend", &groups, &paths.home, &paths, &cfg).unwrap();
    let state = provider::ProviderState::open(&paths).unwrap();
    let calls = Cell::new(0);
    let send = || {
        calls.set(calls.get() + 1);
        Ok(bytes(&response()))
    };
    state.decide(&request, "credential-1", &cfg, send).unwrap();
    state
        .decide(&request, "credential-1", &cfg, || {
            panic!("should reuse validated cache")
        })
        .unwrap();
    state.decide(&request, "credential-2", &cfg, send).unwrap();
    assert_eq!(calls.get(), 2);
    cfg.provider = Provider::Jev;
    assert!(
        state
            .decide(&request, "credential-1", &cfg, || panic!(
                "must not send mismatched provider"
            ))
            .is_err()
    );
    cfg.provider = Provider::OpenAI;
    cfg.set_credential_source("file");
    assert!(cfg.validate().is_err());
}
