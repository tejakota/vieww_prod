//! The body itself — a 3D anatomical figure, its organs, a camera rig that
//! flies between them, and the blur that focuses attention.
//!
//! # The look
//!
//! A real anatomical muscle figure, standing in a dark imaging suite. The
//! body is grown from a picture — `assets/body/base.png`, a public-domain
//! rendering of the classical muscle figure — by the framework's
//! image-to-3D layer (`vieww_mesh::relief::silhouette_solid`): the
//! illustration's alpha silhouette becomes a watertight rounded solid, and
//! its shading sculpts the depth into a bas-relief, so the muscle striations
//! the picture shows are the ones the geometry has. The illustration itself
//! is the shell's texture. The organs sit *inside* the shell, hidden at
//! rest; when a search names one, the shell fades toward glass and the
//! organ lights up while the camera flies to it. X-ray as a search result.
//!
//! A tap on the body picks through the live camera onto the shell, and the
//! hit's texture coordinate — which pixel of the illustration was touched —
//! names the region: [`region_at`] is the map from picture to anatomy, the
//! same coordinates the mesh grew from, so what a tap *looks like* it hit
//! is what it hit.
//!
//! # The performance contract
//!
//! The renderer runs at a fixed low internal resolution (a fraction of the
//! viewport, see [`RENDER_SCALE`]) and the `Image` widget scales it up —
//! the softness reads as depth of field on a phone-sized screen, and the
//! software rasterizer holds its frame budget. The blur is a separable box
//! pass over that same small buffer, with a downsample step for large
//! radii, so its cost is bounded no matter how dreamy the idle state is.
//! Nothing here renders while the rig is at rest: a still body costs zero
//! frames, and the idle sway is paused whenever an organ is targeted.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use vieww::animation::{SpringAnimation, SpringPreset, SpringSpec, Ticker};
use vieww::element::Signal;
use vieww::foundation::{Color, Image};
use vieww_3d::{
    geometry, raycast, Content, Light, Material, Node, NodeId, Quat, RenderStats, Renderer, Rgb,
    Scene, Texture,
};

use crate::db::Db;

/// The 3D vocabulary the screens use, re-exported so a screen never needs
/// to depend on `vieww_3d` directly — the studio renders a screen with the
/// project's own rlib, and the fewer direct extern crates a screen names,
/// the simpler that contract is.
pub use vieww_3d::{Camera, Vec3};

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------

/// The imaging-suite ground the body floats in.
pub const GROUND: Rgb = Rgb::new(0.043, 0.055, 0.082);
/// Bone.
const BONE: Color = Color::rgb(219, 212, 188);
/// The accent the selected organ glows in (clinical teal).
pub const ACCENT: Rgb = Rgb::new(0.35, 0.92, 0.82);

/// How much of the viewport's logical size the 3D buffer targets. Lower is
/// faster and softer; 0.5 holds 60fps on mid-range phones with margin.
pub const RENDER_SCALE: f32 = 0.5;
/// The widest the internal buffer may be, in pixels, whatever the screen.
pub const MAX_INTERNAL_WIDTH: u32 = 540;
/// The idle blur radius, in internal pixels.
const IDLE_BLUR: f32 = 6.0;

// ---------------------------------------------------------------------------
// Materials
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// The body: a picture, grown into geometry
// ---------------------------------------------------------------------------

/// The base figure: a public-domain rendering of the classical anatomical
/// muscle figure, front view, arms outstretched, cleaned onto transparency
/// and cropped tight. The 3D body grows from this image.
const BODY_BASE_PNG: &[u8] = include_bytes!("../assets/body/base.png");

/// The body's height in world units. The organ anchors in the database are
/// expressed against this same height, so head, heart and heels all agree.
const BODY_HEIGHT: f32 = 1.72;

/// Build the body: the silhouette extrusion of the base illustration.
///
/// `size` is the figure's height; `depth` is the half-thickness at the
/// torso's heart; `sculpt` hands the picture's shading the depth, which is
/// what turns the extrusion from a pillow into a muscle figure — highlights
/// (the muscle bellies) stand proud, the dark separations between them
/// recede. One call at startup, ~12k triangles, cached in the scene.
fn build_body() -> (vieww_mesh::Mesh, Material) {
    let decoded = vieww_image::codec::decode(BODY_BASE_PNG)
        .ok()
        .map(|d| d.into_image())
        .expect("the base figure is a PNG the framework decodes");
    let spec = vieww_mesh::relief::SolidSpec {
        size: BODY_HEIGHT,
        depth: 0.115,
        // 128: ~13k triangles (192 was ~28k). At rest the body is blurred,
        // in a close-up it is see-through; the extra density was invisible.
        resolution: 128,
        smoothing: 2,
        sculpt: 0.62,
    };
    let mesh = vieww_mesh::relief::silhouette_solid(&spec, &decoded);
    let mut material = Material::lambert(Color::rgb(232, 226, 218));
    material.map = Some(Texture::from_image(&decoded));
    (mesh, material)
}

/// The anatomy a point on the base illustration belongs to.
///
/// `u` runs 0 (the figure's left) to 1, `v` from the top of the frame to
/// the bottom — the mesh's own UV convention, so a ray-pick's UV answers
/// here directly. The bands were measured off the illustration itself
/// (head to ~0.115, shoulders to ~0.20, the outstretched arms to ~0.55,
/// the legs below), and every key is an organ the database knows.
#[must_use]
pub fn region_at(u: f32, v: f32) -> &'static str {
    // 0 at the figure's spine, 1 at its fingertips, for side decisions.
    let side = ((u - 0.5) / 0.475).abs();
    if v < 0.055 {
        // The crown of the skull.
        "brain"
    } else if v < 0.115 {
        // The face: eyes wide, mouth low, ears to the sides, brain around.
        if v > 0.06 && v < 0.085 && side < 0.14 {
            "eyes"
        } else if v >= 0.085 && v < 0.105 && side < 0.11 {
            "mouth"
        } else if v > 0.06 && v < 0.10 && side > 0.16 && side < 0.27 {
            "ears"
        } else {
            "head"
        }
    } else if v < 0.15 {
        "thyroid"
    } else if v < 0.20 {
        if side > 0.5 {
            "muscles"
        } else {
            "lungs"
        }
    } else if v < 0.42 {
        if side > 0.60 {
            "muscles" // the outstretched upper arm
        } else if v < 0.31 {
            "heart"
        } else {
            "lungs"
        }
    } else if v < 0.55 {
        if side > 0.60 {
            "muscles" // forearm to hand
        } else if v < 0.48 {
            "stomach"
        } else {
            "intestines"
        }
    } else if v < 0.65 {
        if side > 0.62 {
            "skin"
        } else {
            "reproductive"
        }
    } else {
        // Thigh, calf, foot — the long bones and their joints.
        "bones"
    }
}

