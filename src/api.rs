use crate::proto::{
    decode_stream_cpp_response, decode_stream_next_cursor_response, frame_message,
    parse_frames, StreamCppResponse, StreamNextCursorResponse,
};
use futures::StreamExt;
use reqwest::Client;
use tokio_util::sync::CancellationToken;

const BASE_URL: &str = "https://api2.cursor.sh";

pub struct CursorApi {
    client: Client,
    access_token: String,
    checksum: String,
}

fn build_client() -> Client {
    let mut builder = Client::builder();

    // On NixOS, SSL_CERT_FILE is not set by default; load the system bundle explicitly
    let cert_paths = [
        std::env::var("SSL_CERT_FILE").ok(),
        Some("/etc/ssl/certs/ca-bundle.crt".to_string()),
        Some("/etc/ssl/certs/ca-certificates.crt".to_string()),
    ];
    for path_opt in &cert_paths {
        if let Some(path) = path_opt {
            if let Ok(pem) = std::fs::read(path) {
                if let Ok(cert) = reqwest::Certificate::from_pem(&pem) {
                    builder = builder.add_root_certificate(cert);
                    log::debug!("loaded CA bundle from {}", path);
                    break;
                }
            }
        }
    }

    builder.build().unwrap_or_else(|_| Client::new())
}

impl CursorApi {
    pub fn new(access_token: String, checksum: String) -> Self {
        Self {
            client: build_client(),
            access_token,
            checksum,
        }
    }

    async fn post_stream(&self, path: &str, body: Vec<u8>) -> reqwest::Result<reqwest::Response> {
        let framed = frame_message(&body);
        self.client
            .post(format!("{}{}", BASE_URL, path))
            .header("content-type", "application/connect+proto")
            .header("authorization", format!("bearer {}", self.access_token))
            .header("x-cursor-client-version", "0.45.0")
            .header("x-cursor-checksum", &self.checksum)
            .body(framed)
            .send()
            .await
    }

    pub async fn stream_cpp(
        &self,
        body: Vec<u8>,
        cancel: CancellationToken,
    ) -> Vec<StreamCppResponse> {
        let resp = match self.post_stream("/aiserver.v1.AiService/StreamCpp", body).await {
            Ok(r) => r,
            Err(e) => {
                log::error!("stream_cpp request failed: {}", e);
                return vec![];
            }
        };

        log::info!("stream_cpp HTTP {}", resp.status());
        let mut stream = resp.bytes_stream();
        let mut results = Vec::new();
        let mut leftover = Vec::new();

        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                chunk = stream.next() => {
                    match chunk {
                        None => break,
                        Some(Err(e)) => { log::error!("stream_cpp chunk: {}", e); break; }
                        Some(Ok(bytes)) => {
                            log::debug!("stream_cpp chunk {} bytes", bytes.len());
                            leftover.extend_from_slice(&bytes);
                            let frames = parse_frames(&leftover);
                            log::debug!("stream_cpp parsed {} frames", frames.len());
                            for msg_bytes in frames {
                                let msg = decode_stream_cpp_response(&msg_bytes);
                                log::debug!("stream_cpp msg: text={:?} start={:?} end={:?} done={:?}",
                                    &msg.text[..msg.text.len().min(40)], msg.start_line, msg.end_line_inclusive, msg.done_stream);
                                let done = msg.done_stream.unwrap_or(false);
                                results.push(msg);
                                if done { return results; }
                            }
                            leftover = remaining_after_frames(&leftover);
                        }
                    }
                }
            }
        }
        log::debug!("stream_cpp done, {} msgs total", results.len());
        results
    }

    pub async fn stream_next_cursor(
        &self,
        body: Vec<u8>,
        cancel: CancellationToken,
    ) -> Vec<StreamNextCursorResponse> {
        let resp = match self
            .post_stream("/aiserver.v1.AiService/StreamNextCursorPrediction", body)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                log::error!("stream_next_cursor request failed: {}", e);
                return vec![];
            }
        };

        let mut stream = resp.bytes_stream();
        let mut results = Vec::new();
        let mut leftover = Vec::new();

        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                chunk = stream.next() => {
                    match chunk {
                        None => break,
                        Some(Err(e)) => { log::error!("stream_next_cursor chunk: {}", e); break; }
                        Some(Ok(bytes)) => {
                            leftover.extend_from_slice(&bytes);
                            for msg_bytes in parse_frames(&leftover) {
                                let msg = decode_stream_next_cursor_response(&msg_bytes);
                                let not_in_range = msg.is_not_in_range;
                                results.push(msg);
                                if not_in_range { return results; }
                            }
                            leftover = remaining_after_frames(&leftover);
                        }
                    }
                }
            }
        }
        results
    }
}

fn remaining_after_frames(data: &[u8]) -> Vec<u8> {
    let mut offset = 0;
    while offset + 5 <= data.len() {
        let len = u32::from_be_bytes([
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
            data[offset + 4],
        ]) as usize;
        if offset + 5 + len > data.len() {
            break;
        }
        offset += 5 + len;
    }
    data[offset..].to_vec()
}
