//! `cargo run -p cadenza-decode --example probe -- FILE [OUT.f32 [SEEK_FRAME]]`: prints the stream spec and the
//! decoded length; optionally writes the samples as raw interleaved f32 for analysis.

use std::io::Write;

use cadenza_decode::{Chunk, SampleFormat, open};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args
        .first()
        .ok_or("usage: probe FILE [OUT.f32 [SEEK_FRAME]]")?;
    let mut dec = open(std::path::Path::new(path))?;
    let spec = dec.spec().clone();
    println!("{spec:?}");
    let mut out = args.get(1).map(std::fs::File::create).transpose()?;
    if let Some(at) = args.get(2) {
        dec.seek(at.parse()?)?;
    }
    let mut frames = 0u64;
    while let Some(chunk) = dec.next_chunk()? {
        frames += chunk.frames(spec.channels) as u64;
        if let Some(f) = out.as_mut() {
            let samples: Vec<f32> = match (chunk, spec.format) {
                (Chunk::Int(s), SampleFormat::Int(bits)) => s
                    .iter()
                    .map(|&v| v as f32 / (1u64 << (bits - 1)) as f32)
                    .collect(),
                (Chunk::Float(s), _) => s.to_vec(),
                _ => Vec::new(),
            };
            for v in samples {
                f.write_all(&v.to_le_bytes())?;
            }
        }
    }
    println!("decoded {frames} frames");
    Ok(())
}
