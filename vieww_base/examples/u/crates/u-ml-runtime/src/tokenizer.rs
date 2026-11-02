//! The tokenizer, loaded and validated — §5.3's own checklist item, and
//! the one a quantisation comparison quietly depends on: every format is
//! compared through *this* exact file, pinned by digest in
//! [`TokenizerManifest`](crate::TokenizerManifest).
//!
//! The wrap is deliberately thin. Qwen's tokenizer is a byte-level BPE
//! with 151,936 vocab entries whose `tokenizer.json` already carries its
//! special tokens as *added tokens* — which the `tokenizers` crate
//! recognises during encoding even with `add_special_tokens` off, since
//! the template writes its own markers. That property is what makes the
//! prompt builder's job possible: the markup it writes becomes exactly
//! those single control tokens, and nothing a person typed can, because
//! [`sanitize`](crate::prompt::sanitize) broke the spellings first.
//!
//! Everything here is behind the `model` feature; the app as shipped
//! holds no tokenizer, no weights, and no network permission, and the
//! plan's §15 forbids pretending otherwise.

use std::path::Path;

use tokenizers::Tokenizer as HfTokenizer;
use u_core::Error;

/// Qwen3's tokenizer, opened from the exact file its manifest vouches for.
#[derive(Debug, Clone)]
pub struct Tokenizer {
    inner: HfTokenizer,
}

/// `<|im_end|>` — the chat template's turn end, and the model's natural
/// stopping token.
pub const IM_END: &str = "<|im_end|>";
/// `<|endoftext|>` — the base EOS the generation config names.
pub const END_OF_TEXT: &str = "<|endoftext|>";

impl Tokenizer {
    /// Open the tokenizer file — the file the caller has already verified
    /// against [`crate::TokenizerManifest`]; this type trusts that check
    /// and adds one of its own: the special tokens must exist in the
    /// vocabulary, because a tokenizer without them cannot render the
    /// chat template the prompts are written in.
    ///
    /// # Errors
    /// [`Error::NoRuntime`] when the file cannot be opened or parsed, and
    /// when the vocabulary is missing either stopping token — a tokenizer
    /// that cannot say "turn over" cannot be asked to stop.
    pub fn load(path: &Path) -> Result<Self, Error> {
        let inner =
            HfTokenizer::from_file(path).map_err(|_| Error::NoRuntime("a loadable tokenizer"))?;
        let me = Self { inner };
        for marker in [IM_END, END_OF_TEXT] {
            if me.id_of(marker).is_none() {
                return Err(Error::NoRuntime("a tokenizer with the chat markers"));
            }
        }
        Ok(me)
    }

    /// The id of a special-token spelling, if the vocabulary has it.
    #[must_use]
    pub fn id_of(&self, spelling: &str) -> Option<u32> {
        self.inner.token_to_id(spelling)
    }

    /// The token ids that end a turn, for the [`Stopper`](crate::generation::Stopper).
    ///
    /// `<|im_end|>` first: in the chat template it is the one the model
    /// was trained to write. The base end-of-text rides along because a
    /// small model sometimes reaches for it, and a stopper that ignored
    /// it would let the run eat its whole token cap on padding.
    #[must_use]
    pub fn eos_ids(&self) -> Vec<u32> {
        [IM_END, END_OF_TEXT]
            .iter()
            .filter_map(|s| self.id_of(s))
            .collect()
    }

    /// Encode prompt text to token ids.
    ///
    /// Special tokens are *off*: the template's own markup is already in
    /// the text and must tokenise as the control tokens it spells, while
    /// the post-processor's automatic additions (a leading BOS, say)
    /// would change the prompt the chat template defines.
    ///
    /// # Errors
    /// [`Error::NoRuntime`] if encoding fails — which for a file-backed
    /// BPE is a corruption signal, not an input signal.
    pub fn encode(&self, text: &str) -> Result<Vec<u32>, Error> {
        self.inner
            .encode(text, false)
            .map(|e| e.get_ids().to_vec())
            .map_err(|_| Error::NoRuntime("an encoding tokenizer"))
    }

    /// Decode generated ids back to text, special tokens skipped.
    ///
    /// # Errors
    /// [`Error::NoRuntime`] on decode failure.
    pub fn decode(&self, ids: &[u32]) -> Result<String, Error> {
        let ids: Vec<u32> = ids.to_vec();
        self.inner
            .decode(&ids, true)
            .map_err(|_| Error::NoRuntime("a decoding tokenizer"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real tokenizer file, when the developer has fetched it (see
    /// `docs/models.md`); without it these tests skip, because a fake
    /// vocabulary would prove nothing about the real one.
    fn real_path() -> Option<std::path::PathBuf> {
        let dir = std::env::var_os("U_MODELS_DIR")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::path::Path::new(&std::env::var("HOME").ok()?)
                    .join(".u")
                    .join("models")
                    .into()
            })?;
        let file = dir.join("tokenizer.json");
        file.is_file().then_some(file)
    }

    #[test]
    fn the_chat_markers_tokenise_as_single_control_tokens() {
        let Some(path) = real_path() else {
            eprintln!("skipping: no tokenizer.json in U_MODELS_DIR");
            return;
        };
        let tok = Tokenizer::load(&path).expect("the pinned tokenizer loads");
        let ids = tok
            .encode("<|im_start|>user\nhello<|im_end|>")
            .expect("encodes");
        let im_start = tok.id_of("<|im_start|>").expect("present");
        let im_end = tok.id_of(IM_END).expect("present");
        // The whole markup is exactly three control tokens around "hello".
        assert_eq!(ids.first(), Some(&im_start));
        assert_eq!(ids.last(), Some(&im_end));
        assert!(ids.len() < 10, "the markers are single tokens: {ids:?}");
        // And the stopper's ids resolve in this vocabulary.
        assert!(tok.eos_ids().contains(&im_end));
        // Round-trip: decode strips the specials and keeps the words —
        // including "user", which is plain text in the template, not a
        // special token, and must survive a skip-special decode.
        let text = tok.decode(&ids).expect("decodes");
        assert_eq!(text.trim(), "user\nhello");
        assert!(!text.contains("<|"), "the control tokens are gone");
    }

    #[test]
    fn a_missing_tokenizer_file_is_no_runtime() {
        assert_eq!(
            Tokenizer::load(std::path::Path::new("/nonexistent/tokenizer.json")).unwrap_err(),
            Error::NoRuntime("a loadable tokenizer")
        );
    }
}
