use crate::{
    Error, Result,
    config::{Config, Paths},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};
use unicode_casefold::{Locale, UnicodeCaseFold, Variant};
use unicode_normalization::UnicodeNormalization;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub id: String,
    pub path: PathBuf,
    pub count: u32,
    pub last_seen: i64,
    /// Current decayed visit weight at last_seen; independent of lifetime count.
    pub weight: f64,
}
pub fn key(s: &str) -> String {
    // ASCII is already NFC, and Unicode simple case folding on ASCII is exactly
    // ASCII lower-casing, so the common path can skip normalisation. The result
    // is byte-identical; the fast path matters because matching scans every
    // inventory row.
    if s.is_ascii() {
        return s.to_ascii_lowercase();
    }
    // '/' prevents NFC composition between path components. Normalize each
    // component independently so an ASCII parent path need not pass through
    // Unicode normalization merely because one directory name is non-ASCII.
    // Splitting arbitrary ASCII runs would be wrong: a following combining
    // mark can compose with their final character. Preserve every separator.
    let mut out = String::with_capacity(s.len());
    for (index, component) in s.split('/').enumerate() {
        if index != 0 {
            out.push('/');
        }
        if component.is_ascii() {
            let start = out.len();
            out.push_str(component);
            out[start..].make_ascii_lowercase();
        } else {
            out.extend(
                component
                    .nfc()
                    .case_fold_with(Variant::Simple, Locale::NonTurkic),
            );
        }
    }
    out
}
const WEIGHT_HALF_LIFE_SECONDS: f64 = 604800.0;

/// Remaining visit weight after seven-day half-lives. Clock rollback never
/// increases the weight; the recorder owns the update timestamp and adds one.
pub fn decayed_weight(weight: f64, last: i64, now: i64) -> f64 {
    weight * (-(now.saturating_sub(last).max(0) as f64) / WEIGHT_HALF_LIFE_SECONDS).exp2()
}

/// Compare weights in the log domain. Exponentiating ancient timestamps can
/// underflow to zero and erase their ordering; subtracting elapsed half-lives
/// retains it. Zero weight naturally maps to negative infinity.
fn weight_score(weight: f64, last: i64, now: i64) -> f64 {
    weight.log2() - (now.saturating_sub(last).max(0) as f64) / WEIGHT_HALF_LIFE_SECONDS
}

pub fn effective_score(candidate: &Candidate, now: i64) -> f64 {
    weight_score(candidate.weight, candidate.last_seen, now)
}
/// True when the process may actually traverse into this directory.
///
/// A directory the process cannot enter is not a usable destination: emitting
/// it as a success would make the caller's `cd` fail and would suppress a
/// lower-scoring but enterable alternative. Only a permission rejection counts;
/// an unusual or transient errno must not disqualify a normal directory.
fn enterable(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(raw) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    if unsafe { libc::access(raw.as_ptr(), libc::X_OK) } == 0 {
        return true;
    }
    !matches!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::EACCES | libc::EPERM)
    )
}
/// Canonical identity of each candidate that can still be resolved.
///
/// Selection binds to the snapshot through this map. Callers pass only the
/// candidates they actually offer, so the cost stays proportional to the
/// offered set rather than the whole inventory.
pub fn identities(candidates: &[Candidate]) -> BTreeMap<PathBuf, PathBuf> {
    candidates
        .iter()
        .filter_map(|c| c.path.canonicalize().ok().map(|r| (c.path.clone(), r)))
        .collect()
}
pub fn valid_path(path: &Path) -> Result<PathBuf> {
    let s = path
        .to_str()
        .ok_or(Error(6, "non UTF-8 directory rejected".into()))?;
    // A control byte inside the success frame would let a directory name inject
    // terminal escapes or break the one-path-plus-LF contract. Escaping would
    // change the bytes the caller must `cd` to, so the destination is refused.
    if s.chars().any(char::is_control) {
        return Err(Error(
            6,
            "directory name contains a control character; destination refused".into(),
        ));
    }
    let p = if path.is_absolute() {
        path.to_path_buf()
    } else {
        logical_cwd()?.join(path)
    };
    if !p.is_dir() {
        return Err(Error(
            6,
            "directory is unavailable; check spelling and permissions".into(),
        ));
    }
    if !enterable(&p) {
        return Err(Error(
            6,
            "directory cannot be entered by this process; check its permissions".into(),
        ));
    }
    // Normalize lexical dot components, preserving symlink spelling like native logical cd.
    let mut out = PathBuf::new();
    for part in p.components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            _ => out.push(part.as_os_str()),
        }
    }
    Ok(out)
}
pub fn logical_cwd() -> Result<PathBuf> {
    let actual =
        std::env::current_dir().map_err(|_| Error(6, "current directory unavailable".into()))?;
    if let Some(p) = std::env::var_os("PWD").map(PathBuf::from)
        && p.is_absolute()
        && p.canonicalize().ok() == actual.canonicalize().ok()
    {
        return Ok(p);
    }
    Ok(actual)
}
pub fn direct(args: &[String], explicit: bool) -> Result<Option<PathBuf>> {
    if args.is_empty() {
        return Ok(Some(valid_path(&PathBuf::from(
            std::env::var_os("HOME").ok_or(Error(6, "HOME unavailable".into()))?,
        ))?));
    }
    if args.len() == 1 {
        let s = &args[0];
        if s == "-" && !explicit {
            return Ok(Some(valid_path(&PathBuf::from(
                std::env::var_os("OLDPWD").ok_or(Error(6, "no previous directory".into()))?,
            ))?));
        }
        if explicit
            || Path::new(s).is_dir()
            || s.starts_with('/')
            || s.starts_with("./")
            || s.starts_with("../")
            || s == "."
            || s == ".."
            || s.ends_with('/')
        {
            return Ok(Some(valid_path(Path::new(s))?));
        }
    }
    if explicit {
        return Err(Error(2, "-- requires exactly one directory path".into()));
    }
    Ok(None)
}
pub fn tokens(args: &[String]) -> Vec<String> {
    args.iter()
        .flat_map(|s| s.split_whitespace())
        .map(key)
        .collect()
}
pub fn matches(path: &Path, terms: &[String]) -> bool {
    let p = key(&path.to_string_lossy());
    let mut offset = 0;
    for term in terms {
        if let Some(i) = p[offset..].find(term) {
            offset += i + term.len()
        } else {
            return false;
        }
    }
    true
}
pub fn ranked(all: Vec<Candidate>, now: i64) -> Vec<Candidate> {
    ranked_lexical(all, &[], now)
}

