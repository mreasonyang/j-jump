use super::*;
use serde_json::Map;
fn task() -> Value {
    json!({"state":{"query":"backend"},"questions":{"destination":{"type":"choice","instructions":"Choose a match.","criteria":{"c1":{"name":"alpha"},"none":"No match"}}}})
}
fn full_task() -> Value {
    let mut criteria: Map<String, Value> = (0..254)
        .map(|i| {
            (
                format!("c{i}"),
                json!({"name":format!("synthetic-directory-name-{i:04}"),"parent":format!("/synthetic/projects/workspace/parent-directory-{i:04}/x"),"usage":"low"}),
            )
        })
        .collect();
    criteria.insert("none".into(), json!("No match"));
    json!({"state":{"query":"backend","cwd":"src"},"questions":{"destination":{"type":"choice","instructions":"Choose a match.","criteria":criteria}}})
}
fn answer(model: &str) -> Value {
    json!({"model":model,"answers":{"destination":{"type":"choice","choice":"c1","probabilities":{"c1":0.9,"none":0.1},"confidence":0.9}}})
}
fn definition() -> Definition {
    Definition {
        version: 1,
        id: "synthetic-api".into(),
        model: "synthetic-model".into(),
        url: "http://127.0.0.1:11434/decision".into(),
        transport: "loopback".into(),
        auth: Auth::None,
        limits: Limits::default(),
        response_models: vec![],
        wire: super::super::wire("jev").clone(),
        probe: None,
    }
}
fn config(d: Definition) -> Config {
    Config {
        provider: crate::config::Provider::parse("dsl").unwrap(),
        api_definition: Some(d),
        semantic: true,
        ..Config::default()
    }
}
fn expr(v: Value) -> Expr {
    serde_json::from_value(v).unwrap()
}
#[test]
fn shipped_wire_roundtrips() {
    for id in ["jev", "clef", "tev1", "openai"] {
        let w = super::super::wire(id);
        let b = w.prepare(task(), "synthetic-model").unwrap();
        let q = w.questions(&serde_json::from_slice(&b).unwrap()).unwrap();
        assert_eq!(
            q["destination"]["criteria"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            vec!["c1", "none"]
        );
        assert!(!String::from_utf8(b).unwrap().contains("api-key"));
    }
}
#[test]
fn arbitrary_wire_and_same_model_are_selected_by_config() {
    let mut d = definition();
    d.model = "gpt-6-luna".into(); // Deliberately shares a built-in model name.
    d.wire.request = expr(
        json!({"op":"object","fields":{"model":{"op":"ref","path":"/model"},"payload":{"op":"ref","path":"/task"}}}),
    );
    d.wire.questions = expr(json!({"op":"ref","path":"/request/payload/questions"}));
    d.wire.response = expr(json!({"op":"ref","path":"/response/result"}));
    let cfg = config(d);
    cfg.validate().unwrap();
    let b = cfg.driver().prepare(task(), "gpt-6-luna").unwrap();
    assert_eq!(
        crate::provider::validate_for(
            &cfg,
            &serde_json::to_vec(&json!({"result":answer("gpt-6-luna")})).unwrap(),
            &b
        )
        .unwrap()
        .as_deref(),
        Some("c1")
    );
    assert!(
        crate::provider::validate(&serde_json::to_vec(&answer("gpt-6-luna")).unwrap(), &b).is_err()
    );
}
#[test]
fn canonical_validation_cannot_be_disabled() {
    let cfg = config(definition());
    let b = cfg.driver().prepare(task(), "synthetic-model").unwrap();
    for (path, v) in [
        ("/model", json!("wrong")),
        ("/answers/destination/choice", json!("forged")),
        ("/answers/destination/probabilities/c1", json!(1.5)),
        ("/answers/destination/confidence", json!(-1)),
    ] {
        let mut a = answer("synthetic-model");
        *a.pointer_mut(path).unwrap() = v;
        assert!(
            crate::provider::validate_for(&cfg, &serde_json::to_vec(&a).unwrap(), &b).is_err(),
            "{path}"
        );
    }
    for t in ["refusal", "choice"] {
        let mut a = answer("synthetic-model");
        if t == "refusal" {
            a["answers"]["destination"] = json!({"type":"refusal"});
        } else {
            a["answers"]["destination"] = json!({"type":"choice","choice":"none","probabilities":{"c1":0.1,"none":0.9},"confidence":0.9});
        }
        assert_eq!(
            crate::provider::validate_for(&cfg, &serde_json::to_vec(&a).unwrap(), &b).unwrap(),
            None
        );
    }
}
#[test]
fn explicit_aliases_and_definition_identity() {
    let mut d = definition();
    d.response_models.push("versioned-model".into());
    let old = d.identity();
    let cfg = config(d.clone());
    let b = d.prepare(task(), &d.model).unwrap();
    assert_eq!(
        crate::provider::validate_for(
            &cfg,
            &serde_json::to_vec(&answer("versioned-model")).unwrap(),
            &b
        )
        .unwrap()
        .as_deref(),
        Some("c1")
    );
    assert!(
        crate::provider::validate_for(
            &cfg,
            &serde_json::to_vec(&answer("fallback-model")).unwrap(),
            &b
        )
        .is_err()
    );
    d.response_models.clear();
    assert_ne!(old, d.identity());
    assert!(d.connection(&cfg, Some(&old)).is_err());
}
#[test]
fn malformed_definition_import_fails_closed() {
    let v = serde_json::to_value(definition()).unwrap();
    for (k, x) in [
        ("version", json!(2)),
        ("unknown", json!(true)),
        ("id", json!("../jev")),
        ("transport", json!("shell")),
        ("url", json!("file:///tmp/test")),
        ("model", json!("bad\nmodel")),
    ] {
        let mut v = v.clone();
        v[k] = x;
        assert!(
            Definition::parse(&serde_json::to_vec(&v).unwrap()).is_err(),
            "{k}"
        );
    }
    assert!(Definition::parse(br#"{"version":1,"version":1}"#).is_err());
    assert!(Definition::parse(&vec![b' '; 32769]).is_err());
    assert!(serde_json::from_value::<Expr>(json!({"op":"shell","command":"whoami"})).is_err());
    assert!(
        serde_json::from_value::<Expr>(json!({"op":"ref","path":"/task","env":"HOME"})).is_err()
    );
}
#[test]
fn transport_and_auth_boundaries() {
    for url in [
        "http://example.invalid/api",
        "http://localhost:11434/api",
        "http://127.0.0.2/api",
        "http://127.0.0.1@remote.invalid/api",
        "http://127.0.0.1:0/api",
        "http://127.0.0.1/api?key=test",
        "http://127.0.0.1/api#fragment",
    ] {
        let mut d = definition();
        d.url = url.into();
        assert!(d.validate().is_err(), "{url}");
    }
    let mut d = definition();
    d.url = "http://[::1]:11434/api".into();
    d.validate().unwrap();
    d.auth = Auth::Bearer {
        env: "TEST_API_KEY".into(),
    };
    assert!(d.validate().is_err());
    d.transport = "https".into();
    d.url = "https://example.invalid/decision".into();
    d.validate().unwrap();
    for env in ["", "HOME=test", "lower_case", "1KEY"] {
        d.auth = Auth::Bearer { env: env.into() };
        assert!(d.validate().is_err());
    }
    for name in [
        "host",
        "CONTENT-LENGTH",
        "connection",
        "proxy-authorization",
        "bad\nheader",
    ] {
        d.auth = Auth::Header {
            env: "TEST_API_KEY".into(),
            name: name.into(),
        };
        assert!(d.validate().is_err());
    }
    d.auth = Auth::Header {
        env: "TEST_API_KEY".into(),
        name: "x-api-key".into(),
    };
    d.validate().unwrap();
}
#[test]
fn duplicate_index_keys_and_missing_references_fail() {
    let e = expr(
        json!({"op":"index","input":{"op":"ref","path":"/array"},"key":{"op":"ref","path":"/item/id"},"value":{"op":"ref","path":"/item"}}),
    );
    assert!(
        e.evaluate(&json!({"array":[{"id":"x"},{"id":"x"}]}))
            .is_err()
    );
    assert!(
        expr(json!({"op":"ref","path":"/secret"}))
            .evaluate(&json!({}))
            .is_err()
    );
    assert_eq!(
        expr(json!({"op":"ref","path":"/missing","default":null}))
            .evaluate(&json!({}))
            .unwrap(),
        Value::Null
    );
}
#[test]
fn bounded_depth_collections_and_amplification() {
    let e = Expr::Literal {
        value: json!(vec![0; 1025]),
    };
    assert!(e.evaluate(&json!({})).is_err());
    let e = Expr::Literal {
        value: json!("x".repeat(262145)),
    };
    assert!(e.evaluate(&json!({})).is_err());
    let mut e = Expr::Literal { value: json!(true) };
    for _ in 0..33 {
        e = Expr::Not { input: Box::new(e) };
    }
    assert!(e.check(0, &mut 0).is_err());
    assert!(e.evaluate(&json!({})).is_err());
    let e =
        expr(json!({"op":"map","input":{"op":"ref","path":"/a"},"body":{"op":"ref","path":"/a"}}));
    assert!(
        e.evaluate(&json!({"a":vec!["x".repeat(128);1024]}))
            .is_err()
    );
}
#[test]
fn inverse_mapping_cannot_drop_or_invent_choices() {
    let mut d = definition();
    d.wire.questions = Expr::Literal {
        value: json!({"destination":{"criteria":{"none":"No match"}}}),
    };
    assert!(d.prepare(task(), &d.model).is_err());
}
#[test]
fn response_assertions_share_one_work_budget() {
    let mut w = super::super::wire("jev").clone();
    let check = expr(
        json!({"op":"eq","left":{"op":"length","input":{"op":"map","input":{"op":"ref","path":"/response/metadata"},"body":{"op":"literal","value":0}}},"right":{"op":"literal","value":1000}}),
    );
    w.response_checks = vec![check.clone(); 100];
    w.check().unwrap();
    let response = json!({"metadata":vec!["x".repeat(128);1000]});
    assert_eq!(check.evaluate(&json!({"response":response})).unwrap(), true);
    w.response_checks.truncate(10);
    w.decode(response.clone()).unwrap();
    w.response_checks = vec![check; 100];
    assert!(is_resource_limit(&w.decode(response).unwrap_err()));
}
#[test]
fn evaluation_cost_tracks_output_not_phase_root() {
    // Binding an item no longer copies the phase root, so unrelated input
    // size does not multiply per-element cost.
    let tiny = expr(
        json!({"op":"map","input":{"op":"ref","path":"/a"},"body":{"op":"literal","value":null}}),
    );
    tiny.evaluate(&json!({"a":vec!["x".repeat(128);1024]}))
        .unwrap();
    let small = expr(
        json!({"op":"map","input":{"op":"ref","path":"/a"},"body":{"op":"ref","path":"/item"}}),
    );
    assert_eq!(
        small
            .evaluate(&json!({"a":[1,2,3],"big":vec!["x".repeat(200);1000]}))
            .unwrap(),
        json!([1, 2, 3])
    );
    // Copies and per-element work are still charged, so amplification is bounded.
    for e in [
        json!({"op":"map","input":{"op":"ref","path":"/a"},"body":{"op":"length","input":{"op":"ref","path":"/a"}}}),
        json!({"op":"map","input":{"op":"ref","path":"/a"},"body":{"op":"map","input":{"op":"ref","path":"/a"},"body":{"op":"literal","value":null}}}),
        json!({"op":"map","input":{"op":"ref","path":"/a"},"body":{"op":"filter","input":{"op":"ref","path":"/a"},"condition":{"op":"literal","value":false}}}),
        json!({"op":"map","input":{"op":"ref","path":"/a"},"body":{"op":"index","input":{"op":"ref","path":"/a"},"key":{"op":"text","input":{"op":"ref","path":"/item"}},"value":{"op":"literal","value":null}}}),
    ] {
        let a: Vec<_> = (0..1024).map(|i| format!("{i:0128}")).collect();
        let err = expr(e).evaluate(&json!({ "a": a })).unwrap_err();
        assert!(is_resource_limit(&err), "{}", err.1);
    }
}
#[test]
fn composed_sizes_match_serialization() {
    let v = json!({"a\"\n":["é\u{1}",1.5,-3,u64::MAX,null,true,{}],"b":[[],{"k":"\u{2028}"}]});
    assert_eq!(
        measure(&v).unwrap().bytes,
        serde_json::to_vec(&v).unwrap().len()
    );
    let e = expr(
        json!({"op":"object","fields":{"x\"":{"op":"map","input":{"op":"ref","path":"/a"},"body":{"op":"array","items":[{"op":"ref","path":"/item"},{"op":"literal","value":"\t"}]}},"y":{"op":"index","input":{"op":"ref","path":"/a"},"key":{"op":"text","input":{"op":"ref","path":"/item"}},"value":{"op":"stringify","input":{"op":"ref","path":"/item"}}}}}),
    );
    let root = json!({"a":["p","q\"",3]});
    let (v, size) = e
        .eval(
            Env {
                root: &root,
                item: None,
            },
            &mut FUEL.clone(),
            0,
        )
        .unwrap();
    assert_eq!(size.bytes, serde_json::to_vec(&v).unwrap().len());
    assert_eq!(size.height, 3);
}
#[test]
fn item_binding_is_innermost_and_other_paths_read_the_root() {
    let e = expr(
        json!({"op":"map","input":{"op":"ref","path":"/rows"},"body":{"op":"map","input":{"op":"ref","path":"/item/cells"},"body":{"op":"array","items":[{"op":"ref","path":"/item"},{"op":"ref","path":"/model"},{"op":"ref","path":"/itemx"}]}}}),
    );
    assert_eq!(
        e.evaluate(&json!({"rows":[{"cells":[1,2]},{"cells":[3]}],"model":"m","itemx":0}))
            .unwrap(),
        json!([[[1, "m", 0], [2, "m", 0]], [[3, "m", 0]]])
    );
    let outside = expr(json!({"op":"ref","path":"/item"}))
        .evaluate(&json!({"task":{}}))
        .unwrap_err();
    assert_eq!(outside.0, 5);
    assert!(!is_resource_limit(&outside));
}
#[test]
fn shipped_openai_wire_prepares_full_candidate_set() {
    for id in ["openai", "jev", "clef", "tev1"] {
        let b = super::super::wire(id)
            .prepare(full_task(), "synthetic-model")
            .unwrap();
        let q = super::super::wire(id)
            .questions(&serde_json::from_slice(&b).unwrap())
            .unwrap();
        assert_eq!(
            q["destination"]["criteria"].as_object().unwrap().len(),
            255,
            "{id}"
        );
    }
}
fn https() -> Definition {
    Definition {
        transport: "https".into(),
        url: "https://example.invalid/decision".into(),
        auth: Auth::Bearer {
            env: "TEST_API_KEY".into(),
        },
        ..definition()
    }
}
fn reason(d: &Definition) -> String {
    let e = d.validate().unwrap_err();
    assert_eq!(e.0, 2, "{}", e.1);
    e.1.strip_prefix("invalid provider definition: ")
        .unwrap()
        .to_owned()
}
#[test]
fn definition_failures_are_specific_usage_errors() {
    let v = serde_json::to_value(definition()).unwrap();
    let parse = |v: &Value| Definition::parse(&serde_json::to_vec(v).unwrap()).unwrap_err();
    for (bytes, want) in [
        (b"{".to_vec(), "not strict JSON or duplicate key"),
        (
            br#"{"version":1,"version":1}"#.to_vec(),
            "not strict JSON or duplicate key",
        ),
        (vec![b' '; 32769], "definition exceeds 32 KiB"),
    ] {
        let e = Definition::parse(&bytes).unwrap_err();
        assert_eq!(
            (e.0, e.1.as_ref()),
            (2, format!("invalid provider definition: {want}").as_str())
        );
    }
    for (k, x, want) in [
        ("version", json!(2), "unsupported version"),
        ("unknown", json!(true), "unknown field `unknown`"),
        (
            "id",
            json!("../jev"),
            "id must be 1-64 lowercase letters, digits or -",
        ),
        ("model", json!("bad\nmodel"), "model must be 1-256 bytes"),
        (
            "transport",
            json!("shell"),
            "url must be https, or http to 127.0.0.1/[::1]",
        ),
        (
            "url",
            json!("file:///tmp/test"),
            "url must be absolute with a host",
        ),
        ("limits", json!({"candidates":0}), "limits out of range"),
        (
            "response_models",
            json!(vec!["m"; 17]),
            "response_models must be at most 16",
        ),
    ] {
        let mut v = v.clone();
        v[k] = x;
        let e = parse(&v);
        assert_eq!(e.0, 2, "{k}");
        assert!(
            e.1.starts_with(&format!("invalid provider definition: {want}")),
            "{k}: {}",
            e.1
        );
    }
    let mut w = v.clone();
    w["wire"]["request"] = json!({"op":"shell","command":"whoami"});
    assert!(parse(&w).1.contains("unknown variant `shell`"));
    w["wire"]["request"] = json!({"op":"ref","path":"task"});
    assert!(
        parse(&w)
            .1
            .ends_with("ref path must start with / and be at most 1024 bytes")
    );
    let mut deep = json!({"op":"literal","value":true});
    for _ in 0..33 {
        deep = json!({"op":"not","input":deep});
    }
    w["wire"]["request"] = deep;
    assert!(
        parse(&w)
            .1
            .ends_with("expression tree exceeds 4096 nodes or depth 32")
    );
    w["wire"]["request"] = v["wire"]["request"].clone();
    w["wire"]["request_shape"] = json!({"kind":"text"});
    assert!(parse(&w).1.contains("shape kind must be"));
}
#[test]
fn runtime_failures_are_value_free_and_limits_are_recognizable() {
    let secret = "SYNTHETIC-SECRET-VALUE";
    let root = json!({"s":secret,"n":{"k":secret}});
    for e in [
        json!({"op":"not","input":{"op":"ref","path":"/s"}}),
        json!({"op":"map","input":{"op":"ref","path":"/n"},"body":{"op":"literal","value":0}}),
        json!({"op":"ref","path":"/missing"}),
        json!({"op":"version_at_least","input":{"op":"ref","path":"/s"},"minimum":[1,0,0]}),
        json!({"op":"index","input":{"op":"array","items":[{"op":"ref","path":"/s"},{"op":"ref","path":"/s"}]},"key":{"op":"ref","path":"/item"},"value":{"op":"literal","value":0}}),
        json!({"op":"add","left":{"op":"literal","value":u64::MAX},"right":{"op":"literal","value":1}}),
    ] {
        let err = expr(e).evaluate(&root).unwrap_err();
        assert_eq!(err.0, 5);
        assert!(err.1.starts_with("provider DSL "), "{}", err.1);
        assert!(
            !err.1.contains(secret) && !is_resource_limit(&err),
            "{}",
            err.1
        );
    }
    let w = super::super::wire("openai");
    let err = w.decode(json!({"model":secret})).unwrap_err();
    assert_eq!(err.1, "provider DSL response does not match response_shape");
    let mut deep = json!(0);
    for _ in 0..32 {
        deep = json!([deep]);
    }
    let mut fuel = Expr::Literal { value: Value::Null };
    for _ in 0..4 {
        fuel = Expr::Map {
            input: Box::new(Expr::Literal {
                value: json!(vec![0; 1024]),
            }),
            body: Box::new(fuel),
        };
    }
    for (e, root) in [
        (
            Expr::Literal {
                value: json!(vec![0; 1025]),
            },
            json!({}),
        ),
        (
            Expr::Literal {
                value: json!("x".repeat(262145)),
            },
            json!({}),
        ),
        (
            Expr::Array {
                items: vec![Expr::Literal {
                    value: deep.clone(),
                }],
            },
            json!({}),
        ),
        (expr(json!({"op":"ref","path":"/d"})), json!({"d":[deep]})),
        (fuel, json!({})),
    ] {
        let err = e.evaluate(&root).unwrap_err();
        assert_eq!((err.0, err.1.as_ref()), (5, LIMIT));
        assert!(is_resource_limit(&err));
    }
    assert!(!is_resource_limit(&Error(
        5,
        "provider response exceeds limit".into()
    )));
    assert!(!is_resource_limit(&Error(2, LIMIT.into())));
}
#[test]
fn partial_limits_use_field_defaults() {
    let mut v = serde_json::to_value(https()).unwrap();
    v["limits"] = json!({"cache":true});
    let d = Definition::parse(&serde_json::to_vec(&v).unwrap()).unwrap();
    assert_eq!(
        d.limits,
        Limits {
            cache: true,
            ..Limits::default()
        }
    );
    v["limits"] = json!({"candidates":23,"context_bytes":1600});
    let d = Definition::parse(&serde_json::to_vec(&v).unwrap()).unwrap();
    assert_eq!((d.limits.candidates, d.limits.body_bytes), (23, 65536));
    v["limits"] = json!({});
    assert_eq!(
        Definition::parse(&serde_json::to_vec(&v).unwrap())
            .unwrap()
            .limits,
        Limits::default()
    );
}
#[test]
fn serialization_is_compact_and_roundtrips() {
    let text = serde_json::to_string(&definition()).unwrap();
    assert!(
        !text.contains("\"limits\"") && !text.contains("\"extra\"") && !text.contains("\"probe\"")
    );
    for text in [
        include_str!("../../../examples/providers/openrouter-jev.json"),
        include_str!("../../../examples/providers/vercel-openai.json"),
        include_str!("../../../examples/providers/local-tev1.json"),
        include_str!("../../../examples/providers/openrouter-microsoft.json"),
        include_str!("../../../examples/providers/openrouter-openai.json"),
        include_str!("../../../examples/providers/openrouter-clef.json"),
    ] {
        let d = Definition::parse(text.as_bytes()).unwrap();
        let minified = serde_json::to_vec(&serde_json::from_str::<Value>(text).unwrap()).unwrap();
        let b = serde_json::to_vec(&d).unwrap();
        assert!(
            b.len() <= minified.len(),
            "{} > {}",
            b.len(),
            minified.len()
        );
        assert_eq!(serde_json::from_slice::<Definition>(&b).unwrap(), d);
    }
    for text in [
        include_str!("../definitions/jev.json"),
        include_str!("../definitions/clef.json"),
        include_str!("../definitions/openai.json"),
        include_str!("../definitions/tev1.json"),
    ] {
        let w: Wire = serde_json::from_str(text).unwrap();
        let minified = serde_json::to_vec(&serde_json::from_str::<Value>(text).unwrap()).unwrap();
        let b = serde_json::to_vec(&w).unwrap();
        assert!(b.len() <= minified.len());
        assert_eq!(serde_json::from_slice::<Wire>(&b).unwrap(), w);
    }
    let mut d = definition();
    d.limits = Limits {
        candidates: 23,
        body_bytes: 4096,
        context_bytes: Some(1600),
        diverse: true,
        cache: false,
    };
    d.response_models = vec!["alias".into()];
    let b = serde_json::to_vec(&d).unwrap();
    assert_eq!(serde_json::from_slice::<Definition>(&b).unwrap(), d);
    // The size limit binds both the source bytes and the compact form.
    d.wire.response = Expr::Object {
        fields: (0..800)
            .map(|i| (format!("field{i}"), Expr::Literal { value: Value::Null }))
            .collect(),
    };
    assert_eq!(reason(&d), "definition exceeds 32 KiB");
    let mut padded = serde_json::to_vec_pretty(&definition()).unwrap();
    padded.resize(32769, b' ');
    assert_eq!(
        Definition::parse(&padded).unwrap_err().1,
        "invalid provider definition: definition exceeds 32 KiB"
    );
}
#[test]
fn shape_keywords_must_apply_to_their_kind() {
    let check = |v: Value| {
        serde_json::from_value::<Shape>(v)
            .unwrap()
            .definition_check(0)
    };
    for v in [
        json!({"kind":"uint","min":1}),
        json!({"max":3}),
        json!({"kind":"boolean","min":0,"max":1}),
        json!({"kind":"array","properties":{"a":{}}}),
        json!({"kind":"string","required":["a"]}),
        json!({"kind":"array","values":{}}),
        json!({"extra":true}),
        json!({"kind":"object","items":{}}),
        json!({"kind":"string","items":{}}),
        json!({"kind":"object","values":{},"extra":true}),
        json!({"kind":"object","any":[{}]}),
        json!({"choices":[1],"any":[{}]}),
        json!({"min":1,"any":[{"kind":"string"}]}),
        json!({"any":[{"kind":"string","items":{}}]}),
        json!({"kind":"object","properties":{"a":{"kind":"null","max":1}}}),
    ] {
        let e = check(v.clone()).unwrap_err();
        assert_eq!(e.0, 2, "{v}");
        assert!(
            e.1.starts_with("invalid provider definition: shape "),
            "{v}"
        );
    }
    for v in [
        json!({}),
        json!({"kind":"string","min":1,"max":3,"choices":["ab"]}),
        json!({"kind":"array","min":1,"items":{"kind":"uint"}}),
        json!({"kind":"object","max":2,"required":["a"],"properties":{"a":{}},"values":{"kind":"null"}}),
        json!({"kind":"object","extra":true}),
        json!({"kind":"","any":[{"kind":"null"},{"kind":"array","max":0}]}),
        json!({"kind":"uint","choices":[1,2]}),
    ] {
        check(v.clone()).unwrap_or_else(|e| panic!("{v}: {}", e.1));
    }
}
#[test]
fn probe_paths_reject_whitespace_controls_and_other_origins() {
    let mut d = definition();
    d.probe = Some(Probe {
        steps: vec![ProbeStep {
            path: "/api/version".into(),
            task: None,
            expect_choice: None,
            checks: vec![],
        }],
        report: Expr::Literal { value: json!({}) },
    });
    d.validate().unwrap();
    for path in [
        "/\t/remote.invalid",
        "/\n/remote.invalid",
        "/\r/remote.invalid",
        "/api version",
        "/api\u{7f}",
        "/api\u{0}",
        "/api\u{85}",
        "/api\u{3000}",
        "api/version",
    ] {
        d.probe.as_mut().unwrap().steps[0].path = path.into();
        assert!(
            reason(&d).starts_with("probe step path must be a same-origin"),
            "{path:?}"
        );
    }
    d.probe.as_mut().unwrap().steps[0].path = "/api/%20tags".into();
    d.validate().unwrap();
}
#[test]
fn auth_header_rejects_framing_hop_by_hop_and_long_names() {
    let mut d = https();
    for name in [
        "te",
        "Trailer",
        "upgrade",
        "keep-alive",
        "proxy-connection",
        "proxy-authenticate",
        "expect",
        "host",
        "content-type",
    ] {
        d.auth = Auth::Header {
            env: "TEST_API_KEY".into(),
            name: name.into(),
        };
        assert_eq!(
            reason(&d),
            "auth header name is reserved for transport",
            "{name}"
        );
    }
    for (name, ok) in [
        ("x".repeat(64), true),
        ("x".repeat(65), false),
        ("bad header".into(), false),
    ] {
        d.auth = Auth::Header {
            env: "TEST_API_KEY".into(),
            name,
        };
        assert_eq!(d.validate().is_ok(), ok);
    }
}
#[test]
fn version_prereleases_rank_below_their_release() {
    let at_least = |v: &str| {
        expr(json!({"op":"version_at_least","input":{"op":"literal","value":v},"minimum":[0,35,0]}))
            .evaluate(&json!({}))
            .map(|v| v == true)
    };
    for (v, want) in [
        ("0.35.0", true),
        ("0.35.0+build.7", true),
        ("0.35.0-rc1", false),
        ("0.35.0-rc.1+build", false),
        ("0.35.1-rc1", true),
        ("0.36.0-rc1", true),
        ("1.0.0", true),
        ("0.34.99", false),
        ("0.34.9-rc1", false),
    ] {
        assert_eq!(at_least(v).unwrap(), want, "{v}");
    }
    for v in [
        "0.35", "0.35.0.1", "0.35.0-", "0.35.0+", "v0.35.0", "garbage", "",
    ] {
        assert!(at_least(v).is_err(), "{v}");
    }
}
#[test]
fn builtin_credential_variables_are_reserved() {
    let reserved: Vec<&str> = DRIVERS
        .iter()
        .filter_map(|d| d.descriptor().credential.as_ref())
        .flat_map(|c| c.env.iter().copied())
        .collect();
    for env in [
        "TYPESAFE_API_KEY",
        "OPENAI_API_KEY",
        "CLOUDFLARE_AUTH_TOKEN",
        "CLOUDFLARE_API_TOKEN",
    ] {
        assert!(reserved.contains(&env));
    }
    let mut d = https();
    for env in reserved {
        for auth in [
            Auth::Bearer { env: env.into() },
            Auth::Header {
                env: env.into(),
                name: "x-api-key".into(),
            },
        ] {
            d.auth = auth;
            assert_eq!(
                reason(&d),
                "credential variable reserved for built-in provider"
            );
        }
    }
    d.auth = Auth::Bearer {
        env: "OPENROUTER_API_KEY".into(),
    };
    d.validate().unwrap();
}
#[test]
fn shape_validates_nested_keys_types_bounds_and_overflow() {
    let s: Shape = serde_json::from_value(
        json!({"kind":"object","required":["n"],"properties":{"n":{"kind":"uint"}}}),
    )
    .unwrap();
    s.validate(&json!({"n":1})).unwrap();
    for v in [json!({}), json!({"n":-1}), json!({"n":1,"extra":true})] {
        assert!(s.validate(&v).is_err());
    }
    let e = expr(
        json!({"op":"add","left":{"op":"literal","value":u64::MAX},"right":{"op":"literal","value":1}}),
    );
    assert!(e.evaluate(&json!({})).is_err());
}
#[test]
fn gateway_examples_are_valid_and_compact_in_config() {
    for text in [
        include_str!("../../../examples/providers/openrouter-jev.json"),
        include_str!("../../../examples/providers/vercel-openai.json"),
        include_str!("../../../examples/providers/local-tev1.json"),
        include_str!("../../../examples/providers/openrouter-microsoft.json"),
        include_str!("../../../examples/providers/openrouter-openai.json"),
        include_str!("../../../examples/providers/openrouter-clef.json"),
    ] {
        let d = Definition::parse(text.as_bytes()).unwrap();
        let b = d.prepare(task(), &d.model).unwrap();
        assert!(!b.is_empty());
        let cfg = config(d);
        cfg.validate().unwrap();
        let serialized = serde_json::to_vec(&cfg).unwrap();
        assert!(serialized.len() < 65536);
        assert_eq!(serde_json::from_slice::<Config>(&serialized).unwrap(), cfg);
    }
}
#[test]
fn oversized_serialized_config_is_refused_without_replacing_saved_state() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().canonicalize().unwrap();
    let paths = crate::config::Paths {
        config: home.join("config/config.json"),
        data: home.join("data"),
        cache: home.join("cache"),
        home,
    };
    paths.save(&Config::default(), "absent").unwrap();
    let before = std::fs::read(&paths.config).unwrap();
    let mut d = definition();
    d.wire.response = Expr::Object {
        fields: (0..750)
            .map(|i| (format!("field{i}"), Expr::Literal { value: Value::Null }))
            .collect(),
    };
    d.validate().unwrap();
    let cfg = config(d);
    assert!(serde_json::to_vec_pretty(&cfg).unwrap().len() > 65536);
    assert!(paths.save(&cfg, &paths.fingerprint().unwrap()).is_err());
    assert_eq!(std::fs::read(&paths.config).unwrap(), before);
    paths.load().unwrap();
}
#[test]
fn definition_digest_isolates_cache_and_circuit() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().canonicalize().unwrap();
    let paths = crate::config::Paths {
        config: home.join("config/config.json"),
        data: home.join("data"),
        cache: home.join("cache"),
        home,
    };
    let mut d = definition();
    d.transport = "https".into();
    d.url = "https://example.invalid/decision".into();
    d.limits.cache = true;
    let mut cfg = config(d);
    let request = cfg.driver().prepare(task(), "synthetic-model").unwrap();
    let state = crate::provider::ProviderState::open(&paths).unwrap();
    let calls = std::cell::Cell::new(0);
    let send = || {
        calls.set(calls.get() + 1);
        Ok(serde_json::to_vec(&answer("synthetic-model")).unwrap())
    };
    state
        .decide(&request, "synthetic-credential", &cfg, send)
        .unwrap();
    state
        .decide(&request, "synthetic-credential", &cfg, || {
            panic!("validated cache should be reused")
        })
        .unwrap();
    cfg.api_definition.as_mut().unwrap().id = "different-definition".into();
    state
        .decide(&request, "synthetic-credential", &cfg, send)
        .unwrap();
    assert_eq!(calls.get(), 2);
}
#[test]
fn local_preset_checks_version_weights_and_synthetic_choice() {
    let d = Definition::parse(include_bytes!(
        "../../../examples/providers/local-tev1.json"
    ))
    .unwrap();
    let cfg = config(d.clone());
    for v in ["0.34.9", "0.35", "garbage"] {
        assert!(
            d.diagnostic(&cfg, DiagnosticKind::Check, &[json!({"version":v})])
                .is_err()
        );
    }
    let responses = vec![
        json!({"version":"0.35.1"}),
        json!({"models":[{"name":d.model,"details":{"format":"gguf"}}]}),
    ];
    assert!(matches!(
        d.diagnostic(&cfg, DiagnosticKind::Check, &responses)
            .unwrap(),
        Diagnostic::Request { .. }
    ));
    let mut wrong = responses.clone();
    wrong[1]["models"][0]["details"]["format"] = json!("mlx");
    assert!(d.diagnostic(&cfg, DiagnosticKind::Check, &wrong).is_err());
    wrong[1]["models"] = json!([]);
    assert!(d.diagnostic(&cfg, DiagnosticKind::Check, &wrong).is_err());
}
#[test]
fn gateway_responses_normalize_only_declared_metadata() {
    let d = Definition::parse(include_bytes!(
        "../../../examples/providers/openrouter-jev.json"
    ))
    .unwrap();
    let cfg = config(d.clone());
    let b = d.prepare(task(), &d.model).unwrap();
    let mut response = answer(&d.model);
    response["usage"] = json!({"input_tokens":10,"output_tokens":0,"cost":0.001});
    response["id"] = json!("synthetic-request");
    response["provider"] = json!("TypeSafe");
    assert_eq!(
        crate::provider::validate_for(&cfg, &serde_json::to_vec(&response).unwrap(), &b)
            .unwrap()
            .as_deref(),
        Some("c1")
    );
    response["model"] = json!("typesafe/jev-1.13-20260917");
    assert_eq!(
        crate::provider::validate_for(&cfg, &serde_json::to_vec(&response).unwrap(), &b)
            .unwrap()
            .as_deref(),
        Some("c1")
    );
    response["unexpected"] = json!(true);
    assert!(d.decode(response).is_err());
    let d = Definition::parse(include_bytes!(
        "../../../examples/providers/vercel-openai.json"
    ))
    .unwrap();
    let cfg = config(d.clone());
    let b = d.prepare(task(), &d.model).unwrap();
    let mut response = json!({"model":d.model,"answers":[{"name":"destination","type":"choice","choice":"c1","confidence":0.9,"probabilities":[{"value":"c1","probability":0.9},{"value":"none","probability":0.1}]}],"usage":{"input_tokens":10,"output_tokens":0,"total_tokens":10},"provider_metadata":{"gateway":{"cost":"synthetic"}}});
    assert_eq!(
        crate::provider::validate_for(&cfg, &serde_json::to_vec(&response).unwrap(), &b)
            .unwrap()
            .as_deref(),
        Some("c1")
    );
    response["usage"]["input_tokens_details"] = json!({"cached_tokens":0,"cache_write_tokens":0});
    response["usage"]["output_tokens_details"] = json!({"reasoning_tokens":0});
    assert_eq!(
        crate::provider::validate_for(&cfg, &serde_json::to_vec(&response).unwrap(), &b)
            .unwrap()
            .as_deref(),
        Some("c1")
    );
    response["usage"]["input_tokens_details"]["cached_tokens"] = json!(11);
    assert!(d.decode(response.clone()).is_err());
    response["usage"]["input_tokens_details"]["cached_tokens"] = json!(0);
    response["answers"][0]["probabilities"]
        .as_array_mut()
        .unwrap()
        .push(json!({"value":"c1","probability":0.9}));
    assert!(
        crate::provider::validate_for(&cfg, &serde_json::to_vec(&response).unwrap(), &b).is_err()
    );
}
#[test]
fn finite_probe_paths_and_expected_choice_are_validated() {
    let mut d = definition();
    d.probe = Some(Probe {
        steps: vec![ProbeStep {
            path: "/decision".into(),
            task: Some(task()),
            expect_choice: Some("c1".into()),
            checks: vec![],
        }],
        report: Expr::Literal {
            value: json!({"synthetic_request":"passed"}),
        },
    });
    d.validate().unwrap();
    let cfg = config(d.clone());
    assert!(matches!(
        d.diagnostic(&cfg, DiagnosticKind::Check, &[]).unwrap(),
        Diagnostic::Request { .. }
    ));
    assert!(matches!(
        d.diagnostic(&cfg, DiagnosticKind::Check, &[answer(&d.model)])
            .unwrap(),
        Diagnostic::Complete(_)
    ));
    assert!(
        d.diagnostic(&cfg, DiagnosticKind::Check, &[answer("wrong")])
            .is_err()
    );
    for path in [
        "//remote.invalid",
        "/api?query",
        "/api#fragment",
        "/\\remote",
    ] {
        d.probe.as_mut().unwrap().steps[0].path = path.into();
        assert!(d.validate().is_err());
    }
}
