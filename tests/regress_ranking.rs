//! Iteration 0030: local match strength and decayed visit ordering contracts.

use j_jump::engine::{self, Candidate};
use std::path::Path;
use unicode_casefold::{Locale, UnicodeCaseFold, Variant};
use unicode_normalization::UnicodeNormalization;

const HALF_LIFE: i64 = 604800;
const NOW: i64 = 1_800_000_000;

fn candidate(id: &str, path: &str, count: u32, last_seen: i64) -> Candidate {
    Candidate {
        id: id.into(),
        path: path.into(),
        count,
        last_seen,
        weight: f64::from(count),
    }
}

fn terms(query: &str) -> Vec<String> {
    engine::tokens(&[query.into()])
}

fn ids(candidates: &[Candidate]) -> Vec<&str> {
    candidates
        .iter()
        .map(|candidate| candidate.id.as_str())
        .collect()
}

fn reference_key(value: &str) -> String {
    value
        .nfc()
        .case_fold_with(Variant::Simple, Locale::NonTurkic)
        .collect()
}

#[test]
fn component_key_is_byte_identical_to_whole_string_unicode_normalization() {
    let cases = [
        "",
        "/",
        "//ASCII///PATH/",
        "/ASCII/PARENTS/Cafe\u{301}/",
        "A\u{30a}/A/\u{30a}",
        "\u{301}\u{323}/ASCII/\u{301}\u{323}B",
        "/E\u{301}\u{323}//浏览器///CAFÉ/",
        "foo/CAFÉ",
        "\u{1100}\u{1161}\u{11a8}/\u{1100}/\u{1161}/\u{11a8}",
        "Σ/ς/σ/İ/I/ı/ẞ/ß/K/K/ſ/S",
        "\u{0345}\u{0301}/ᾼ/\u{212b}/Å/A\u{30a}",
        "/中文路径/日本語/한국어/ÉMOJI-🦀/",
        "\u{ff0f}/\u{2215}/\u{2044}",
    ];
    for value in cases {
        assert_eq!(engine::key(value), reference_key(value), "{value:?}");
    }
    assert_eq!(engine::key("A\u{30a}"), "å");
    assert_eq!(engine::key("A/\u{30a}"), "a/\u{30a}");
    assert_eq!(engine::key("//CAFÉ///"), "//café///");
}

#[test]
fn generated_mixed_keys_match_the_frozen_unicode_reference() {
    // Fixed seed and fragments cover reordered combining marks, case folding,
    // Hangul composition, multibyte text, and separators at arbitrary positions.
    let fragments = [
        "",
        "/",
        "//",
        "ABC",
        "xyZ",
        "-_.",
        "浏览器",
        "界",
        "日",
        "韓",
        "🦀",
        "é",
        "E\u{301}",
        "A",
        "\u{30a}",
        "\u{301}",
        "\u{323}",
        "\u{345}",
        "\u{1100}",
        "\u{1161}",
        "\u{11a8}",
        "Σ",
        "ς",
        "İ",
        "ı",
        "ẞ",
        "ß",
        "K",
        "ſ",
        "\u{ff0f}",
        "\u{2044}",
    ];
    let mut state = 0x31a0_cafe_1200_0029_u64;
    for sample in 0..4096 {
        let mut value = String::new();
        for _ in 0..(sample % 32 + 1) {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            value.push_str(fragments[(state >> 32) as usize % fragments.len()]);
        }
        assert_eq!(engine::key(&value), reference_key(&value), "{value:?}");
    }
}

#[test]
fn basename_strength_precedes_frequency_without_removing_ancestor_matches() {
    let query = terms("api");
    let all = vec![
        candidate("ancestor", "/api-project/docs", 10000, NOW),
        candidate("substring", "/work/my-api-helper", 1000, NOW),
        candidate("prefix", "/work/api-helper", 100, NOW),
        candidate("exact", "/work/api", 1, NOW),
    ];
    let eligible = engine::local(&all, &query, Path::new("/neutral"), false, false);
    assert_eq!(eligible.len(), 4, "strength must not narrow eligibility");
    assert_eq!(
        ids(&engine::ranked_lexical(eligible, &query, NOW)),
        ["exact", "prefix", "substring", "ancestor"]
    );
}

#[test]
fn last_path_component_controls_strength() {
    let query = terms("work foo/api");
    let all = vec![
        candidate("ancestor", "/work/foo/api-project/docs", 1000, NOW),
        candidate("prefix", "/work/foo/api-extra", 100, NOW),
        candidate("exact", "/work/foo/api", 1, NOW),
        candidate("wrong-order", "/foo/api/work", 10000, NOW),
    ];
    let eligible = engine::local(&all, &query, Path::new("/neutral"), false, false);
    assert_eq!(
        ids(&engine::ranked_lexical(eligible, &query, NOW)),
        ["exact", "prefix", "ancestor"]
    );
}

#[test]
fn unicode_strength_uses_the_same_keys_and_preserves_actual_paths() {
    let query = terms("浏览器 CAFÉ");
    let all = vec![
        candidate("prefix", "/Work/浏览器/Café-app", 100, NOW),
        candidate("exact", "/Work/浏览器/Cafe\u{301}", 1, NOW),
        candidate("wrong-order", "/Work/Café/浏览器", 10000, NOW),
    ];
    let eligible = engine::local(&all, &query, Path::new("/neutral"), false, false);
    let ordered = engine::ranked_lexical(eligible, &query, NOW);
    assert_eq!(ids(&ordered), ["exact", "prefix"]);
    assert_eq!(ordered[0].path, Path::new("/Work/浏览器/Cafe\u{301}"));
}

