use super::ServiceLogRecord;

const ORIGIN: &str = "origin";
const IS_ERROR: &str = "is_error";
const MESSAGE: &str = "message";
const TIMESTAMP: &str = "timestamp";
const JOB_ID: &str = "job_id";

pub fn encode_eventd_log_record(record: &ServiceLogRecord) -> Vec<u8> {
    let mut out = Vec::new();
    write_map_len(&mut out, if record.job_id.is_some() { 5 } else { 4 });
    write_str(&mut out, ORIGIN);
    write_str(&mut out, &record.origin);
    write_str(&mut out, IS_ERROR);
    write_bool(&mut out, record.is_error);
    write_str(&mut out, MESSAGE);
    write_str(&mut out, &record.message);
    write_str(&mut out, TIMESTAMP);
    write_u64(&mut out, record.timestamp_ns);
    if let Some(job_id) = record.job_id {
        write_str(&mut out, JOB_ID);
        write_bin(&mut out, &job_id.as_bytes());
    }
    out
}

fn write_map_len(out: &mut Vec<u8>, len: u8) {
    debug_assert!(len <= 15);
    out.push(0x80 | len);
}

fn write_bool(out: &mut Vec<u8>, value: bool) {
    out.push(if value { 0xc3 } else { 0xc2 });
}

fn write_u64(out: &mut Vec<u8>, value: u64) {
    match value {
        0..=0x7f => out.push(value as u8),
        0x80..=0xff => out.extend_from_slice(&[0xcc, value as u8]),
        0x100..=0xffff => {
            out.push(0xcd);
            out.extend_from_slice(&(value as u16).to_be_bytes());
        }
        0x1_0000..=0xffff_ffff => {
            out.push(0xce);
            out.extend_from_slice(&(value as u32).to_be_bytes());
        }
        _ => {
            out.push(0xcf);
            out.extend_from_slice(&value.to_be_bytes());
        }
    }
}

fn write_str(out: &mut Vec<u8>, value: &str) {
    let bytes = value.as_bytes();
    match bytes.len() {
        0..=31 => out.push(0xa0 | bytes.len() as u8),
        32..=0xff => out.extend_from_slice(&[0xd9, bytes.len() as u8]),
        0x100..=0xffff => {
            out.push(0xda);
            out.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
        }
        _ => {
            out.push(0xdb);
            out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        }
    }
    out.extend_from_slice(bytes);
}

fn write_bin(out: &mut Vec<u8>, value: &[u8]) {
    match value.len() {
        0..=0xff => out.extend_from_slice(&[0xc4, value.len() as u8]),
        0x100..=0xffff => {
            out.push(0xc5);
            out.extend_from_slice(&(value.len() as u16).to_be_bytes());
        }
        _ => {
            out.push(0xc6);
            out.extend_from_slice(&(value.len() as u32).to_be_bytes());
        }
    }
    out.extend_from_slice(value);
}

#[cfg(test)]
mod tests;
