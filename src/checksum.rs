use base64::{engine::general_purpose::STANDARD, Engine};

pub fn generate_checksum(machine_id: &str) -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    // Upper 6 bytes of timestamp (big-endian)
    let mut ts_bytes = [
        ((ts >> 40) & 0xff) as u8,
        ((ts >> 32) & 0xff) as u8,
        ((ts >> 24) & 0xff) as u8,
        ((ts >> 16) & 0xff) as u8,
        ((ts >> 8) & 0xff) as u8,
        (ts & 0xff) as u8,
    ];

    // Encrypt: w=165, each byte = (byte XOR w) + i; w = result
    let mut w: u8 = 165;
    for (i, b) in ts_bytes.iter_mut().enumerate() {
        *b = (*b ^ w).wrapping_add(i as u8);
        w = *b;
    }

    format!("{}{}", STANDARD.encode(ts_bytes), machine_id)
}
