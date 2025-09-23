use super::StreamDecoder;
use crate::{DecodeError, Frame, Table};
use std::sync::Arc;

/// Decode a full message.
///
/// `data` must be a full rzCOBS encoded message. Decoding partial
/// messages is not possible. `data` must NOT include any `0x00` separator byte.
pub fn rzcobs_decode(data: &[u8]) -> Result<Vec<u8>, DecodeError> {
    let mut res = vec![];
    let mut data = data.iter().rev().cloned();
    while let Some(x) = data.next() {
        match x {
            0 => return Err(DecodeError::Malformed),
            0x01..=0x7f => {
                for i in 0..7 {
                    if x & (1 << (6 - i)) == 0 {
                        res.push(data.next().ok_or(DecodeError::Malformed)?);
                    } else {
                        res.push(0);
                    }
                }
            }
            0x80..=0xfe => {
                let n = (x & 0x7f) + 7;
                res.push(0);
                for _ in 0..n {
                    res.push(data.next().ok_or(DecodeError::Malformed)?);
                }
            }
            0xff => {
                for _ in 0..134 {
                    res.push(data.next().ok_or(DecodeError::Malformed)?);
                }
            }
        }
    }

    res.reverse();
    Ok(res)
}

pub struct Rzcobs<'a> {
    table: &'a Table,
    raw: Vec<u8>,
}

pub struct RzcobsOwned {
    table: Arc<Table>,
    raw: Vec<u8>,
}

impl<'a> Rzcobs<'a> {
    pub fn new(table: &'a Table) -> Self {
        Self {
            table,
            raw: Vec::new(),
        }
    }
}

impl StreamDecoder for Rzcobs<'_> {
    fn received(&mut self, data: &[u8]) {
        received_inner(&mut self.raw, data);
    }

    fn decode(&mut self) -> Result<Frame<'_>, DecodeError> {
        // Find frame separator. If not found, we don't have enough data yet.
        let zero = self
            .raw
            .iter()
            .position(|&x| x == 0)
            .ok_or(DecodeError::UnexpectedEof)?;

        let frame = rzcobs_decode(&self.raw[..zero]);
        advance_inner(&mut self.raw, zero);

        debug_assert!(self.raw.is_empty() || self.raw[0] != 0);

        let frame: Vec<u8> = frame?;
        match self.table.decode(&frame) {
            Ok((frame, _consumed)) => Ok(frame),
            Err(DecodeError::UnexpectedEof) => Err(DecodeError::Malformed),
            Err(DecodeError::Malformed) => Err(DecodeError::Malformed),
        }
    }
}

impl RzcobsOwned {
    pub fn new(table: Arc<Table>) -> Self {
        Self {
            table,
            raw: Vec::new(),
        }
    }

    pub fn table(&self) -> Arc<Table> {
        self.table.clone()
    }
}

impl RzcobsOwned {
    pub fn received(&mut self, data: &[u8]) {
        received_inner(&mut self.raw, data);
    }

    pub fn frame_and_decode<F: FnMut(&[u8], Option<Frame<'_>>, usize)>(
        &mut self,
        mut f: F,
    ) -> bool {
        // Find frame separator. If not found, we don't have enough data yet.
        let Some(zero) = self.raw.iter().position(|&x| x == 0) else {
            return false;
        };

        let frame = rzcobs_decode(&self.raw[..zero]);
        let decoded_len = frame.as_ref().map(|f| f.len()).unwrap_or(0);

        match frame.map(|f| self.table.decode(&f)) {
            Ok(Ok((frame, _consumed))) => {
                f(&self.raw[..zero], Some(frame), decoded_len);
            }
            Ok(Err(_e)) | Err(_e) => {
                f(&self.raw[..zero], None, decoded_len);
            }
        }

        advance_inner(&mut self.raw, zero);
        // debug_assert!(raw.is_empty() || raw[0] != 0);
        true
    }
}

fn received_inner(raw: &mut Vec<u8>, mut data: &[u8]) {
    // Trim zeros from the left, start storing at first non-zero byte.
    if raw.is_empty() {
        while data.first() == Some(&0) {
            data = &data[1..]
        }
    }

    raw.extend_from_slice(data);
}

fn advance_inner(raw: &mut Vec<u8>, zero: usize) {
    // Even if rzcobs_decode failed, pop the data off so we don't get stuck.
    // Pop off the frame + 1 or more separator zero-bytes
    if let Some(nonzero) = raw[zero..].iter().position(|&x| x != 0) {
        raw.drain(0..zero + nonzero);
    } else {
        raw.clear();
    }
}
