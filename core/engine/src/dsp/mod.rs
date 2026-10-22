//! The DSP chain, run on the output thread so changes are heard at once:
//! preamp → EQ → crossfeed → convolution → limiter. Volume and dither follow in the output.
//!
//! Everything is f64. When nothing is enabled the chain is absent and the signal is untouched,
//! so bit-perfect playback is unaffected. DSD streams bypass it.

mod biquad;
mod convolver;
mod crossfeed;
mod limiter;

use std::path::Path;
use std::sync::Arc;

use cadenza_decode::{Chunk, SampleFormat};
use serde::{Deserialize, Serialize};

pub use biquad::{EqBand, FilterKind};
pub use crossfeed::CrossfeedLevel;

use crate::convert::Resampler;
use crate::{Error, Result};
use biquad::{Biquad, Coefficients};
use convolver::{Convolver, Kernel};
use crossfeed::Crossfeed;
use limiter::Limiter;

/// Longest impulse response accepted (10 s): longer ones are almost certainly the wrong file.
const MAX_IR_SECONDS: f64 = 10.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DspSettings {
    /// Gain before the EQ, usually negative to leave headroom for boosts.
    pub preamp_db: f64,
    pub eq_enabled: bool,
    pub eq: Vec<EqBand>,
    pub crossfeed_enabled: bool,
    pub crossfeed: CrossfeedLevel,
    pub convolution_enabled: bool,
    /// Impulse response file (WAV, FLAC, …), mono or one channel per output channel.
    pub convolution_path: Option<String>,
    pub limiter: bool,
}

impl Default for DspSettings {
    fn default() -> Self {
        Self {
            preamp_db: 0.0,
            eq_enabled: false,
            eq: Vec::new(),
            crossfeed_enabled: false,
            crossfeed: CrossfeedLevel::Default,
            convolution_enabled: false,
            convolution_path: None,
            limiter: false,
        }
    }
}

impl DspSettings {
    /// True if any stage would change the signal.
    pub fn is_active(&self) -> bool {
        self.preamp_db != 0.0
            || (self.eq_enabled
                && self
                    .eq
                    .iter()
                    .any(|b| b.gain_db != 0.0 || !b.kind.has_gain()))
            || self.crossfeed_enabled
            || (self.convolution_enabled && self.convolution_path.is_some())
            || self.limiter
    }
}

impl FilterKind {
    fn has_gain(self) -> bool {
        matches!(self, Self::Peak | Self::LowShelf | Self::HighShelf)
    }
}

/// The last impulse response loaded (or the reason it could not be), so EQ tweaks neither
/// reload and re-transform it nor repeat its error.
pub(crate) struct IrCache {
    key: Option<(String, u32, u16)>,
    loaded: std::result::Result<Vec<Arc<Kernel>>, String>,
}

impl Default for IrCache {
    fn default() -> Self {
        Self {
            key: None,
            loaded: Ok(Vec::new()),
        }
    }
}

impl IrCache {
    /// The kernels, or `Err(Some(reason))` the first time loading fails and `Err(None)` after.
    fn get(
        &mut self,
        path: &str,
        rate: u32,
        channels: u16,
    ) -> std::result::Result<Vec<Arc<Kernel>>, Option<String>> {
        let key = (path.to_owned(), rate, channels);
        if self.key.as_ref() == Some(&key) {
            return self.loaded.clone().map_err(|_| None);
        }
        self.key = Some(key);
        self.loaded = load_ir(Path::new(path), rate, channels).map_err(|e| e.to_string());
        self.loaded.clone().map_err(Some)
    }
}

/// The chain for one output stream (rate and channel count fixed).
pub(crate) struct Chain {
    preamp: f64,
    channels: usize,
    /// Per channel, the EQ's biquads in series.
    eq: Vec<Vec<Biquad>>,
    crossfeed: Option<Crossfeed>,
    convolver: Option<Convolver>,
    limiter: Option<Limiter>,
    /// What runs, for the signal path panel.
    pub(crate) stages: Vec<String>,
}

impl std::fmt::Debug for Chain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Chain")
            .field("stages", &self.stages)
            .finish_non_exhaustive()
    }
}

