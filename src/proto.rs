/// Hand-rolled protobuf encode/decode for the Cursor AI messages we need.
/// Field numbers from aiserver.pb.go protobuf tags.
use bytes::{Buf, BufMut, Bytes, BytesMut};

// ── encode helpers ──────────────────────────────────────────────────────────

fn tag(field: u32, wire: u32) -> u32 {
    (field << 3) | wire
}

fn write_varint(buf: &mut BytesMut, mut v: u64) {
    loop {
        if v < 0x80 {
            buf.put_u8(v as u8);
            break;
        }
        buf.put_u8((v as u8 & 0x7f) | 0x80);
        v >>= 7;
    }
}

fn write_tag(buf: &mut BytesMut, field: u32, wire: u32) {
    write_varint(buf, tag(field, wire) as u64);
}

fn write_bytes_field(buf: &mut BytesMut, field: u32, data: &[u8]) {
    if data.is_empty() {
        return;
    }
    write_tag(buf, field, 2);
    write_varint(buf, data.len() as u64);
    buf.put_slice(data);
}

fn write_string_field(buf: &mut BytesMut, field: u32, s: &str) {
    write_bytes_field(buf, field, s.as_bytes());
}

fn write_bool_field(buf: &mut BytesMut, field: u32, v: bool) {
    write_tag(buf, field, 0);
    buf.put_u8(if v { 1 } else { 0 });
}

fn write_i32_field(buf: &mut BytesMut, field: u32, v: i32) {
    write_tag(buf, field, 0);
    write_varint(buf, v as u64);
}

fn write_message_field(buf: &mut BytesMut, field: u32, inner: &[u8]) {
    if inner.is_empty() {
        return;
    }
    write_tag(buf, field, 2);
    write_varint(buf, inner.len() as u64);
    buf.put_slice(inner);
}

// ── CursorPosition (fields: line=1, column=2) ───────────────────────────────

pub fn encode_cursor_position(line: i32, column: i32) -> Vec<u8> {
    let mut buf = BytesMut::new();
    write_i32_field(&mut buf, 1, line);
    write_i32_field(&mut buf, 2, column);
    buf.to_vec()
}

// ── CurrentFileInfo (fields: relative_workspace_path=1, contents=2, cursor_position=3) ─

pub fn encode_current_file_info(
    relative_workspace_path: &str,
    contents: &str,
    cursor_line: i32,
    cursor_col: i32,
    file_version: i32,
) -> Vec<u8> {
    let mut buf = BytesMut::new();
    write_string_field(&mut buf, 1, relative_workspace_path);
    write_string_field(&mut buf, 2, contents);
    let cursor = encode_cursor_position(cursor_line, cursor_col);
    write_message_field(&mut buf, 3, &cursor);
    // FileVersion field — not in CurrentFileInfo directly, skip
    let _ = file_version;
    buf.to_vec()
}

// ── CppFileDiffHistory (fields: file_name=1, diff_history=2) ────────────────

pub fn encode_cpp_file_diff_history(file_name: &str, diff_history: &[String]) -> Vec<u8> {
    let mut buf = BytesMut::new();
    write_string_field(&mut buf, 1, file_name);
    for d in diff_history {
        write_string_field(&mut buf, 2, d);
    }
    buf.to_vec()
}

// ── CppIntentInfo (fields: source=1) ────────────────────────────────────────

pub fn encode_cpp_intent_info(source: &str) -> Vec<u8> {
    let mut buf = BytesMut::new();
    write_string_field(&mut buf, 1, source);
    buf.to_vec()
}

// ── StreamCppRequest ─────────────────────────────────────────────────────────
// current_file=1, file_diff_histories=7, is_debug=11, give_debug_output=6,
// cpp_intent_info=16, workspace_id=18

pub fn encode_stream_cpp_request(
    current_file: &[u8],
    file_diff_histories: &[Vec<u8>],
    workspace_id: &str,
    source: &str,
) -> Vec<u8> {
    let mut buf = BytesMut::new();
    write_message_field(&mut buf, 1, current_file);
    for h in file_diff_histories {
        write_message_field(&mut buf, 7, h);
    }
    write_bool_field(&mut buf, 11, false); // is_debug
    write_bool_field(&mut buf, 6, false);  // give_debug_output
    let intent = encode_cpp_intent_info(source);
    write_message_field(&mut buf, 16, &intent);
    write_string_field(&mut buf, 18, workspace_id);
    buf.to_vec()
}

// ── StreamNextCursorPredictionRequest ────────────────────────────────────────
// current_file=1, diff_history=2, file_diff_histories=7, is_debug=11,
// give_debug_output=6, cpp_intent_info (not present in this message), workspace_id=?
// Looking at Go: WorkspaceId, DiffHistory, CurrentFile, FileDiffHistories, IsDebug, GiveDebugOutput, CppIntentInfo

pub fn encode_stream_next_cursor_request(
    current_file: &[u8],
    diff_history: &[String],
    file_diff_histories: &[Vec<u8>],
    workspace_id: &str,
) -> Vec<u8> {
    let mut buf = BytesMut::new();
    write_message_field(&mut buf, 1, current_file);
    for d in diff_history {
        write_string_field(&mut buf, 2, d);
    }
    for h in file_diff_histories {
        write_message_field(&mut buf, 7, h);
    }
    write_bool_field(&mut buf, 11, false); // is_debug
    write_bool_field(&mut buf, 6, false);  // give_debug_output
    // workspace_id field number from Go: WorkspaceId *string `protobuf:"bytes,18,..."`
    write_string_field(&mut buf, 18, workspace_id);
    buf.to_vec()
}

