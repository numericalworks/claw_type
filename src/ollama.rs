//! Talking to Ollama.
//!
//! Only what the settings screen needs so far: asking a server which models it
//! has. Ollama's HTTP API is documented at <https://github.com/ollama/ollama/blob/main/docs/api.md>.

use std::time::Duration;

use crate::settings::Settings;

/// Long enough for a slow server, short enough that a wrong address does not
/// leave the settings screen hanging.
const TIMEOUT: Duration = Duration::from_secs(15);

/// The names of the models a server has available.
///
/// Blocking; call it off the UI thread.
pub fn list_models(settings: &Settings) -> Result<Vec<String>, String> {
    let url = format!("{}/api/tags", settings.base_url());

    let mut request = ureq::get(&url)
        .config()
        .timeout_global(Some(TIMEOUT))
        .build();

    // A hosted server wants a bearer token; a local one ignores it, so it is
    // only sent when the user has given a key.
    let key = settings.api_key.trim();
    if !key.is_empty() {
        request = request.header("Authorization", format!("Bearer {key}"));
    }

    let mut response = request.call().map_err(|error| describe(&error))?;
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|error| format!("could not read the reply: {error}"))?;

    models_from_json(&body)
}

/// The model names in an `/api/tags` response.
fn models_from_json(body: &str) -> Result<Vec<String>, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|_| "the reply was not JSON".to_owned())?;

    // Ollama reports failures in the body, so surface those rather than
    // complaining about a missing `models` field.
    if let Some(error) = value.get("error").and_then(|error| error.as_str()) {
        return Err(error.to_owned());
    }

    let models = value
        .get("models")
        .and_then(|models| models.as_array())
        .ok_or("the reply had no models")?;

    Ok(models
        .iter()
        .filter_map(|model| model.get("name").and_then(|name| name.as_str()))
        .map(str::to_owned)
        .collect())
}

/// A message worth showing to someone who typed an address.
pub(crate) fn describe(error: &ureq::Error) -> String {
    match error {
        ureq::Error::StatusCode(code) => match code {
            401 | 403 => "the server rejected the API key".to_owned(),
            404 => "no Ollama API at that address".to_owned(),
            code => format!("the server answered {code}"),
        },
        ureq::Error::Timeout(_) => "the server did not answer in time".to_owned(),
        ureq::Error::ConnectionFailed => "could not reach that address".to_owned(),
        ureq::Error::BadUri(_) => "that does not look like a URL".to_owned(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    #[test]
    fn reads_the_model_names() {
        let body = r#"{"models":[
            {"name":"llama3.2:latest","model":"llama3.2:latest","size":2019393189},
            {"name":"qwen2.5-coder:7b","model":"qwen2.5-coder:7b"}
        ]}"#;
        assert_eq!(
            models_from_json(body).expect("models"),
            vec!["llama3.2:latest", "qwen2.5-coder:7b"]
        );
    }

    #[test]
    fn an_empty_server_is_not_an_error() {
        assert_eq!(
            models_from_json(r#"{"models":[]}"#).expect("models"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn an_error_body_is_reported_as_such() {
        let error = models_from_json(r#"{"error":"model requires more system memory"}"#)
            .expect_err("should fail");
        assert!(error.contains("more system memory"), "{error}");
    }

    #[test]
    fn nonsense_is_rejected_without_panicking() {
        assert!(models_from_json("<html>not json</html>").is_err());
        assert!(models_from_json("{}").is_err());
    }

    /// A one-shot server that answers with a canned body, so the whole request
    /// path (URL, header, status, parsing) is exercised without Ollama.
    fn serve_once(body: &'static str) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                let mut reader = BufReader::new(&stream);
                let mut request = String::new();
                let _ = reader.read_line(&mut request);
                let mut line = String::new();
                while let Ok(read) = reader.read_line(&mut line) {
                    if read == 0 || line == "\r\n" {
                        break;
                    }
                    request.push_str(&line);
                    line.clear();
                }
                let _ = tx.send(request);

                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let mut writer = &stream;
                let _ = writer.write_all(response.as_bytes());
                let _ = writer.flush();
            }
        });

        (format!("http://127.0.0.1:{port}"), rx)
    }

    #[test]
    fn asks_the_right_endpoint_and_parses_the_reply() {
        let (url, request) = serve_once(r#"{"models":[{"name":"gemma2:2b"}]}"#);
        let settings = Settings {
            url,
            ..Settings::default()
        };

        let models = list_models(&settings).expect("models");
        assert_eq!(models, vec!["gemma2:2b"]);

        let request = request.recv_timeout(Duration::from_secs(5)).expect("request");
        assert!(request.starts_with("GET /api/tags "), "{request}");
        assert!(!request.contains("Authorization"), "no key, no header: {request}");
    }

    #[test]
    fn sends_the_api_key_when_there_is_one() {
        let (url, request) = serve_once(r#"{"models":[]}"#);
        let settings = Settings {
            url,
            api_key: "sk-secret".to_owned(),
            ..Settings::default()
        };

        list_models(&settings).expect("models");

        let request = request.recv_timeout(Duration::from_secs(5)).expect("request");
        assert!(
            request.contains("authorization: Bearer sk-secret")
                || request.contains("Authorization: Bearer sk-secret"),
            "{request}"
        );
    }
}
