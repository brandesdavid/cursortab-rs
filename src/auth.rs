use std::path::PathBuf;

pub fn cursor_db_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let linux = PathBuf::from(format!(
        "{}/.config/Cursor/User/globalStorage/state.vscdb",
        home
    ));
    if linux.exists() {
        return Some(linux);
    }
    let macos = PathBuf::from(format!(
        "{}/Library/Application Support/Cursor/User/globalStorage/state.vscdb",
        home
    ));
    if macos.exists() {
        return Some(macos);
    }
    None
}

pub fn get_access_token() -> Result<String, String> {
    let db_path = cursor_db_path()
        .ok_or_else(|| "Cursor database not found (is Cursor installed?)".to_string())?;

    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("Failed to open Cursor DB at {}: {}", db_path.display(), e))?;

    let token: String = conn
        .query_row(
            "SELECT value FROM ItemTable WHERE key = 'cursorAuth/accessToken'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("Failed to read access token: {}", e))?;

    Ok(token.trim().to_string())
}