/// An organ material: translucent body + soft emissive of its own hue.
fn organ_mat(color: Color, glow: f32) -> Material {
    let mut m = Material::lambert(color);
    let base = Rgb::from_color(color);
    m.emissive = base.scale(glow);
    m.opacity = 0.96;
    m
}

// ---------------------------------------------------------------------------
// The scene
// ---------------------------------------------------------------------------

/// Everything the rig needs to address after the scene is built.
pub struct Parts {
    /// The body shell — grown from the base illustration. A `Vec` because
    /// the pose pass is written over lists, and a future layer (a back
    /// view, a second figure) slots in without a reshape.
    body: Vec<NodeId>,
    organs: Vec<NodeId>,
    /// Each organ node's resting colour and glow, parallel to `organs`. The
    /// selection tints a node with the accent; leaving it restores these.
    organ_rest: Vec<(Rgb, f32)>,
    pub marker_ring: NodeId,
    marker_dot: NodeId,
    glow: NodeId,
}

/// One organ's 3D placement. `placements[i]` belongs to `db.organs[i]` —
/// the same index the app's `selected` signal holds, so a selection names
/// the meshes it lights, never a neighbour's.
#[derive(Debug, Clone)]
pub struct Placement {
    /// The organ's meshes: none for a region ("skin"), two for a pair
    /// (lungs, kidneys).
    pub nodes: Vec<NodeId>,
    /// Where the camera flies to: the centre of the meshes.
    pub anchor: Vec3,
    pub zoom: f32,
    /// Bounding radius around `anchor`, world units — what the camera frames.
    pub extent: f32,
}

/// The organ pictures, cut out of their illustrations by
/// `tools/cut_organs.py` (alpha = the organ alone), each grown into a solid
/// at startup by the same image-to-3D builder as the body. Key, PNG, long
/// axis in world units (the organ's real size against the 1.72 figure).
const GROWN: &[(&str, &[u8], f32)] = &[
    (
        "heart",
        include_bytes!("../assets/organs_cut/heart.png"),
        0.15,
    ),
    (
        "liver",
        include_bytes!("../assets/organs_cut/liver.png"),
        0.22,
    ),
    (
        "stomach",
        include_bytes!("../assets/organs_cut/stomach.png"),
        0.14,
    ),
    (
        "kidneys",
        include_bytes!("../assets/organs_cut/kidneys.png"),
        0.11,
    ),
    (
        "brain",
        include_bytes!("../assets/organs_cut/brain.png"),
        0.17,
    ),
    (
        "pancreas",
        include_bytes!("../assets/organs_cut/pancreas.png"),
        0.15,
    ),
    (
        "liver_gall",
        include_bytes!("../assets/organs_cut/liver_gall.png"),
        0.06,
    ),
    (
        "intestines",
        include_bytes!("../assets/organs_cut/intestines.png"),
        0.24,
    ),
    (
        "spine",
        include_bytes!("../assets/organs_cut/spine.png"),
        0.55,
    ),
    (
        "spleen",
        include_bytes!("../assets/organs_cut/spleen.png"),
        0.11,
    ),
];

/// Grow an organ from its picture: the silhouette becomes a rounded solid,
/// the shading sculpts it, and the picture is its skin. `None` when the
/// organ has no cut picture (it keeps its procedural shape).
fn grown_organ(key: &str, glow: f32) -> Option<(vieww_mesh::Mesh, Material, f32)> {
    let &(_, png, size) = GROWN.iter().find(|(k, _, _)| *k == key)?;
    let picture = vieww_image::codec::decode(png).ok()?.into_image();
    let spec = vieww_mesh::relief::SolidSpec {
        size,
        depth: size * 0.2,
        // 32 cells on the long axis: ~2k triangles an organ. 64 was ~7.5k,
        // and twelve organ meshes at that density tripled the body's
        // geometry — the per-frame cost a phone pays whatever the output
        // resolution, because every vertex is transformed every frame.
        resolution: 32,
        smoothing: 2,
        sculpt: 0.45,
    };
    let mesh = vieww_mesh::relief::silhouette_solid(&spec, &picture);
    let mut material = Material::lambert(Color::rgb(255, 255, 255));
    material.map = Some(Texture::from_image(&picture));
    material.emissive = Rgb::new(1.0, 1.0, 1.0).scale(glow * 0.35);
    material.opacity = 0.96;
    Some((mesh, material, size * 0.5))
}