impl Chain {
    /// Builds the chain, or `None` when nothing would change the signal. Problems that disable
    /// one stage (an unreadable impulse response) come back as warnings; the rest still runs.
    pub(crate) fn build(
        settings: &DspSettings,
        rate: u32,
        channels: u16,
        irs: &mut IrCache,
    ) -> (Option<Self>, Vec<String>) {
        let mut warnings = Vec::new();
        if !settings.is_active() {
            return (None, warnings);
        }
        let ch = usize::from(channels.max(1));
        let mut stages = Vec::new();
        if settings.preamp_db != 0.0 {
            stages.push(format!("Preamp {:+.1} dB", settings.preamp_db));
        }
        let mut coefficients = Vec::new();
        if settings.eq_enabled {
            for band in &settings.eq {
                match Coefficients::new(band, rate) {
                    Some(c) => coefficients.push(c),
                    None => warnings.push(format!(
                        "EQ band at {:.0} Hz skipped: not possible at {rate} Hz",
                        band.freq
                    )),
                }
            }
            if !coefficients.is_empty() {
                let n = coefficients.len();
                stages.push(format!("EQ · {n} band{}", if n == 1 { "" } else { "s" }));
            }
        }
        let eq = (0..ch)
            .map(|_| coefficients.iter().map(|c| Biquad::new(*c)).collect())
            .collect();
        let crossfeed = (settings.crossfeed_enabled && ch == 2).then(|| {
            stages.push(format!("Crossfeed · {}", settings.crossfeed.label()));
            Crossfeed::new(settings.crossfeed, rate)
        });
        if settings.crossfeed_enabled && ch != 2 {
            warnings.push("Crossfeed needs a stereo output".into());
        }
        let convolver = match (&settings.convolution_path, settings.convolution_enabled) {
            (Some(path), true) => match irs.get(path, rate, channels) {
                Ok(kernels) => {
                    let name = Path::new(path)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    stages.push(format!("Convolution · {name}"));
                    Some(Convolver::new(kernels))
                }
                Err(reason) => {
                    if let Some(e) = reason {
                        warnings.push(format!("Convolution off: {e}"));
                    }
                    None
                }
            },
            _ => None,
        };
        let limiter = settings.limiter.then(|| {
            stages.push("Limiter".into());
            Limiter::new(rate, channels)
        });
        let chain = Self {
            preamp: 10f64.powf(settings.preamp_db / 20.0),
            channels: ch,
            eq,
            crossfeed,
            convolver,
            limiter,
            stages,
        };
        (Some(chain), warnings)
    }

    /// Frames the chain holds back (convolution block, limiter lookahead).
    pub(crate) fn latency(&self) -> usize {
        self.convolver.as_ref().map_or(0, Convolver::latency)
            + self.limiter.as_ref().map_or(0, Limiter::latency)
    }

    /// Processes interleaved frames in place.
    pub(crate) fn process(&mut self, buf: &mut [f64]) {
        if self.preamp != 1.0 {
            buf.iter_mut().for_each(|s| *s *= self.preamp);
        }
        if self.eq.first().is_some_and(|f| !f.is_empty()) {
            for frame in buf.chunks_exact_mut(self.channels) {
                for (s, filters) in frame.iter_mut().zip(&mut self.eq) {
                    for f in filters.iter_mut() {
                        *s = f.process(*s);
                    }
                }
            }
        }
        if let Some(cf) = self.crossfeed.as_mut() {
            cf.process(buf);
        }
        if let Some(conv) = self.convolver.as_mut() {
            conv.process(buf);
        }
        if let Some(l) = self.limiter.as_mut() {
            l.process(buf);
        }
    }

    /// Forgets all history (seek, flush).
    pub(crate) fn reset(&mut self) {
        self.eq.iter_mut().flatten().for_each(Biquad::reset);
        if let Some(cf) = self.crossfeed.as_mut() {
            cf.reset();
        }
        if let Some(conv) = self.convolver.as_mut() {
            conv.reset();
        }
        if let Some(l) = self.limiter.as_mut() {
            l.reset();
        }
    }
}

/// Reads an impulse response: one kernel per output channel (a mono IR is shared), resampled to
/// the output rate.
fn load_ir(path: &Path, rate: u32, channels: u16) -> Result<Vec<Arc<Kernel>>> {
    let mut dec = cadenza_decode::open(path)?;
    let spec = dec.spec().clone();
    let ir_ch = usize::from(spec.channels.max(1));
    if ir_ch != 1 && spec.channels != channels {
        return Err(Error::Output(format!(
            "the impulse response has {} channels; the output has {channels}",
            spec.channels
        )));
    }
    let limit = (f64::from(spec.sample_rate) * MAX_IR_SECONDS) as usize * ir_ch;
    let mut samples: Vec<f64> = Vec::new();
    while let Some(chunk) = dec.next_chunk()? {
        match (chunk, spec.format) {
            (Chunk::Int(s), SampleFormat::Int(bits)) => {
                let scale = 1.0 / (1u64 << (bits.clamp(1, 32) - 1)) as f64;
                samples.extend(s.iter().map(|&v| f64::from(v) * scale));
            }
            (Chunk::Float(s), _) => samples.extend(s.iter().map(|&v| f64::from(v))),
            _ => return Err(Error::Output("the impulse response is not PCM".into())),
        }
        if samples.len() > limit {
            return Err(Error::Output(format!(
                "the impulse response is longer than {MAX_IR_SECONDS} s"
            )));
        }
    }
    if spec.sample_rate != rate {
        let mut r = Resampler::new(spec.sample_rate, rate, spec.channels)?;
        let mut out = Vec::new();
        r.process(&samples, &mut out)?;
        let produced = (out.len() / ir_ch) as u64;
        let expected =
            (samples.len() / ir_ch) as u64 * u64::from(rate) / u64::from(spec.sample_rate);
        r.flush(&mut out, Some(expected), produced)?;
        samples = out;
    }
    let per_channel: Vec<Vec<f64>> = (0..ir_ch)
        .map(|c| samples.iter().skip(c).step_by(ir_ch).copied().collect())
        .collect();
    let kernels: Vec<Arc<Kernel>> = per_channel
        .iter()
        .map(|ir| Arc::new(Kernel::new(ir)))
        .collect();
    Ok((0..usize::from(channels))
        .map(|c| Arc::clone(&kernels[c.min(kernels.len() - 1)]))
        .collect())
}

