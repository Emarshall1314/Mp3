//! Packs raw DSD (byte-interleaved per channel, MSB = earliest bit) into output samples for DoP or
//! native DSD formats. The packed words travel through the f64 pipeline exactly (they are integers
//! of at most 32 bits) and the output's quantiser restores them bit for bit.

use super::DeviceFormat;

/// DoP v1.1 marker bytes, alternating every frame.
const DOP_MARKERS: [u32; 2] = [0x05, 0xFA];

#[derive(Debug, Clone)]
pub(crate) struct DsdPacker {
    /// DSD bytes per channel per output word (2 for DoP).
    bytes: usize,
    dop: bool,
    /// Significant bits of the word as it sits in the f64 pipeline.
    bits: u8,
    channels: usize,
    /// Bytes per channel left over from the last chunk (less than one word).
    carry: Vec<Vec<u8>>,
    marker: usize,
}

impl DsdPacker {
    /// `None` for PCM formats.
    pub(crate) fn new(format: DeviceFormat, channels: u16) -> Option<Self> {
        let (bytes, dop, bits) = match format {
            DeviceFormat::Dop { .. } => (2, true, 24),
            DeviceFormat::Dsd { bytes, .. } => (usize::from(bytes.max(1)), false, bytes.max(1) * 8),
            _ => return None,
        };
        let channels = usize::from(channels.max(1));
        Some(Self {
            bytes,
            dop,
            bits,
            channels,
            carry: vec![Vec::new(); channels],
            marker: 0,
        })
    }

    /// Appends whole output frames packed from `input`; a partial word waits for the next chunk.
    pub(crate) fn pack(&mut self, input: &[u8], out: &mut Vec<f64>) {
        let ch = self.channels;
        for frame in input.chunks_exact(ch) {
            for (c, &b) in frame.iter().enumerate() {
                self.carry[c].push(b);
            }
        }
        let words = self.carry[0].len() / self.bytes;
        let scale = 1.0 / f64::from(1u32 << (self.bits - 1));
        for w in 0..words {
            let marker = DOP_MARKERS[self.marker];
            self.marker ^= 1;
            for c in 0..ch {
                let word = self.carry[c][w * self.bytes..(w + 1) * self.bytes]
                    .iter()
                    .fold(0u32, |acc, &b| (acc << 8) | u32::from(b));
                let word = if self.dop {
                    (marker << 16) | word
                } else {
                    word
                };
                // As a signed integer of `bits` bits.
                let signed = if word >= 1 << (self.bits - 1) {
                    i64::from(word) - (1i64 << self.bits)
                } else {
                    i64::from(word)
                };
                out.push(signed as f64 * scale);
            }
        }
        for c in &mut self.carry {
            c.drain(..words * self.bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::Quantizer;

    fn ints(packer: &mut DsdPacker, bits: u8, input: &[u8]) -> Vec<u32> {
        let mut f = Vec::new();
        packer.pack(input, &mut f);
        let mut q = Quantizer::new(bits);
        let mut out = Vec::new();
        q.convert(&f, &mut out);
        out.into_iter()
            .map(|v| v as u32 & ((1u64 << bits) - 1) as u32)
            .collect()
    }

    #[test]
    fn dop_words_carry_markers_and_bytes_in_order() {
        let mut p = DsdPacker::new(DeviceFormat::Dop { bits: 24 }, 2).unwrap();
        // L0 R0 L1 R1 | L2 R2 L3 R3, then half a word.
        let words = ints(
            &mut p,
            24,
            &[0x11, 0xAA, 0x22, 0xBB, 0x33, 0xCC, 0x44, 0xDD, 0x55, 0xEE],
        );
        assert_eq!(words, [0x05_1122, 0x05_AABB, 0xFA_3344, 0xFA_CCDD]);
        // The carried byte completes with the next chunk; markers keep alternating.
        let words = ints(&mut p, 24, &[0x66, 0xFF]);
        assert_eq!(words, [0x05_5566, 0x05_EEFF]);
    }

    #[test]
    fn dop_in_a_32_bit_container_sits_in_the_top_24_bits() {
        let mut p = DsdPacker::new(DeviceFormat::Dop { bits: 32 }, 1).unwrap();
        let words = ints(&mut p, 32, &[0x69, 0x96]);
        assert_eq!(words, [0x0569_9600]);
    }

    #[test]
    fn native_u32_keeps_the_oldest_byte_in_the_msb() {
        let mut p = DsdPacker::new(
            DeviceFormat::Dsd {
                bytes: 4,
                big_endian: true,
            },
            1,
        )
        .unwrap();
        assert_eq!(ints(&mut p, 32, &[0x01, 0x02, 0x03, 0xF4]), [0x0102_03F4]);
        let mut p = DsdPacker::new(
            DeviceFormat::Dsd {
                bytes: 1,
                big_endian: false,
            },
            2,
        )
        .unwrap();
        assert_eq!(ints(&mut p, 8, &[0x69, 0xF0]), [0x69, 0xF0]);
    }
}
