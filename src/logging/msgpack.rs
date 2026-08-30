use super::ServiceLogRecord;

const ORIGIN: &str = "origin";
const IS_ERROR: &str = "is_error";
const MESSAGE: &str = "message";
const TIMESTAMP: &str = "timestamp";
const JOB_ID: &str = "job_id";

pub fn encode_eventd_log_record(record: &ServiceLogRecord) -> Vec<u8> {
    let mut out = Vec::with_capacity(encoded_eventd_log_record_len(record));
    write_record(&mut out, record);
    out
}

/// Encode one eventd ingestion datagram containing a MessagePack array of
/// records. The caller-owned allocation is retained between sends on the hot
/// path.
pub fn encode_eventd_log_records_into(out: &mut Vec<u8>, records: &[ServiceLogRecord]) {
    out.clear();
    out.reserve(
        array_header_len(records.len())
            + records
                .iter()
                .map(encoded_eventd_log_record_len)
                .sum::<usize>(),
    );
    write_array_len(out, records.len());
    for record in records {
        write_record(out, record);
    }
}

/// Return the largest non-empty prefix which fits in one eventd datagram.
/// Zero means that even the first record exceeds the configured ceiling.
pub fn eventd_log_batch_prefix_len<'a>(
    records: impl IntoIterator<Item = &'a ServiceLogRecord>,
    max_datagram_bytes: usize,
) -> usize {
    let mut payload_bytes = 0usize;
    let mut accepted = 0usize;
    for (index, record) in records.into_iter().enumerate() {
        payload_bytes = payload_bytes.saturating_add(encoded_eventd_log_record_len(record));
        let count = index + 1;
        let encoded_bytes = array_header_len(count).saturating_add(payload_bytes);
        if encoded_bytes > max_datagram_bytes {
            break;
        }
        accepted = count;
    }
    accepted
}

pub fn encoded_eventd_log_record_len(record: &ServiceLogRecord) -> usize {
    1 + encoded_str_len(ORIGIN)
        + encoded_str_len(&record.origin)
        + encoded_str_len(IS_ERROR)
        + 1
        + encoded_str_len(MESSAGE)
        + encoded_str_len(&record.message)
        + encoded_str_len(TIMESTAMP)
        + encoded_u64_len(record.timestamp_ns)
        + record
            .job_id
            .map(|job_id| encoded_str_len(JOB_ID) + encoded_bin_len(job_id.as_bytes().len()))
            .unwrap_or(0)
}

fn write_record(out: &mut Vec<u8>, record: &ServiceLogRecord) {
    write_map_len(out, if record.job_id.is_some() { 5 } else { 4 });
    write_str(out, ORIGIN);
    write_str(out, &record.origin);
    write_str(out, IS_ERROR);
    write_bool(out, record.is_error);
    write_str(out, MESSAGE);
    write_str(out, &record.message);
    write_str(out, TIMESTAMP);
    write_u64(out, record.timestamp_ns);
    if let Some(job_id) = record.job_id {
        write_str(out, JOB_ID);
        write_bin(out, &job_id.as_bytes());
    }
}

fn write_array_len(out: &mut Vec<u8>, len: usize) {
    match len {
        0..=15 => out.push(0x90 | len as u8),
        16..=0xffff => {
            out.push(0xdc);
            out.extend_from_slice(&(len as u16).to_be_bytes());
        }
        _ => {
            out.push(0xdd);
            out.extend_from_slice(&(len as u32).to_be_bytes());
        }
    }
}

fn array_header_len(len: usize) -> usize {
    match len {
        0..=15 => 1,
        16..=0xffff => 3,
        _ => 5,
    }
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

fn encoded_u64_len(value: u64) -> usize {
    match value {
        0..=0x7f => 1,
        0x80..=0xff => 2,
        0x100..=0xffff => 3,
        0x1_0000..=0xffff_ffff => 5,
        _ => 9,
    }
}

fn encoded_str_len(value: &str) -> usize {
    let header = match value.len() {
        0..=31 => 1,
        32..=0xff => 2,
        0x100..=0xffff => 3,
        _ => 5,
    };
    header + value.len()
}

fn encoded_bin_len(len: usize) -> usize {
    let header = match len {
        0..=0xff => 2,
        0x100..=0xffff => 3,
        _ => 5,
    };
    header + len
}

#[cfg(test)]
mod tests;