/// Rank an already-matched set without changing ordered-substring eligibility.
/// Exact basename, basename prefix and basename substring matches outrank
/// ancestor-only matches. The last query component supplies that evidence;
/// empty queries use only decayed frequency. Compute each key once per row.
pub fn ranked_lexical(all: Vec<Candidate>, terms: &[String], now: i64) -> Vec<Candidate> {
    let last_component = terms
        .last()
        .and_then(|term| term.rsplit('/').find(|part| !part.is_empty()));
    let mut keyed: Vec<_> = all
        .into_iter()
        .map(|candidate| {
            let strength = last_component.map_or(0, |term| {
                let name = key(candidate
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(""));
                if name == term {
                    3
                } else if name.starts_with(term) {
                    2
                } else if name.contains(term) {
                    1
                } else {
                    0
                }
            });
            let score = effective_score(&candidate, now);
            (candidate, strength, score)
        })
        .collect();
    keyed.sort_by(|(a, a_strength, a_score), (b, b_strength, b_score)| {
        b_strength
            .cmp(a_strength)
            .then_with(|| b_score.total_cmp(a_score))
            .then_with(|| b.last_seen.cmp(&a.last_seen))
            .then_with(|| {
                a.path
                    .as_os_str()
                    .as_encoded_bytes()
                    .cmp(b.path.as_os_str().as_encoded_bytes())
            })
    });
    keyed
        .into_iter()
        .map(|(candidate, _, _)| candidate)
        .collect()
}
pub fn local(
    all: &[Candidate],
    terms: &[String],
    cwd: &Path,
    child: bool,
    interactive: bool,
) -> Vec<Candidate> {
    all.iter()
        .filter(|c| {
            (interactive || c.path != cwd)
                && (!child || (c.path != cwd && c.path.starts_with(cwd)))
                && matches(&c.path, terms)
        })
        .cloned()
        .collect()
}
/// One Jev option: the identity actually disclosed for the configured privacy
/// level, and every eligible directory sharing it, in rank order.
#[derive(Clone, Debug)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub parent: String,
    pub usage: &'static str,
    pub members: Vec<Candidate>,
}
/// Structural names say little about intent and are ambiguous across projects.
const GENERIC_NAMES: &[&str] = &[
    "src",
    "source",
    "sources",
    "lib",
    "libs",
    "bin",
    "build",
    "builds",
    "dist",
    "out",
    "output",
    "target",
    "tmp",
    "temp",
    "test",
    "tests",
    "spec",
    "specs",
    "doc",
    "docs",
    "script",
    "scripts",
    "config",
    "configs",
    "conf",
    "assets",
    "static",
    "public",
    "internal",
    "pkg",
    "cmd",
    "vendor",
    "node_modules",
    "include",
    "examples",
    "example",
    "resources",
    "res",
    "debug",
    "release",
    "main",
    "util",
    "utils",
    "common",
    "core",
    "misc",
    "old",
    "new",
    "backup",
    ".git",
    ".github",
    "__pycache__",
    ".venv",
    "venv",
    "env",
    "logs",
    "log",
    "cache",
    ".cache",
    ".config",
    "fixtures",
];
fn generic(name: &str) -> bool {
    let folded = key(name);
    GENERIC_NAMES.contains(&folded.as_str())
        || (!folded.is_empty()
            && folded
                .chars()
                .all(|c| c.is_ascii_digit() || c == '-' || c == '.'))
}
fn usage(count: u32) -> &'static str {
    if count > 10 {
        "high"
    } else if count > 2 {
        "medium"
    } else {
        "low"
    }
}
/// The identity sent to Jev: name only (strict), parent and name (balanced) or
/// full parent path and name (full).
fn disclosed(path: &Path, privacy: &str) -> (String, String) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent = match privacy {
        "strict" => String::new(),
        "balanced" => path
            .parent()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        _ => path
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };
    (name, parent)
}
/// Shortlist v2. Lexical matches, when present, are the only options (Jev then
/// disambiguates them). Otherwise every eligible directory is grouped by its
/// disclosed identity and groups are taken in priority order: query-term
/// evidence, informative before generic names, then shallow and frequently used
/// groups interleaved, up to `candidate_limit`. Returns the groups and whether
/// any eligible group did not fit.
pub fn grouped_shortlist(
    all: &[Candidate],
    lexical: &[Candidate],
    terms: &[String],
    paths: &Paths,
    config: &Config,
) -> (Vec<Group>, bool) {
    grouped_shortlist_in(all, lexical, terms, paths, config, None)
}
fn grouped_shortlist_in(
    all: &[Candidate],
    lexical: &[Candidate],
    terms: &[String],
    paths: &Paths,
    config: &Config,
    cwd: Option<&Path>,
) -> (Vec<Group>, bool) {
    let policy = paths.policy(config, true);
    let source = if lexical.is_empty() { all } else { lexical };
    let mut order: Vec<(String, String)> = Vec::new();
    let mut groups: std::collections::HashMap<(String, String), Vec<Candidate>> =
        std::collections::HashMap::new();
    let mut seen = HashSet::new();
    for c in source.iter().filter(|c| policy.allows(&c.path)) {
        // Two spellings of one physical directory are one candidate.
        let Ok(real) = c.path.canonicalize() else {
            continue;
        };
        if !seen.insert(real) {
            continue;
        }
        let identity = disclosed(&c.path, &config.privacy);
        if identity.0.is_empty() {
            continue;
        }
        let entry = groups.entry(identity.clone()).or_default();
        if entry.is_empty() {
            order.push(identity);
        }
        entry.push(c.clone());
    }
    let capabilities = config.provider.driver().capabilities();
    let limit = config.candidate_limit.min(capabilities.max_candidates);
    let picked: Vec<(String, String)> = if !lexical.is_empty() {
        order
    } else {
        let depth = |members: &[Candidate]| {
            members
                .iter()
                .map(|c| {
                    c.path
                        .strip_prefix(&paths.home)
                        .unwrap_or(&c.path)
                        .components()
                        .count()
                })
                .min()
                .unwrap_or(usize::MAX)
        };
        let evidence = |identity: &(String, String)| {
            let text = key(&format!("{}/{}", identity.1, identity.0));
            terms.iter().any(|t| text.contains(t.as_str()))
        };
        let (with_terms, rest): (Vec<_>, Vec<_>) = order.into_iter().partition(evidence);
        let (generic_names, informative): (Vec<_>, Vec<_>) =
            rest.into_iter().partition(|identity| generic(&identity.0));
        // `informative` is already in frecency order; interleave it with the
        // same groups sorted by depth so neither shallow projects nor busy
        // subdirectories crowd the other out.
        let mut shallow = informative.clone();
        shallow.sort_by_key(|identity| depth(&groups[identity]));
        let mut merged = Vec::with_capacity(informative.len());
        let mut taken = HashSet::new();
        for (a, b) in shallow.iter().zip(informative.iter()) {
            for identity in [a, b] {
                if taken.insert(identity.clone()) {
                    merged.push(identity.clone());
                }
            }
        }
        if capabilities.diverse_shortlist {
            // Each tier receives slots before any tier consumes the entire
            // budget. Generic directory names are useful semantic destinations,
            // not a reason to exclude them from a small model's candidate set.
            let ancestor = cwd.and_then(|cwd| {
                all.iter()
                    .map(|c| c.path.as_path())
                    .filter(|p| *p != cwd && cwd.starts_with(p))
                    .max_by_key(|p| p.components().count())
            });
            let in_scope = |identity: &(String, String)| {
                cwd.is_some_and(|cwd| {
                    groups[identity].iter().any(|c| {
                        c.path != cwd
                            && (c.path.starts_with(cwd)
                                || ancestor.is_some_and(|a| c.path.starts_with(a)))
                    })
                })
            };
            let (local_generic, global_generic): (Vec<_>, Vec<_>) =
                generic_names.into_iter().partition(in_scope);
            let (local_named, global_named): (Vec<_>, Vec<_>) =
                merged.into_iter().partition(in_scope);
            let mut tiers = [
                with_terms.into_iter(),
                local_generic.into_iter(),
                local_named.into_iter(),
                global_generic.into_iter(),
                global_named.into_iter(),
            ];
            let mut diverse = Vec::new();
            loop {
                let before = diverse.len();
                for tier in &mut tiers {
                    if let Some(item) = tier.next() {
                        diverse.push(item);
                    }
                }
                if diverse.len() == before {
                    break;
                }
            }
            diverse
        } else {
            with_terms
                .into_iter()
                .chain(merged)
                .chain(generic_names)
                .collect()
        }
    };
    let truncated = picked.len() > limit;
    let out = picked
        .into_iter()
        .take(limit)
        .enumerate()
        .map(|(i, identity)| {
            let members = groups.remove(&identity).unwrap_or_default();
            let count = members.iter().map(|c| c.count).max().unwrap_or(0);
            Group {
                id: format!("c{i}"),
                name: identity.0,
                parent: identity.1,
                usage: usage(count),
                members,
            }
        })
        .collect();
    (out, truncated)
}