#[test]
fn ancestor_only_and_empty_queries_remain_usable() {
    let all = vec![
        candidate("ancestor", "/api-project/docs", 100, NOW),
        candidate("other", "/elsewhere/src", 1, NOW),
    ];
    let query = terms("api");
    let eligible = engine::local(&all, &query, Path::new("/neutral"), false, false);
    assert_eq!(
        ids(&engine::ranked_lexical(eligible, &query, NOW)),
        ["ancestor"]
    );
    assert_eq!(
        ids(&engine::ranked_lexical(all.clone(), &[], NOW)),
        ids(&engine::ranked(all, NOW))
    );
}

#[test]
fn within_one_strength_tier_weight_then_time_then_path_break_ties() {
    let a = candidate("a", "/a/api", 4, NOW);
    let b = candidate("b", "/b/api", 4, NOW);
    let old = candidate("old", "/old/api", 8, NOW - HALF_LIFE);
    let mut heavy_count = candidate("heavy-count", "/count/api", 10000, NOW);
    heavy_count.weight = 1.0;
    let query = terms("api");
    for all in [
        vec![heavy_count.clone(), old.clone(), b.clone(), a.clone()],
        vec![b, a, old, heavy_count],
    ] {
        assert_eq!(
            ids(&engine::ranked_lexical(all, &query, NOW)),
            ["a", "b", "old", "heavy-count"]
        );
    }
}

#[test]
fn final_tie_uses_path_bytes_instead_of_component_order() {
    let all = vec![
        candidate("slash", "/a/b/api", 1, NOW),
        candidate("dash", "/a-b/api", 1, NOW),
    ];
    assert_eq!(
        ids(&engine::ranked_lexical(all, &terms("api"), NOW)),
        ["dash", "slash"]
    );
}

#[test]
fn decay_halves_weight_and_clamps_clock_rollback() {
    assert_eq!(engine::decayed_weight(8.0, NOW, NOW + HALF_LIFE), 4.0);
    assert_eq!(engine::decayed_weight(8.0, NOW, NOW + 2 * HALF_LIFE), 2.0);
    assert_eq!(engine::decayed_weight(8.0, NOW, NOW - HALF_LIFE), 8.0);
    assert_eq!(engine::decayed_weight(0.0, NOW, NOW + HALF_LIFE), 0.0);
    assert_eq!(engine::decayed_weight(8.0, i64::MIN, i64::MAX), 0.0);
    assert_eq!(
        engine::effective_score(&candidate("clock", "/clock", 8, NOW), NOW - HALF_LIFE),
        3.0
    );
    assert_eq!(
        engine::effective_score(&candidate("clock", "/clock", 8, NOW), NOW + HALF_LIFE),
        2.0
    );
}

#[test]
fn ancient_timestamps_keep_relative_order_when_actual_weights_underflow() {
    let larger = candidate("larger", "/z", 100, 100);
    let smaller = candidate("smaller", "/a", 1, 100);
    let mut zero = candidate("zero", "/zero", 10000, NOW);
    zero.weight = 0.0;
    assert_eq!(engine::decayed_weight(100.0, 100, NOW), 0.0);
    assert_eq!(engine::decayed_weight(1.0, 100, NOW), 0.0);
    assert!(engine::effective_score(&larger, NOW).is_finite());
    assert!(engine::effective_score(&larger, NOW) > engine::effective_score(&smaller, NOW));
    assert_eq!(engine::effective_score(&zero, NOW), f64::NEG_INFINITY);
    assert_eq!(
        ids(&engine::ranked(vec![zero, smaller, larger], NOW)),
        ["larger", "smaller", "zero"]
    );
}

#[test]
fn one_revisit_cannot_revive_expired_lifetime_weight() {
    let mut old = candidate("old", "/old/api", 1000, NOW - 365 * 86400);
    let active = candidate("active", "/active/api", 20, NOW);
    let remaining = engine::decayed_weight(f64::from(old.count), old.last_seen, NOW);
    old.weight = remaining + 1.0;
    old.count += 1;
    old.last_seen = NOW;
    assert!(old.weight < 1.000001);
    assert_eq!(
        old.count, 1001,
        "lifetime count is an independent aggregate"
    );
    assert_eq!(
        ids(&engine::ranked(vec![old, active], NOW)),
        ["active", "old"]
    );
}

#[test]
fn ordinary_idle_time_does_not_reverse_frequency_order() {
    let all = vec![
        candidate("old", "/old/api", 100, NOW - 4 * HALF_LIFE),
        candidate("recent", "/recent/api", 7, NOW),
    ];
    for elapsed in [0, 5 * 86400, 365 * 86400] {
        assert_eq!(
            ids(&engine::ranked(all.clone(), NOW + elapsed)),
            ["recent", "old"]
        );
    }
}

#[test]
fn current_weight_is_required_and_roundtrips() {
    let mut value = serde_json::to_value(candidate("current", "/work/api", 20, NOW)).unwrap();
    assert_eq!(value["weight"], 20.0);
    value["weight"] = serde_json::json!(3.5);
    let restored: Candidate = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(restored.weight, 3.5);
    assert_eq!(restored.count, 20);
    assert_eq!(engine::effective_score(&restored, NOW), 3.5_f64.log2());
    value.as_object_mut().unwrap().remove("weight");
    assert!(serde_json::from_value::<Candidate>(value.clone()).is_err());
    value["weight"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<Candidate>(value).is_err());
}