// ── Connect RPC framing ──────────────────────────────────────────────────────
// 5-byte prefix: [compressed_flag(1)] [length_be(4)]

pub fn frame_message(msg: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + msg.len());
    out.push(0u8); // not compressed
    let len = msg.len() as u32;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(msg);
    out
}

// ── decode helpers ───────────────────────────────────────────────────────────

fn read_varint(buf: &mut &[u8]) -> Option<u64> {
    let mut result: u64 = 0;
    let mut shift = 0u32;
    loop {
        if buf.is_empty() {
            return None;
        }
        let byte = buf[0];
        *buf = &buf[1..];
        result |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Some(result);
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
}

fn read_length_delimited<'a>(buf: &mut &'a [u8]) -> Option<&'a [u8]> {
    let len = read_varint(buf)? as usize;
    if buf.len() < len {
        return None;
    }
    let data = &buf[..len];
    *buf = &buf[len..];
    Some(data)
}

fn skip_field(buf: &mut &[u8], wire: u64) -> Option<()> {
    match wire {
        0 => { read_varint(buf)?; }
        1 => { if buf.len() < 8 { return None; } *buf = &buf[8..]; }
        2 => { read_length_delimited(buf)?; }
        5 => { if buf.len() < 4 { return None; } *buf = &buf[4..]; }
        _ => return None,
    }
    Some(())
}

// ── StreamCppResponse decode ─────────────────────────────────────────────────
// text=1, suggestion_start_line=2, done_stream=4, range_to_replace=11

#[derive(Debug, Default)]
pub struct StreamCppResponse {
    pub text: String,
    pub suggestion_start_line: Option<i32>,
    pub done_stream: Option<bool>,
    pub start_line: Option<i32>,
    pub end_line_inclusive: Option<i32>,
}

pub fn decode_stream_cpp_response(data: &[u8]) -> StreamCppResponse {
    let mut resp = StreamCppResponse::default();
    let mut buf = data;

    while !buf.is_empty() {
        let tag_val = match read_varint(&mut buf) {
            Some(v) => v,
            None => break,
        };
        let field = (tag_val >> 3) as u32;
        let wire = tag_val & 0x7;

        match (field, wire) {
            (1, 2) => {
                if let Some(b) = read_length_delimited(&mut buf) {
                    resp.text = String::from_utf8_lossy(b).into_owned();
                }
            }
            (2, 0) => {
                if let Some(v) = read_varint(&mut buf) {
                    resp.suggestion_start_line = Some(v as i32);
                }
            }
            (4, 0) => {
                if let Some(v) = read_varint(&mut buf) {
                    resp.done_stream = Some(v != 0);
                }
            }
            (11, 2) => {
                // LineRange: start_line_number=1, end_line_number_inclusive=2
                if let Some(inner) = read_length_delimited(&mut buf) {
                    let mut ibuf = inner;
                    while !ibuf.is_empty() {
                        let itag = match read_varint(&mut ibuf) {
                            Some(v) => v,
                            None => break,
                        };
                        match (itag >> 3, itag & 0x7) {
                            (1, 0) => { resp.start_line = read_varint(&mut ibuf).map(|v| v as i32); }
                            (2, 0) => { resp.end_line_inclusive = read_varint(&mut ibuf).map(|v| v as i32); }
                            (f, w) => { skip_field(&mut ibuf, w); }
                        }
                    }
                }
            }
            (_, w) => { skip_field(&mut buf, w); }
        }
    }
    resp
}

// ── StreamNextCursorPredictionResponse decode ────────────────────────────────
// line_number=2, is_not_in_range=3

#[derive(Debug, Default)]
pub struct StreamNextCursorResponse {
    pub line_number: i32,
    pub is_not_in_range: bool,
}

pub fn decode_stream_next_cursor_response(data: &[u8]) -> StreamNextCursorResponse {
    let mut resp = StreamNextCursorResponse::default();
    let mut buf = data;

    while !buf.is_empty() {
        let tag_val = match read_varint(&mut buf) {
            Some(v) => v,
            None => break,
        };
        let field = (tag_val >> 3) as u32;
        let wire = tag_val & 0x7;

        match (field, wire) {
            (2, 0) => { resp.line_number = read_varint(&mut buf).unwrap_or(0) as i32; }
            (3, 0) => { resp.is_not_in_range = read_varint(&mut buf).unwrap_or(0) != 0; }
            (_, w) => { skip_field(&mut buf, w); }
        }
    }
    resp
}

/// Parse all framed messages from a streaming response body chunk.
/// Returns list of decoded byte slices (each is one protobuf message).
pub fn parse_frames(data: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut buf = data;
    while buf.len() >= 5 {
        let compressed = buf[0];
        let len = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]) as usize;
        buf = &buf[5..];
        if buf.len() < len {
            break;
        }
        if compressed == 0 {
            out.push(buf[..len].to_vec());
        }
        buf = &buf[len..];
    }
    out
}
