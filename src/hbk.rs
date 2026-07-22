use std::fs;
use std::io::Read;
use flate2::bufread::DeflateDecoder;

fn u32_from_bytes(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn u16_from_bytes(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

fn parse_block_header(data: &[u8], offset: usize) -> Result<Option<(u32, u32, u32)>, String> {
    if offset + 31 > data.len() {
        return Ok(None);
    }
    let hdr = &data[offset..offset + 31];
    if hdr[0] != b'\r' || hdr[1] != b'\n' {
        return Err("Invalid block header: missing leading CRLF".to_string());
    }
    if hdr[29] != b'\r' || hdr[30] != b'\n' {
        return Err("Invalid block header: missing trailing CRLF".to_string());
    }
    if hdr[8] != b' ' || hdr[17] != b' ' || hdr[26] != b' ' {
        return Err("Invalid block header: missing field separators".to_string());
    }

    let payload_hex =
        std::str::from_utf8(&hdr[2..8]).map_err(|e| format!("Bad payload hex: {}", e))?;
    let _block_hex =
        std::str::from_utf8(&hdr[9..17]).map_err(|e| format!("Bad block size hex: {}", e))?;
    let next_hex =
        std::str::from_utf8(&hdr[18..26]).map_err(|e| format!("Bad next block hex: {}", e))?;

    let payload_size = u32::from_str_radix(payload_hex, 16)
        .map_err(|e| format!("Bad payload size value: {}", e))?;
    let _block_size = u32::from_str_radix(_block_hex, 16)
        .map_err(|e| format!("Bad block size value: {}", e))?;
    let next_block = u32::from_str_radix(next_hex, 16)
        .map_err(|e| format!("Bad next block value: {}", e))?;

    Ok(Some((payload_size, _block_size, next_block)))
}

fn read_block_chain(data: &[u8], start_block: u32, default_block_size: u32) -> Result<Vec<u8>, String> {
    let mut result = Vec::new();
    let mut block = start_block;

    loop {
        let offset = 16 + block as usize * default_block_size as usize;
        if offset + 31 > data.len() {
            return Err(format!("Block header at offset {} exceeds file bounds", offset));
        }

        let (payload_size, _, next_block) = match parse_block_header(data, offset)? {
            Some(v) => v,
            None => return Err(format!("Could not parse block header at offset {}", offset)),
        };

        let payload_start = offset + 31;
        let payload_end = payload_start + payload_size as usize;
        if payload_end > data.len() {
            return Err(format!("Block payload at offset {} exceeds file bounds", offset));
        }

        result.extend_from_slice(&data[payload_start..payload_end]);

        if next_block == 0x7FFFFFFF {
            break;
        }
        block = next_block;
    }

    Ok(result)
}

fn extract_utf16le_null_terminated(data: &[u8]) -> Result<String, String> {
    let mut end = 0;
    while end + 1 < data.len() {
        if data[end] == 0 && data[end + 1] == 0 {
            break;
        }
        end += 2;
    }
    let u16s: Vec<u16> = data[..end]
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16(&u16s).map_err(|e| format!("Invalid UTF-16LE: {}", e))
}

fn parse_zip_entries(data: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut entries = Vec::new();
    let mut offset = 0;

    while offset + 30 <= data.len() {
        let sig = u32_from_bytes(&data[offset..]);
        if sig == 0x04034b50 {
            let method = u16_from_bytes(&data[offset + 8..]) as u32;
            let compressed_size = u32_from_bytes(&data[offset + 18..]);
            let filename_len = u16_from_bytes(&data[offset + 26..]) as usize;
            let extra_len = u16_from_bytes(&data[offset + 28..]) as usize;

            if offset + 30 + filename_len + extra_len + compressed_size as usize > data.len() {
                offset += 1;
                continue;
            }

            let name_bytes = &data[offset + 30..offset + 30 + filename_len];
            let name = String::from_utf8_lossy(name_bytes).to_string();

            let data_start = offset + 30 + filename_len + extra_len;
            let compressed_end = data_start + compressed_size as usize;
            let raw_data = &data[data_start..compressed_end];

            let decompressed = if method == 0 {
                raw_data.to_vec()
            } else if method == 8 {
                let mut decoder = DeflateDecoder::new(raw_data);
                let mut buf = Vec::new();
                if decoder.read_to_end(&mut buf).is_err() {
                    offset += 1;
                    continue;
                }
                buf
            } else {
                offset += 1;
                continue;
            };

            entries.push((name, decompressed));
            offset = compressed_end;
        } else if sig == 0x02014b50 {
            break;
        } else {
            offset += 1;
        }
    }

    Ok(entries)
}

fn count_html_in_zip(data: &[u8]) -> u32 {
    let mut count = 0u32;
    let mut offset = 0;
    while offset + 30 <= data.len() {
        let sig = u32_from_bytes(&data[offset..]);
        if sig == 0x04034b50 {
            let compressed_size = u32_from_bytes(&data[offset + 18..]) as usize;
            let filename_len = u16_from_bytes(&data[offset + 26..]) as usize;
            let extra_len = u16_from_bytes(&data[offset + 28..]) as usize;

            if offset + 30 + filename_len + extra_len + compressed_size <= data.len() {
                let name = String::from_utf8_lossy(&data[offset + 30..offset + 30 + filename_len]);
                let lower = name.to_lowercase();
                if lower.ends_with(".html") || lower.ends_with(".htm") {
                    count += 1;
                }
            }
            offset += 30 + filename_len + extra_len + compressed_size;
        } else if sig == 0x02014b50 {
            break;
        } else {
            offset += 1;
        }
    }
    count
}

pub struct HbkPage {
    pub name: String,
    pub html: String,
}

pub fn iter_hbk_pages(file_path: &str, on_progress: impl Fn(u32, u32)) -> Result<Vec<HbkPage>, String> {
    let data = fs::read(file_path).map_err(|e| format!("Failed to read file: {}", e))?;

    if data.len() < 16 {
        return Err("File too small for header".to_string());
    }

    let _first_free_block = u32_from_bytes(&data[0..]);
    let default_block_size = u32_from_bytes(&data[4..]);

    let toc_data = read_block_chain(&data, 0, default_block_size)?;

    if toc_data.len() < 7 * 12 {
        return Err("TOC too short".to_string());
    }

    let fs_body_addr = u32_from_bytes(&toc_data[16..]);

    if fs_body_addr == 0 {
        return Err("FileStorage body address is 0".to_string());
    }

    let zip_data = read_block_chain(&data, fs_body_addr, default_block_size)?;
    let entries = parse_zip_entries(&zip_data)?;

    let total = entries.len() as u32;
    let mut pages = Vec::new();
    let mut parsed = 0u32;

    for (i, (name, content)) in entries.iter().enumerate() {
        let lower = name.to_lowercase();
        if lower.ends_with(".html") || lower.ends_with(".htm") {
            let html = String::from_utf8_lossy(content).to_string();
            pages.push(HbkPage {
                name: name.clone(),
                html,
            });
            parsed += 1;
        }
        if i % 10 == 0 {
            on_progress(parsed, total);
        }
    }

    on_progress(parsed, total);
    Ok(pages)
}

pub fn estimate_hbk_page_count(file_path: &str) -> u32 {
    let data = match fs::read(file_path) {
        Ok(d) => d,
        Err(_) => return 0,
    };

    if data.len() < 16 {
        return 0;
    }

    let default_block_size = u32_from_bytes(&data[4..]);
    let first_free_block = u32_from_bytes(&data[0..]);
    if first_free_block == 0 {
        return 0;
    }

    let toc_data = match read_block_chain(&data, 0, default_block_size) {
        Ok(t) => t,
        Err(_) => return 0,
    };

    if toc_data.len() < 7 * 12 {
        return 0;
    }

    let fs_body_addr = u32_from_bytes(&toc_data[16..]);
    if fs_body_addr == 0 {
        return 0;
    }

    let zip_data = match read_block_chain(&data, fs_body_addr, default_block_size) {
        Ok(z) => z,
        Err(_) => return 0,
    };

    count_html_in_zip(&zip_data)
}
