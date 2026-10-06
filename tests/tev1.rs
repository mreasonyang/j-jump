//! Offline contracts. These tests never access a real credential store or model.
use j_jump::{
    Error,
    config::{Config, Paths, Provider},
    engine, provider, providers,
};
use serde_json::{Value, json};
fn fixture() -> (tempfile::TempDir, Paths, Config) {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().canonicalize().unwrap();
    let p = Paths {
        config: home.join("config/config.json"),
        data: home.join("data"),
        cache: home.join("cache"),
        home,
    };
    (
        t,
        p,
        Config {
            provider: Provider::Tev1,
            semantic: true,
            ..Config::default()
        },
    )
}
fn groups(paths: &Paths, n: usize) -> Vec<engine::Group> {
    (0..n)
        .map(|i| {
            let name = format!("project{i}");
            let path = paths.home.join(&name);
            std::fs::create_dir_all(&path).unwrap();
            engine::Group {
                id: format!("c{i}"),
                name,
                parent: String::new(),
                usage: "low",
                members: vec![engine::Candidate {
                    id: format!("c{i}"),
                    path,
                    count: 1,
                    last_seen: 1,
                    weight: 1.0,
                }],
            }
        })
        .collect()
}
fn response(request: &[u8]) -> Vec<u8> {
    let req: Value = serde_json::from_slice(request).unwrap();
    let ps: serde_json::Map<String, Value> = req["questions"]["destination"]["criteria"]
        .as_object()
        .unwrap()
        .keys()
        .map(|k| (k.clone(), json!(if k == "c0" { 1.0 } else { 0.0 })))
        .collect();
    serde_json::to_vec(&json!({"model":req["model"],"answers":{"destination":{"type":"choice","choice":"c0","probabilities":ps,"confidence":1.0}}})).unwrap()
}
#[test]
fn endpoints_and_settings_reject_nonlocal_or_ambiguous_origins() {
    let (_t, _p, mut c) = fixture();
    let f = providers::field("ollama_url").unwrap();
    for url in ["http://127.0.0.1:11434", "http://[::1]:11434"] {
        (f.set)(&mut c, url).unwrap();
        assert_eq!(
            c.provider.driver().connection(&c, None).unwrap().transport,
            providers::Transport::Loopback
        );
    }
    for url in [
        "https://127.0.0.1:11434",
        "http://localhost:11434",
        "http://example.test",
        "http://127.0.0.1:0",
        "http://127.0.0.1/x",
        "http://synthetic:pass@127.0.0.1",
        "http://127.0.0.1/?x=y",
        "http://127.0.0.1/#x",
        "file:///tmp/a",
    ] {
        assert!((f.set)(&mut c, url).is_err(), "{url}");
    }
    c.providers
        .entry("tev1".into())
        .or_default()
        .insert("secret".into(), "synthetic".into());
    assert!(c.validate().is_err());
    c.providers.clear();
    c.providers
        .entry("clef-flash".into())
        .or_default()
        .insert("cloudflare_account_id".into(), "misplaced".into());
    assert_eq!(c.validate().unwrap_err().0, 7);
    c.providers.clear();
    c.providers
        .entry("tev1".into())
        .or_default()
        .insert("ollama_url".into(), "http://remote.test".into());
    assert_eq!(c.validate().unwrap_err().0, 7);
    assert!(Provider::parse("unknown").is_err());
}
#[test]
fn compact_encoding_and_budget_are_bound_to_exact_ids() {
    let (_t, p, c) = fixture();
    let gs = groups(&p, 50);
    let (request, sent) = provider::request("project", &gs, &p.home, &p, &c).unwrap();
    let v: Value = serde_json::from_slice(&request).unwrap();
    let criteria = v["questions"]["destination"]["criteria"]
        .as_object()
        .unwrap();
    assert!(sent > 0 && sent <= 23);
    assert_eq!(criteria.len(), sent + 1);
    assert!(criteria.values().all(Value::is_string));
    assert!(request.len() <= 1600);
    assert_eq!(
        provider::validate(&response(&request), &request).unwrap(),
        Some("c0".into())
    );
    let mut bad: Value = serde_json::from_slice(&response(&request)).unwrap();
    bad["model"] = json!(provider::MODEL);
    assert!(provider::validate(&serde_json::to_vec(&bad).unwrap(), &request).is_err());
    assert!(provider::request(&"长".repeat(400), &gs, &p.home, &p, &c).is_err());
}
#[test]
fn mutable_model_tags_never_reuse_persistent_answers_and_old_cache_is_preserved() {
    let (_t, p, c) = fixture();
    let (r, _) = provider::request("project", &groups(&p, 2), &p.home, &p, &c).unwrap();
    j_jump::config::private_dir(&p.cache).unwrap();
    let old = p.cache.join("semantic-cache.db");
    std::fs::write(&old, b"older untouched cache").unwrap();
    let state = provider::ProviderState::open(&p).unwrap();
    assert!(
        state
            .decide(&r, "same", &c, || Ok(response(&r)))
            .unwrap()
            .is_some()
    );
    assert!(
        state
            .decide(&r, "same", &c, || Err(Error(5, "synthetic offline".into())))
            .is_err()
    );
    assert_eq!(std::fs::read(old).unwrap(), b"older untouched cache");
}
#[test]
fn circuit_is_shared_across_queries_but_isolated_between_providers() {
    let (_t, p, c) = fixture();
    let gs = groups(&p, 2);
    let (r, _) = provider::request("project", &gs, &p.home, &p, &c).unwrap();
    let state = provider::ProviderState::open(&p).unwrap();
    for i in 0..3 {
        assert!(
            state
                .decide(&r, &format!("query{i}"), &c, || Err(Error(
                    5,
                    "synthetic failure".into()
                )))
                .is_err()
        );
    }
    assert!(
        state
            .decide(&r, "fourth", &c, || panic!("circuit did not block"))
            .is_err()
    );
    let cloud = Config {
        provider: Provider::Jev,
        ..c
    };
    let (r, _) = provider::request("project", &gs, &p.home, &p, &cloud).unwrap();
    assert!(
        state
            .decide(&r, "fourth", &cloud, || Ok(response(&r)))
            .unwrap()
            .is_some()
    );
}
#[test]
fn limited_shortlist_keeps_generic_destinations_for_unrelated_language_queries() {
    let (_t, p, c) = fixture();
    let mut gs = groups(&p, 30);
    for name in ["docs", "tests", "assets"] {
        let path = p.home.join(name);
        std::fs::create_dir(&path).unwrap();
        gs.push(engine::Group {
            id: name.into(),
            name: name.into(),
            parent: String::new(),
            usage: "low",
            members: vec![engine::Candidate {
                id: name.into(),
                path,
                count: 1,
                last_seen: 1,
                weight: 1.0,
            }],
        });
    }
    let all: Vec<_> = gs.into_iter().flat_map(|g| g.members).collect();
    for query in ["文档", "测试", "静态资源"] {
        let (short, truncated) = engine::semantic_groups(
            &all,
            &[],
            &engine::tokens(&[query.into()]),
            &p.home,
            &p,
            &c,
            false,
        );
        assert!(truncated);
        assert!(short.len() <= 23);
        let (bytes, _) = provider::request(query, &short, &p.home, &p, &c).unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        let descriptions = v["questions"]["destination"]["criteria"].to_string();
        for name in ["docs", "tests", "assets"] {
            assert!(descriptions.contains(name), "{query}: {descriptions}");
        }
    }
}