/// Build the anatomical scene: flesh, muscles, organs, lights, marker.
fn build_scene(db: &Db) -> (Scene, Parts, Vec<Placement>) {
    let mut s = Scene::new();
    s.background = GROUND;
    // A faint fog keeps the figure grounded without a floor plane.
    s.fog = Some((GROUND, 6.0, 16.0));

    let mut parts = Parts {
        body: Vec::new(),
        organs: Vec::new(),
        organ_rest: Vec::new(),
        marker_ring: NodeId(0),
        marker_dot: NodeId(0),
        glow: NodeId(0),
    };

    // --- lights: a cool key from upper left, a warm rim from behind, a
    // hemisphere fill. Shadowless — shadows double the geometry cost and
    // the x-ray reveal reads better without them.
    s.add(
        Node::new(
            "key",
            Content::Light(Light::Directional {
                color: Rgb::new(1.0, 0.97, 0.92),
                intensity: 1.6,
                direction: Vec3::new(-0.45, -0.85, -0.5),
                shadow: false,
            }),
        ),
        None,
    );
    s.add(
        Node::new(
            "rim",
            Content::Light(Light::Directional {
                color: Rgb::new(0.45, 0.65, 0.75),
                intensity: 1.05,
                direction: Vec3::new(0.35, -0.35, 0.85),
                shadow: false,
            }),
        ),
        None,
    );
    s.add(
        Node::new(
            "fill",
            Content::Light(Light::Hemisphere {
                sky: Rgb::new(0.30, 0.36, 0.48),
                ground: Rgb::new(0.16, 0.10, 0.09),
                intensity: 0.65,
            }),
        ),
        None,
    );

    let ellipsoid = |r: f32| geometry::sphere(r, 22, 14);
    let tube = |r: f32, h: f32| geometry::cylinder(r, r, h, 18);

    // --- the body shell ----------------------------------------------------
    // Grown from the base illustration: one watertight solid, textured
    // with the picture it grew from. The node is centered at half the
    // figure's height so the heels land on y = 0, matching every organ
    // anchor in the database.
    let (body_mesh, body_material) = build_body();
    let body = s.add(
        Node::new("body", Content::mesh(body_mesh, body_material)).at(Vec3::new(
            0.0,
            BODY_HEIGHT * 0.5,
            0.0,
        )),
        None,
    );
    parts.body.push(body);

    // --- organ layer -------------------------------------------------------
    // Staged by key first, then laid out in the database's organ order.
    let mut staged: Vec<(&'static str, NodeId, Vec3, f32)> = Vec::new();
    let organ_at = |s: &mut Scene,
                    parts: &mut Parts,
                    staged: &mut Vec<(&'static str, NodeId, Vec3, f32)>,
                    key: &'static str,
                    mesh: vieww_mesh::Mesh,
                    at: Vec3,
                    scale: Vec3,
                    color: Color,
                    glow: f32,
                    mirror: bool| {
        // The organ's picture, grown into geometry, when it has one; the
        // procedural shape otherwise. A grown organ keeps its picture's own
        // proportions (no non-uniform scale), and a mirrored twin (the
        // second kidney) is the same solid turned to face the other way.
        let (mesh, mat, scale, extent) = match grown_organ(key, glow) {
            Some((m, mat, half)) => (m, mat, Vec3::ONE, half),
            None => {
                let r = mesh.bounds().map_or(0.05, |(lo, hi)| {
                    let d = Vec3::new(hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]);
                    0.5 * (d.x * scale.x).max(d.y * scale.y).max(d.z * scale.z)
                });
                (mesh, organ_mat(color, glow), scale, r)
            }
        };
        let mut node = Node::new(key, Content::mesh(mesh, mat))
            .at(at)
            .scaled(scale);
        if mirror {
            node.rotation = Quat::from_axis_angle(Vec3::Y, std::f32::consts::PI);
        }
        let id = s.add(node, None);
        parts.organs.push(id);
        parts.organ_rest.push((Rgb::from_color(color), glow));
        staged.push((key, id, at, extent));
    };

    // The organs the corpus zooms to. Keys match the database organ keys.
    // Placements are tuned to the picture-grown shell's proportions — the
    // figure's torso is narrower than a sphere-sculpted one, so every
    // organ sits inside the silhouette the illustration draws.
    let st = &mut staged;
    organ_at(
        &mut s,
        &mut parts,
        st,
        "brain",
        ellipsoid(0.082),
        Vec3::new(0.0, 1.635, -0.015),
        Vec3::new(0.50, 0.44, 0.6),
        Color::rgb(230, 158, 153),
        0.25,
        false,
    );
    organ_at(
        &mut s,
        &mut parts,
        st,
        "heart",
        ellipsoid(0.062),
        Vec3::new(0.024, 1.185, 0.05),
        Vec3::new(0.9, 1.1, 0.8),
        Color::rgb(217, 89, 82),
        0.75,
        false,
    );
    for side in [-1.0_f32, 1.0] {
        organ_at(
            &mut s,
            &mut parts,
            st,
            "lungs",
            ellipsoid(0.095),
            Vec3::new(0.072 * side, 1.235, -0.012),
            Vec3::new(0.62, 1.42, 0.7),
            Color::rgb(224, 168, 168),
            0.5,
            false,
        );
    }
    organ_at(
        &mut s,
        &mut parts,
        st,
        "liver",
        ellipsoid(0.10),
        Vec3::new(0.055, 1.06, 0.03),
        Vec3::new(1.05, 0.68, 0.68),
        Color::rgb(148, 56, 46),
        0.5,
        false,
    );
    organ_at(
        &mut s,
        &mut parts,
        st,
        "stomach",
        ellipsoid(0.06),
        Vec3::new(-0.045, 1.08, 0.035),
        Vec3::new(0.95, 0.9, 0.72),
        Color::rgb(209, 128, 122),
        0.5,
        false,
    );
    organ_at(
        &mut s,
        &mut parts,
        st,
        "pancreas",
        ellipsoid(0.03),
        Vec3::new(-0.005, 1.03, -0.005),
        Vec3::new(2.4, 0.75, 0.75),
        Color::rgb(230, 168, 86),
        0.55,
        false,
    );
    organ_at(
        &mut s,
        &mut parts,
        st,
        "liver_gall",
        ellipsoid(0.022),
        Vec3::new(0.082, 1.0, 0.045),
        Vec3::new(0.95, 1.3, 0.85),
        Color::rgb(133, 199, 133),
        0.6,
        false,
    );
    organ_at(
        &mut s,
        &mut parts,
        st,
        "spleen",
        ellipsoid(0.045),
        Vec3::new(-0.082, 1.09, 0.0),
        Vec3::new(0.7, 1.15, 0.7),
        Color::rgb(184, 122, 169),
        0.5,
        false,
    );
    for side in [-1.0_f32, 1.0] {
        organ_at(
            &mut s,
            &mut parts,
            st,
            "kidneys",
            ellipsoid(0.045),
            Vec3::new(0.072 * side, 1.06, -0.072),
            Vec3::new(0.72, 1.25, 0.78),
            Color::rgb(179, 61, 61),
            0.55,
            side > 0.0,
        );
    }
    organ_at(
        &mut s,
        &mut parts,
        st,
        "intestines",
        ellipsoid(0.088),
        Vec3::new(0.0, 0.94, 0.025),
        Vec3::new(1.0, 0.82, 0.6),
        Color::rgb(217, 153, 107),
        0.45,
        false,
    );
    organ_at(
        &mut s,
        &mut parts,
        st,
        "bladder",
        ellipsoid(0.042),
        Vec3::new(0.0, 0.72, 0.042),
        Vec3::ONE,
        Color::rgb(230, 168, 86),
        0.5,
        false,
    );
    organ_at(
        &mut s,
        &mut parts,
        st,
        "thyroid",
        ellipsoid(0.02),
        Vec3::new(0.0, 1.478, 0.042),
        Vec3::new(1.7, 0.8, 0.8),
        Color::rgb(217, 89, 82),
        0.6,
        false,
    );
    organ_at(
        &mut s,
        &mut parts,
        st,
        "spine",
        tube(0.024, 0.52),
        Vec3::new(0.0, 1.15, -0.085),
        Vec3::ONE,
        BONE,
        0.35,
        false,
    );
    organ_at(
        &mut s,
        &mut parts,
        st,
        "reproductive",
        ellipsoid(0.048),
        Vec3::new(0.0, 0.715, 0.0),
        Vec3::ONE,
        Color::rgb(184, 122, 169),
        0.45,
        false,
    );

    // Regions without their own mesh zoom to an anchor point.
    let anchors: &[(&str, Vec3, f32)] = &[
        ("skin", Vec3::new(0.0, 1.05, 0.16), 1.55),
        ("muscles", Vec3::new(0.0, 1.10, 0.15), 1.15),
        ("bones", Vec3::new(0.0, 0.95, 0.0), 1.5),
        ("blood", Vec3::new(0.028, 1.20, 0.055), 0.5),
        ("nerves", Vec3::new(0.0, 1.30, -0.03), 0.85),
        ("head", Vec3::new(0.0, 1.56, 0.05), 0.6),
        ("mouth", Vec3::new(0.0, 1.52, 0.095), 0.4),
        ("ears", Vec3::new(0.0, 1.56, 0.0), 0.5),
        ("eyes", Vec3::new(0.0, 1.585, 0.10), 0.38),
        ("whole", Vec3::new(0.0, 1.02, 0.0), 1.45),
    ];

    // Lay out by the database's organ index — `placements[i]` IS organ i.
    let placements: Vec<Placement> = db
        .organs
        .iter()
        .map(|o| {
            let mine: Vec<&(&str, NodeId, Vec3, f32)> =
                staged.iter().filter(|m| m.0 == o.key).collect();
            if !mine.is_empty() {
                let n = mine.len() as f32;
                let anchor = mine.iter().fold(Vec3::ZERO, |a, m| a + m.2) * (1.0 / n);
                let extent = mine
                    .iter()
                    .map(|m| (m.2 - anchor).length() + m.3)
                    .fold(0.0_f32, f32::max);
                Placement {
                    nodes: mine.iter().map(|m| m.1).collect(),
                    anchor,
                    zoom: o.zoom,
                    extent,
                }
            } else {
                // A region with no mesh of its own (skin, blood, the whole
                // body): framed as wide as its tuned zoom always flew.
                let (at, zoom) = anchors.iter().find(|a| a.0 == o.key).map_or(
                    (Vec3::new(o.anchor[0], o.anchor[1], o.anchor[2]), o.zoom),
                    |&(_, at, zoom)| (at, zoom),
                );
                Placement {
                    nodes: Vec::new(),
                    anchor: at,
                    zoom,
                    extent: extent_for_distance(1.35 + zoom * 2.2),
                }
            }
        })
        .collect();

    // --- the marker: a ring, a dot and a halo, parked and invisible until
    // a selection exists.
    let mut ring_mat = Material::basic(Color::rgb(89, 235, 209));
    ring_mat.emissive = ACCENT.scale(1.6);
    ring_mat.opacity = 0.0;
    parts.marker_ring = s.add(
        Node::new(
            "marker-ring",
            Content::mesh(geometry::torus(0.055, 0.008, 8, 40), ring_mat),
        )
        .at(Vec3::new(0.0, 1.0, 0.2)),
        None,
    );
    let mut dot_mat = Material::basic(Color::rgb(89, 235, 209));
    dot_mat.emissive = ACCENT;
    dot_mat.opacity = 0.0;
    parts.marker_dot = s.add(
        Node::new(
            "marker-dot",
            Content::mesh(geometry::sphere(0.012, 12, 8), dot_mat),
        )
        .at(Vec3::new(0.0, 1.0, 0.2)),
        None,
    );
    let mut glow_mat = Material::basic(Color::rgb(89, 235, 209));
    glow_mat.opacity = 0.0;
    parts.glow = s.add(
        Node::new(
            "marker-glow",
            Content::mesh(geometry::sphere(0.10, 16, 12), glow_mat),
        )
        .at(Vec3::new(0.0, 1.0, 0.2)),
        None,
    );

    (s, parts, placements)
}

// ---------------------------------------------------------------------------
// The rig
// ---------------------------------------------------------------------------

/// The camera's resting framing: the whole figure, slightly below eye line.
const REST_TARGET: Vec3 = Vec3::new(0.0, 1.06, 0.0);
const REST_DIST: f32 = 4.05;
const REST_YAW: f32 = 0.0;
const REST_PITCH: f32 = 0.075;
/// Field of view (vertical, radians) — a mild portrait lens.
const FOV: f32 = 0.62;

/// A soft critically damped spring: no overshoot, ~450ms glide. "Flowy."
const FLOW: SpringSpec = SpringSpec {
    stiffness: 105.0,
    damping_ratio: 1.0,
    initial_velocity: 0.0,
};

/// The live pose the UI reads every animated frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub camera: Camera,
    /// 0 = crisp, 1 = fully dreamy blur.
    pub blur: f32,
    /// 0 = opaque shell, 1 = x-ray.
    pub xray: f32,
    /// 0 = collapsed bottom sheet, 1 = expanded.
    pub panel: f32,
    /// 0 = the search floats low, 1 = it rides high — the inverse of the
    /// target organ's height, so the answer and the box never fight for
    /// the same pixels. `run_search` sets it; the spring glides it.
    pub pill: f32,
}

