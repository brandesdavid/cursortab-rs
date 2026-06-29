use nvim_rs::{Buffer, Neovim, Value};
use nvim_rs::compat::tokio::Compat;
use tokio::io::Stdout;

pub type NvimWriter = Compat<Stdout>;

#[derive(Default, Clone)]
pub struct BufferState {
    pub lines: Vec<String>,
    pub row: i64,    // 0-based line (col in Go)
    pub col: i64,    // 0-based column (row in Go)
    pub path: String,
    pub version: i64,
    pub id: i64,
    pub diff_history: Vec<String>,
}

impl BufferState {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn sync_in(&mut self, nvim: &Neovim<NvimWriter>) {
        let buf = match nvim.get_current_buf().await {
            Ok(b) => b,
            Err(e) => { log::error!("sync_in: get_current_buf: {}", e); return; }
        };

        let buf_id = match buf.get_number().await {
            Ok(n) => n,
            Err(e) => { log::error!("sync_in: get_number: {}", e); return; }
        };

        let path = match buf.get_name().await {
            Ok(p) => p,
            Err(e) => { log::error!("sync_in: get_name: {}", e); return; }
        };

        let raw_lines = match buf.get_lines(0, -1, false).await {
            Ok(l) => l,
            Err(e) => { log::error!("sync_in: get_lines: {}", e); return; }
        };

        let win = match nvim.get_current_win().await {
            Ok(w) => w,
            Err(e) => { log::error!("sync_in: get_current_win: {}", e); return; }
        };

        let cursor = match win.get_cursor().await {
            Ok(c) => c,
            Err(e) => { log::error!("sync_in: get_cursor: {}", e); return; }
        };

        self.lines = raw_lines;
        // nvim cursor: (1-based line, 0-based col)
        self.row = cursor.0 - 1; // 0-based line
        self.col = cursor.1;     // 0-based col

        self.path = path;

        if self.id != buf_id {
            self.id = buf_id;
            self.diff_history = vec![];
            self.version = 0;
        }

        log::debug!("sync_in: row={} col={} path={}", self.row, self.col, self.path);
    }

    pub fn contents(&self) -> String {
        self.lines.join("\n")
    }
}
