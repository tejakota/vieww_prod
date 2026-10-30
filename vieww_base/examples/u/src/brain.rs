//! The 3D neuron: one soma, a six-branch dendritic arbor with instanced spine
//! dots, and an axon ending in three synaptic bulbs — orbited by a slow camera.
//!
//! The scene is built **once** and kept in an `Rc<RefCell<Scene>>`; the only
//! thing that changes per frame is where the camera sits. That is the same
//! split the `80-3d-scene` example settled on, for the same reason: meshes are
//! the expensive part of a software-rasterised scene and a static object graph
//! means they are allocated exactly once.
//!
//! # Why the camera moves and the neuron does not
//!
//! Node rotations are local — `s.node_mut(id).rotation = ..` spins a segment
//! about *its own* centre, which would take the arbor apart limb by limb.
//! Orbiting the camera gives the whole-object turntable for one matrix, and
//! the gentle pitch bob makes it feel held in a hand rather than mounted on
//! an axle.

use std::cell::RefCell;
use std::f32::consts::{PI, TAU};
use std::rc::Rc;

use vieww_3d::{
    geometry, Content, Light, Mat4, Material, Node, OrbitControls, Quat, RenderStats, Renderer,
    Rgb, Scene, Vec3, Viewport3D,
};
use vieww_foundation::Color;

use crate::palette::{AXON, MEMBRANE, SOMA, SYNAPSE};

/// The neuron's private void — matched to the card it sits in so the viewport
/// reads as a window into the cell rather than a picture on it.
const VOID: Rgb = Rgb::new(0.043, 0.063, 0.118);

/// The render target, in physical pixels. Sized to the slot the home screen
/// gives it; `samples(2)` because the whole point of this card is that it looks
/// like tissue, and aliasing reads as wireframe.
const VIEW: (u32, u32) = (348, 252);

/// Build the neuron once. See the module docs for why nothing here animates.
#[must_use]
pub fn scene() -> Scene {
    let mut s = Scene::new();
    s.background = VOID;
    s.fog = Some((VOID, 5.5, 12.0));

    // The soma — the persona's body, in its own colour.
    s.add(
        Node::new(
            "soma",
            Content::mesh(
                geometry::sphere(0.72, 40, 20),
                Material::standard(AXON, 0.28, 0.46),
            ),
        ),
        None,
    );

    let dendrite = Material::lambert(Color::rgb(92, 200, 196));
    let tip = Material::standard(MEMBRANE, 0.1, 0.35);
    let spines: Vec<Mat4> = arbor(&mut s, &dendrite, &tip);

    // Spines: one instanced draw call for every dendritic spine dot.
    s.add(
        Node::new(
            "spines",
            Content::instanced(
                geometry::sphere(0.03, 8, 6),
                Material::lambert(MEMBRANE),
                spines,
            ),
        ),
        None,
    );

    // The axon, leaving the soma's base and ending in three warm bulbs —
    // where U's advice would leave, if it could.
    let axon_mat = Material::standard(SYNAPSE, 0.2, 0.4);
    let start = Vec3::new(0.0, -0.52, 0.0);
    let end = Vec3::new(0.16, -1.85, -0.1);
    segment(&mut s, "axon", start, end, 0.075, 0.045, axon_mat.clone());
    for t in 0..3 {
        #[allow(clippy::cast_precision_loss)]
        let ang = t as f32 / 3.0 * TAU + 0.4;
        let dir = Vec3::new(ang.cos() * 0.8, 0.5, ang.sin() * 0.8).normalize();
        let fork = end + dir * 0.5;
        segment(
            &mut s,
            &format!("terminal-{t}"),
            end,
            fork,
            0.045,
            0.018,
            axon_mat.clone(),
        );
        s.add(
            Node::new(
                &format!("bulb-{t}"),
                Content::mesh(
                    geometry::sphere(0.07, 12, 8),
                    Material::standard(SOMA, 0.1, 0.3),
                ),
            )
            .at(fork),
            None,
        );
    }

    // Light: a cool sky, a warm sun, and one cyan lamp near the soma so the
    // cell's own colour keeps glowing out of the shadows.
    s.add(
        Node::new(
            "sky",
            Content::Light(Light::Hemisphere {
                sky: Rgb::new(0.42, 0.48, 0.72),
                ground: Rgb::new(0.07, 0.09, 0.15),
                intensity: 0.6,
            }),
        ),
        None,
    );
    s.add(
        Node::new(
            "sun",
            Content::Light(Light::Directional {
                color: Rgb::new(1.0, 0.94, 0.86),
                intensity: 1.2,
                direction: Vec3::new(-0.45, -1.0, -0.3),
                shadow: false,
            }),
        ),
        None,
    );
    s.add(
        Node::new(
            "lamp",
            Content::Light(Light::Point {
                color: Rgb::new(0.3, 0.95, 0.88),
                intensity: 1.7,
                range: 6.5,
            }),
        )
        .at(Vec3::new(1.4, 0.9, 1.6)),
        None,
    );

    s
}

