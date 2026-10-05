//! Offline research harness. Calls production engine functions without opening
//! a store, touching directories, or adding a runtime command.
use j_jump::engine::{Candidate, decayed_weight, ranked};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{self, BufRead, Write},
    path::PathBuf,
};

const CAP: u32 = 2_147_483_647;

fn order(state: &BTreeMap<String, Candidate>, now: i64) -> Vec<String> {
    ranked(state.values().cloned().collect(), now)
        .into_iter()
        .map(|candidate| candidate.id)
        .collect()
}

fn replay(input: Value, out: &mut impl Write) -> Result<(), Box<dyn std::error::Error>> {
    let trace_id = input["id"].as_str().ok_or("trace id must be a string")?;
    let paths: BTreeMap<String, String> = serde_json::from_value(input["paths"].clone())?;
    let initial: Vec<Candidate> =
        serde_json::from_value(input.get("initial").cloned().unwrap_or(json!([])))?;
    let mut state = BTreeMap::new();
    for candidate in initial {
        if candidate.count == 0
            || candidate.count > CAP
            || !candidate.weight.is_finite() || !(0.0..=f64::from(CAP)).contains(&candidate.weight)
            || paths.get(&candidate.id).map(PathBuf::from).as_ref() != Some(&candidate.path)
        {
            return Err("invalid initial candidate".into());
        }
        if state.insert(candidate.id.clone(), candidate).is_some() {
            return Err("duplicate initial candidate".into());
        }
    }
    let events = input["events"]
        .as_array()
        .ok_or("events must be an array")?;
    for (index, event) in events.iter().enumerate() {
        let now = event["now"].as_i64().ok_or("event time must be an i64")?;
        let before = order(&state, now);
        let updated = match event.get("target") {
            None | Some(Value::Null) => None,
            Some(value) => {
                let id = value.as_str().ok_or("target must be an ID")?;
                let path = paths.get(id).ok_or("target has no declared path")?;
                let candidate = if let Some(old) = state.get(id) {
                    // Mirror the declared recorder transition; the actual decay
                    // quantity is computed by the linked production function.
                    Candidate {
                        id: id.to_owned(),
                        path: PathBuf::from(path),
                        count: old.count.saturating_add(1).min(CAP),
                        last_seen: old.last_seen.max(now),
                        weight: (decayed_weight(old.weight, old.last_seen, now) + 1.0).min(f64::from(CAP)),
                    }
                } else {
                    Candidate {
                        id: id.to_owned(),
                        path: PathBuf::from(path),
                        count: 1,
                        last_seen: now,
                        weight: 1.0,
                    }
                };
                state.insert(id.to_owned(), candidate.clone());
                Some(candidate)
            }
        };
        serde_json::to_writer(
            &mut *out,
            &json!({"kind":"event","index":index,"before":before,
                    "after":order(&state,now),"updated":updated}),
        )?;
        writeln!(out)?;
    }
    serde_json::to_writer(
        &mut *out,
        &json!({"kind":"done","id":trace_id,"events":events.len(),
                "state":state.values().collect::<Vec<_>>()}),
    )?;
    writeln!(out)?;
    out.flush()?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let mut out = io::BufWriter::new(io::stdout().lock());
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            return Err("empty bridge input".into());
        }
        replay(serde_json::from_str(&line)?, &mut out)?;
    }
    Ok(())
}
