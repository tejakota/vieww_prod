//! Random graph edits (AU-20): add, connect, disconnect, retarget the
//! output and render, in any order. The graph must refuse cycles and bad
//! ids with errors, never panic, and render finite samples.

#![no_main]

use libfuzzer_sys::fuzz_target;
use vieww_audio::graph::{DelayNode, GainNode, Graph, NodeId, PanNode, SourceNode};
use vieww_audio::synth::osc::{Oscillator, Shape};

fuzz_target!(|data: &[u8]| {
    let mut g = Graph::new(48_000.0);
    let mut ids: Vec<NodeId> = Vec::new();
    let mut out = [0.0f32; 256];
    for op in data.chunks(3).take(200) {
        let pick = |b: u8, ids: &[NodeId]| ids.get(usize::from(b) % ids.len().max(1)).copied();
        match op[0] % 6 {
            0 if ids.len() < 32 => ids.push(g.add(SourceNode(Oscillator::new(
                Shape::Saw,
                55.0 + f32::from(op.get(1).copied().unwrap_or(0)),
                48_000.0,
            )))),
            1 if ids.len() < 32 => ids.push(g.add(GainNode::new(0.5, 2))),
            2 if ids.len() < 32 => ids.push(g.add(DelayNode::new(0.01, 0.05, 2, 48_000.0))),
            3 if ids.len() < 32 => ids.push(g.add(PanNode::new(0.0, 2))),
            4 => {
                if let (Some(a), Some(b)) = (
                    pick(op.get(1).copied().unwrap_or(0), &ids),
                    pick(op.get(2).copied().unwrap_or(0), &ids),
                ) {
                    if op.len() > 2 && op[2] % 2 == 0 {
                        let _ = g.connect(a, b);
                    } else {
                        g.disconnect(a, b);
                    }
                }
            }
            _ => {
                if let Some(a) = pick(op.get(1).copied().unwrap_or(0), &ids) {
                    g.set_output(a);
                }
                g.render_interleaved(&mut out, 2);
                assert!(out.iter().all(|v| v.is_finite()), "a graph rendered a non-finite sample");
            }
        }
    }
});