/// Drives every animated value with springs and publishes the whole pose as
/// one signal, so the tree rebuilds once per frame rather than once per
/// spring.
pub struct Rig {
    target_x: SpringAnimation,
    target_y: SpringAnimation,
    target_z: SpringAnimation,
    dist: SpringAnimation,
    yaw: SpringAnimation,
    pitch: SpringAnimation,
    blur: SpringAnimation,
    xray: SpringAnimation,
    panel: SpringAnimation,
    pill: SpringAnimation,
    last: Option<Duration>,
    /// When the person last did something (typed, tapped, dragged). The
    /// idle sway runs for [`SWAY_SECS`] after it, then fades out and the
    /// body holds still: a resting screen costs zero frames.
    last_input: Option<Duration>,
    /// Set by an input between ticks; the next tick stamps `last_input`.
    input_pending: bool,
    /// The person has taken the camera (orbit, zoom, pan). No sway while
    /// they hold it; a programmatic flight hands it back.
    user_camera: bool,
    /// Where the framed organ should sit on screen: -1 low, 0 centre, +1
    /// high — the opposite end from the answer card, so they never overlap.
    bias: SpringAnimation,
    /// The last flight's organ framing, for "recenter".
    focus_target: Option<(Vec3, f32)>,
    pose: Signal<Pose>,
    selected: Signal<Option<usize>>,
}

/// How long the resting body sways after the last input, seconds.
const SWAY_SECS: f32 = 8.0;
/// Camera distance limits for the person's zoom, world units.
const MIN_DIST: f32 = 0.35;
const MAX_DIST: f32 = 6.0;
/// The closest a flight frames anything: a tiny organ (the gallbladder)
/// still shows a little of what surrounds it.
const MIN_FRAME: f32 = 0.55;
/// Pitch limit, radians (about 63 degrees up or down).
const MAX_PITCH: f32 = 1.1;

impl Rig {
    /// Build the scene and the rig together. The signals are created by the
    /// app (`Runtime::signal`); the rig writes them, the screen reads them.
    #[must_use]
    pub fn new(
        db: &Db,
        pose: Signal<Pose>,
        selected: Signal<Option<usize>>,
    ) -> (Rc<RefCell<Scene>>, Parts, Vec<Placement>, Rc<RefCell<Rig>>) {
        let (scene, parts, placements) = build_scene(db);
        let rig = Rig {
            target_x: SpringAnimation::with_spec(REST_TARGET.x, FLOW),
            target_y: SpringAnimation::with_spec(REST_TARGET.y, FLOW),
            target_z: SpringAnimation::with_spec(REST_TARGET.z, FLOW),
            dist: SpringAnimation::with_spec(REST_DIST, FLOW),
            yaw: SpringAnimation::with_spec(REST_YAW, FLOW),
            pitch: SpringAnimation::with_spec(REST_PITCH, FLOW),
            blur: SpringAnimation::with_spec(1.0, FLOW),
            xray: SpringAnimation::with_spec(0.0, FLOW),
            panel: SpringAnimation::with_spec(0.0, FLOW),
            pill: SpringAnimation::with_spec(0.0, FLOW),
            last: None,
            last_input: None,
            input_pending: true,
            user_camera: false,
            bias: SpringAnimation::with_spec(0.0, FLOW),
            focus_target: None,
            pose,
            selected,
        };
        let _ = SpringPreset::Custom(FLOW);
        (
            Rc::new(RefCell::new(scene)),
            parts,
            placements,
            Rc::new(RefCell::new(rig)),
        )
    }

