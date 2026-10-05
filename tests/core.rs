use j_jump::{
    config::{Config, Paths},
    engine::{self, Candidate},
    provider,
    store::Store,
};
use serde_json::json;
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};
fn candidate(id: &str, path: PathBuf, count: u32, last_seen: i64) -> Candidate {
    Candidate {
        id: id.into(),
        path,
        count,
        last_seen,
        weight: f64::from(count),
    }
}
fn fixture() -> (tempfile::TempDir, Paths) {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let home = root.join("home");
    fs::create_dir(&home).unwrap();
    let paths = Paths {
        config: root.join("private/config.json"),
        data: root.join("data"),
        cache: root.join("cache"),
        home,
    };
    (t, paths)
}
#[test]
fn ordered_unicode_matching() {
    assert!(engine::matches(
        std::path::Path::new("/Work/浏览器/Cafe\u{301}"),
        &["浏览器".into(), "café".into()]
    ));
    assert!(!engine::matches(
        std::path::Path::new("/foo/bar"),
        &["bar".into(), "foo".into()]
    ));
    assert!(engine::matches(
        std::path::Path::new("/a/$(touch nope)"),
        &["$(touch".into()]
    ));
}
#[test]
fn score_clock_and_determinism() {
    assert_eq!(
        engine::effective_score(&candidate("clock", "/clock".into(), 3, 200), 100),
        3.0_f64.log2()
    );
    let a = candidate("a", "/a".into(), 2, 100);
    let b = candidate("b", "/b".into(), 2, 100);
    assert_eq!(engine::ranked(vec![b, a], 100)[0].id, "a");
}
#[test]
fn subtree_boundary() {
    let all = vec![
        candidate("a", "/work/api".into(), 1, 0),
        candidate("b", "/workspace/api".into(), 1, 0),
    ];
    assert_eq!(
        engine::local(&all, &[], std::path::Path::new("/work"), true, false).len(),
        1
    );
}
#[test]
fn config_atomic_conflict_and_permissions() {
    let (_t, p) = fixture();
    let c = Config::default();
    p.save(&c, "absent").unwrap();
    assert!(p.save(&c, "absent").is_err());
    assert_eq!(p.load().unwrap(), c);
    assert_eq!(
        fs::metadata(&p.config).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(p.config.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}
#[test]
fn corrupt_and_future_config_fail_closed() {
    let (_t, p) = fixture();
    p.save(&Config::default(), "absent").unwrap();
    fs::write(&p.config, b"{}").unwrap();
    assert!(p.load().is_err());
    fs::write(&p.config, br#"{"schema_version":99}"#).unwrap();
    assert!(p.load().is_err());
    fs::write(&p.config, br#"{"api_key":"accidental-secret"}"#).unwrap();
    assert!(p.load().is_err());
}
#[test]
fn config_symlink_rejected() {
    let (t, p) = fixture();
    fs::create_dir(p.config.parent().unwrap()).unwrap();
    let file = t.path().join("other");
    fs::write(&file, b"{}").unwrap();
    std::os::unix::fs::symlink(file, &p.config).unwrap();
    assert!(p.load().is_err());
}
#[test]
fn privacy_hard_roots_and_symlinks() {
    let (t, p) = fixture();
    let ssh = p.home.join(".ssh");
    let ordinary = p.home.join(".ssh-backup");
    fs::create_dir(&ssh).unwrap();
    fs::create_dir(&ordinary).unwrap();
    let link = t.path().join("alias");
    std::os::unix::fs::symlink(&ssh, &link).unwrap();
    let c = Config::default();
    assert!(!p.allowed(&c, &ssh, false));
    assert!(!p.allowed(&c, &link, true));
    assert!(p.allowed(&c, &ordinary, false));
    assert!(!p.allowed(&c, &p.home, false));
    assert!(p.allowed(&c, &p.home, true));
}
#[test]
fn no_send_does_not_disable_local_history() {
    let (_t, p) = fixture();
    let dir = p.home.join("project");
    fs::create_dir(&dir).unwrap();
    let c = Config {
        no_send: vec![dir.clone()],
        ..Config::default()
    };
    assert!(p.allowed(&c, &dir, false));
    assert!(!p.allowed(&c, &dir, true));
}
#[test]
fn store_schema_and_clear_generation() {
    let (_t, p) = fixture();
    let mut s = Store::open(&p, false).unwrap();
    s.db.execute(
        "INSERT INTO visits(path,key,count,last_seen,weight) VALUES('/fixture','fixture',1,0,1)",
        [],
    )
    .unwrap();
    let g = s.generation().unwrap();
    assert_eq!(s.clear(None, false).unwrap(), 1);
    assert_eq!(s.generation().unwrap(), g);
    assert_eq!(s.clear(None, true).unwrap(), 1);
    assert_ne!(s.generation().unwrap(), g);
    s.db.execute_batch("PRAGMA user_version=99").unwrap();
    drop(s);
    assert!(Store::open(&p, false).is_err());
}
#[test]
fn store_corruption_not_recreated() {
    let (_t, p) = fixture();
    let s = Store::open(&p, false).unwrap();
    drop(s);
    let file = p.data.join("visits.db");
    fs::write(&file, b"not sqlite").unwrap();
    assert!(Store::open(&p, false).is_err());
    assert_eq!(fs::read(file).unwrap(), b"not sqlite");
}
#[test]
fn malicious_terminal_text_is_escaped() {
    let s = j_jump::display("x\x1b]52;c;bad\x07\u{202e}\t\r");
    assert!(!s.contains('\x1b'));
    assert!(!s.contains('\u{202e}'));
    assert!(s.contains("\\u{"));
}
#[test]
fn shell_names_never_inject() {
    for name in [
        "jjump",
        "cd",
        "x;touch a",
        "if",
        "__jj_resolve",
        "$(bad)",
        "-x",
        "中",
        "a\nb",
        "",
    ] {
        assert!(j_jump::shell::init("bash", name).is_err());
    }
    assert!(j_jump::shell::init("zsh", "jump_2").is_ok());
    assert!(j_jump::shell::init("sh", "j").is_err());
}
fn group(c: engine::Candidate) -> engine::Group {
    engine::Group {
        id: c.id.clone(),
        name: c.path.file_name().unwrap().to_string_lossy().into_owned(),
        parent: String::new(),
        usage: "low",
        members: vec![c],
    }
}
fn request_fixture() -> (tempfile::TempDir, Vec<u8>) {
    let (t, p) = fixture();
    let dir = p.home.join("server");
    fs::create_dir(&dir).unwrap();
    let c = candidate("d1", dir, 1, 1);
    let bytes = provider::request("backend", &[group(c)], &p.home, &p, &Config::default()).unwrap();
    (t, bytes.0)
}
fn response() -> serde_json::Value {
    json!({"model":provider::MODEL,"answers":{"destination":{"type":"choice","choice":"d1","probabilities":{"d1":0.9,"none":0.1},"confidence":0.8}},"usage":{"input_tokens":1,"output_tokens":1}})
}
#[test]
fn jev_fixed_id_contract() {
    let (_t, req) = request_fixture();
    let mut v = response();
    assert_eq!(
        provider::validate(&serde_json::to_vec(&v).unwrap(), &req).unwrap(),
        Some("d1".into())
    );
    v["answers"]["destination"]["choice"] = json!("/attacker");
    assert!(provider::validate(&serde_json::to_vec(&v).unwrap(), &req).is_err());
}
#[test]
fn jev_rejects_schema_and_probability_errors() {
    let (_t, req) = request_fixture();
    for bad in [json!(true), json!(-1), json!(2), json!("0.9"), json!(null)] {
        let mut v = response();
        v["answers"]["destination"]["confidence"] = bad;
        assert!(provider::validate(&serde_json::to_vec(&v).unwrap(), &req).is_err());
    }
    for field in ["model", "answers"] {
        let mut v = response();
        v.as_object_mut().unwrap().remove(field);
        assert!(provider::validate(&serde_json::to_vec(&v).unwrap(), &req).is_err());
    }
}
#[test]
fn jev_cent_grid_sum_compatibility_is_bounded() {
    let (_t, req) = request_fixture();
    for probabilities in [
        json!({"d1": 0.81, "none": 0.18}),
        json!({"d1": 0.82, "none": 0.19}),
    ] {
        let mut v = response();
        v["answers"]["destination"]["probabilities"] = probabilities;
        assert_eq!(
            provider::validate(&serde_json::to_vec(&v).unwrap(), &req).unwrap(),
            Some("d1".into())
        );
    }
    for probabilities in [
        json!({"d1": 0.80, "none": 0.18}),
        json!({"d1": 0.83, "none": 0.19}),
        json!({"d1": 0.805, "none": 0.185}),
        json!({"d1": 1.01, "none": 0.0}),
        json!({"d1": 0.81}),
        json!({"d1": 0.81, "none": 0.17, "unknown": 0.01}),
    ] {
        let mut v = response();
        v["answers"]["destination"]["probabilities"] = probabilities;
        assert!(provider::validate(&serde_json::to_vec(&v).unwrap(), &req).is_err());
    }
    let mut v = response();
    v["answers"]["destination"]["probabilities"] = json!({"d1": 0.18, "none": 0.81});
    v["answers"]["destination"]["choice"] = json!("none");
    assert_eq!(
        provider::validate(&serde_json::to_vec(&v).unwrap(), &req).unwrap(),
        None
    );
    v["answers"]["destination"]["probabilities"] = json!({"d1": 0.81, "none": 0.18});
    v["answers"]["destination"]["choice"] = json!("d1");
    v["answers"]["destination"]["probabilities"] = json!({"d1":0.5,"none":0.5});
    assert_eq!(
        provider::validate(&serde_json::to_vec(&v).unwrap(), &req).unwrap(),
        None
    );
    v["answers"]["destination"]["probabilities"] = json!({"d1": 0.49, "none": 0.52});
    assert!(provider::validate(&serde_json::to_vec(&v).unwrap(), &req).is_err());
}
#[test]
fn jev_cent_grid_normalized_border_abstains() {
    let (_t, req) = request_fixture();
    let mut request: serde_json::Value = serde_json::from_slice(&req).unwrap();
    request["questions"]["destination"]["criteria"]["d2"] = json!({"name":"other"});
    let request = serde_json::to_vec(&request).unwrap();
    let mut v = response();
    v["answers"]["destination"]["probabilities"] = json!({"d1":0.50,"none":0.49,"d2":0.02});
    assert_eq!(
        provider::validate(&serde_json::to_vec(&v).unwrap(), &request).unwrap(),
        None
    );
    v["answers"]["destination"]["probabilities"] = json!({"d1":0.50,"none":0.48,"d2":0.01});
    assert_eq!(
        provider::validate(&serde_json::to_vec(&v).unwrap(), &request).unwrap(),
        Some("d1".into())
    );
    v["answers"]["destination"]["probabilities"] = json!({"d1":0.49,"none":0.49,"d2":0.01});
    assert_eq!(
        provider::validate(&serde_json::to_vec(&v).unwrap(), &request).unwrap(),
        None
    );
}
#[test]
fn jev_none_tie_and_contradiction_abstain() {
    let (_t, req) = request_fixture();
    let mut v = response();
    v["answers"]["destination"]["choice"] = json!("none");
    v["answers"]["destination"]["probabilities"] = json!({"d1":0.1,"none":0.9});
    assert_eq!(
        provider::validate(&serde_json::to_vec(&v).unwrap(), &req).unwrap(),
        None
    );
    v = response();
    v["answers"]["destination"]["probabilities"] = json!({"d1":0.5,"none":0.5});
    assert_eq!(
        provider::validate(&serde_json::to_vec(&v).unwrap(), &req).unwrap(),
        None
    );
}
#[test]
fn strict_duplicate_and_size_rejection() {
    assert!(provider::strict_json(br#"{"a":1,"a":2}"#).is_err());
    assert!(provider::strict_json(br#"{"x":{"a":1,"a":2}}"#).is_err());
    assert!(provider::strict_json(&vec![b' '; 262145]).is_err());
    assert!(provider::strict_json(br#"{"x":NaN}"#).is_err());
}
#[test]
fn payload_strict_excludes_paths_and_usage() {
    let (_t, req) = request_fixture();
    let v: serde_json::Value = serde_json::from_slice(&req).unwrap();
    assert!(v["state"].get("cwd").is_none());
    assert!(
        v["questions"]["destination"]["criteria"]["d1"]
            .get("parent")
            .is_none()
    );
    assert!(
        v["questions"]["destination"]["criteria"]["d1"]
            .get("usage")
            .is_none()
    );
    assert_eq!(v["state"]["query"], "backend");
}
#[test]
fn shortlist_cap_and_physical_dedup() {
    let (_t, p) = fixture();
    let mut all = vec![];
    for i in 0..100 {
        let dir = p.home.join(format!("project{i}"));
        fs::create_dir(&dir).unwrap();
        all.push(candidate(&format!("d{i}"), dir, 1, 1));
    }
    let c = Config::default();
    let (short, truncated) = engine::grouped_shortlist(&all, &[], &[], &p, &c);
    assert_eq!(short.len(), 100);
    assert!(!truncated);
    assert_eq!(engine::local(&all, &[], &p.home, false, false).len(), 100);
}
#[test]
fn invalid_path_bytes_and_explicit_errors() {
    assert!(engine::valid_path(std::path::Path::new("/a\nb")).is_err());
    assert!(engine::direct(&["/does-not-exist-jjump-fixture".into()], false).is_err());
}

#[test]
fn provider_has_no_request_quota_and_cache_binds_generation() {
    let (_t, paths) = fixture();
    let (_f, req) = request_fixture();
    let state = provider::ProviderState::open(&paths).unwrap();
    let cfg = Config {
        semantic: true,
        ..Config::default()
    };
    assert!(
        state
            .decide(&req, "failed-first", &cfg, || Err(j_jump::Error(
                5,
                "synthetic failure".into()
            )))
            .is_err()
    );
    let mut calls = 0;
    for epoch in 0..100 {
        let binding = format!("epoch:{epoch}");
        assert_eq!(
            state
                .decide(&req, &binding, &cfg, || {
                    calls += 1;
                    Ok(serde_json::to_vec(&response()).unwrap())
                })
                .unwrap(),
            Some("d1".into())
        );
        state
            .decide(&req, &binding, &cfg, || panic!("cache must not dispatch"))
            .unwrap();
    }
    assert_eq!(calls, 100);
    assert!(
        state
            .decide(&req, "epoch:0", &Config::default(), || panic!(
                "off cannot use cache"
            ))
            .is_err()
    );
    state.clear(true).unwrap();
    state
        .decide(&req, "epoch:0", &cfg, || {
            Ok(serde_json::to_vec(&response()).unwrap())
        })
        .unwrap();
    let db = rusqlite::Connection::open(paths.cache.join("semantic-cache.db")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='budget'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}
#[test]
fn provider_lock_failure_accounting_and_circuit() {
    let (_t, paths) = fixture();
    let (_f, req) = request_fixture();
    let state = provider::ProviderState::open(&paths).unwrap();
    assert!(provider::ProviderState::open(&paths).is_err());
    let cfg = Config {
        semantic: true,
        ..Config::default()
    };
    for _ in 0..3 {
        assert!(
            state
                .decide(&req, "binding", &cfg, || Err(j_jump::Error(
                    5,
                    "mock outage".into()
                )))
                .is_err()
        );
    }
    assert!(
        state
            .decide(&req, "binding", &cfg, || panic!(
                "circuit should block network"
            ))
            .is_err()
    );
    assert_eq!(state.clear(false).unwrap(), 0);
}
#[test]
fn strict_sensitive_payload_is_blocked() {
    let (_t, p) = fixture();
    let dir = p.home.join("project");
    fs::create_dir(&dir).unwrap();
    let c = candidate("d1", dir, 1, 0);
    assert!(
        provider::request(
            "~/.ssh/secret",
            std::slice::from_ref(&group(c.clone())),
            &p.home,
            &p,
            &Config::default()
        )
        .is_err()
    );
    assert!(
        provider::request(
            &"x".repeat(1025),
            &[group(c)],
            &p.home,
            &p,
            &Config::default()
        )
        .is_err()
    );
}

#[test]
fn visit_cap_evicts_by_frozen_score() {
    let (_t, p) = fixture();
    let mut s = Store::open(&p, false).unwrap();
    {
        let tx = s.db.transaction().unwrap();
        for i in 0..10000 {
            tx.execute(
                "INSERT INTO visits(path,key,count,last_seen,weight) VALUES(?1,?1,1,0,1)",
                [format!("/synthetic/project{i:05}")],
            )
            .unwrap();
        }
        tx.commit().unwrap();
    }
    let cwd = std::env::current_dir().unwrap();
    s.record(&p, &Config::default(), &cwd, None).unwrap();
    let rows = s.list().unwrap();
    assert_eq!(rows.len(), 9000);
    assert!(rows.iter().any(|r| r.path == cwd));
    assert!(
        !rows
            .iter()
            .any(|r| r.path == std::path::Path::new("/synthetic/project00000"))
    );
}
#[test]
fn counts_saturate_without_overflow() {
    let (_t, p) = fixture();
    let mut s = Store::open(&p, false).unwrap();
    let cwd = std::env::current_dir().unwrap();
    s.record(&p, &Config::default(), &cwd, None).unwrap();
    s.db.execute("UPDATE visits SET count=2147483647", [])
        .unwrap();
    s.record(&p, &Config::default(), &cwd, None).unwrap();
    assert_eq!(s.list().unwrap()[0].count, 2147483647);
}
#[test]
fn provider_metadata_cannot_smuggle_history_into_cache() {
    let (_t, req) = request_fixture();
    let mut v = response();
    v["usage"]["raw_query"] = json!("private text");
    assert!(provider::validate(&serde_json::to_vec(&v).unwrap(), &req).is_err());
}

#[test]
fn existing_state_symlink_ancestor_is_accepted() {
    let (t, p) = fixture();
    let real = t.path().join("real");
    fs::create_dir(&real).unwrap();
    fs::set_permissions(&real, fs::Permissions::from_mode(0o700)).unwrap();
    let child = real.join("state");
    fs::create_dir(&child).unwrap();
    fs::set_permissions(&child, fs::Permissions::from_mode(0o700)).unwrap();
    let alias = p.home.join("alias");
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    // An ordinary private directory reached through a symbolic-linked ancestor
    // is accepted; only the leaf is required to be a real private directory.
    j_jump::config::private_dir(&alias.join("state")).unwrap();
    // A state directory that IS a symbolic link is still refused.
    let dirlink = t.path().join("dirlink");
    std::os::unix::fs::symlink(&child, &dirlink).unwrap();
    assert!(j_jump::config::private_dir(&dirlink).is_err());
}

#[test]
fn forced_scope_includes_current_directory_members() {
    let cwd = PathBuf::from("/root/project");
    let all = vec![
        candidate("d1", PathBuf::from("/root/project/stage/templates"), 8, 0),
        candidate(
            "d2",
            PathBuf::from("/root/other/stage/templates-extra"),
            80,
            0,
        ),
        candidate("d3", PathBuf::from("/root/project/assets/fonts"), 5, 0),
        candidate("d4", PathBuf::from("/root/elsewhere/x"), 90, 0),
    ];
    let lexical = vec![all[0].clone(), all[1].clone()];
    let out = engine::scoped(&all, &lexical, &cwd, 2);
    assert_eq!(out.len(), 2);
    // Scope members win over the higher-frecency out-of-scope lexical match.
    assert_eq!(out[0].id, "d1");
    assert_eq!(out[1].id, "d3");
}

#[test]
fn forced_scope_uses_the_nearest_recorded_ancestor() {
    let cwd = PathBuf::from("/root/project/sub");
    let all = vec![
        candidate("d1", PathBuf::from("/root/project"), 1, 0),
        candidate("d2", PathBuf::from("/root/project/other/target"), 4, 0),
        candidate("d3", PathBuf::from("/root/far/target"), 50, 0),
    ];
    let lexical = vec![all[2].clone()];
    let out = engine::scoped(&all, &lexical, &cwd, 3);
    assert_eq!(
        out.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        ["d1", "d2", "d3"]
    );
}

#[test]
fn forced_scope_deduplicates_and_caps() {
    let cwd = PathBuf::from("/root/project");
    let all = vec![
        candidate("d1", PathBuf::from("/root/project/a"), 3, 0),
        candidate("d2", PathBuf::from("/root/project/a"), 3, 0),
        candidate("d3", PathBuf::from("/root/project/b"), 2, 0),
    ];
    let out = engine::scoped(&all, &[], &cwd, 5);
    assert_eq!(out.len(), 2);
    let capped = engine::scoped(&all, &[], &cwd, 1);
    assert_eq!(capped.len(), 1);
}

#[test]
fn semantic_route_defaults_local_first_and_validates_values() {
    let base = Config::default();
    assert_eq!(base.semantic_route, "local_first");
    base.validate().unwrap();
    let forced = Config {
        semantic_route: "force".into(),
        ..Config::default()
    };
    forced.validate().unwrap();
    let bad = Config {
        semantic_route: "sometimes".into(),
        ..Config::default()
    };
    assert!(bad.validate().is_err());
}

#[test]
fn unreadable_cache_does_not_become_a_network_cache_miss() {
    let (_t, paths) = fixture();
    let (_f, req) = request_fixture();
    let state = provider::ProviderState::open(&paths).unwrap();
    let db = rusqlite::Connection::open(paths.cache.join("semantic-cache.db")).unwrap();
    db.execute_batch("DROP TABLE cache").unwrap();
    let cfg = Config {
        semantic: true,
        ..Config::default()
    };
    let error = state
        .decide(&req, "test", &cfg, || {
            panic!("unreadable cache must not dispatch")
        })
        .unwrap_err();
    assert_eq!(error.0, 7);
    assert_eq!(
        db.query_row("SELECT count(*) FROM dispatch", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}
