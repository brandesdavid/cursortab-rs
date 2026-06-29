use std::sync::Arc;
use async_trait::async_trait;
use nvim_rs::{Handler, Neovim, Value};
use nvim_rs::compat::tokio::Compat;
use tokio::io::Stdout;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::api::CursorApi;
use crate::buffer::{BufferState, NvimWriter};
use crate::proto::{
    encode_current_file_info, encode_cpp_file_diff_history,
    encode_stream_cpp_request, encode_stream_next_cursor_request,
};

pub struct State {
    pub buffer: BufferState,
    pub api: CursorApi,
    pub workspace_id: String,
    pub cancel: CancellationToken,
    /// Pending suggestion: (start_line 0-based, end_line_inclusive 0-based, replacement_lines)
    pub pending: Option<(i64, i64, Vec<String>)>,
    pub ns_id: i64,
}

#[derive(Clone)]
pub struct PluginHandler {
    state: Arc<Mutex<State>>,
}

impl PluginHandler {
    pub fn new(api: CursorApi) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                buffer: BufferState::new(),
                api,
                workspace_id: "a-b-c-d-e-f-g".to_string(),
                cancel: CancellationToken::new(),
                pending: None,
                ns_id: 0,
            })),
        }
    }

    async fn handle_sync(&self, nvim: Neovim<NvimWriter>, ns_id: i64) {
        let state_arc = self.state.clone();
        tokio::spawn(async move {
            // Cancel previous in-flight request
            let cancel = {
                let mut st = state_arc.lock().await;
                st.ns_id = ns_id;
                st.cancel.cancel();
                st.cancel = CancellationToken::new();
                st.cancel.clone()
            };

            // Sync buffer then build request
            let body = {
                let mut st = state_arc.lock().await;
                st.buffer.sync_in(&nvim).await;

                let file_info = encode_current_file_info(
                    &st.buffer.path,
                    &st.buffer.contents(),
                    st.buffer.row as i32 + 1,
                    st.buffer.col as i32,
                    st.buffer.version as i32,
                );
                let diff_hist = encode_cpp_file_diff_history(
                    &st.buffer.path,
                    &st.buffer.diff_history,
                );
                encode_stream_cpp_request(&file_info, &[diff_hist], &st.workspace_id, "typing")
            };

            let msgs = {
                let st = state_arc.lock().await;
                st.api.stream_cpp(body, cancel).await
            };

            if msgs.is_empty() { return; }

            // Accumulate full suggestion
            let mut start_line: Option<i64> = None;
            let mut end_line: Option<i64> = None;
            let mut text = String::new();
            for msg in &msgs {
                if let Some(s) = msg.start_line { start_line = Some(s as i64 - 1); }
                if let Some(e) = msg.end_line_inclusive { end_line = Some(e as i64 - 1); }
                text.push_str(&msg.text);
            }

            let start = match start_line { Some(s) => s, None => return };
            let end = end_line.unwrap_or(start);
            let replacement: Vec<String> = text.lines().map(|s| s.to_string()).collect();
            if replacement.is_empty() { return; }

            {
                let mut st = state_arc.lock().await;
                st.pending = Some((start, end, replacement.clone()));

                let buf = match nvim.get_current_buf().await { Ok(b) => b, Err(_) => return };
                let _ = buf.clear_namespace(ns_id, 0, -1).await;

                // Show inline ghost text for first differing line
                for (i, new_line) in replacement.iter().enumerate() {
                    let buf_line = start as usize + i;
                    if buf_line >= st.buffer.lines.len() { break; }
                    let existing = &st.buffer.lines[buf_line];
                    if new_line == existing { continue; }

                    // Ghost = chars after the common prefix
                    let prefix_len = existing.chars()
                        .zip(new_line.chars())
                        .take_while(|(a, b)| a == b)
                        .count();
                    let ghost: String = new_line.chars().skip(prefix_len).collect();
                    if ghost.is_empty() { continue; }

                    let col = prefix_len as i64;
                    let _ = buf.set_extmark(ns_id, buf_line as i64, col, vec![
                        (Value::from("virt_text"), Value::from(vec![
                            Value::from(vec![Value::from(ghost), Value::from("CursorTabSuggestion")])
                        ])),
                        (Value::from("virt_text_pos"), Value::from("inline")),
                        (Value::from("hl_mode"), Value::from("combine")),
                    ]).await;
                    break;
                }
            }
        });
    }

    async fn handle_tab(&self, nvim: Neovim<NvimWriter>, ns_id: i64) {
        let state_arc = self.state.clone();
        tokio::spawn(async move {
            let pending = {
                let mut st = state_arc.lock().await;
                st.pending.take()
            };
            let (start, end, replacement) = match pending { Some(p) => p, None => return };

            let buf = match nvim.get_current_buf().await { Ok(b) => b, Err(e) => { log::error!("tab get_buf: {}", e); return; } };
            let _ = buf.clear_namespace(ns_id, 0, -1).await;

            // Apply lines — set_lines end is exclusive
            if let Err(e) = buf.set_lines(start, end + 1, false, replacement.clone()).await {
                log::error!("tab set_lines: {}", e);
                return;
            }

            // Move cursor to end of last replaced line
            let last_line = start + replacement.len() as i64 - 1;
            let last_col = replacement.last().map(|s| s.len() as i64).unwrap_or(0);
            if let Ok(win) = nvim.get_current_win().await {
                let _ = win.set_cursor((last_line + 1, last_col)).await;
            }

            // Update diff history and buffer version
            {
                let mut st = state_arc.lock().await;
                let mut diff = String::new();
                let lines = &st.buffer.lines;
                for i in (start as usize)..=(end as usize).min(lines.len().saturating_sub(1)) {
                    diff.push_str(&format!("{}-|{}\n", i + 1, lines[i]));
                }
                for (i, l) in replacement.iter().enumerate() {
                    diff.push_str(&format!("{}+|{}\n", start as usize + i + 1, l));
                }
                st.buffer.diff_history.push(diff);
                if st.buffer.diff_history.len() > 3 {
                    let len = st.buffer.diff_history.len();
                    st.buffer.diff_history = st.buffer.diff_history[len - 3..].to_vec();
                }
                st.buffer.version += 1;
            }

            // Re-sync and predict next cursor
            let cancel = {
                let mut st = state_arc.lock().await;
                st.buffer.sync_in(&nvim).await;
                st.cancel.cancel();
                st.cancel = CancellationToken::new();
                st.cancel.clone()
            };

            let body = {
                let st = state_arc.lock().await;
                let file_info = encode_current_file_info(
                    &st.buffer.path,
                    &st.buffer.contents(),
                    st.buffer.row as i32 + 1,
                    st.buffer.col as i32,
                    st.buffer.version as i32,
                );
                let diff_hist = encode_cpp_file_diff_history(&st.buffer.path, &st.buffer.diff_history);
                encode_stream_next_cursor_request(
                    &file_info,
                    &st.buffer.diff_history,
                    &[diff_hist],
                    &st.workspace_id,
                )
            };

            let msgs = {
                let st = state_arc.lock().await;
                st.api.stream_next_cursor(body, cancel).await
            };

            let mut predicted_line: Option<i64> = None;
            for msg in &msgs {
                if msg.is_not_in_range { predicted_line = None; break; }
                if msg.line_number != 0 { predicted_line = Some(msg.line_number as i64 - 1); }
            }

            if let Some(hint_line) = predicted_line {
                let _ = buf.clear_namespace(ns_id, 0, -1).await;
                let _ = buf.set_extmark(ns_id, hint_line, 0, vec![
                    (Value::from("virt_text"), Value::from(vec![
                        Value::from(vec![Value::from("«"), Value::from("CursorTabNextHint")])
                    ])),
                    (Value::from("virt_text_pos"), Value::from("overlay")),
                    (Value::from("hl_mode"), Value::from("combine")),
                ]).await;

                if let Ok(win) = nvim.get_current_win().await {
                    let _ = win.set_cursor((hint_line + 1, 0)).await;
                }
                log::debug!("next cursor hint at line {}", hint_line);
            }
        });
    }
}

#[async_trait]
impl Handler for PluginHandler {
    type Writer = NvimWriter;

    async fn handle_request(
        &self,
        name: String,
        args: Vec<Value>,
        nvim: Neovim<Self::Writer>,
    ) -> Result<Value, Value> {
        let ns_id = args.first().and_then(|v| v.as_i64()).unwrap_or(0);
        match name.as_str() {
            "cursortab_sync" => { self.handle_sync(nvim, ns_id).await; Ok(Value::Nil) }
            "cursortab_tab_key" => { self.handle_tab(nvim, ns_id).await; Ok(Value::Nil) }
            _ => Err(Value::from(format!("unknown method: {}", name))),
        }
    }

    async fn handle_notify(&self, _name: String, _args: Vec<Value>, _nvim: Neovim<Self::Writer>) {}
}
