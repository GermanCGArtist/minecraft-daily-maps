//! Chunk decompression (region file compression types 1 = gzip, 2 = zlib, 3 = none).

use std::io::Read;

pub fn decompress(kind: u8, data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(data.len().saturating_mul(4).min(1 << 24));
    match kind {
        1 => {
            flate2::read::GzDecoder::new(data).read_to_end(&mut out).map_err(|e| format!("gzip: {e}"))?;
        }
        2 => {
            flate2::read::ZlibDecoder::new(data).read_to_end(&mut out).map_err(|e| format!("zlib: {e}"))?;
        }
        3 => out.extend_from_slice(data),
        4 => return Err("chunk uses LZ4 compression (region-file-compression=lz4), not supported yet".into()),
        k => return Err(format!("unknown chunk compression type {k}")),
    }
    Ok(out)
}
