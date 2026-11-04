//! `he-aac-enc <in.wav> <out> <aot> <bitrate> <adts|asc>` — 16-bit WAV in;
//! `adts` writes an ADTS stream (implicit SBR/PS signalling, as ADTS must),
//! `asc` writes raw access units as `<out>` (u32-BE length-prefixed) plus
//! the explicit AudioSpecificConfig as `<out>.asc`, for `mux_m4a.py`.

use std::ffi::c_void;
use std::ptr;

use fdk_aac_sys as f;

fn wav(path: &str) -> (u32, u32, Vec<i16>) {
    let b = std::fs::read(path).expect("read wav");
    let mut at = 12;
    let (mut ch, mut rate, mut data) = (0, 0, Vec::new());
    while at + 8 <= b.len() {
        let id = &b[at..at + 4];
        let len = u32::from_le_bytes(b[at + 4..at + 8].try_into().unwrap()) as usize;
        let body = &b[at + 8..at + 8 + len];
        if id == b"fmt " {
            ch = u32::from(u16::from_le_bytes([body[2], body[3]]));
            rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
        } else if id == b"data" {
            data = body.chunks_exact(2).map(|s| i16::from_le_bytes([s[0], s[1]])).collect();
        }
        at += 8 + len + (len & 1);
    }
    (ch, rate, data)
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (input, output, aot, bitrate, mode) = (&a[1], &a[2], a[3].parse::<u32>().unwrap(), a[4].parse::<u32>().unwrap(), a[5].as_str());
    let (ch, rate, pcm) = wav(input);
    let adts = mode == "adts";
    unsafe {
        let mut h: f::HANDLE_AACENCODER = ptr::null_mut();
        assert_eq!(f::aacEncOpen(&mut h, 0, ch), f::AACENC_ERROR_AACENC_OK);
        let set = |p, v| assert_eq!(f::aacEncoder_SetParam(h, p, v), f::AACENC_ERROR_AACENC_OK, "param {p} = {v}");
        set(f::AACENC_PARAM_AACENC_AOT, aot);
        set(f::AACENC_PARAM_AACENC_SAMPLERATE, rate);
        set(f::AACENC_PARAM_AACENC_CHANNELMODE, ch);
        set(f::AACENC_PARAM_AACENC_BITRATE, bitrate);
        set(f::AACENC_PARAM_AACENC_TRANSMUX, if adts { 2 } else { 0 });
        // Explicit hierarchical signalling for the MP4 case (the ASC names
        // AOT 5/29 up front); ADTS can only signal implicitly.
        set(f::AACENC_PARAM_AACENC_SIGNALING_MODE, if adts { 0 } else { 2 });
        assert_eq!(f::aacEncEncode(h, ptr::null(), ptr::null(), ptr::null(), ptr::null_mut()), f::AACENC_ERROR_AACENC_OK);
        let mut info: f::AACENC_InfoStruct = std::mem::zeroed();
        assert_eq!(f::aacEncInfo(h, &mut info), f::AACENC_ERROR_AACENC_OK);
        if !adts {
            std::fs::write(format!("{output}.asc"), &info.confBuf[..info.confSize as usize]).unwrap();
        }
        let mut out = Vec::new();
        let mut obuf = vec![0u8; 20_000];
        let mut at = 0usize;
        let step = info.frameLength as usize * ch as usize;
        loop {
            let flushing = at >= pcm.len();
            let n = if flushing { 0 } else { step.min(pcm.len() - at) };
            let mut inp = pcm.as_ptr().add(at) as *mut c_void;
            let (mut iid, mut isz, mut iel) = (f::AACENC_BufferIdentifier_IN_AUDIO_DATA as i32, (n * 2) as i32, 2i32);
            let mut ib = f::AACENC_BufDesc { numBufs: 1, bufs: &mut inp, bufferIdentifiers: &mut iid, bufSizes: &mut isz, bufElSizes: &mut iel };
            let mut op = obuf.as_mut_ptr() as *mut c_void;
            let (mut oid, mut osz, mut oel) = (f::AACENC_BufferIdentifier_OUT_BITSTREAM_DATA as i32, obuf.len() as i32, 1i32);
            let mut ob = f::AACENC_BufDesc { numBufs: 1, bufs: &mut op, bufferIdentifiers: &mut oid, bufSizes: &mut osz, bufElSizes: &mut oel };
            let ia = f::AACENC_InArgs { numInSamples: if flushing { -1 } else { n as i32 }, numAncBytes: 0 };
            let mut oa: f::AACENC_OutArgs = std::mem::zeroed();
            let e = f::aacEncEncode(h, &ib as *const _ as *mut _, &ob as *const _ as *mut _, &ia as *const _ as *mut _, &mut oa);
            let _ = (&mut ib, &mut ob);
            if e == f::AACENC_ERROR_AACENC_ENCODE_EOF {
                break;
            }
            assert_eq!(e, f::AACENC_ERROR_AACENC_OK);
            let bytes = &obuf[..oa.numOutBytes as usize];
            if !bytes.is_empty() {
                if !adts {
                    out.extend((bytes.len() as u32).to_be_bytes());
                }
                out.extend(bytes);
            }
            at += oa.numInSamples as usize;
        }
        std::fs::write(output, out).unwrap();
        eprintln!("aot {aot}: {} Hz x{ch}, frame {} (core delay {}, total {}), asc {:02x?}", rate, info.frameLength, info.nDelayCore, info.nDelay, &info.confBuf[..info.confSize as usize]);
        f::aacEncClose(&mut h);
    }
}
