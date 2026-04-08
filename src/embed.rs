/// Reads the JSON config embedded by the `config_embed` tool.
///
/// Binary layout (appended to the EXE):
///   [JSON bytes][json_len: u64 LE][b"DARTCFG1"]
use std::io::{Read, Seek, SeekFrom};

const MAGIC: &[u8] = b"DARTCFG1";
const LEN_SIZE: u64 = 8;
const FOOTER_SIZE: u64 = LEN_SIZE + MAGIC.len() as u64; // 16

pub fn read_embedded_config() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let mut f = std::fs::File::open(&exe).ok()?;
    let file_len = f.metadata().ok()?.len();

    if file_len < FOOTER_SIZE {
        return None;
    }

    // Read footer (last 16 bytes)
    f.seek(SeekFrom::End(-(FOOTER_SIZE as i64))).ok()?;
    let mut footer = [0u8; 16];
    f.read_exact(&mut footer).ok()?;

    // Verify magic (bytes 8..16)
    if &footer[LEN_SIZE as usize..] != MAGIC {
        return None;
    }

    // JSON byte count (bytes 0..8, little-endian u64)
    let json_len = u64::from_le_bytes(footer[..8].try_into().ok()?);
    if json_len == 0 || json_len > file_len - FOOTER_SIZE {
        return None;
    }

    f.seek(SeekFrom::Start(file_len - FOOTER_SIZE - json_len)).ok()?;
    let mut buf = vec![0u8; json_len as usize];
    f.read_exact(&mut buf).ok()?;

    String::from_utf8(buf).ok()
}
