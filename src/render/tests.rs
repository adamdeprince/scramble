//! Renderer tests: the bytes it would write are decoded and checked
//! against what a terminal would then hold.

use super::*;
use crate::render::text::{Anchor, Line, Style};
use crate::render::SpriteSource;
use crate::world::level;

fn renderer(vector: bool) -> Renderer {
    let mut r = Renderer::new(RenderConfig {
        vector_scenery: vector,
        uploader: Uploader {
            temp_file: false,
            gamut_p3: false,
        },
        sync_output: true,
        cell_alpha: vector,
        hdr_boost: 1.0,
        sprites: SpriteSource::Painted,
    })
    .unwrap();
    r.set_layout(Layout::new(120, 40, 10, 20, 32, 18));
    r
}

fn scene() -> SceneRef {
    SceneRef {
        key: "meadow".into(),
        level: Arc::new(level::builtin(0).unwrap()),
    }
}

fn hen(x: f32) -> Sprite {
    Sprite {
        key: 7,
        sheet: SheetId::Hen,
        anim: "walk_right".into(),
        frame: 0,
        pos: Vec2::new(x, 10.5),
        layer: Layer::Body,
        z_bias: 0,
    }
}

/// Graphics commands in a byte stream, as their control strings.
fn commands(bytes: &[u8]) -> Vec<String> {
    let s = String::from_utf8_lossy(bytes);
    s.split("\x1b_G")
        .skip(1)
        .map(|c| c.split([';', '\x1b']).next().unwrap_or("").to_string())
        .collect()
}

#[test]
fn first_frame_uploads_then_only_moves() {
    let mut r = renderer(false);
    let mut f = FrameModel {
        scene: Some(scene()),
        sprites: vec![hen(3.5)],
        top: "top".into(),
        bottom: "bottom".into(),
        ..Default::default()
    };
    let first = r.render(&f);
    let cmds = commands(&first);
    let uploads = cmds.iter().filter(|c| c.starts_with("a=t")).count();
    assert_eq!(uploads, 4, "the hen's sheet (only what is on screen), ground, canopy, fences");
    assert!(r.is_uploaded(SheetId::Hen.image_id()) && !r.is_uploaded(SheetId::Hawk.image_id()));
    assert!(r.is_uploaded(IMG_GROUND));
    // Text goes first and the frame is not held back by a synchronized
    // update while big uploads stream in.
    assert!(first.starts_with(b"\x1b[1;1H"), "{:?}", String::from_utf8_lossy(&first[..20]));
    assert!(!first.starts_with(b"\x1b[?2026h"));

    // Nothing changed: nothing is written.
    assert!(r.render(&f).is_empty());

    // The hen moves: exactly one placement, no upload.
    f.sprites = vec![hen(3.7)];
    let second = r.render(&f);
    let cmds = commands(&second);
    assert_eq!(cmds.len(), 1, "{cmds:?}");
    assert!(cmds[0].starts_with(&format!("a=p,i={},p=7,", SheetId::Hen.image_id())));
    assert!(cmds[0].contains("C=1"), "the cursor must not move");
}

#[test]
fn small_resizes_resend_no_images() {
    let mut r = renderer(false);
    let f = FrameModel {
        scene: Some(scene()),
        sprites: vec![hen(3.5)],
        ..Default::default()
    };
    r.render(&f);
    // One column more: the tile grows by under a pixel.
    r.set_layout(Layout::new(121, 40, 10, 20, 32, 18));
    let out = r.render(&f);
    let cmds = commands(&out);
    assert!(!cmds.iter().any(|c| c.starts_with(&format!("a=t,i={}", SheetId::Hen.image_id()))), "same sprite step");
    assert!(!cmds.iter().any(|c| c.starts_with(&format!("a=t,i={IMG_GROUND}"))), "the terminal rescales the lawn");
    assert!(cmds.iter().any(|c| c.starts_with(&format!("a=p,i={IMG_GROUND},p=1")) && c.contains(",c=")));
}

#[test]
fn sprite_steps_fit_the_tile() {
    assert_eq!(sprite_res(96.0), 96);
    assert_eq!(sprite_res(99.0), 96);
    assert_eq!(sprite_res(106.0), 107);
    assert_eq!(sprite_res(10.0), 24);
    assert_eq!(sprite_res(1000.0), 254);
}

#[test]
fn removed_sprites_lose_their_placement() {
    let mut r = renderer(false);
    let mut f = FrameModel {
        scene: Some(scene()),
        sprites: vec![hen(3.5)],
        ..Default::default()
    };
    r.render(&f);
    assert_eq!(r.placements_of(SheetId::Hen.image_id()), 1);
    f.sprites.clear();
    let out = r.render(&f);
    let cmds = commands(&out);
    assert_eq!(cmds, vec![format!("a=d,d=i,i={},p=7,q=2", SheetId::Hen.image_id())]);
    assert_eq!(r.placements_of(SheetId::Hen.image_id()), 0);
}

#[test]
fn resize_rescales_sheets_and_replaces_placements() {
    let mut r = renderer(false);
    let f = FrameModel {
        scene: Some(scene()),
        sprites: vec![hen(3.5)],
        ..Default::default()
    };
    r.render(&f);
    r.set_layout(Layout::new(160, 50, 10, 20, 32, 18));
    let out = r.render(&f);
    let cmds = commands(&out);
    assert!(cmds.iter().any(|c| c.starts_with(&format!("a=t,i={}", SheetId::Hen.image_id()))));
    assert!(cmds.iter().any(|c| c.starts_with(&format!("a=t,i={IMG_GROUND}"))), "raster scenery is redrawn at the new step");
    assert!(cmds.iter().any(|c| c.starts_with(&format!("a=p,i={},p=7", SheetId::Hen.image_id()))));
}

