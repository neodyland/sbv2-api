use axum::{extract::Query, routing::get, Router};
use sbv2_core::{jtalk::JTalk, tts_util::preprocess_parse_text};
use serde::Deserialize;
use tokio::net::TcpListener;

use error::AppResult;

mod error;

#[derive(Deserialize)]
struct RequestCreateAudioQuery {
    text: String,
}

async fn create_audio_query(Query(request): Query<RequestCreateAudioQuery>) -> AppResult<String> {
    let (normalized_text, process) = preprocess_parse_text(&request.text, &JTalk::new()?)?;
    let kana_tone_list = process.g2kana_tone()?;
    println!("{:?}", kana_tone_list);
    Ok(normalized_text)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app = Router::new()
        .route("/", get(|| async { "Hello, world!" }))
        .route("/audio_query", get(create_audio_query));
    let listener = TcpListener::bind("0.0.0.0:8080").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