/// Grow the dendritic arbor: six main branches, each forking into two, with a
/// growth-cone bulb at every tip and spine dots scattered along the way.
/// Returns the spine transforms for instancing.
#[allow(clippy::cast_precision_loss)]
fn arbor(s: &mut Scene, dendrite: &Material, tip: &Material) -> Vec<Mat4> {
    let mut spines = Vec::new();
    for i in 0..6usize {
        let yaw = i as f32 / 6.0 * TAU;
        let pitch = 0.42 + ((i * 5) % 3) as f32 * 0.24;
        let dir = Vec3::new(
            pitch.cos() * yaw.cos(),
            pitch.sin(),
            pitch.cos() * yaw.sin(),
        )
        .normalize();
        let base = dir * 0.6;
        let elbow = base + dir * 0.78 + Vec3::new(0.0, 0.1, 0.0);
        segment(
            s,
            &format!("branch-{i}"),
            base,
            elbow,
            0.085,
            0.055,
            dendrite.clone(),
        );

        for (c, k) in [0.0f32, 1.0].into_iter().enumerate() {
            let spread = Quat::from_axis_angle(dir, (k - 0.5) * 1.15);
            let mut fork_dir = spread.rotate(dir);
            fork_dir.y += 0.22 - (i % 2) as f32 * 0.42;
            let tip_at = elbow + fork_dir.normalize() * 0.6;
            segment(
                s,
                &format!("twig-{i}-{c}"),
                elbow,
                tip_at,
                0.055,
                0.026,
                dendrite.clone(),
            );
            s.add(
                Node::new(
                    &format!("cone-{i}-{c}"),
                    Content::mesh(geometry::sphere(0.055, 12, 8), tip.clone()),
                )
                .at(tip_at),
                None,
            );
            for step in 1..=3usize {
                let u = step as f32 / 4.0;
                let p = elbow.lerp(tip_at, u);
                let wob = 0.55 + 0.2 * (step % 2) as f32;
                spines.push(Mat4::compose(p, Quat::IDENTITY, Vec3::splat(wob)));
            }
        }
        for step in 1..=2usize {
            let u = step as f32 / 3.0;
            spines.push(Mat4::compose(
                base.lerp(elbow, u),
                Quat::IDENTITY,
                Vec3::splat(0.6),
            ));
        }
    }
    spines
}

/// One tapered segment from `a` to `b`, as a capped cylinder rotated from the
/// local +Y axis onto the travel direction.
///
/// `cylinder(top, bottom, ..)` puts `top` at the +Y end, and the rotation maps
/// +Y onto `dir` — so the radius *at `b` is the top*, which is easy to get
/// backwards and turns a tapering branch into a thickening one.
fn segment(s: &mut Scene, name: &str, a: Vec3, b: Vec3, at_a: f32, at_b: f32, mat: Material) {
    let travel = b - a;
    let len = travel.length();
    let dir = travel.normalize();
    let axis = Vec3::Y.cross(dir);
    let rot = if axis.length() < 1.0e-4 {
        // Parallel to the axis: already aligned, or upside-down.
        if dir.y > 0.0 {
            Quat::IDENTITY
        } else {
            Quat::from_axis_angle(Vec3::X, PI)
        }
    } else {
        Quat::from_axis_angle(axis.normalize(), Vec3::Y.dot(dir).clamp(-1.0, 1.0).acos())
    };
    s.add(
        Node::new(
            name,
            Content::mesh(geometry::cylinder(at_b, at_a, len, 10), mat),
        )
        .at(a.lerp(b, 0.5))
        .rotated(rot),
        None,
    );
}

/// The orbited viewport for a moment in time. Rebuilt every frame by the home
/// screen — the sanctioned pattern from the 3D example, since a `Viewport3D`
/// is a cheap description around an expensive, already-built scene.
#[must_use]
pub fn viewport(
    scene: &Rc<RefCell<Scene>>,
    stats: &Rc<RefCell<RenderStats>>,
    time: f32,
    burst: f32,
) -> Viewport3D {
    let mut orbit = OrbitControls::new(Vec3::new(0.0, -0.2, 0.0), 4.3 - burst * 0.25);
    orbit.yaw = 0.55 + time * 0.22;
    orbit.pitch = 0.36 + 0.09 * (time * 0.35).sin();
    Viewport3D::new(
        scene.clone(),
        orbit.camera(0.85),
        Renderer::new(VIEW.0, VIEW.1).samples(2),
    )
    .report_to(stats.clone())
}
