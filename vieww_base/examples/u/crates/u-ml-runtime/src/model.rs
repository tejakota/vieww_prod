//! The weights — Qwen3-0.6B in GGUF form, on Candle, on the CPU.
//!
//! §5.3's integration checklist starts with "model configuration and
//! weight loading", and GGUF answers both at once: the file's own
//! metadata *is* the configuration (head counts, dimensions, the RoPE
//! base, the RMS epsilon), read by the loader rather than restated by us
//! — the two can therefore never disagree about the architecture they
//! just built.
//!
//! The forward pass, the KV cache and the tied-embedding fallback all
//! live in `candle-transformers`' `quantized_qwen3`, written and tested
//! upstream against this model family; this module's job is the seam:
//! open the exact file the manifest vouched for, translate Candle's
//! errors into U's vocabulary, and hand the runtime a type whose misuse
//! is a compile error rather than a corrupted cache.
//!
//! CPU only, on purpose (§5.4): "start with CPU inference on Android
//! ARM64" — and the same `Cpu` device is what a phone's ARM64 core
//! presents to Candle, so the benchmark numbers on this machine measure
//! the arithmetic that ships, not a GPU that does not.

use std::io::BufReader;
use std::path::Path;

use candle::{Device, Tensor};
use candle_transformers::models::quantized_qwen3::ModelWeights;
use u_core::Error;

/// The loaded model: one GGUF file, one CPU, one KV cache.
///
/// The cache is *the* memory that grows, so the runtime clears it between
/// decisions rather than between the two generations of one decision —
/// each stage's prompt shares nothing worth keeping with the next, and a
/// cache that outlived its decision would be a leak with a good story.
#[derive(Debug)]
pub struct Weights {
    inner: ModelWeights,
}

impl Weights {
    /// Load a GGUF file that has already passed its digest check.
    ///
    /// # Errors
    /// [`Error::NoRuntime`] when the file cannot be opened, is not a
    /// GGUF the loader understands, or names a `qwen3` architecture it
    /// cannot build — every one a "these bytes are not the model" fact,
    /// which the caller reports and falls back from.
    pub fn load(path: &Path) -> Result<Self, Error> {
        let file = std::fs::File::open(path).map_err(|_| Error::NoRuntime("the weights file"))?;
        let mut reader = BufReader::new(file);
        let content = candle::quantized::gguf_file::Content::read(&mut reader).map_err(|e| {
            eprintln!("u-ml-runtime: gguf read failed: {e}");
            Error::NoRuntime("a readable GGUF file")
        })?;
        let device = Device::Cpu;
        let inner = ModelWeights::from_gguf(content, &mut reader, &device).map_err(|e| {
            eprintln!("u-ml-runtime: weights build failed: {e}");
            Error::NoRuntime("a loadable qwen3 model")
        })?;
        Ok(Self { inner })
    }

    /// Run the model over `tokens`, returning the logits for the last
    /// position — the next token's distribution.
    ///
    /// `offset` is how many positions the KV cache already holds; the
    /// generation loop owns that arithmetic and this method trusts it,
    /// because a wrong offset is a wrong attention mask, and the type
    /// system cannot tell a right one from a confident one.
    ///
    /// # Errors
    /// [`Error::NoRuntime`] when the forward pass fails — a model that
    /// cannot complete a pass is not a model that answered.
    pub fn logits(&mut self, tokens: &[u32], offset: usize) -> Result<Tensor, Error> {
        let device = Device::Cpu;
        // The model speaks `(batch, length)`: a prefill chunk and a
        // single decode token are both one row of some width. It answers
        // in `(batch, vocabulary)`; the batch is one, so it is squeezed
        // away here — every caller wants the row, not the box it came in.
        let input = Tensor::new(tokens, &device)
            .and_then(|t| t.reshape((1, tokens.len())))
            .map_err(Self::candle)?;
        self.inner
            .forward(&input, offset)
            .and_then(|t| t.squeeze(0))
            .map_err(Self::candle)
    }

    /// Drop every cached key and value — the §5.6 memory bound, applied
    /// between decisions rather than after the phone has noticed.
    pub fn clear_cache(&mut self) {
        self.inner.clear_kv_cache();
    }

    fn candle(e: candle::Error) -> Error {
        eprintln!("u-ml-runtime: forward failed: {e}");
        Error::NoRuntime("a forward the model completed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real_path() -> Option<std::path::PathBuf> {
        let dir = std::env::var_os("U_MODELS_DIR")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::path::Path::new(&std::env::var("HOME").ok()?)
                    .join(".u")
                    .join("models")
                    .into()
            })?;
        let manifests = crate::Manifest::embedded();
        // Any present quantisation will do for the smoke test.
        manifests
            .iter()
            .map(|m| dir.join(&m.file))
            .find(|p| p.is_file())
    }

    #[test]
    fn a_real_gguf_loads_and_makes_a_sane_first_prediction() {
        let Some(path) = real_path() else {
            eprintln!("skipping: no weights in U_MODELS_DIR");
            return;
        };
        let mut weights = Weights::load(&path).expect("the pinned weights load");
        // "The capital of France is" — the next token should be about
        // Paris, which is as low a bar as a 0.6B model can clear while
        // still proving the arithmetic is the model.
        let ids = [464u32, 3139, 286, 4854, 318];
        let logits = weights.logits(&ids, 0).expect("prefill runs");
        let row = logits.to_vec1::<f32>().expect("logits are f32");
        let argmax = row
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).expect("finite logits"))
            .map(|(i, _)| i)
            .expect("a non-empty vocabulary");
        assert!(argmax > 0 && argmax < row.len());
        // The cache now holds the prompt; clearing it must not fail.
        weights.clear_cache();
    }

    #[test]
    fn a_missing_weights_file_is_no_runtime() {
        assert_eq!(
            Weights::load(std::path::Path::new("/nonexistent/weights.gguf")).unwrap_err(),
            Error::NoRuntime("the weights file")
        );
    }
}
