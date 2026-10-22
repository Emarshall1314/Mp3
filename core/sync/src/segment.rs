//! Log segments: one file per push, never changed after it's written. `CDZS`, a format version
//! byte, then zstd-compressed CBOR of the device, the segment's number and its ops.

use serde::{Deserialize, Serialize};

use crate::hlc::DeviceId;
use crate::op::Op;
use crate::{Error, Result};

const MAGIC: &[u8; 4] = b"CDZS";
const VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub device: DeviceId,
    pub seq: u64,
    pub ops: Vec<Op>,
}

/// The segment's name in its device's log folder: zero-padded so names sort in order.
pub fn name(seq: u64) -> String {
    format!("{seq:016}.seg")
}

/// The number in a segment's name; none for anything else.
pub fn parse_name(name: &str) -> Option<u64> {
    let digits = name.strip_suffix(".seg")?;
    (digits.len() == 16 && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| digits.parse().ok())
        .flatten()
}

pub fn encode(segment: &Segment) -> Result<Vec<u8>> {
    let mut cbor = Vec::new();
    ciborium::into_writer(segment, &mut cbor).map_err(|e| Error::Encode(e.to_string()))?;
    let packed = ruzstd::encoding::compress_to_vec(
        cbor.as_slice(),
        ruzstd::encoding::CompressionLevel::Fastest,
    );
    let mut out = Vec::with_capacity(packed.len() + 5);
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.extend_from_slice(&packed);
    Ok(out)
}

pub fn decode(bytes: &[u8]) -> Result<Segment> {
    let body = bytes
        .strip_prefix(MAGIC.as_slice())
        .ok_or_else(|| Error::Segment("wrong signature".into()))?;
    let (&version, packed) = body
        .split_first()
        .ok_or_else(|| Error::Segment("truncated".into()))?;
    if version != VERSION {
        return Err(Error::Segment(format!(
            "format {version} is newer than this Cadenza understands"
        )));
    }
    let mut packed = packed;
    let decoder = ruzstd::decoding::StreamingDecoder::new(&mut packed)
        .map_err(|e| Error::Segment(e.to_string()))?;
    ciborium::from_reader(decoder).map_err(|e| Error::Segment(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hlc::Hlc;
    use crate::op::{Change, TrackKey};

    #[test]
    fn segments_round_trip_and_reject_other_files() {
        let device = DeviceId::new();
        let ops = (0..50)
            .map(|i| Op {
                hlc: Hlc {
                    ms: 1_700_000_000_000 + i,
                    n: 0,
                    device,
                },
                seen: vec![],
                change: Change::SetField {
                    track: TrackKey::new(format!("hash{i}")),
                    field: "title".into(),
                    values: vec![format!("Song {i}")],
                },
            })
            .collect();
        let segment = Segment {
            device,
            seq: 7,
            ops,
        };
        let bytes = encode(&segment).expect("encode");
        assert_eq!(decode(&bytes).expect("decode"), segment);
        assert!(decode(b"CDZS\x09rest").is_err());
        assert!(decode(b"hello").is_err());
        assert!(decode(&bytes[..bytes.len() / 2]).is_err());
    }

    #[test]
    fn names_sort_in_order() {
        assert_eq!(name(42), "0000000000000042.seg");
        assert_eq!(parse_name(&name(42)), Some(42));
        assert_eq!(parse_name("42.seg"), None);
        assert_eq!(parse_name(".0000000000000042.seg.part"), None);
        assert!(name(9) < name(10));
    }
}
