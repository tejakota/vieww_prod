//! One row of the parity certificate (FW-26), written by every GPU suite's
//! comparison against the CPU oracle.
//!
//! When `VIEWW_PARITY_OUT` names a directory, each comparison appends a
//! JSON line to `<dir>/<backend>.jsonl`. The line records the test, the
//! contract (interior pixels within the tolerance, edges free), how many
//! interior pixels broke it, and the worst channel delta anywhere.
//! `ci/check/parity-certificate.py` turns the lines into the release's
//! `parity-certificate.json`. Without the variable this does nothing.

use std::io::Write;

/// Append this comparison's row, if a certificate is being collected.
pub(crate) fn record(
    backend: &str,
    tolerance: u8,
    gpu: &[u8],
    cpu: &[u8],
    interior_violations: usize,
) {
    let Some(dir) = std::env::var_os("VIEWW_PARITY_OUT") else {
        return;
    };
    let worst = gpu
        .iter()
        .zip(cpu)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0);
    let test = std::thread::current()
        .name()
        .unwrap_or("unnamed")
        .replace('"', "'");
    let adapter = std::env::var("VIEWW_PARITY_ADAPTER")
        .unwrap_or_default()
        .replace('"', "'");
    let line = format!(
        "{{\"backend\":\"{backend}\",\"adapter\":\"{adapter}\",\"test\":\"{test}\",\"contract\":\"interior<={tolerance}/255,edges-free\",\"tolerance\":{tolerance},\"interior_violations\":{interior_violations},\"worst_delta\":{worst},\"pixels\":{}}}\n",
        gpu.len() / 4
    );
    let dir = std::path::PathBuf::from(dir);
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(format!("{backend}.jsonl")))
    {
        let _ = f.write_all(line.as_bytes());
    }
}