    /// Fly to the rest pose: whole body, blurred, flesh opaque.
    pub fn rest(&mut self) {
        self.selected.set(None);
        self.user_camera = false;
        self.focus_target = None;
        self.bias.retarget(0.0);
        self.retarget_camera(REST_TARGET, REST_DIST, REST_YAW, REST_PITCH);
        self.blur.retarget(1.0);
        self.xray.retarget(0.0);
        self.poke();
    }

    /// Fly to an organ: near, crisp, flesh fading to glass. `extent` is the
    /// organ's bounding radius, so a large organ is framed whole and a small
    /// one fills the view; `bias` places it away from the answer card.
    pub fn focus(&mut self, anchor: Vec3, _zoom: f32, extent: f32, bias: f32) {
        let dist = frame_distance(extent).clamp(MIN_FRAME, MAX_DIST);
        let target = anchor + Vec3::new(0.0, 0.01, 0.0);
        // A three-quarter view reads as 3D; straight-on reads as a diagram.
        // If the person has turned the body to a side, keep their side.
        let yaw = self.keep_side(0.42);
        self.user_camera = false;
        self.focus_target = Some((target, dist));
        self.bias.retarget(bias.clamp(-1.0, 1.0));
        self.retarget_camera(target, dist, yaw, 0.10);
        self.blur.retarget(0.0);
        self.xray.retarget(1.0);
        self.poke();
    }

    /// A mid-state while typing: the camera already gliding to the leading
    /// hit's organ, the blur easing off in proportion to match confidence,
    /// the flesh barely ghosting yet.
    pub fn preview(&mut self, anchor: Vec3, _zoom: f32, extent: f32, strength: f32, bias: f32) {
        let strength = strength.clamp(0.0, 1.0);
        // While typing, a little wider than the open answer's framing.
        let dist = (frame_distance(extent) * 1.35).clamp(MIN_FRAME, MAX_DIST);
        let yaw = self.keep_side(0.22);
        self.user_camera = false;
        self.focus_target = Some((anchor, dist));
        self.bias.retarget(bias.clamp(-1.0, 1.0) * strength);
        self.retarget_camera(anchor, dist, yaw, 0.08);
        self.blur.retarget(1.0 - strength);
        self.xray.retarget(strength * 0.55);
        self.poke();
    }

    /// The yaw a flight should land on: the preferred three-quarter view,
    /// unless the person has turned the body well away from it, in which
    /// case their side stays (the organ is visible through the x-rayed
    /// shell either way). Unwrapped to the turn nearest the current yaw so
    /// a flight never spins the long way round.
    fn keep_side(&self, preferred: f32) -> f32 {
        let current = self.yaw.target();
        let turns = ((current - preferred) / std::f32::consts::TAU).round();
        let near = preferred + turns * std::f32::consts::TAU;
        if (current - near).abs() > 0.7 {
            current
        } else {
            near
        }
    }

    // --- the person's camera ------------------------------------------------