#[test]
fn selected_model_binding_and_legacy_profiles_survive_switching() {
    let (_t, p, mut cfg) = fixture();
    assert_eq!(cfg.provider.driver().selected_model(&cfg), "tev1:4b");
    providers::select(&mut cfg, Provider::Jev).unwrap();
    providers::select(&mut cfg, Provider::Tev1).unwrap();
    assert_eq!(cfg.provider.driver().selected_model(&cfg), "tev1:4b");
    let mut fresh = Config::default();
    providers::select(&mut fresh, Provider::Tev1).unwrap();
    assert_eq!(
        fresh.provider.driver().selected_model(&fresh),
        "tev1:4b-q8_0"
    );
    let f = providers::field("ollama_model").unwrap();
    for model in ["tev1:4b", "tev1:4b-q8_0", "tev1:4b-q4_K_M", "tev1:4b-bf16"] {
        (f.set)(&mut cfg, model).unwrap();
        let (r, _) = provider::request("project", &groups(&p, 2), &p.home, &p, &cfg).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&r).unwrap()["model"], model);
        assert!(provider::validate(&response(&r), &r).is_ok());
        let mut bad: Value = serde_json::from_slice(&response(&r)).unwrap();
        bad["model"] = json!(if model == "tev1:4b" {
            "tev1:4b-q8_0"
        } else {
            "tev1:4b"
        });
        assert!(provider::validate(&serde_json::to_vec(&bad).unwrap(), &r).is_err());
    }
    for model in [
        "tev1",
        "tev1:0.5b",
        "tev1:4b-mlx",
        "Tev1:4b",
        "tev1:4b\n",
        "custom-alias",
    ] {
        let before = cfg.clone();
        assert!((f.set)(&mut cfg, model).is_err());
        assert_eq!(
            serde_json::to_value(&cfg).unwrap(),
            serde_json::to_value(before).unwrap()
        );
    }
}

