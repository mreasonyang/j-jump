//! Official Decisions API wire format, normalized into the shared choice contract.
use super::*;
use serde::Deserialize;
use serde_json::json;

pub struct OpenAI;
pub static OPENAI: OpenAI = OpenAI;
const MODEL: &str = "gpt-6-luna";
fn source(cfg: &Config) -> &str {
    cfg.providers
        .get("openai")
        .and_then(|s| s.get("credential"))
        .map(String::as_str)
        .unwrap_or("environment")
}
static DESCRIPTION: Descriptor = Descriptor {
    id: "openai",
    label: "OpenAI",
    summary: Text(
        "OpenAI Decisions API (gpt-6-luna); needs an OpenAI API key.",
        "OpenAI Decisions API（gpt-6-luna），需要 OpenAI API Key。",
    ),
    missing: Text(
        "OpenAI skipped: no key configured. Add one with jjump setup or OPENAI_API_KEY.",
        "已跳过 OpenAI：尚未配置 Key。可用 jjump setup 或 OPENAI_API_KEY 添加。",
    ),
    fields: &[],
    credential: Some(CredentialSpec {
        env: &["OPENAI_API_KEY"],
        service: "j-jump.openai",
        account: "openai-api-key",
        setting: Some("credential"),
        source,
        set_source: |cfg, value| {
            cfg.providers
                .entry("openai".into())
                .or_default()
                .insert("credential".into(), value.into());
        },
    }),
};
fn invalid() -> Error {
    Error(5, "OpenAI Decisions schema mismatch".into())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    model: String,
    input: String,
    questions: Vec<Question>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Question {
    r#type: String,
    name: String,
    instructions: String,
    choices: Vec<Choice>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Choice {
    value: String,
    description: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Decision {
    model: String,
    answers: Vec<Answer>,
    usage: Usage,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
enum Answer {
    Choice {
        name: String,
        choice: String,
        confidence: f64,
        probabilities: Vec<Probability>,
    },
    Refusal {
        name: String,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Probability {
    value: String,
    probability: f64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Usage {
    input_tokens: u64,
    input_tokens_details: InputTokens,
    output_tokens: u64,
    output_tokens_details: OutputTokens,
    total_tokens: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputTokens {
    cached_tokens: u64,
    cache_write_tokens: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputTokens {
    reasoning_tokens: u64,
}

impl ProviderDriver for OpenAI {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTION
    }
    fn model(&self) -> &'static str {
        MODEL
    }
    fn connection(&self, _: &Config, context: Option<&str>) -> Result<Connection> {
        if context.is_some_and(|s| !s.is_empty()) {
            return Err(Error(5, "invalid provider binding".into()));
        }
        Ok(Connection {
            url: "https://api.openai.com/v1/decisions".into(),
            context: String::new(),
            transport: Transport::OfficialHttps,
        })
    }
    fn prepare(&self, task: Value, model: &str) -> Result<Vec<u8>> {
        if !self.accepts_model(model) {
            return Err(Error(5, "unsupported request model".into()));
        }
        let questions = task["questions"].as_object().ok_or_else(invalid)?;
        let mut wire_questions = Vec::new();
        for (name, question) in questions {
            if question["type"] != "choice" {
                return Err(invalid());
            }
            let criteria = question["criteria"].as_object().ok_or_else(invalid)?;
            if !(2..=255).contains(&criteria.len()) {
                return Err(invalid());
            }
            let choices: Vec<_> = criteria
                .iter()
                .map(|(id, entry)| {
                    // Candidate metadata is already filtered by the shared privacy policy.
                    let description = entry
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| entry.to_string());
                    json!({"value": id, "description": description})
                })
                .collect();
            wire_questions.push(json!({"type":"choice", "name":name,
                "instructions":question["instructions"].as_str().ok_or_else(invalid)?,
                "choices":choices}));
        }
        serde_json::to_vec(
            &json!({"model":model, "input":task["state"].to_string(), "questions":wire_questions}),
        )
        .map_err(|_| invalid())
    }
    fn questions(&self, request: &Value) -> Result<Value> {
        let request: Request = serde_json::from_value(request.clone()).map_err(|_| invalid())?;
        if request.model != MODEL || request.input.is_empty() || request.questions.len() != 1 {
            return Err(invalid());
        }
        let mut questions = serde_json::Map::new();
        for question in request.questions {
            if question.r#type != "choice"
                || question.name != "destination"
                || question.instructions.is_empty()
                || !(2..=255).contains(&question.choices.len())
            {
                return Err(invalid());
            }
            let mut criteria = serde_json::Map::new();
            for choice in question.choices {
                if criteria
                    .insert(choice.value, json!(choice.description))
                    .is_some()
                {
                    return Err(invalid());
                }
            }
            if !criteria.contains_key("none") {
                return Err(invalid());
            }
            questions.insert(question.name, json!({"criteria":criteria}));
        }
        Ok(Value::Object(questions))
    }
    fn decode(&self, value: Value) -> Result<Value> {
        let decision: Decision = serde_json::from_value(value).map_err(|_| invalid())?;
        let mut answers = serde_json::Map::new();
        for answer in decision.answers {
            let (name, answer) = match answer {
                Answer::Choice {
                    name,
                    choice,
                    confidence,
                    probabilities,
                } => {
                    let mut distribution = serde_json::Map::new();
                    for p in probabilities {
                        if distribution.insert(p.value, json!(p.probability)).is_some() {
                            return Err(invalid());
                        }
                    }
                    (
                        name,
                        json!({"type":"choice", "choice":choice, "confidence":confidence, "probabilities":distribution}),
                    )
                }
                Answer::Refusal { name } => (name, json!({"type":"refusal"})),
            };
            if answers.insert(name, answer).is_some() {
                return Err(invalid());
            }
        }
        let usage = decision.usage;
        if usage.input_tokens.checked_add(usage.output_tokens) != Some(usage.total_tokens)
            || usage.input_tokens_details.cached_tokens > usage.input_tokens
            || usage.input_tokens_details.cache_write_tokens > usage.input_tokens
            || usage.output_tokens_details.reasoning_tokens > usage.output_tokens
        {
            return Err(invalid());
        }
        Ok(json!({"model":decision.model, "answers":answers,
            "usage":{"input_tokens":usage.input_tokens, "output_tokens":usage.output_tokens}}))
    }
}
