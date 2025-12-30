use serde::Serialize;

#[derive(Serialize)]
struct SendMessageBody<'a> {
    chat_id: &'a str,
    text: &'a str,
    disable_web_page_preview: bool,
}

pub async fn send_message(text: String) -> Result<(), String> {
    let token = std::env::var("FLOWSURFACE_TELEGRAM_BOT_TOKEN")
        .map_err(|_| "missing env FLOWSURFACE_TELEGRAM_BOT_TOKEN".to_string())?;
    let chat_id = std::env::var("FLOWSURFACE_TELEGRAM_CHAT_ID")
        .map_err(|_| "missing env FLOWSURFACE_TELEGRAM_CHAT_ID".to_string())?;

    let url = format!("https://api.telegram.org/bot{token}/sendMessage");
    let client = reqwest::Client::new();

    client
        .post(url)
        .json(&SendMessageBody {
            chat_id: &chat_id,
            text: &text,
            disable_web_page_preview: true,
        })
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("telegram error: {e}"))?;

    Ok(())
}


