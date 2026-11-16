//! OpenAI-compatible `POST /v1/images/generations`, served by Gemini Enterprise's image tool
//! through the extension (`RUN_IMAGE` out, `IMAGE_DONE` back).

use serde::Deserialize;
use serde_json::{json, Value};

pub const PROMPT_MAX_CHARS: usize = 4000;
/// OpenMausBot gives a custom image provider 120 s; answer before that so it sees our error, not its timeout.
pub const IMAGE_TIMEOUT_SECS: u64 = 115;

#[derive(Debug, Deserialize)]
pub struct ImageRequest {
    pub prompt: Option<String>,
    pub size: Option<String>,
    pub n: Option<u32>,
    pub response_format: Option<String>,
}

#[derive(Debug, PartialEq)]
pub struct ValidRequest {
    pub prompt: String,
    pub aspect: Option<&'static str>,
    pub as_url: bool,
}

#[derive(Debug, Clone)]
pub struct GeneratedImage {
    pub mime: String,
    pub b64: String,
}

/// "1536x1024" -> 16:9, "1024x1536" -> 9:16, square -> 1:1, "auto"/absent/unparseable -> no hint.
pub fn aspect_for_size(size: Option<&str>) -> Option<&'static str> {
    let (w, h) = size?.trim().split_once(['x', 'X'])?;
    let (w, h): (f64, f64) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let r = w / h;
    Some(if r > 1.2 { "16:9" } else if r < 1.0 / 1.2 { "9:16" } else { "1:1" })
}

pub fn validate(req: &ImageRequest) -> Result<ValidRequest, String> {
    let prompt = req.prompt.as_deref().map(str::trim).unwrap_or("");
    if prompt.is_empty() {
        return Err("prompt is required".into());
    }
    if prompt.chars().count() > PROMPT_MAX_CHARS {
        return Err(format!("prompt is longer than {PROMPT_MAX_CHARS} characters"));
    }
    if req.n.unwrap_or(1) != 1 {
        return Err("Only n=1 is supported: Gemini Enterprise makes one image per request.".into());
    }
    let as_url = match req.response_format.as_deref() {
        None | Some("b64_json") => false,
        Some("url") => true,
        Some(other) => return Err(format!("response_format '{other}' is not supported; use b64_json or url")),
    };
    Ok(ValidRequest { prompt: prompt.to_string(), aspect: aspect_for_size(req.size.as_deref()), as_url })
}

/// `url` is a data URL: ge-bridge does not host files.
pub fn response_body(img: &GeneratedImage, as_url: bool, created: u64) -> Value {
    let item = if as_url {
        json!({ "url": format!("data:{};base64,{}", img.mime, img.b64) })
    } else {
        json!({ "b64_json": img.b64 })
    };
    json!({ "created": created, "data": [item] })
}

/// The extension's `IMAGE_DONE` frame.
pub fn parse_image_done(payload: &Value) -> Result<GeneratedImage, String> {
    if let Some(err) = payload.get("error").and_then(|v| v.as_str()) {
        return Err(err.to_string());
    }
    let b64 = payload.get("b64").and_then(|v| v.as_str()).unwrap_or("");
    if b64.is_empty() {
        return Err("The extension returned no image data.".into());
    }
    let mime = payload.get("mime").and_then(|v| v.as_str()).unwrap_or("image/png");
    if !matches!(mime, "image/png" | "image/jpeg" | "image/webp") {
        return Err(format!("Gemini returned an unsupported image type ({mime})."));
    }
    Ok(GeneratedImage { mime: mime.to_string(), b64: b64.to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(v: Value) -> ImageRequest {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn sizes_map_to_aspects() {
        assert_eq!(aspect_for_size(Some("1024x1024")), Some("1:1"));
        assert_eq!(aspect_for_size(Some("1536x1024")), Some("16:9"));
        assert_eq!(aspect_for_size(Some("1024x1536")), Some("9:16"));
        assert_eq!(aspect_for_size(Some("auto")), None);
        assert_eq!(aspect_for_size(None), None);
        assert_eq!(aspect_for_size(Some("0x10")), None);
    }

    #[test]
    fn validates_openmausbot_request_and_rejects_bad_ones() {
        let ok = validate(&req(json!({ "model": "x", "prompt": " apple ", "response_format": "b64_json", "size": "1024x1024", "n": 1 }))).unwrap();
        assert_eq!(ok, ValidRequest { prompt: "apple".into(), aspect: Some("1:1"), as_url: false });
        assert!(validate(&req(json!({ "prompt": "  " }))).is_err());
        assert!(validate(&req(json!({ "prompt": "a", "n": 2 }))).is_err());
        assert!(validate(&req(json!({ "prompt": "a", "response_format": "png" }))).is_err());
        assert!(validate(&req(json!({ "prompt": "a", "response_format": "url" }))).unwrap().as_url);
    }

    #[test]
    fn response_and_image_done_shapes() {
        let img = parse_image_done(&json!({ "jobId": "j", "mime": "image/png", "b64": "QUJD" })).unwrap();
        assert_eq!(response_body(&img, false, 7), json!({ "created": 7, "data": [{ "b64_json": "QUJD" }] }));
        assert_eq!(response_body(&img, true, 7)["data"][0]["url"], "data:image/png;base64,QUJD");
        assert_eq!(parse_image_done(&json!({ "error": "no image" })).unwrap_err(), "no image");
        assert!(parse_image_done(&json!({ "b64": "" })).is_err());
        assert!(parse_image_done(&json!({ "b64": "QUJD", "mime": "image/gif" })).is_err());
    }
}