    /// Orbit by a drag, in logical pixels: horizontal turns the body,
    /// vertical tilts it (clamped short of straight down or up).
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.take_camera();
        let yaw = self.yaw.value() - dx * 0.008;
        let pitch = (self.pitch.value() + dy * 0.006).clamp(-MAX_PITCH, MAX_PITCH);
        self.yaw.jump(yaw);
        self.pitch.jump(pitch);
    }

    /// Zoom by a factor (pinch spread, wheel notch): above 1 moves closer.
    pub fn zoom_by(&mut self, factor: f32) {
        self.take_camera();
        let d = (self.dist.value() / factor.max(0.05)).clamp(MIN_DIST, MAX_DIST);
        self.dist.jump(d);
    }

    /// Pan by a two-finger drag, in logical pixels, across the screen plane.
    /// Clamped to the figure's box so the body cannot be lost off-screen.
    pub fn pan(&mut self, dx: f32, dy: f32) {
        self.take_camera();
        let k = self.dist.value() * 0.0016;
        let yaw = self.yaw.value();
        let right = Vec3::new(yaw.cos(), 0.0, -yaw.sin());
        let x = (self.target_x.value() - right.x * dx * k).clamp(-0.6, 0.6);
        let y = (self.target_y.value() + dy * k).clamp(0.0, 1.8);
        let z = (self.target_z.value() - right.z * dx * k).clamp(-0.4, 0.4);
        self.target_x.jump(x);
        self.target_y.jump(y);
        self.target_z.jump(z);
    }

    /// Animated steps for the on-screen buttons: turn by `angle` radians, or
    /// zoom by `factor`, with the flight's glide rather than a jump.
    pub fn turn_by(&mut self, angle: f32) {
        self.take_camera();
        self.yaw.retarget(self.yaw.target() + angle);
    }

    /// See [`turn_by`](Self::turn_by).
    pub fn step_zoom(&mut self, factor: f32) {
        self.take_camera();
        self.dist
            .retarget((self.dist.target() / factor).clamp(MIN_DIST, MAX_DIST));
    }

    /// Back to the last framing: the open organ, or the whole body.
    pub fn recenter(&mut self) {
        match self.focus_target {
            Some((target, dist)) => {
                let yaw = self.keep_side(0.42);
                self.user_camera = false;
                self.retarget_camera(target, dist, yaw, 0.10);
            }
            None => {
                self.user_camera = false;
                self.retarget_camera(REST_TARGET, REST_DIST, REST_YAW, REST_PITCH);
            }
        }
        self.poke();
    }

    /// Whether the person has moved the camera off the app's framing.
    #[must_use]
    pub fn camera_is_user(&self) -> bool {
        self.user_camera
    }

    fn take_camera(&mut self) {
        // A gesture stops a flight in place: every camera spring lands where
        // it is, so the drag continues from what the person sees.
        if !self.user_camera {
            for s in [
                &mut self.target_x,
                &mut self.target_y,
                &mut self.target_z,
                &mut self.dist,
                &mut self.yaw,
                &mut self.pitch,
            ] {
                let v = s.value();
                s.jump(v);
            }
        }
        self.user_camera = true;
        self.poke();
    }

    /// Note an input: restarts the idle sway's clock.
    pub fn poke(&mut self) {
        self.input_pending = true;
    }

    /// Publish the pose now, outside a tick — a gesture's frame must show the
    /// move it made without waiting for an animation to be scheduled.
    pub fn publish(&mut self) {
        let t = self.last.map_or(0.0, |d| d.as_secs_f32());
        let pose = self.compose(t);
        if self.pose.peek() != pose {
            self.pose.set(pose);
        }
    }

    /// The idle sway's strength right now, 0..=1: full for [`SWAY_SECS`]
    /// after the last input, then a one-second fade to stillness.
    fn sway_envelope(&self, t: f32) -> f32 {
        if self.user_camera || self.selected.peek().is_some() {
            return 0.0;
        }
        let since = self.last_input.map_or(0.0, |d| t - d.as_secs_f32());
        (1.0 - (since - SWAY_SECS)).clamp(0.0, 1.0)
    }

    fn compose(&self, t: f32) -> Pose {
        let env = self.sway_envelope(t);
        let sway = (t * 0.22).sin() * 0.055 * env;
        let breathe = (t * 0.9).sin() * 0.008 * env;
        let dist = self.dist.value();
        // The framing bias: look past the organ so it lands in the middle of
        // the part of the screen the card leaves free. tan(FOV/2) ~ 0.32 of
        // the distance is the half-height at the organ's depth; 0.62 of that
        // puts the organ about a third of the screen off centre.
        let bias = self.bias.value() * dist * 0.32 * 0.62;
        let target = Vec3::new(
            self.target_x.value(),
            self.target_y.value() + breathe - bias,
            self.target_z.value(),
        );
        let yaw = self.yaw.value() + sway;
        let pitch = self.pitch.value();
        let (sy, cy) = yaw.sin_cos();
        let (sp, cp) = pitch.sin_cos();
        let eye = target + Vec3::new(sy * cp, sp, cy * cp) * dist;
        Pose {
            camera: Camera::perspective(eye, target, FOV),
            blur: self.blur.value().clamp(0.0, 1.0),
            xray: self.xray.value().clamp(0.0, 1.0),
            panel: self.panel.value().clamp(0.0, 1.0),
            pill: self.pill.value().clamp(0.0, 1.0),
        }
    }

    fn retarget_camera(&mut self, target: Vec3, dist: f32, yaw: f32, pitch: f32) {
        self.target_x.retarget(target.x);
        self.target_y.retarget(target.y);
        self.target_z.retarget(target.z);
        self.dist.retarget(dist);
        self.yaw.retarget(yaw);
        self.pitch.retarget(pitch);
    }

    /// Open (true) or close (false) the bottom panel.
    pub fn set_panel(&mut self, open: bool) {
        self.panel.retarget(if open { 1.0 } else { 0.0 });
    }

    /// Float the search high (`up` toward 1) or let it rest low (0). The
    /// caller decides from the target organ's height — the rig only owns
    /// the glide.
    pub fn set_pill(&mut self, up: f32) {
        self.pill.retarget(up.clamp(0.0, 1.0));
    }

    fn springs_settled(&self) -> bool {
        [
            self.target_x.is_animating(),
            self.target_y.is_animating(),
            self.target_z.is_animating(),
            self.dist.is_animating(),
            self.yaw.is_animating(),
            self.pitch.is_animating(),
            self.blur.is_animating(),
            self.xray.is_animating(),
            self.panel.is_animating(),
            self.pill.is_animating(),
            self.bias.is_animating(),
        ]
        .iter()
        .all(|a| !*a)
    }
}

/// How much of the view's half-height a framed organ's radius fills: the
/// organ reads clearly and its neighbours stay in view around it.
const FRAME_FILL: f32 = 0.36;

/// The camera distance that frames a sphere of radius `extent` at
/// [`FRAME_FILL`] of the view's half-height.
fn frame_distance(extent: f32) -> f32 {
    // half-height at distance d is d * tan(FOV / 2).
    extent / (FRAME_FILL * (FOV * 0.5).tan())
}

/// The inverse of [`frame_distance`]: the radius a distance frames.
fn extent_for_distance(dist: f32) -> f32 {
    dist * FRAME_FILL * (FOV * 0.5).tan()
}

impl Ticker for Rig {
    fn tick(&mut self, now: Duration) -> bool {
        let t = now.as_secs_f32();
        self.last = Some(now);
        if self.input_pending || self.last_input.is_none() {
            self.last_input = Some(now);
            self.input_pending = false;
        }

        for s in [
            &mut self.target_x,
            &mut self.target_y,
            &mut self.target_z,
            &mut self.dist,
            &mut self.yaw,
            &mut self.pitch,
            &mut self.blur,
            &mut self.xray,
            &mut self.panel,
            &mut self.pill,
            &mut self.bias,
        ] {
            s.tick(now);
        }

        let pose = self.compose(t);
        if self.pose.peek() != pose {
            self.pose.set(pose);
        }
        true
    }

    fn is_animating(&self) -> bool {
        // Frames while a spring settles, or while the idle sway is still
        // inside its window (and its fade). After that the resting body is
        // a still picture: zero frames until the next input.
        if self.input_pending || !self.springs_settled() {
            return true;
        }
        let t = self.last.map_or(0.0, |d| d.as_secs_f32());
        self.sway_envelope(t) > 0.0
    }
}

// ---------------------------------------------------------------------------
// Per-frame application of a pose to the scene
// ---------------------------------------------------------------------------

