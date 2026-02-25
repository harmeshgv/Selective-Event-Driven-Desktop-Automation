//! LLM client integration for automation assistance.
//!
//! Supports provider-backed note generation and repair planning for failed
//! automation runs.

use std::env;
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::task;

use crate::config::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmProvider {
    Disabled,
    Groq,
    Ollama,
}

impl LlmProvider {
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "groq" => Self::Groq,
            "ollama" => Self::Ollama,
            _ => Self::Disabled,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Groq => "groq",
            Self::Ollama => "ollama",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskSummaryRequest {
    pub pattern_hash: String,
    pub sequence_label: String,
    pub frequency: i64,
    pub step_count: usize,
    pub sample_apps: Vec<String>,
    pub sample_domains: Vec<String>,
    pub sample_queries: Vec<String>,
    pub sample_steps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSummaryResponse {
    pub summary: String,
    pub intent: String,
    #[serde(default)]
    pub noise_explanation: String,
    #[serde(default)]
    pub suggestions: Vec<String>,
    #[serde(default)]
    pub plan_steps: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepairPlanRequest {
    pub pattern_hash: String,
    pub failed_step_id: Option<String>,
    pub failed_action: String,
    pub failure_reason: String,
    pub target_app: Option<String>,
    pub selector_hint: serde_json::Value,
    pub variables: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairPlanResponse {
    pub diagnosis: String,
    #[serde(default)]
    pub steps: Vec<RepairToolStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairToolStep {
    pub tool: String,
    #[serde(default)]
    pub params: serde_json::Value,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub save_as: Option<String>,
}

#[derive(Clone)]
pub struct LlmClient {
    provider: LlmProvider,
    model: String,
    base_url: String,
    timeout_seconds: u64,
    api_key: Option<String>,
}

impl LlmClient {
    pub fn disabled(timeout_seconds: u64) -> Self {
        Self {
            provider: LlmProvider::Disabled,
            model: "disabled".to_string(),
            base_url: String::new(),
            timeout_seconds: timeout_seconds.max(5),
            api_key: None,
        }
    }

    pub fn from_config(config: &Config) -> Result<Self> {
        let provider = LlmProvider::parse(&config.llm_provider);
        let model = if config.llm_model.trim().is_empty() {
            match provider {
                LlmProvider::Groq => "llama-3.1-8b-instant".to_string(),
                LlmProvider::Ollama => "llama3.1:8b".to_string(),
                LlmProvider::Disabled => "disabled".to_string(),
            }
        } else {
            config.llm_model.trim().to_string()
        };
        let base_url = config
            .llm_base_url
            .clone()
            .unwrap_or_else(|| match provider {
                LlmProvider::Groq => "https://api.groq.com/openai/v1".to_string(),
                LlmProvider::Ollama => "http://127.0.0.1:11434".to_string(),
                LlmProvider::Disabled => String::new(),
            });

        let timeout_seconds = config.llm_timeout_seconds.max(5);
        let api_key = match provider {
            LlmProvider::Groq => env::var("SEDA_GROQ_API_KEY")
                .ok()
                .or_else(|| env::var("GROQ_API_KEY").ok()),
            _ => None,
        };

        Ok(Self {
            provider,
            model,
            base_url,
            timeout_seconds,
            api_key,
        })
    }

    pub fn provider(&self) -> LlmProvider {
        self.provider
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn timeout_seconds(&self) -> u64 {
        self.timeout_seconds
    }

    pub fn is_enabled(&self) -> bool {
        match self.provider {
            LlmProvider::Disabled => false,
            LlmProvider::Groq => self.api_key.is_some(),
            LlmProvider::Ollama => true,
        }
    }

    pub fn disabled_reason(&self) -> Option<String> {
        match self.provider {
            LlmProvider::Disabled => Some("LLM provider is disabled".to_string()),
            LlmProvider::Groq if self.api_key.is_none() => {
                Some("Groq selected but SEDA_GROQ_API_KEY is not set".to_string())
            }
            _ => None,
        }
    }

    pub async fn summarize_repeated_task(
        &self,
        request: &TaskSummaryRequest,
    ) -> Result<TaskSummaryResponse> {
        if !self.is_enabled() {
            return Err(anyhow!(
                self.disabled_reason()
                    .unwrap_or_else(|| "LLM provider unavailable".to_string())
            ));
        }

        let system_prompt =
            "You infer the user's real goal from repeated desktop actions and explain it in plain English. Output strict JSON only.";
        let user_prompt = format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            "Create JSON with keys:",
            r#"{"summary":"one concise approval note","intent":"short task intent","noise_explanation":"one sentence about mixed/unrelated middle actions","suggestions":["option 1","option 2","option 3"],"plan_steps":["step 1","step 2"]}"#,
            "Style rules:",
            "- summary must be 2-3 sentences in natural, human language about the user's likely goal.",
            "- Use concrete clues from context (apps, websites, search queries) and explain the task like a person would.",
            "- Do NOT describe low-level UI events such as SWITCH_APP, INTERACT, selectors, node ids, or field types.",
            "- End summary with a simple question asking whether to automate this routine.",
            "- suggestions must contain exactly 3 short options (<=140 chars each), each phrased as a likely user intent.",
            "- noise_explanation must state that some middle actions may be unrelated but the core repeated flow is still identified.",
            "- plan_steps must be 8-20 short imperative steps in simple language (example: Open Chrome, Go to ums.lpu.in, Enter login credentials, Search for internships).",
            "Example tone: You usually open Chrome, go to ums.lpu.in, and complete your university portal routine. I also saw occasional unrelated actions in between. Should I automate this core flow?",
            "Context:",
            serde_json::to_string_pretty(request)?,
            "No markdown. JSON only."
        );

        let content = self.chat_completion(system_prompt, &user_prompt).await?;
        let mut parsed = parse_llm_json::<TaskSummaryResponse>(&content)?;

        parsed.summary = parsed.summary.trim().to_string();
        parsed.intent = parsed.intent.trim().to_string();
        parsed.noise_explanation = parsed.noise_explanation.trim().to_string();
        parsed.suggestions = parsed
            .suggestions
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .take(3)
            .collect();
        parsed.plan_steps = parsed
            .plan_steps
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .take(20)
            .collect();

        Ok(parsed)
    }

    pub async fn create_repair_plan(
        &self,
        request: &RepairPlanRequest,
    ) -> Result<RepairPlanResponse> {
        if !self.is_enabled() {
            return Err(anyhow!(
                self.disabled_reason()
                    .unwrap_or_else(|| "LLM provider unavailable".to_string())
            ));
        }

        let system_prompt =
            "You produce a safe MCP repair plan in strict JSON for desktop automation.";
        let user_prompt = format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n{}",
            "Return JSON with keys:",
            r#"{"diagnosis":"short reason","steps":[{"tool":"list_windows|get_window_tree|activate_element|press_key|set_clipboard|get_transitions|get_patterns","params":{},"note":"optional","save_as":"optional"}]}"#,
            "Only allowed tools above. Max 12 steps.",
            "You may use placeholders in params as strings: $TARGET_HWND, $TARGET_ELEMENT_ID.",
            "If no safe plan exists, return steps as [].",
            "Failure context:",
            serde_json::to_string_pretty(request)?
        );

        let content = self.chat_completion(system_prompt, &user_prompt).await?;
        parse_llm_json::<RepairPlanResponse>(&content)
    }

    async fn chat_completion(&self, system_prompt: &str, user_prompt: &str) -> Result<String> {
        match self.provider {
            LlmProvider::Groq => self.chat_completion_groq(system_prompt, user_prompt).await,
            LlmProvider::Ollama => self.chat_completion_ollama(system_prompt, user_prompt).await,
            LlmProvider::Disabled => Err(anyhow!("LLM provider is disabled")),
        }
    }

    async fn chat_completion_groq(&self, system_prompt: &str, user_prompt: &str) -> Result<String> {
        let api_key = self
            .api_key
            .as_ref()
            .ok_or_else(|| anyhow!("Missing Groq API key"))?;
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let body = json!({
            "model": self.model,
            "temperature": 0.1,
            "response_format": { "type": "json_object" },
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": user_prompt }
            ]
        });

        let headers = vec![
            "Content-Type: application/json".to_string(),
            format!("Authorization: Bearer {}", api_key),
        ];
        let payload = run_curl_post_json(url, headers, body.to_string(), self.timeout_seconds).await?;

        let parsed: OpenAiChatResponse =
            serde_json::from_str(&payload).context("Failed to parse Groq response")?;
        let content = parsed
            .choices
            .first()
            .map(|choice| choice.message.content.clone())
            .unwrap_or_default();
        if content.trim().is_empty() {
            return Err(anyhow!("Groq returned an empty completion"));
        }
        Ok(content)
    }

    async fn chat_completion_ollama(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String> {
        let url = format!("{}/api/chat", self.base_url.trim_end_matches('/'));
        let body = json!({
            "model": self.model,
            "stream": false,
            "format": "json",
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": user_prompt }
            ]
        });
        let headers = vec!["Content-Type: application/json".to_string()];
        let payload =
            run_curl_post_json(url, headers, body.to_string(), self.timeout_seconds).await?;

        let parsed: OllamaChatResponse =
            serde_json::from_str(&payload).context("Failed to parse Ollama response")?;
        let content = parsed.message.content;
        if content.trim().is_empty() {
            return Err(anyhow!("Ollama returned an empty completion"));
        }
        Ok(content)
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiChatResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    message: OllamaMessage,
}

#[derive(Debug, Deserialize)]
struct OllamaMessage {
    content: String,
}

async fn run_curl_post_json(
    url: String,
    headers: Vec<String>,
    body_json: String,
    timeout_seconds: u64,
) -> Result<String> {
    let timeout = timeout_seconds.max(5).to_string();
    let output = task::spawn_blocking(move || {
        let mut command = Command::new("curl");
        command
            .arg("-sS")
            .arg("--fail")
            .arg("-X")
            .arg("POST")
            .arg("-m")
            .arg(&timeout);

        for header in headers {
            command.arg("-H").arg(header);
        }

        command.arg("-d").arg(body_json).arg(url);
        command.output()
    })
    .await
    .context("Failed to join curl task")?
    .context("Failed to run curl command")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(anyhow!(
            "curl request failed: status={} stderr={} stdout={}",
            output.status,
            stderr.trim(),
            stdout.trim()
        ));
    }

    let payload =
        String::from_utf8(output.stdout).context("curl returned non-UTF8 response body")?;
    if payload.trim().is_empty() {
        return Err(anyhow!("curl returned empty response body"));
    }
    Ok(payload)
}

fn parse_llm_json<T: for<'de> Deserialize<'de>>(content: &str) -> Result<T> {
    if let Ok(value) = serde_json::from_str::<T>(content) {
        return Ok(value);
    }

    let start = content.find('{');
    let end = content.rfind('}');
    if let (Some(start_idx), Some(end_idx)) = (start, end) {
        if end_idx > start_idx {
            let slice = &content[start_idx..=end_idx];
            if let Ok(value) = serde_json::from_str::<T>(slice) {
                return Ok(value);
            }
        }
    }

    Err(anyhow!("LLM output was not valid JSON: {}", content))
}
