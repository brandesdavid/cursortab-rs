mod api;
mod auth;
mod buffer;
mod checksum;
mod handler;
mod proto;

use handler::PluginHandler;
use nvim_rs::create::tokio as create;
use simplelog::{Config, LevelFilter, WriteLogger};
use std::fs::OpenOptions;

#[tokio::main]
async fn main() {
    // Log to ~/cursortab-rs.log
    if let Ok(home) = std::env::var("HOME") {
        let log_path = format!("{}/cursortab-rs.log", home);
        if let Ok(file) = OpenOptions::new().create(true).append(true).open(&log_path) {
            let _ = WriteLogger::init(LevelFilter::Debug, Config::default(), file);
        }
    }

    log::info!("cursortab-rs starting");

    let access_token = match auth::get_access_token() {
        Ok(t) => t,
        Err(e) => {
            log::error!("Failed to get access token: {}", e);
            std::process::exit(1);
        }
    };

    log::info!("got access token");

    let checksum = checksum::generate_checksum("hi");
    let api = api::CursorApi::new(access_token, checksum);
    let handler = PluginHandler::new(api);

    let (nvim, io_handler) = create::new_parent(handler).await;

    // Setup highlight groups
    let _ = nvim.exec(
        r#"
highlight default CursorTabSuggestion guifg=#808080 ctermfg=8 gui=italic
highlight default CursorTabNextHint guifg=#d0a060 ctermfg=3
"#,
        false,
    ).await;

    log::info!("cursortab-rs ready, serving RPC");

    match io_handler.await {
        Ok(Ok(())) => log::info!("RPC connection closed cleanly"),
        Ok(Err(e)) => log::error!("RPC error: {}", e),
        Err(e) => log::error!("IO handler panic: {:?}", e),
    }
}