/// Apply a pose to the scene: flesh/muscle opacity by x-ray amount, organ
/// emissive by selection, marker position and visibility.
pub fn apply_pose(
    scene: &mut Scene,
    parts: &Parts,
    placements: &[Placement],
    pose: &Pose,
    selected: Option<usize>,
) {
    let x = pose.xray;
    let shell_opacity = 1.0 - 0.74 * x;
    // Under an opaque shell the organs cannot be seen: skip them entirely
    // (their triangles are most of the scene), until the x-ray begins.
    let organs_visible = x > 0.02;
    for &id in &parts.organs {
        scene.node_mut(id).visible = organs_visible;
    }
    for &id in &parts.body {
        set_opacity(scene, id, shell_opacity);
    }

    // Organs: brighter with x-ray; the selected one (both halves of a pair)
    // takes the accent, and every other organ is restored to its own hue.
    let lit: &[NodeId] = selected
        .and_then(|sel| placements.get(sel))
        .map_or(&[], |p| p.nodes.as_slice());
    for (slot, &id) in parts.organs.iter().enumerate() {
        let (rest_color, rest_glow) = parts.organ_rest[slot];
        let node = scene.node_mut(id);
        if let Content::Mesh { material, .. } = &mut node.content {
            if let Some(m) = Arc::get_mut(material) {
                let textured = m.map.is_some();
                if lit.contains(&id) {
                    // A picture-grown organ keeps its picture, lifted by a
                    // light accent wash (more would bleach the picture out);
                    // a procedural one takes the accent colour.
                    if textured {
                        m.emissive = ACCENT.scale(0.10 + 0.16 * x);
                    } else {
                        m.color = ACCENT;
                        m.emissive = ACCENT.scale(0.35 + 0.55 * x);
                    }
                } else if textured {
                    m.emissive = Rgb::new(1.0, 1.0, 1.0).scale(rest_glow * (0.12 + 0.3 * x));
                } else {
                    m.color = rest_color;
                    m.emissive = rest_color.scale(0.14 + 0.38 * x);
                }
                m.opacity = (0.28 + 0.6 * x).min(1.0);
            }
        }
    }

    // The marker: at the selected anchor, faded in with the x-ray.
    match selected
        .and_then(|sel| placements.get(sel))
        .map(|p| p.anchor)
    {
        Some(at) => {
            let show = 0.25 + 0.75 * x;
            // An organ with its own mesh is its own marker: it wears the
            // accent, and the callout names it. The ring, dot and halo mark
            // the regions that have no mesh (skin, blood, the whole body).
            let meshless = selected
                .and_then(|sel| placements.get(sel))
                .is_some_and(|p| p.nodes.is_empty());
            let show = if meshless { show } else { 0.0 };
            for id in [parts.marker_ring, parts.marker_dot] {
                scene.node_mut(id).position = at + Vec3::new(0.0, 0.0, 0.03);
                set_opacity(scene, id, show * 0.6);
            }
            scene.node_mut(parts.glow).position = at;
            set_opacity(scene, parts.glow, show * 0.16);
            // Face the ring toward the camera.
            let yaw = (pose.camera.position.x - at.x).atan2(pose.camera.position.z - at.z);
            scene.node_mut(parts.marker_ring).rotation = Quat::from_axis_angle(Vec3::Y, yaw);
        }
        None => {
            for id in [parts.marker_ring, parts.marker_dot, parts.glow] {
                set_opacity(scene, id, 0.0);
            }
        }
    }
}

fn set_opacity(scene: &mut Scene, id: NodeId, opacity: f32) {
    let node = scene.node_mut(id);
    if let Content::Mesh { material, .. } = &mut node.content {
        if let Some(m) = Arc::get_mut(material) {
            m.opacity = opacity;
        }
    }
}

// ---------------------------------------------------------------------------
// The blur
// ---------------------------------------------------------------------------

/// Blur an RGBA8 buffer by `radius` pixels. Two box passes per axis
/// approximate a Gaussian; radii above the threshold blur a half-scale
/// copy instead, which bounds the cost of the dreamy idle state.
fn box_blur_rgba(px: &mut [u8], w: usize, h: usize, r: usize) {
    if w == 0 || h == 0 || r == 0 {
        return;
    }
    let window = 2 * r + 1;
    SCRATCH.with(|cell| {
        let mut tmp = cell.borrow_mut();
        tmp.resize(px.len(), 0);
        blur_passes(px, &mut tmp, w, h, r, window);
    });
}