/// The EQ's combined response in dB (preamp included) at each of `freqs`, for drawing the curve.
pub fn eq_curve(preamp_db: f64, bands: &[EqBand], freqs: &[f64]) -> Vec<f64> {
    const RATE: u32 = 96_000;
    let coefficients: Vec<Coefficients> = bands
        .iter()
        .filter_map(|b| Coefficients::new(b, RATE))
        .collect();
    freqs
        .iter()
        .map(|&f| {
            preamp_db
                + coefficients
                    .iter()
                    .map(|c| c.response_db(f, RATE))
                    .sum::<f64>()
        })
        .collect()
}

/// An EQ imported from AutoEq's `ParametricEQ.txt` (Equalizer APO syntax).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedEq {
    pub preamp_db: f64,
    pub bands: Vec<EqBand>,
}

/// Parses `Preamp: -6.2 dB` and `Filter 1: ON PK Fc 105 Hz Gain -2.4 dB Q 0.70` lines (PK, LS/LSC,
/// HS/HSC, LP/LPQ, HP/HPQ). Filters marked OFF are skipped.
pub fn parse_autoeq(text: &str) -> std::result::Result<ImportedEq, String> {
    let mut preamp_db = 0.0;
    let mut bands = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        let bad = |what: &str| format!("line {}: {what}", n + 1);
        if let Some(rest) = line.strip_prefix("Preamp:") {
            preamp_db = rest
                .trim()
                .trim_end_matches("dB")
                .trim()
                .parse()
                .map_err(|_| bad("unreadable preamp"))?;
            continue;
        }
        let Some((_, spec)) = line.split_once(':').filter(|_| line.starts_with("Filter")) else {
            continue;
        };
        let words: Vec<&str> = spec.split_whitespace().collect();
        if words.first() != Some(&"ON") {
            continue;
        }
        let kind = match words.get(1).copied() {
            Some("PK" | "PEQ") => FilterKind::Peak,
            Some("LS" | "LSC") => FilterKind::LowShelf,
            Some("HS" | "HSC") => FilterKind::HighShelf,
            Some("LP" | "LPQ") => FilterKind::LowPass,
            Some("HP" | "HPQ") => FilterKind::HighPass,
            other => return Err(bad(&format!("unsupported filter type {other:?}"))),
        };
        let value = |key: &str| {
            words
                .iter()
                .position(|w| *w == key)
                .and_then(|i| words.get(i + 1))
                .and_then(|v| v.parse::<f64>().ok())
        };
        bands.push(EqBand {
            kind,
            freq: value("Fc").ok_or_else(|| bad("missing Fc"))?,
            gain_db: value("Gain").unwrap_or(0.0),
            q: value("Q").unwrap_or(std::f64::consts::FRAC_1_SQRT_2),
        });
    }
    if bands.is_empty() {
        return Err("no filters found: expected an AutoEq ParametricEQ.txt".into());
    }
    Ok(ImportedEq { preamp_db, bands })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autoeq_files_parse() {
        let text = "Preamp: -6.4 dB\n\
                    Filter 1: ON LSC Fc 105 Hz Gain 5.5 dB Q 0.70\n\
                    Filter 2: ON PK Fc 2481 Hz Gain -3.1 dB Q 1.85\n\
                    Filter 3: OFF PK Fc 3000 Hz Gain 9 dB Q 1\n\
                    Filter 4: ON HSC Fc 10000 Hz Gain -2.0 dB Q 0.70\n";
        let eq = parse_autoeq(text).unwrap();
        assert_eq!(eq.preamp_db, -6.4);
        assert_eq!(eq.bands.len(), 3);
        assert_eq!(eq.bands[1].kind, FilterKind::Peak);
        assert_eq!(
            (eq.bands[1].freq, eq.bands[1].gain_db, eq.bands[1].q),
            (2481.0, -3.1, 1.85)
        );
        assert!(parse_autoeq("Filter 1: ON XX Fc 1 Hz").is_err());
        assert!(parse_autoeq("hello").is_err());
    }

    #[test]
    fn an_inactive_chain_is_absent() {
        let mut s = DspSettings::default();
        assert!(
            Chain::build(&s, 44_100, 2, &mut IrCache::default())
                .0
                .is_none()
        );
        s.eq_enabled = true;
        s.eq = vec![EqBand {
            kind: FilterKind::Peak,
            freq: 1000.0,
            gain_db: 0.0,
            q: 1.0,
        }];
        assert!(!s.is_active(), "a flat EQ changes nothing");
        s.eq[0].gain_db = 3.0;
        let (chain, warnings) = Chain::build(&s, 44_100, 2, &mut IrCache::default());
        assert!(warnings.is_empty());
        assert_eq!(chain.unwrap().stages, ["EQ · 1 band"]);
    }
}
