use std::env;

use reqwest::Client;

pub async fn send_message(text: String) -> Result<(), String> {
    let base = env::var("FLOWSURFACE_NTFY_URL").unwrap_or_else(|_| "https://ntfy.sh".to_string());
    let topic = env::var("FLOWSURFACE_NTFY_TOPIC")
        .map_err(|_| "Missing env FLOWSURFACE_NTFY_TOPIC (ntfy topic)".to_string())?;

    let base = base.trim_end_matches('/');
    let url = format!("{base}/{topic}");

    let client = Client::new();
    let res = client
        .post(url)
        .header("Title", "Flowsurface")
        .body(text)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if res.status().is_success() {
        Ok(())
    } else {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        Err(format!("ntfy error: {status} {body}"))
    }
}