thread_local! {
    /// The blur's intermediate row buffer, kept between frames: the same
    /// size every frame, so after the first it is never reallocated.
    static SCRATCH: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

fn blur_passes(px: &mut [u8], tmp: &mut [u8], w: usize, h: usize, r: usize, window: usize) {
    // Horizontal pass.
    for y in 0..h {
        let row = y * w;
        let mut sum = [0u32; 4];
        let primed = r.min(w - 1) + 1;
        for i in 0..primed {
            let p = (row + i) * 4;
            for c in 0..4 {
                sum[c] += px[p + c] as u32;
            }
        }
        for c in 0..4 {
            sum[c] += (window as u32 - primed as u32) * px[row * 4 + c] as u32;
        }
        for x in 0..w {
            let out = (row + x) * 4;
            for c in 0..4 {
                tmp[out + c] = (sum[c] / window as u32) as u8;
            }
            let add = (row + (x + r + 1).min(w - 1)) * 4;
            let sub = (row + x.saturating_sub(r)) * 4;
            for c in 0..4 {
                sum[c] = sum[c].saturating_sub(px[sub + c] as u32) + px[add + c] as u32;
            }
        }
    }

    // Vertical pass.
    for x in 0..w {
        let mut sum = [0u32; 4];
        let primed = r.min(h - 1) + 1;
        for i in 0..primed {
            let p = (i * w + x) * 4;
            for c in 0..4 {
                sum[c] += tmp[p + c] as u32;
            }
        }
        for c in 0..4 {
            sum[c] += (window as u32 - primed as u32) * tmp[x * 4 + c] as u32;
        }
        for y in 0..h {
            let out = (y * w + x) * 4;
            for c in 0..4 {
                px[out + c] = (sum[c] / window as u32) as u8;
            }
            let add = ((y + r + 1).min(h - 1) * w + x) * 4;
            let sub = (y.saturating_sub(r) * w + x) * 4;
            for c in 0..4 {
                sum[c] = sum[c].saturating_sub(tmp[sub + c] as u32) + tmp[add + c] as u32;
            }
        }
    }
}

fn downsample(src: &[u8], sw: usize, sh: usize, dst: &mut [u8], dw: usize, dh: usize) {
    for y in 0..dh {
        for x in 0..dw {
            let (x0, y0) = (x * 2, y * 2);
            let x1 = (x0 + 1).min(sw - 1);
            let y1 = (y0 + 1).min(sh - 1);
            for c in 0..4 {
                let a = src[(y0 * sw + x0) * 4 + c] as u32;
                let b = src[(y0 * sw + x1) * 4 + c] as u32;
                let d = src[(y1 * sw + x0) * 4 + c] as u32;
                let e = src[(y1 * sw + x1) * 4 + c] as u32;
                dst[(y * dw + x) * 4 + c] = ((a + b + d + e) / 4) as u8;
            }
        }
    }
}

fn upsample(src: &[u8], sw: usize, sh: usize, dst: &mut [u8], dw: usize, dh: usize) {
    for y in 0..dh {
        let sy = (y * sh / dh.max(1)).min(sh - 1);
        for x in 0..dw {
            let sx = (x * sw / dw.max(1)).min(sw - 1);
            let s = (sy * sw + sx) * 4;
            let d = (y * dw + x) * 4;
            dst[d..d + 4].copy_from_slice(&src[s..s + 4]);
        }
    }
}

/// Render the scene for a pose and apply the blur — the one call the UI
/// makes per animated frame. The blur radius comes from the pose, so the
/// camera flight and the focus pull are the same motion.
#[must_use]
pub fn render_frame(
    scene: &mut Scene,
    parts: &Parts,
    placements: &[Placement],
    pose: &Pose,
    selected: Option<usize>,
    width: u32,
    height: u32,
    stats: Option<&Rc<RefCell<RenderStats>>>,
) -> Image {
    apply_pose(scene, parts, placements, pose, selected);
    let mut renderer = Renderer::new(width.max(2), height.max(2)).samples(1);
    renderer.shadow_size = 0;
    let (image, s) = renderer.render(scene, &pose.camera);
    if let Some(into) = stats {
        *into.borrow_mut() = s;
    }
    if pose.blur > 0.015 {
        let radius = pose.blur * IDLE_BLUR;
        let (w, h) = (image.width() as usize, image.height() as usize);
        let mut px = image.pixels().to_vec();
        let r = radius as usize;
        if r >= 4 && w > 4 && h > 4 {
            // Blur at half scale: a quarter of the pixels, half the radius,
            // then scale back — visually identical past ~4px.
            let (hw, hh) = (w / 2, h / 2);
            let mut small = vec![0u8; hw * hh * 4];
            downsample(&px, w, h, &mut small, hw, hh);
            box_blur_rgba(&mut small, hw, hh, (r / 2).max(1));
            upsample(&small, hw, hh, &mut px, w, h);
        } else {
            box_blur_rgba(&mut px, w, h, r);
        }
        Image::from_rgba8(px, w as u32, h as u32)
    } else {
        image
    }
}

/// The whole pipeline, headless — what the render command and the tests
/// see: a body, posed and photographed, no rig required.
#[must_use]
pub fn still(db: &Db, width: u32, height: u32, focus_organ: Option<&str>) -> Image {
    let (mut scene, parts, placements) = build_scene(db);
    // `placements[i]` belongs to `db.organs[i]`: the key's index is the
    // selection, and its placement is where the meshes actually are.
    let selected = focus_organ.and_then(|key| db.organs.iter().position(|o| o.key == key));
    let anchor = selected.and_then(|i| placements.get(i)).map(|p| p.anchor);
    let pose = match anchor {
        Some(at) => {
            let eye = at + Vec3::new(0.55, 0.12, 0.95);
            Pose {
                camera: Camera::perspective(eye, at, FOV),
                blur: 0.0,
                xray: 1.0,
                panel: 1.0,
                pill: 0.0,
            }
        }
        None => Pose {
            camera: Camera::perspective(Vec3::new(0.0, 1.35, 3.9), REST_TARGET, FOV),
            blur: 0.0,
            xray: 0.0,
            panel: 0.0,
            pill: 0.0,
        },
    };
    render_frame(
        &mut scene,
        &parts,
        &placements,
        &pose,
        selected,
        width,
        height,
        None,
    )
}

/// World point -> viewport fraction, for placing 2D callouts over the body.
#[must_use]
pub fn project(camera: &Camera, width: f32, height: f32, point: Vec3) -> Option<(f32, f32, f32)> {
    raycast::project(camera, width, height, point)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blur_smooths_and_preserves_size() {
        let w = 32_u32;
        let h = 24_u32;
        let mut px = vec![0u8; (w * h * 4) as usize];
        // A white square in the middle on black.
        for y in 8..16 {
            for x in 12..20 {
                let p = ((y * w + x) * 4) as usize;
                px[p..p + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        box_blur_rgba(&mut px, w as usize, h as usize, 3);
        // The corners stay black-ish; the edge of the square is a gradient.
        let corner = px[0];
        assert!(corner < 30, "corner should stay dark, got {corner}");
        let mid = ((10 * w + 16) * 4) as usize;
        assert!(px[mid] > corner, "gradient should reach outside the square");
    }

    #[test]
    fn down_up_roundtrip_keeps_extremes() {
        let (w, h) = (8_usize, 8_usize);
        let src: Vec<u8> = (0..w * h * 4)
            .map(|i| if i % 7 == 0 { 255 } else { 10 })
            .collect();
        let (hw, hh) = (4, 4);
        let mut small = vec![0u8; hw * hh * 4];
        downsample(&src, w, h, &mut small, hw, hh);
        let mut back = vec![0u8; w * h * 4];
        upsample(&small, hw, hh, &mut back, w, h);
        assert!(back.len() == src.len());
    }

    #[test]
    fn every_organ_lights_its_own_meshes() {
        // The selection is an index into `db.organs`; whatever it lights must
        // be named for that organ — both halves of a pair included.
        let db = crate::db::parse();
        let (scene, _parts, placements) = build_scene(&db);
        assert_eq!(placements.len(), db.organs.len(), "one placement per organ");
        let mut meshes = 0;
        for (i, organ) in db.organs.iter().enumerate() {
            for &id in &placements[i].nodes {
                assert_eq!(
                    scene.node(id).name,
                    organ.key,
                    "organ {i} lights a neighbour"
                );
                meshes += 1;
            }
        }
        assert!(meshes >= 16, "the organ layer is placed ({meshes} meshes)");
        let pair = |key: &str| {
            let i = db.organs.iter().position(|o| o.key == key).unwrap();
            placements[i].nodes.len()
        };
        assert_eq!(pair("lungs"), 2);
        assert_eq!(pair("kidneys"), 2);
    }

    #[test]
    fn pictured_organs_grow_from_their_cutouts() {
        for &(key, _, size) in GROWN {
            let (mesh, material, half) = grown_organ(key, 0.5).expect(key);
            assert!(mesh.triangles() > 150, "{key} grew a real solid");
            assert!(material.map.is_some(), "{key} wears its picture");
            assert!((half - size * 0.5).abs() < 1e-6);
        }
    }

    #[test]
    fn regions_keep_their_tuned_distance() {
        assert!((frame_distance(extent_for_distance(2.0)) - 2.0).abs() < 1e-5);
    }

    #[test]
    fn framing_fits_the_organ() {
        // A 0.1-radius organ at the framing distance fills FRAME_FILL of
        // the view's half-height.
        let d = frame_distance(0.1);
        let half_height = d * (FOV * 0.5).tan();
        assert!((0.1 / half_height - FRAME_FILL).abs() < 1e-4);
    }

    #[test]
    fn the_body_builds_from_the_picture() {
        let db = crate::db::parse();
        let (scene, parts, placements) = build_scene(&db);
        // The shell: one node, real volume, every organ placed inside it.
        assert_eq!(parts.body.len(), 1, "the single picture-grown shell");
        assert!(
            placements.len() >= db.organs.len() - 2,
            "every organ region placed"
        );
        assert!(scene.len() > parts.body.len() + parts.organs.len());
        // The region map covers the whole picture with known organs.
        for (u, v) in [
            (0.5_f32, 0.01),
            (0.5, 0.12),
            (0.5, 0.25),
            (0.1, 0.3),
            (0.5, 0.5),
            (0.5, 0.7),
            (0.5, 0.95),
        ] {
            assert!(
                db.organs.iter().any(|o| o.key == region_at(u, v)),
                "region_at({u}, {v}) = {} is not a known organ",
                region_at(u, v)
            );
        }
    }
}