#[test]
fn model_discovery_filters_unknown_and_mlx_without_inference() {
    use providers::{Diagnostic, DiagnosticKind};
    let (_t, _p, cfg) = fixture();
    let d = cfg.provider.driver();
    let metadata = vec![
        json!({"version":"0.35.1"}),
        json!({"models":[
            {"name":"tev1:4b-q8_0","details":{"format":"gguf"},"size":4500000000u64},
            {"name":"tev1:4b-q8_0","details":{"format":"gguf"}},
            {"name":"tev1:4b","details":{"format":"mlx"}},
            {"name":"unknown\u{1b}[2J","details":{"format":"gguf"}}
        ]}),
    ];
    let Diagnostic::Complete(report) = d
        .diagnostic(&cfg, DiagnosticKind::Models, &metadata)
        .unwrap()
    else {
        panic!("discovery must not infer")
    };
    assert_eq!(report["models"].as_array().unwrap().len(), 1);
    assert_eq!(report["models"][0]["id"], "tev1:4b-q8_0");
    assert!(
        d.diagnostic(&cfg, DiagnosticKind::Check, &metadata)
            .is_err()
    );
    for version in ["0.34.9", "0.35", "garbage", "0.35.0.1"] {
        assert!(
            d.diagnostic(&cfg, DiagnosticKind::Models, &[json!({"version":version})])
                .is_err()
        );
    }
    assert!(
        d.diagnostic(
            &cfg,
            DiagnosticKind::Models,
            &[json!({"version":"0.35.1"}), json!({"models":{}})]
        )
        .is_err()
    );
}

#[test]
fn model_switch_rejects_stale_payload_and_has_a_separate_circuit() {
    let (_t, p, mut cfg) = fixture();
    let gs = groups(&p, 2);
    let state = provider::ProviderState::open(&p).unwrap();
    let (old, _) = provider::request("project", &gs, &p.home, &p, &cfg).unwrap();
    for _ in 0..3 {
        assert!(
            state
                .decide(&old, "old", &cfg, || Err(Error(
                    5,
                    "synthetic failure".into()
                )))
                .is_err()
        );
    }
    (providers::field("ollama_model").unwrap().set)(&mut cfg, "tev1:4b-q8_0").unwrap();
    assert!(
        state
            .decide(&old, "stale", &cfg, || panic!("stale payload sent"))
            .is_err()
    );
    let (new, _) = provider::request("project", &gs, &p.home, &p, &cfg).unwrap();
    assert!(
        state
            .decide(&new, "new", &cfg, || Ok(response(&new)))
            .unwrap()
            .is_some()
    );
}