/// Candidate set for the opt-in forced semantic route (RQ-AI-014).
///
/// The default lexical shortlist shares the local matcher, so a semantically
/// correct directory whose name does not contain the query can never reach the
/// provider. This builder adds the current directory's scope first: recorded
/// directories inside `cwd` and inside the nearest recorded ancestor of `cwd`
/// (the enclosing project root), then the lexical matches, then the globally
/// ranked remainder. Order within each tier follows the caller's native
/// frecency order, identities are deduplicated by canonical path, and the total
/// is capped by `limit` so the provider payload stays bounded.
pub fn scoped(
    all: &[Candidate],
    lexical: &[Candidate],
    cwd: &Path,
    limit: usize,
) -> Vec<Candidate> {
    let ancestor = all
        .iter()
        .map(|c| c.path.as_path())
        .filter(|p| *p != cwd && cwd.starts_with(p))
        .max_by_key(|p| p.components().count());
    let in_scope = |c: &Candidate| {
        c.path != cwd
            && (c.path.starts_with(cwd) || ancestor.is_some_and(|a| c.path.starts_with(a)))
    };
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |c: &Candidate| {
        if out.len() >= limit {
            return;
        }
        let real = c.path.canonicalize().unwrap_or_else(|_| c.path.clone());
        if seen.insert(real) {
            out.push(c.clone());
        }
    };
    for c in all.iter().filter(|c| in_scope(c)) {
        push(c);
    }
    for c in lexical {
        push(c);
    }
    for c in all {
        push(c);
    }
    out
}

pub fn semantic_groups(
    all: &[Candidate],
    lexical: &[Candidate],
    terms: &[String],
    cwd: &Path,
    paths: &Paths,
    cfg: &Config,
    force: bool,
) -> (Vec<Group>, bool) {
    if force {
        let ordered = scoped(all, lexical, cwd, usize::MAX);
        if cfg.provider.driver().capabilities().diverse_shortlist {
            grouped_shortlist_in(&ordered, &[], terms, paths, cfg, Some(cwd))
        } else {
            grouped_shortlist_in(&ordered, &ordered, terms, paths, cfg, Some(cwd))
        }
    } else {
        grouped_shortlist_in(all, lexical, terms, paths, cfg, Some(cwd))
    }
}
