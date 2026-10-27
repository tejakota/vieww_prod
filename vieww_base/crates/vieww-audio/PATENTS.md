# Patents and the vieww-audio codecs

This page says what is known about patents on each format vieww-audio
decodes, so that a team shipping a product can start its own legal review
from facts rather than from silence. **It is not legal advice.** Patent
status differs by country and changes over time; if you distribute a
product, ask your own counsel.

The code in this crate is copyright-licensed under the workspace licence,
and its third-party tables and ports are credited in
[`tools/THIRD_PARTY.md`](tools/THIRD_PARTY.md). That covers copyright only.
A copyright licence grants no patent rights.

## What vieww ships

vieww is distributed as **source code**. Patent pools license *products*:
the encoders and decoders in devices and applications that end users
receive. Whoever builds and distributes such a product is usually the
party a pool would ask to take a licence. Platform vendors such as Apple,
Google and Microsoft license the codecs *their operating systems* provide.
That does not extend to a decoder compiled into your application, such as
this one.

## Format by format

| Format | Decoder | Patent status (summary) |
|---|---|---|
| WAV, AIFF (PCM) | `wav`, `aiff` | Uncompressed PCM. No known patents. |
| FLAC | `flac` | Xiph.Org format, published as royalty-free. No known licensing programme. |
| Ogg Vorbis | `vorbis` | Xiph.Org format, published as royalty-free. No known licensing programme. |
| MP3 (MPEG-1/2 Layer III) | `mp3` | The licensing programme run by Technicolor and Fraunhofer IIS ended in April 2017, after the last patents it administered expired. Decoding MP3 is generally regarded as unencumbered. |
| AAC-LC (ADTS, MP4/M4A) | `aac` (feature `aac`) | **Still covered by an active patent pool in some jurisdictions.** The pool is administered by Via LA (formerly Via Licensing). It licenses AAC encoders and decoders in end products. Many early AAC patents have expired, but not all, and not everywhere. |
| Opus | not decoded yet (AU-06) | IETF RFC 6716. The patent holders who declared patents offer royalty-free licences under their IETF declarations. |

## The `aac` feature

Because AAC is the one format here with an active pool, its decoder sits
behind a Cargo feature. The feature is **on by default**, so AAC works out of
the box. A distributor who prefers not to ship an AAC decoder turns it off:

```toml
vieww-audio = { version = "...", default-features = false }
```

With the feature off:

* the `codec::aac` module and its tables are not compiled into the binary;
* `codec::decode` still recognises ADTS and MP4/M4A input, and returns
  `CodecError::Unsupported`, naming the feature and this file;
* every other format is unchanged.

`tests/aac_feature_off.rs` checks this behaviour. CI runs it with
`--no-default-features`.

A common alternative is to keep the feature off and decode AAC through the
platform instead: MediaCodec on Android, AVFoundation on Apple systems,
Media Foundation on Windows. That puts the operating-system vendor's
licence in charge of AAC. vieww-audio does not provide that bridge today.

## Sources

* MP3: Fraunhofer IIS and Technicolor announced the end of the mp3
  licensing programme on 23 April 2017.
* AAC: Via LA's AAC patent pool and its licensor list are published by
  Via LA.
* Opus: RFC 6716 and the IPR declarations filed with the IETF for it.
* FLAC and Vorbis: the Xiph.Org Foundation's format pages.
