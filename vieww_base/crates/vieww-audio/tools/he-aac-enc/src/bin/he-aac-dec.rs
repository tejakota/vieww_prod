//! `he-aac-dec <in.aac> <out.s16le>` — the FDK reference decoder over an
//! ADTS stream, as a second oracle beside ffmpeg (s16le interleaved; PCM
//! limiter off, so it is the decoder's own output).

use std::ptr;

use fdk_aac_sys as f;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let data = std::fs::read(&a[1]).expect("read");
    let mut out = Vec::new();
    unsafe {
        let h = f::aacDecoder_Open(f::TRANSPORT_TYPE_TT_MP4_ADTS, 1);
        assert!(!h.is_null());
        f::aacDecoder_SetParam(h, f::AACDEC_PARAM_AAC_PCM_LIMITER_ENABLE, 0);
        let mut pcm = vec![0i16; 8 * 2048 * 2];
        let mut at = 0usize;
        loop {
            if at < data.len() {
                let mut p = data.as_ptr().add(at) as *mut u8;
                let mut size = (data.len() - at) as u32;
                let mut valid = size;
                assert_eq!(f::aacDecoder_Fill(h, &mut p, &mut size, &mut valid), f::AAC_DECODER_ERROR_AAC_DEC_OK);
                at += (size - valid) as usize;
            }
            let e = f::aacDecoder_DecodeFrame(h, pcm.as_mut_ptr(), pcm.len() as i32, 0);
            if e == f::AAC_DECODER_ERROR_AAC_DEC_NOT_ENOUGH_BITS {
                if at >= data.len() {
                    break;
                }
                continue;
            }
            if e != f::AAC_DECODER_ERROR_AAC_DEC_OK {
                eprintln!("decode error {e:#x}");
                break;
            }
            let info = &*f::aacDecoder_GetStreamInfo(h);
            let n = (info.frameSize * info.numChannels) as usize;
            for s in &pcm[..n] {
                out.extend(s.to_le_bytes());
            }
            if at >= data.len() && f::aacDecoder_GetFreeBytes(h, &mut 0) == 0 {}
        }
        let info = &*f::aacDecoder_GetStreamInfo(h);
        eprintln!("fdk: {} Hz x{} aot {} ext {}", info.sampleRate, info.numChannels, info.aot, info.extAot);
        f::aacDecoder_Close(h);
        let _ = ptr::null::<u8>();
    }
    std::fs::write(&a[2], out).unwrap();
}