#[test]
fn vector_scenery_is_sent_once_and_survives_resizes() {
    let mut r = renderer(true);
    let f = FrameModel {
        scene: Some(scene()),
        ..Default::default()
    };
    let first = r.render(&f);
    let cmds = commands(&first);
    assert!(cmds.iter().any(|c| c.starts_with(&format!("a=t,f=200,i={IMG_GROUND}"))));
    assert!(cmds.iter().any(|c| c.starts_with(&format!("a=p,i={IMG_GROUND},p=1")) && c.contains(",c=")));
    r.set_layout(Layout::new(160, 50, 10, 20, 32, 18));
    let out = r.render(&f);
    let cmds = commands(&out);
    assert!(!cmds.iter().any(|c| c.starts_with(&format!("a=t,f=200,i={IMG_GROUND}"))), "no retransmission");
    assert!(cmds.iter().any(|c| c.starts_with(&format!("a=p,i={IMG_GROUND},p=1")) && c.contains(",c=")));
}

#[test]
fn fences_are_vector_patches_on_silkie() {
    let mut r = renderer(true);
    let mut f = FrameModel {
        scene: Some(scene()),
        ..Default::default()
    };
    r.render(&f);
    f.fences = vec![(TilePos::new(4, 4), 0)];
    let out = r.render(&f);
    let cmds = commands(&out);
    assert_eq!(cmds.len(), 1, "{cmds:?}");
    assert!(cmds[0].starts_with(&format!("a=e,i={IMG_GROUND}")));
    assert!(r.last.vector_edits >= 4 && r.last.vector_full_redraws == 0);
    f.fences.clear();
    let out = r.render(&f);
    let text = String::from_utf8_lossy(&out).to_string();
    assert!(text.contains("a=e"));
    assert_eq!(r.last.vector_edits, scenery::FENCE_IDS_PER_TILE);
}

#[test]
fn fences_are_atlas_placements_on_kitty() {
    let mut r = renderer(false);
    let mut f = FrameModel {
        scene: Some(scene()),
        ..Default::default()
    };
    r.render(&f);
    f.fences = vec![(TilePos::new(4, 4), 0), (TilePos::new(5, 4), 1)];
    let out = r.render(&f);
    let cmds = commands(&out);
    assert_eq!(cmds.iter().filter(|c| c.starts_with(&format!("a=p,i={IMG_FENCES}"))).count(), 2);
    assert!(!cmds.iter().any(|c| c.starts_with("a=t")), "no upload for a fence");
}

#[test]
fn status_lines_and_overlays_are_diffed() {
    let mut r = renderer(true);
    let mut f = FrameModel {
        top: "Score 10".into(),
        bottom: "Lives 3".into(),
        ..Default::default()
    };
    let a = r.render(&f);
    assert!(String::from_utf8_lossy(&a).contains("Score 10"));
    f.overlays = vec![Overlay::new(Anchor::Center, vec![Line::new("Paused", Style::Heading)])];
    let b = String::from_utf8_lossy(&r.render(&f)).to_string();
    assert!(b.contains("Paused") && !b.contains("Score 10"));
    f.overlays.clear();
    let c = String::from_utf8_lossy(&r.render(&f)).to_string();
    assert!(!c.contains("Paused") && c.contains("  "), "the box is blanked");
}

#[test]
fn teardown_deletes_every_image() {
    let mut r = renderer(false);
    let f = FrameModel {
        scene: Some(scene()),
        sprites: vec![hen(3.5)],
        ..Default::default()
    };
    r.render(&f);
    let out = r.teardown();
    let cmds = commands(&out);
    for id in [SheetId::Hen.image_id(), IMG_GROUND, IMG_CANOPY] {
        assert!(cmds.contains(&format!("a=d,d=I,i={id},q=2")), "{id:#x} deleted");
    }
    assert_eq!(r.placement_count(), 0);
}

#[test]
fn evicted_images_are_sent_again() {
    let mut r = renderer(false);
    let f = FrameModel {
        scene: Some(scene()),
        sprites: vec![hen(3.5)],
        ..Default::default()
    };
    r.render(&f);
    r.on_graphics_error(SheetId::Hen.image_id(), "ENOENT:No such image");
    let out = r.render(&f);
    let cmds = commands(&out);
    assert!(cmds.iter().any(|c| c.starts_with(&format!("a=t,i={}", SheetId::Hen.image_id()))));
}

#[test]
fn hdr_glints_sit_on_sparkles() {
    let mut r = renderer(true);
    r.cfg.hdr_boost = 3.0;
    let sparkle = Sprite {
        key: 9,
        sheet: SheetId::Fx,
        anim: "sparkle".into(),
        frame: 0,
        pos: Vec2::new(5.5, 5.5),
        layer: Layer::Over,
        z_bias: 0,
    };
    let f = FrameModel {
        scene: Some(scene()),
        sprites: vec![hen(3.5), sparkle],
        ..Default::default()
    };
    let cmds = commands(&r.render(&f));
    let upload = cmds
        .iter()
        .find(|c| c.starts_with(&format!("a=t,i={IMG_FX_HDR},f=64")))
        .expect("the glints go as an HDR image");
    assert!(!upload.contains("T=1"), "clipped to the headroom, never compressed: {upload}");
    // The sparkle keeps its ordinary placement, and its glint goes on top;
    // the hen has none.
    assert_eq!(r.placements_of(SheetId::Fx.image_id()), 1);
    assert_eq!(r.placements_of(IMG_FX_HDR), 1);
    let z = |id| r.placed[&(id, 9)].place.z;
    assert_eq!(z(IMG_FX_HDR), z(SheetId::Fx.image_id()) + 1);
}
