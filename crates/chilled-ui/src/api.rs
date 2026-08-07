//! Same-origin JSON fetch helpers over web_sys. Cookies ride along
//! automatically (default `credentials: same-origin`); never set CORS mode.

use serde::de::DeserializeOwned;
use serde::Serialize;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, Response};

#[derive(Debug, Clone, PartialEq)]
pub enum ApiError {
    /// 401 — the caller should offer login, not clear existing state.
    Unauthorized,
    Forbidden,
    Http(u16, String),
    Network(String),
    Decode(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::Unauthorized => write!(f, "login required"),
            ApiError::Forbidden => write!(f, "not allowed"),
            ApiError::Http(status, msg) => write!(f, "{msg} (HTTP {status})"),
            ApiError::Network(msg) => write!(f, "server unreachable: {msg}"),
            ApiError::Decode(msg) => write!(f, "bad response: {msg}"),
        }
    }
}

async fn fetch_text(method: &str, path: &str, body: Option<String>) -> Result<String, ApiError> {
    let init = RequestInit::new();
    init.set_method(method);
    if let Some(body) = body {
        init.set_body(&JsValue::from_str(&body));
    }
    let request = Request::new_with_str_and_init(path, &init)
        .map_err(|e| ApiError::Network(format!("{e:?}")))?;
    request
        .headers()
        .set("accept", "application/json")
        .map_err(|e| ApiError::Network(format!("{e:?}")))?;
    if method != "GET" {
        request
            .headers()
            .set("content-type", "application/json")
            .map_err(|e| ApiError::Network(format!("{e:?}")))?;
    }

    let window = web_sys::window().ok_or_else(|| ApiError::Network("no window".into()))?;
    let response = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| ApiError::Network(format!("{e:?}")))?;
    let response: Response = response
        .dyn_into()
        .map_err(|_| ApiError::Network("not a Response".into()))?;
    let status = response.status();
    let text = match response.text() {
        Ok(promise) => JsFuture::from(promise)
            .await
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default(),
        Err(_) => String::new(),
    };

    if response.ok() {
        return Ok(text);
    }
    // The API's error envelope is {"error": "..."} — surface the message.
    let msg = serde_json::from_str::<chilled_wire::ApiError>(&text)
        .map(|e| e.error)
        .unwrap_or_else(|_| format!("HTTP {status}"));
    Err(match status {
        401 => ApiError::Unauthorized,
        403 => ApiError::Forbidden,
        _ => ApiError::Http(status, msg),
    })
}

fn decode<T: DeserializeOwned>(text: &str) -> Result<T, ApiError> {
    serde_json::from_str(text).map_err(|e| ApiError::Decode(e.to_string()))
}

pub async fn get_json<T: DeserializeOwned>(path: &str) -> Result<T, ApiError> {
    decode(&fetch_text("GET", path, None).await?)
}

pub async fn send_json<B: Serialize, T: DeserializeOwned>(
    method: &str,
    path: &str,
    body: &B,
) -> Result<T, ApiError> {
    let body = serde_json::to_string(body).map_err(|e| ApiError::Decode(e.to_string()))?;
    decode(&fetch_text(method, path, Some(body)).await?)
}

/// A request whose success carries no body (login, logout, delete, refresh).
pub async fn send_empty<B: Serialize>(
    method: &str,
    path: &str,
    body: Option<&B>,
) -> Result<(), ApiError> {
    let body = match body {
        Some(body) => {
            Some(serde_json::to_string(body).map_err(|e| ApiError::Decode(e.to_string()))?)
        }
        None => None,
    };
    fetch_text(method, path, body).await.map(|_| ())
}

/// Awaitable timeout without a timer crate.
pub async fn sleep_ms(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let _ = web_sys::window()
            .expect("window")
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}
