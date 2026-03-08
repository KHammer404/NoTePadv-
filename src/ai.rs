use serde_json::{json, Value};

fn config_dir() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("notepad")
}

fn key_path() -> std::path::PathBuf {
    config_dir().join("api_key")
}

pub fn load_api_key() -> Option<String> {
    // Try config file first, then env var
    if let Ok(key) = std::fs::read_to_string(key_path()) {
        let key = key.trim().to_string();
        if !key.is_empty() {
            return Some(key);
        }
    }
    std::env::var("ANTHROPIC_API_KEY").ok().filter(|k| !k.is_empty())
}

pub fn save_api_key(key: &str) -> Result<(), String> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}", e))?;
    std::fs::write(key_path(), key.trim()).map_err(|e| format!("{}", e))
}

pub struct AiClient {
    api_key: String,
}

impl AiClient {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }

    pub fn query(&self, question: &str, memos: &[(String, String)]) -> Result<String, String> {
        let mut context = String::new();
        for (content, date) in memos {
            context.push_str(&format!("[{}] {}\n", date, content));
        }

        let body = json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 1024,
            "messages": [{
                "role": "user",
                "content": format!(
                    "다음은 사용자가 저장한 메모들입니다:\n\n{}\n\n질문: {}\n\n메모 내용만을 기반으로 간결하게 답변해주세요. 메모에 없는 내용은 만들어내지 마세요. 관련 메모가 없으면 '관련 메모를 찾지 못했습니다'라고 답하세요.",
                    context, question
                )
            }]
        });

        let response = ureq::post("https://api.anthropic.com/v1/messages")
            .set("x-api-key", &self.api_key)
            .set("anthropic-version", "2023-06-01")
            .set("content-type", "application/json")
            .send_json(body)
            .map_err(|e| format!("API 오류: {}", e))?;

        let json: Value = response.into_json()
            .map_err(|e| format!("응답 파싱 오류: {}", e))?;

        json["content"][0]["text"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "응답을 받지 못했습니다".to_string())
    }
}
