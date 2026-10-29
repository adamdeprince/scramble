//! The renderer: turns a frame model (sprites, scenery state, status lines,
//! overlays) into terminal output.
//!
//! It keeps a model of what the terminal currently holds (uploaded images,
//! each placement and where it is, the text of each status line) and emits
//! only the difference each frame. Sprites are placements of uploaded
//! sheets, selected by source rectangle, so animation costs a placement
//! update and never a new upload.
//!
//! The garden goes through one of two scenery back ends built from the same
//! vector description: on Silkie it is a retained vector image (`f=200`)
//! patched one element at a time (a gate swinging, a fence going up); on
//! other terminals it is rasterized here once per level and size, with
//! gates and fences as small placements of pre-rendered atlases.

pub mod layout;
pub mod text;
pub mod upload;

use crate::gfx::scenery::{self, Scenery, TILE};
use crate::gfx::sprites::{self, Sheet};
use crate::gfx::vector::{Edit, Element, Scene};
use crate::sim::gate::Gate;
use crate::term::kitty_gfx::{self, EncodeStats, Place, Quiet};
use crate::world::level::Level;
use crate::world::{TilePos, Topology};
use glam::Vec2;
use layout::Layout;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::sync::Arc;
use text::Overlay;
use tiny_skia::{Pixmap, Transform};
use upload::Uploader;

/// Our image ids start here, well clear of small ids other programs use.
pub const IMG_BASE: u32 = 0x2f5c_0000;
pub const IMG_GROUND: u32 = IMG_BASE + 20;
pub const IMG_CANOPY: u32 = IMG_BASE + 21;
pub const IMG_GATES: u32 = IMG_BASE + 22;
pub const IMG_FENCES: u32 = IMG_BASE + 23;
pub const IMG_TITLE: u32 = IMG_BASE + 24;
pub const IMG_POINTER: u32 = IMG_BASE + 25;
/// The white-hot glints of the effects sheet, as an HDR overlay.
pub const IMG_FX_HDR: u32 = IMG_BASE + 26;

// Stacking. Everything sits below INT32_MIN/2 so that cells with a
// background colour (menus) are drawn over the yard, while ordinary cells
// let it show.
pub const Z_GROUND: i32 = -2_000_000_000;
pub const Z_DECAL: i32 = -1_950_000_000;
pub const Z_SPRITE: i32 = -1_900_000_000;
pub const Z_CANOPY: i32 = -1_800_000_000;
pub const Z_OVER: i32 = -1_700_000_000;
pub const Z_TITLE: i32 = -1_650_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SheetId {
    Hen,
    Chick,
    Fox,
    Weasel,
    Hawk,
    Items,
    Fx,
    /// Generated at start-up from the typeface, for score pop-ups.
    Glyphs,
}

impl SheetId {
    pub const ALL: [SheetId; 8] = [
        SheetId::Hen,
        SheetId::Chick,
        SheetId::Fox,
        SheetId::Weasel,
        SheetId::Hawk,
        SheetId::Items,
        SheetId::Fx,
        SheetId::Glyphs,
    ];

    pub fn name(self) -> &'static str {
        match self {
            SheetId::Hen => "hen",
            SheetId::Chick => "chick",
            SheetId::Fox => "fox",
            SheetId::Weasel => "weasel",
            SheetId::Hawk => "hawk",
            SheetId::Items => "items",
            SheetId::Fx => "fx",
            SheetId::Glyphs => "glyphs",
        }
    }

    pub fn image_id(self) -> u32 {
        IMG_BASE + 1 + self as u32
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    /// On the ground, under everybody (items, shadows).
    Decal,
    /// Creatures, sorted by how far down the screen their feet are.
    Body,
    /// Above everything in the yard (emotes, particles, a diving hawk).
    Over,
}

#[derive(Clone, Debug)]
pub struct Sprite {
    /// Stable for as long as the thing exists; the placement id.
    pub key: u32,
    pub sheet: SheetId,
    pub anim: String,
    /// Advances through the animation, wrapping.
    pub frame: usize,
    /// Ground point in the yard.
    pub pos: Vec2,
    pub layer: Layer,
    /// Extra depth, to draw a carried chick in front of its captor.
    pub z_bias: i32,
}

#[derive(Clone, Debug)]
pub struct GateView {
    pub index: usize,
    pub gate: Gate,
    /// Swing quantized to 0..=3 (open to shut).
    pub stage: u8,
}

/// A fence stroke being drawn (or just finished), in yard coordinates.
#[derive(Clone, Debug)]
pub struct StrokeView {
    pub id: u64,
    pub points: Vec<Vec2>,
    /// The terminal is drawing this one itself (Silkie ink).
    pub terminal_ink: bool,
}

#[derive(Clone)]
pub struct SceneRef {
    /// Identifies the level; scenery is rebuilt when it changes.
    pub key: String,
    pub level: Arc<Level>,
}

#[derive(Default)]
pub struct FrameModel {
    pub scene: Option<SceneRef>,
    pub gates: Vec<GateView>,
    pub fences: Vec<(TilePos, u8)>,
    pub sprites: Vec<Sprite>,
    pub strokes: Vec<StrokeView>,
    pub top: String,
    pub bottom: String,
    pub overlays: Vec<Overlay>,
    pub title: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStats {
    pub bytes: usize,
    pub commands: u64,
    pub placements: u32,
    pub deletions: u32,
    pub uploads: u32,
    pub upload_bytes: usize,
    pub vector_edits: u32,
    pub vector_bytes: usize,
    pub vector_full_redraws: u32,
    pub text_bytes: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Totals {
    pub frames: u64,
    pub bytes: u64,
    pub placements: u64,
    pub uploads: u64,
    pub upload_bytes: u64,
    pub vector_edits: u64,
    pub vector_bytes: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct RenderConfig {
    pub vector_scenery: bool,
    pub uploader: Uploader,
    pub sync_output: bool,
    /// Translucent menu backgrounds (Silkie cell alpha); harmless elsewhere.
    pub cell_alpha: bool,
    /// Sparkles brighter than SDR white, by this factor (1 = off).
    pub hdr_boost: f32,
    /// Where sprite bitmaps come from.
    pub sprites: SpriteSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpriteSource {
    /// Painted by the art module at exactly the size needed: crisp, and
    /// compresses as well as the masters.
    Painted,
    /// The PNG sheets in `assets/sprites`, resampled. For trying out
    /// replacement art without touching code.
    Files,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Placed {
    row: u32,
    col: u32,
    place: Place,
}

struct SceneryState {
    key: String,
    scenery: Scenery,
    topo: Topology,
    /// Pixels per tile the raster ground and canopy were made at.
    raster_for: Option<u32>,
    /// Tile size the gate and fence atlases were made for.
    atlas_for: Option<f32>,
    gates_sent: BTreeMap<usize, (u8, bool)>,
    fences_sent: BTreeMap<TilePos, u8>,
    strokes_sent: BTreeMap<u64, usize>,
    gate_count: usize,
}

pub struct Renderer {
    pub cfg: RenderConfig,
    pub layout: Option<Layout>,
    masters: Vec<Sheet>,
    /// Sheets resampled to `sheets_for` pixels per tile, made on first use.
    sheets: Vec<Option<Sheet>>,
    sheets_for: Option<u32>,
    /// A loading banner written into the field's text cells, to clear.
    banner: Option<(u32, u32, u32)>,
    uploaded: BTreeSet<u32>,
    placed: BTreeMap<(u32, u32), Placed>,
    scenery: Option<SceneryState>,
    text_rows: BTreeMap<u32, String>,
    overlay_rects: Vec<(u32, u32, u32, u32)>,
    overlay_text: Vec<String>,
    title: Option<(Layout, u32, u32)>,
    pub last: FrameStats,
    pub totals: Totals,
    /// The latest error the terminal reported for our graphics commands,
    /// and how many there were.
    pub last_error: Option<String>,
    pub error_count: u64,
    /// Images uploaded or deleted this frame: (id, created).
    pub image_events: Vec<(u32, bool)>,
}

/// Gate atlas cell: three tiles square, the gate tile in the middle.
const GATE_CELL_TILES: f32 = 3.0;
const FENCE_CELL_TILES: f32 = 1.5;

/// Sprite resolutions (pixels per tile), about 11% apart. Sprites are drawn
/// at the largest step that fits the tile, so resizing the window a little
/// re-sends nothing; positions stay exact, only sprite size is quantized.
const LADDER: [u32; 22] = [24, 28, 32, 36, 40, 45, 50, 56, 63, 70, 78, 87, 96, 107, 119, 133, 148, 165, 184, 205, 228, 254];

pub fn sprite_res(tile_px: f32) -> u32 {
    let limit = tile_px * 1.04;
    LADDER.iter().copied().rev().find(|&s| s as f32 <= limit).unwrap_or(LADDER[0])
}

/// Raster scenery for a terminal at the far end of a link (no shared
/// files) is sent at this fraction of the sprite resolution and enlarged by
/// the terminal: a softer lawn for a third of the bytes.
const REMOTE_SCENERY_SCALE: f32 = 0.65;

impl Renderer {
    pub fn new(cfg: RenderConfig) -> anyhow::Result<Self> {
        let mut masters = Vec::new();
        if cfg.sprites == SpriteSource::Files {
            let manifest = sprites::manifest();
            for id in SheetId::ALL {
                if id == SheetId::Glyphs {
                    masters.push(crate::gfx::title::glyph_sheet(crate::art::MASTER_TILE_PX));
                    continue;
                }
                let spec = manifest
                    .sheets
                    .iter()
                    .find(|s| s.name == id.name())
                    .ok_or_else(|| anyhow::anyhow!("sprite manifest has no {} sheet", id.name()))?;
                let png = sprites::sheet_png(id.name()).ok_or_else(|| anyhow::anyhow!("no PNG for {}", id.name()))?;
                masters.push(Sheet::decode(spec, png)?);
            }
        }
        Ok(Renderer {
            cfg,
            layout: None,
            sheets: SheetId::ALL.iter().map(|_| None).collect(),
            masters,
            sheets_for: None,
            banner: None,
            uploaded: BTreeSet::new(),
            placed: BTreeMap::new(),
            scenery: None,
            text_rows: BTreeMap::new(),
            overlay_rects: Vec::new(),
            overlay_text: Vec::new(),
            title: None,
            last: FrameStats::default(),
            totals: Totals::default(),
            last_error: None,
            error_count: 0,
            image_events: Vec::new(),
        })
    }

    /// Changes the layout; images sized to the old one are redone lazily.
    pub fn set_layout(&mut self, l: Option<Layout>) {
        self.layout = l;
    }

    /// Forgets everything the terminal held (after a suspend, a terminal
    /// reset, or a resize that cleared the screen): it is all sent again.
    pub fn invalidate(&mut self) {
        self.uploaded.clear();
        self.placed.clear();
        self.sheets_for = None;
        self.text_rows.clear();
        self.overlay_rects.clear();
        self.overlay_text.clear();
        self.title = None;
        if let Some(s) = &mut self.scenery {
            s.raster_for = None;
            s.atlas_for = None;
            s.gates_sent.clear();
            s.fences_sent.clear();
            s.strokes_sent.clear();
        }
        self.scenery = None;
    }

    /// The terminal reported an error for one of our images (for example
    /// evicted under storage pressure): send it again.
    pub fn on_graphics_error(&mut self, id: u32, message: &str) {
        tracing::warn!("terminal reported a graphics error for image {id:#x}: {message}");
        self.error_count = self.error_count.saturating_add(1);
        self.last_error = Some(format!("{id:#x}: {message}"));
        if message.starts_with("ENOENT") {
            self.uploaded.remove(&id);
            self.placed.retain(|k, _| k.0 != id);
            if id == IMG_GROUND || id == IMG_CANOPY || id == IMG_GATES || id == IMG_FENCES {
                self.scenery = None;
            }
            if id == IMG_FX_HDR {
                self.uploaded.remove(&SheetId::Fx.image_id());
            }
        }
    }

    /// Every image id the renderer may have created, for cleanup.
    pub fn image_ids() -> Vec<u32> {
        let mut v: Vec<u32> = SheetId::ALL.iter().map(|s| s.image_id()).collect();
        v.extend([IMG_GROUND, IMG_CANOPY, IMG_GATES, IMG_FENCES, IMG_TITLE, IMG_POINTER, IMG_FX_HDR]);
        v
    }

    /// Uploads the pointer image for fence drawing (`px` pixels square)
    /// and returns the bytes, with the hotspot.
    pub fn pointer_image(&mut self, px: u32) -> (Vec<u8>, (u32, u32)) {
        let mut out = Vec::new();
        let (pm, hot) = crate::art::fx::stake_cursor(px);
        let s = self.cfg.uploader.pixmap(&mut out, IMG_POINTER, &pm);
        let mut st = FrameStats::default();
        self.note_upload(IMG_POINTER, s, &mut st);
        (out, hot)
    }

    fn note_upload(&mut self, id: u32, s: EncodeStats, st: &mut FrameStats) {
        self.uploaded.insert(id);
        self.placed.retain(|k, _| k.0 != id);
        self.image_events.push((id, true));
        st.uploads += 1;
        st.upload_bytes += s.bytes as usize;
        st.commands += s.commands;
    }

    /// Makes sure a sheet is resampled for the current sprite resolution
    /// and uploaded; sheets are sent the first time something needs them.
    fn ensure_sheet(&mut self, out: &mut Vec<u8>, id: SheetId, res: u32, st: &mut FrameStats) {
        if self.sheets_for != Some(res) {
            self.sheets_for = Some(res);
            self.sheets.fill(None);
            for sid in SheetId::ALL {
                self.uploaded.remove(&sid.image_id());
            }
            self.uploaded.remove(&IMG_FX_HDR);
        }
        let i = id as usize;
        if self.sheets[i].is_none() {
            self.sheets[i] = Some(match self.cfg.sprites {
                SpriteSource::Painted => paint_sheet(id, res),
                SpriteSource::Files => {
                    let factor = res as f32 / crate::art::MASTER_TILE_PX as f32;
                    self.masters[i].scaled(factor)
                }
            });
        }
        if self.uploaded.contains(&id.image_id()) {
            return;
        }
        let sheet = self.sheets[i].as_ref().expect("resampled above");
        let s = self.cfg.uploader.rgba(out, id.image_id(), sheet.width, sheet.height, &sheet.rgba);
        let hdr = (id == SheetId::Fx && self.cfg.hdr_boost > 1.0).then(|| {
            let data = upload::hdr_glints(&sheet.rgba, self.cfg.hdr_boost);
            self.cfg.uploader.hdr(out, IMG_FX_HDR, sheet.width, sheet.height, &data)
        });
        self.note_upload(id.image_id(), s, st);
        if let Some(h) = hdr {
            self.note_upload(IMG_FX_HDR, h, st);
        }
    }

    fn sheet(&self, id: SheetId) -> Option<&Sheet> {
        self.sheets[id as usize].as_ref()
    }

    /// Whether drawing `f` would mean sending a lot of image data first
    /// (a new level, a new size): the app puts up a banner meanwhile.
    pub fn needs_heavy_upload(&self, f: &FrameModel) -> bool {
        let Some(l) = self.layout else { return false };
        let res = sprite_res(l.tile_px);
        let sheets = self.sheets_for != Some(res) || f.sprites.iter().any(|s| !self.uploaded.contains(&s.sheet.image_id()));
        let scenery = f.scene.as_ref().is_some_and(|sr| {
            self.scenery.as_ref().is_none_or(|s| s.key != sr.key || (!self.cfg.vector_scenery && s.raster_for != Some(self.raster_res(&l))))
        });
        (sheets || scenery) && !self.cfg.uploader.temp_file
    }

    fn raster_res(&self, l: &Layout) -> u32 {
        let res = sprite_res(l.tile_px) as f32;
        let k = if self.cfg.uploader.temp_file { 1.0 } else { REMOTE_SCENERY_SCALE };
        ((res * k).round() as u32).max(16)
    }

    /// Renders a frame and returns the bytes to write (empty when nothing
    /// changed).
    pub fn render(&mut self, f: &FrameModel) -> Vec<u8> {
        self.image_events.clear();
        let mut st = FrameStats::default();
        let mut out = Vec::new();
        let Some(l) = self.layout else {
            return out;
        };
        let res = sprite_res(l.tile_px);
        let mut needed: BTreeSet<SheetId> = f.sprites.iter().map(|s| s.sheet).collect();
        if !f.strokes.is_empty() && !self.cfg.vector_scenery {
            needed.insert(SheetId::Fx);
        }
        for id in needed {
            self.ensure_sheet(&mut out, id, res, &mut st);
        }
        let mut desired: BTreeMap<(u32, u32), Placed> = BTreeMap::new();
        match &f.scene {
            Some(sr) => self.scenery_frame(&mut out, &l, sr, f, &mut desired, &mut st),
            None => {
                if self.scenery.is_some() {
                    self.scenery = None;
                }
            }
        }
        if f.title {
            self.title_frame(&mut out, &l, &mut desired, &mut st);
        }
        self.sprite_placements(&l, &f.sprites, &mut desired);
        self.apply_placements(&mut out, desired, &mut st);
        let mut text = Vec::new();
        // The loading banner stays up while uploads stream, and goes with
        // the first frame after them.
        if st.upload_bytes == 0 {
            if let Some((row, col, w)) = self.banner.take() {
                let _ = write!(text, "\x1b[{};{}H\x1b[0m{}", row + 1, col + 1, " ".repeat(w as usize));
            }
        }
        self.status_lines(&mut text, &l, f);
        self.overlays(&mut text, &l, &f.overlays);
        st.text_bytes = text.len();
        if st.upload_bytes > 0 {
            // Big uploads stream for a while: show the text first, outside
            // a synchronized update, so the screen is not frozen meanwhile.
            text.extend_from_slice(&out);
            out = text;
            st.bytes = out.len();
            self.count(&st);
            self.last = st;
            return out;
        }
        out.extend_from_slice(&text);
        if out.is_empty() {
            self.last = st;
            return out;
        }
        if self.cfg.sync_output {
            let mut wrapped = Vec::with_capacity(out.len() + 16);
            wrapped.extend_from_slice(b"\x1b[?2026h");
            wrapped.extend_from_slice(&out);
            wrapped.extend_from_slice(b"\x1b[?2026l");
            out = wrapped;
        }
        st.bytes = out.len();
        self.count(&st);
        self.last = st;
        out
    }

    fn count(&mut self, st: &FrameStats) {
        self.totals.frames += 1;
        self.totals.bytes += st.bytes as u64;
        self.totals.placements += st.placements as u64;
        self.totals.uploads += st.uploads as u64;
        self.totals.upload_bytes += st.upload_bytes as u64;
        self.totals.vector_edits += st.vector_edits as u64;
        self.totals.vector_bytes += st.vector_bytes as u64;
    }

    /// A one-line banner in the middle of the field, cleared by the next
    /// frame. For long uploads over slow links.
    pub fn banner(&mut self, msg: &str) -> Vec<u8> {
        let Some(l) = self.layout else { return Vec::new() };
        let w = text::width(msg) as u32 + 4;
        let row = l.field_row + l.field_rows / 2;
        let col = l.cols.saturating_sub(w) / 2;
        self.banner = Some((row, col, w));
        let mut out = Vec::new();
        let _ = write!(
            out,
            "\x1b[{};{}H\x1b[0m\x1b[48;2;{};{};{}m\x1b[38;2;255;214;102m  {msg}  \x1b[0m",
            row + 1,
            col + 1,
            text::BG.0,
            text::BG.1,
            text::BG.2
        );
        out
    }

    fn scenery_frame(&mut self, out: &mut Vec<u8>, l: &Layout, sr: &SceneRef, f: &FrameModel, desired: &mut BTreeMap<(u32, u32), Placed>, st: &mut FrameStats) {
        let rebuild = self.scenery.as_ref().is_none_or(|s| s.key != sr.key);
        if rebuild {
            let mut scenery = scenery::build(&sr.level);
            let topo = Topology::new(sr.level.grid.clone());
            // Gates live in the ground scene so later swings are patches.
            for g in &f.gates {
                for (id, e) in scenery::gate_elements(&g.gate, g.index, &topo, g.stage as f32 / 3.0) {
                    scenery.ground.set(id, e);
                }
            }
            let gates_sent = f.gates.iter().map(|g| (g.index, (g.stage, g.gate.warning))).collect();
            self.scenery = Some(SceneryState {
                key: sr.key.clone(),
                scenery,
                topo,
                raster_for: None,
                atlas_for: None,
                gates_sent,
                fences_sent: BTreeMap::new(),
                strokes_sent: BTreeMap::new(),
                gate_count: f.gates.len(),
            });
            if self.cfg.vector_scenery {
                self.transmit_vectors(out, st);
            }
        }
        // Vector scenery is never re-sent for a new size: the terminal
        // re-rasterizes it. Raster scenery is re-made only when the sprite
        // resolution step changes, and otherwise enlarged by the terminal.
        if !self.cfg.vector_scenery {
            let res = self.raster_res(l);
            if self.scenery.as_ref().is_some_and(|s| s.raster_for != Some(res)) {
                self.raster_scenery(out, res, f, st);
            }
            if self.scenery.as_ref().is_some_and(|s| s.atlas_for != Some(l.tile_px)) {
                self.raster_atlases(out, l, f, st);
            }
        }
        // Ground and canopy placements.
        let field = |z: i32, id: u32| {
            let place = Place {
                id,
                placement: 1,
                src: None,
                offset: (0, 0),
                z,
                cols: Some(l.field_cols),
                rows: None,
            };
            Placed {
                row: l.field_row,
                col: l.field_col,
                place,
            }
        };
        desired.insert((IMG_GROUND, 1), field(Z_GROUND, IMG_GROUND));
        desired.insert((IMG_CANOPY, 1), field(Z_CANOPY, IMG_CANOPY));
        if self.cfg.vector_scenery {
            self.vector_dynamic(out, f, st);
        } else {
            self.raster_dynamic(l, f, desired);
        }
    }

    fn transmit_vectors(&mut self, out: &mut Vec<u8>, st: &mut FrameStats) {
        let s = self.scenery.as_ref().expect("scenery built");
        let g = &s.scenery.ground;
        let rec = g.records();
        let es = kitty_gfx::vector_transmit(out, IMG_GROUND, g.width as f64, g.height as f64, &rec, Quiet::ErrorsOnly);
        let c = &s.scenery.canopy;
        let rec2 = c.records();
        let es2 = kitty_gfx::vector_transmit(out, IMG_CANOPY, c.width as f64, c.height as f64, &rec2, Quiet::ErrorsOnly);
        st.vector_bytes += (es.bytes + es2.bytes) as usize;
        self.note_upload(IMG_GROUND, es, st);
        self.note_upload(IMG_CANOPY, es2, st);
    }

    /// The ground and canopy as bitmaps at `res` pixels per tile; the
    /// terminal scales them to the field.
    fn raster_scenery(&mut self, out: &mut Vec<u8>, res: u32, f: &FrameModel, st: &mut FrameStats) {
        let scale = res as f32 / TILE;
        let s = self.scenery.as_mut().expect("scenery built");
        let w = ((s.scenery.width * scale).round() as u32).max(1);
        let h = ((s.scenery.height * scale).round() as u32).max(1);
        let t = Transform::from_scale(scale, scale);
        let mut ground = Pixmap::new(w, h).expect("field size");
        // Gates are drawn as separate placements in raster mode.
        let mut base = s.scenery.ground.clone();
        for i in 0..s.gate_count.max(f.gates.len()) {
            base.delete_group_members(i);
        }
        base.render(&mut ground, t);
        let mut canopy = Pixmap::new(w, h).expect("field size");
        s.scenery.canopy.render(&mut canopy, t);
        s.raster_for = Some(res);
        let up = self.cfg.uploader;
        let a = up.pixmap(out, IMG_GROUND, &ground);
        self.note_upload(IMG_GROUND, a, st);
        let b = up.pixmap(out, IMG_CANOPY, &canopy);
        self.note_upload(IMG_CANOPY, b, st);
    }

    /// Gates and fences are placed unscaled at exact positions, so their
    /// atlases follow the tile size exactly. They are small.
    fn raster_atlases(&mut self, out: &mut Vec<u8>, l: &Layout, f: &FrameModel, st: &mut FrameStats) {
        let s = self.scenery.as_mut().expect("scenery built");
        let atlas_gates = gate_atlas(&s.topo, &f.gates, l.tile_px);
        let atlas_fences = fence_atlas(l.tile_px);
        s.atlas_for = Some(l.tile_px);
        let up = self.cfg.uploader;
        if let Some(pm) = atlas_gates {
            let c = up.pixmap(out, IMG_GATES, &pm);
            self.note_upload(IMG_GATES, c, st);
        }
        let d = up.pixmap(out, IMG_FENCES, &atlas_fences);
        self.note_upload(IMG_FENCES, d, st);
    }

    fn raster_dynamic(&mut self, l: &Layout, f: &FrameModel, desired: &mut BTreeMap<(u32, u32), Placed>) {
        let Some(s) = &self.scenery else { return };
        let cell = (GATE_CELL_TILES * l.tile_px).round() as u32;
        for g in &f.gates {
            let col = g.stage as u32 + if g.gate.warning && !g.gate.closed { 4 } else { 0 };
            let src = [col * cell, g.index as u32 * cell, cell, cell];
            let center = g.gate.tile.center();
            let (x, y) = l.world_to_px(center);
            let half = cell as f32 / 2.0;
            if let Some((row, c, xo, yo)) = l.cell_at(x - half, y - half) {
                desired.insert(
                    (IMG_GATES, g.index as u32 + 1),
                    Placed {
                        row,
                        col: c,
                        place: Place {
                            id: IMG_GATES,
                            placement: g.index as u32 + 1,
                            src: Some(src),
                            offset: (xo, yo),
                            z: Z_DECAL,
                            cols: None,
                            rows: None,
                        },
                    },
                );
            }
        }
        let fcell = (FENCE_CELL_TILES * l.tile_px).round() as u32;
        for &(p, stage) in &f.fences {
            let horiz = scenery::corridor_horizontal(&s.topo, p);
            let col = stage.min(2) as u32;
            let row_i = u32::from(!horiz);
            let src = [col * fcell, row_i * fcell, fcell, fcell];
            let (x, y) = l.world_to_px(p.center());
            let half = fcell as f32 / 2.0;
            let pid = (p.y as u32) * (l.grid_w as u32) + p.x as u32 + 1;
            if let Some((row, c, xo, yo)) = l.cell_at(x - half, y - half) {
                desired.insert(
                    (IMG_FENCES, pid),
                    Placed {
                        row,
                        col: c,
                        place: Place {
                            id: IMG_FENCES,
                            placement: pid,
                            src: Some(src),
                            offset: (xo, yo),
                            z: Z_DECAL + 1,
                            cols: None,
                            rows: None,
                        },
                    },
                );
            }
        }
        // Strokes being drawn: a trail of ink dots.
        let Some(fx) = self.sheet(SheetId::Fx) else { return };
        if let Some(frame) = fx.anim("ink_dot").and_then(|a| a.first().copied()) {
            let rect = fx.frame_rect(frame);
            for sv in &f.strokes {
                let mut last: Option<Vec2> = None;
                for (i, &p) in sv.points.iter().enumerate() {
                    if last.is_some_and(|q| q.distance(p) < 0.16) {
                        continue;
                    }
                    last = Some(p);
                    let (x, y) = l.world_to_px(p);
                    if let Some((row, c, xo, yo)) = l.cell_at(x - fx.spec.anchor_x, y - fx.spec.anchor_y) {
                        let pid = 3_000_000u32.wrapping_add((sv.id as u32 % 1000) * 4096 + i as u32 % 4096);
                        desired.insert(
                            (SheetId::Fx.image_id(), pid),
                            Placed {
                                row,
                                col: c,
                                place: Place {
                                    id: SheetId::Fx.image_id(),
                                    placement: pid,
                                    src: Some(rect),
                                    offset: (xo, yo),
                                    z: Z_OVER,
                                    cols: None,
                                    rows: None,
                                },
                            },
                        );
                    }
                }
            }
        }
    }

    fn vector_dynamic(&mut self, out: &mut Vec<u8>, f: &FrameModel, st: &mut FrameStats) {
        let Some(s) = &mut self.scenery else { return };
        let mut edits: Vec<Edit> = Vec::new();
        for g in &f.gates {
            let state = (g.stage, g.gate.warning && !g.gate.closed);
            if s.gates_sent.get(&g.index) != Some(&state) {
                let mut gate = g.gate.clone();
                gate.warning = state.1;
                for (id, e) in scenery::gate_elements(&gate, g.index, &s.topo, g.stage as f32 / 3.0) {
                    edits.push(Edit::Set(id, e));
                }
                s.gates_sent.insert(g.index, state);
            }
        }
        let now: BTreeMap<TilePos, u8> = f.fences.iter().copied().collect();
        for (&p, &stage) in &now {
            if s.fences_sent.get(&p) != Some(&stage) {
                for (id, e) in scenery::fence_tile_elements(&s.topo, p, scenery::wear_of_stage(stage)) {
                    edits.push(Edit::Set(id, e));
                }
            }
        }
        for &p in s.fences_sent.keys() {
            if !now.contains_key(&p) {
                let base = scenery::fence_tile_id(&s.topo, p);
                for k in 0..scenery::FENCE_IDS_PER_TILE {
                    edits.push(Edit::Delete(base + k));
                }
            }
        }
        s.fences_sent = now;
        // Strokes drawn by the game itself: one pen-stroke element each,
        // appended to as points arrive.
        let live: BTreeSet<u64> = f.strokes.iter().map(|sv| sv.id).collect();
        let scale = TILE;
        for sv in &f.strokes {
            if sv.terminal_ink {
                continue;
            }
            let id = scenery::STROKE_BASE + (sv.id % 1_000_000) as u32;
            let sent = s.strokes_sent.get(&sv.id).copied().unwrap_or(0);
            let pts: Vec<[f32; 3]> = sv.points[sent.min(sv.points.len())..]
                .iter()
                .map(|p| [p.x * scale, p.y * scale, 14.0])
                .collect();
            if pts.is_empty() {
                continue;
            }
            if sent == 0 {
                edits.push(Edit::Set(
                    id,
                    Element::Ink {
                        group: scenery::G_INK,
                        pts,
                        fill: scenery::palette::INK.with_alpha(230).into(),
                    },
                ));
            } else {
                edits.push(Edit::AppendPts(id, pts));
            }
            s.strokes_sent.insert(sv.id, sv.points.len());
        }
        let gone: Vec<u64> = s.strokes_sent.keys().copied().filter(|k| !live.contains(k)).collect();
        for k in gone {
            s.strokes_sent.remove(&k);
            edits.push(Edit::Delete(scenery::STROKE_BASE + (k % 1_000_000) as u32));
        }
        if edits.is_empty() {
            return;
        }
        let mut text = String::new();
        for e in &edits {
            if e.redraws_everything(&s.scenery.ground) {
                st.vector_full_redraws += 1;
            }
            s.scenery.ground.apply(e);
            text.push_str(&e.record());
            text.push('\n');
        }
        let es = kitty_gfx::vector_edit(out, IMG_GROUND, &text, Quiet::ErrorsOnly);
        st.vector_edits += edits.len() as u32;
        st.vector_bytes += es.bytes as usize;
        st.commands += es.commands;
    }

    /// Applies an edit to the ground scene the terminal holds (used for
    /// strokes the terminal inked itself, so our mirror stays true).
    pub fn mirror_ground_edit(&mut self, e: &Edit) {
        if let Some(s) = &mut self.scenery {
            s.scenery.ground.apply(e);
        }
    }

    /// Sends a patch to the ground vector image outside the frame diff.
    pub fn ground_patch(&mut self, edits: &[Edit]) -> Vec<u8> {
        let mut out = Vec::new();
        if !self.cfg.vector_scenery || edits.is_empty() {
            return out;
        }
        let Some(s) = &mut self.scenery else { return out };
        let mut text = String::new();
        for e in edits {
            s.scenery.ground.apply(e);
            text.push_str(&e.record());
            text.push('\n');
        }
        kitty_gfx::vector_edit(&mut out, IMG_GROUND, &text, Quiet::ErrorsOnly);
        out
    }

    pub fn has_vector_ground(&self) -> bool {
        self.cfg.vector_scenery && self.scenery.is_some() && self.uploaded.contains(&IMG_GROUND)
    }

    fn title_frame(&mut self, out: &mut Vec<u8>, l: &Layout, desired: &mut BTreeMap<(u32, u32), Placed>, st: &mut FrameStats) {
        use crate::gfx::title::{LOGO_H, LOGO_W};
        // The logo spans most of the field's width, near its top. On Silkie
        // it is a vector image sent once and scaled by the terminal.
        let cols = ((l.field_cols as f32) * 0.72).round().max(1.0) as u32;
        let w = cols * l.cell_w;
        let h = (w as f32 * LOGO_H / LOGO_W).round() as u32;
        if self.cfg.vector_scenery {
            if !self.uploaded.contains(&IMG_TITLE) {
                let scene = crate::gfx::title::logo_scene();
                let s = kitty_gfx::vector_transmit(out, IMG_TITLE, LOGO_W as f64, LOGO_H as f64, &scene.records(), Quiet::ErrorsOnly);
                st.vector_bytes += s.bytes as usize;
                self.note_upload(IMG_TITLE, s, st);
            }
        } else {
            let needs = self.title.is_none_or(|(tl, _, _)| tl != *l) || !self.uploaded.contains(&IMG_TITLE);
            if needs {
                let pm = crate::gfx::title::logo(w, h);
                let s = self.cfg.uploader.pixmap(out, IMG_TITLE, &pm);
                self.note_upload(IMG_TITLE, s, st);
                self.title = Some((*l, w, h));
            }
        }
        let (ox, oy) = l.origin_px();
        let x = ox + ((l.field_cols - cols) / 2 * l.cell_w) as f32;
        let y = oy + l.field_h as f32 * 0.07;
        if let Some((row, col, xo, yo)) = l.cell_at(x, y) {
            desired.insert(
                (IMG_TITLE, 1),
                Placed {
                    row,
                    col,
                    place: Place {
                        id: IMG_TITLE,
                        placement: 1,
                        src: None,
                        offset: (xo, yo),
                        z: Z_TITLE,
                        cols: self.cfg.vector_scenery.then_some(cols),
                        rows: None,
                    },
                },
            );
        }
    }

    fn sprite_placements(&self, l: &Layout, sprites: &[Sprite], desired: &mut BTreeMap<(u32, u32), Placed>) {
        for sp in sprites {
            let Some(sheet) = self.sheet(sp.sheet) else { continue };
            let Some(frames) = sheet.anim(&sp.anim) else { continue };
            if frames.is_empty() {
                continue;
            }
            let frame = frames[sp.frame % frames.len()];
            let (x, y) = l.world_to_px(sp.pos);
            let Some((row, col, xo, yo)) = l.cell_at(x - sheet.spec.anchor_x, y - sheet.spec.anchor_y) else {
                continue;
            };
            let z = match sp.layer {
                Layer::Decal => Z_DECAL + 10 + sp.z_bias,
                Layer::Body => Z_SPRITE + y.round() as i32 + sp.z_bias,
                Layer::Over => Z_OVER + sp.z_bias,
            };
            let place = |id, z| Placed {
                row,
                col,
                place: Place {
                    id,
                    placement: sp.key,
                    src: Some(sheet.frame_rect(frame)),
                    offset: (xo, yo),
                    z,
                    cols: None,
                    rows: None,
                },
            };
            desired.insert((sp.sheet.image_id(), sp.key), place(sp.sheet.image_id(), z));
            // Sparkles, stars and hearts get their white-hot glints on top.
            let glow = sp.sheet == SheetId::Fx && matches!(sp.anim.as_str(), "sparkle" | "star" | "heart");
            if glow && self.cfg.hdr_boost > 1.0 {
                desired.insert((IMG_FX_HDR, sp.key), place(IMG_FX_HDR, z + 1));
            }
        }
    }

    fn apply_placements(&mut self, out: &mut Vec<u8>, desired: BTreeMap<(u32, u32), Placed>, st: &mut FrameStats) {
        let stale: Vec<(u32, u32)> = self.placed.keys().filter(|k| !desired.contains_key(k)).copied().collect();
        for k in stale {
            let s = kitty_gfx::delete_placement(out, k.0, k.1);
            st.commands += s.commands;
            st.deletions += 1;
            self.placed.remove(&k);
        }
        for (k, p) in desired {
            if self.placed.get(&k) == Some(&p) {
                continue;
            }
            if !self.uploaded.contains(&k.0) {
                continue;
            }
            let _ = write!(out, "\x1b[{};{}H", p.row + 1, p.col + 1);
            let s = p.place.write(out, Quiet::ErrorsOnly);
            st.commands += s.commands;
            st.placements += 1;
            self.placed.insert(k, p);
        }
    }

    fn status_lines(&mut self, out: &mut Vec<u8>, l: &Layout, f: &FrameModel) {
        for (row, line) in [(0u32, &f.top), (l.rows - 1, &f.bottom)] {
            if self.text_rows.get(&row) == Some(line) {
                continue;
            }
            let _ = write!(out, "\x1b[{};1H\x1b[0m\x1b[2K{}\x1b[0m", row + 1, text::fit(line, l.cols as usize));
            self.text_rows.insert(row, line.clone());
        }
    }

    fn overlays(&mut self, out: &mut Vec<u8>, l: &Layout, overlays: &[Overlay]) {
        let rendered: Vec<(String, (u32, u32, u32, u32))> = overlays
            .iter()
            .map(|o| text::render_overlay(o, l, self.cfg.cell_alpha))
            .collect();
        let texts: Vec<String> = rendered.iter().map(|r| r.0.clone()).collect();
        if texts == self.overlay_text {
            return;
        }
        // Blank what the previous overlays covered, then draw the new ones.
        for &(col, row, w, h) in &self.overlay_rects {
            for r in row..row + h {
                let _ = write!(out, "\x1b[{};{}H\x1b[0m{}", r + 1, col + 1, " ".repeat(w as usize));
            }
        }
        for (s, _) in &rendered {
            out.extend_from_slice(s.as_bytes());
        }
        out.extend_from_slice(b"\x1b[0m");
        self.overlay_rects = rendered.iter().map(|r| r.1).collect();
        self.overlay_text = texts;
    }

    /// Forgets the status lines and overlays (after the text area was
    /// cleared), so they are written again.
    pub fn reset_text(&mut self) {
        self.text_rows.clear();
        self.overlay_rects.clear();
        self.overlay_text.clear();
    }

    /// Placements currently on screen, for tests and diagnostics.
    pub fn placement_count(&self) -> usize {
        self.placed.len()
    }

    pub fn placements_of(&self, image: u32) -> usize {
        self.placed.keys().filter(|k| k.0 == image).count()
    }

    pub fn is_uploaded(&self, image: u32) -> bool {
        self.uploaded.contains(&image)
    }

    pub fn scenery_scene(&self) -> Option<&Scene> {
        self.scenery.as_ref().map(|s| &s.scenery.ground)
    }

    /// Takes every placement off the screen but keeps the images, for a
    /// window too small to show the garden: when it grows again they are
    /// only placed anew.
    pub fn hide(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        for (image, placement) in std::mem::take(&mut self.placed).into_keys() {
            kitty_gfx::delete_placement(&mut out, image, placement);
        }
        self.text_rows.clear();
        self.overlay_rects.clear();
        self.overlay_text.clear();
        out
    }

    /// Deletes every image (on exit or before handing the screen over).
    pub fn teardown(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        for id in std::mem::take(&mut self.uploaded) {
            kitty_gfx::delete_image(&mut out, id);
        }
        self.placed.clear();
        out
    }
}

/// Paints one sheet at `res` pixels per tile.
pub fn paint_sheet(id: SheetId, res: u32) -> Sheet {
    let (spec, pm) = match id {
        SheetId::Hen => crate::art::hen::sheet(res),
        SheetId::Chick => crate::art::chick::sheet(res),
        SheetId::Fox => crate::art::critters::fox_sheet(res),
        SheetId::Weasel => crate::art::critters::weasel_sheet(res),
        SheetId::Hawk => crate::art::critters::hawk_sheet(res),
        SheetId::Items => crate::art::fx::items_sheet(res),
        SheetId::Fx => crate::art::fx::fx_sheet(res),
        SheetId::Glyphs => return crate::gfx::title::glyph_sheet(res),
    };
    let (width, height) = (pm.width(), pm.height());
    Sheet {
        spec,
        width,
        height,
        rgba: pm.take_demultiplied(),
    }
}

trait SceneGates {
    fn delete_group_members(&mut self, gate_index: usize);
}

impl SceneGates for Scene {
    fn delete_group_members(&mut self, gate_index: usize) {
        let base = scenery::GATE_BASE + gate_index as u32 * 16;
        for k in 0..16 {
            self.delete(base + k);
        }
    }
}

/// Every gate in every state: rows are gates, columns are swing stages
/// 0..=3 and then the warning variants.
fn gate_atlas(topo: &Topology, gates: &[GateView], tile_px: f32) -> Option<Pixmap> {
    if gates.is_empty() {
        return None;
    }
    let cell = (GATE_CELL_TILES * tile_px).round() as u32;
    let mut pm = Pixmap::new(cell * 8, cell * gates.len() as u32)?;
    let scale = tile_px / TILE;
    for g in gates {
        for col in 0..8u32 {
            let stage = col % 4;
            let mut gate = g.gate.clone();
            gate.warning = col >= 4;
            gate.closed = false;
            let mut scene = Scene::new(0.0, 0.0);
            scene.set(scenery::G_GATES, Element::group(0));
            for (id, e) in scenery::gate_elements(&gate, g.index, topo, stage as f32 / 3.0) {
                scene.set(id, e);
            }
            // Map the gate tile's centre to the cell's centre.
            let c = g.gate.tile.center() * TILE;
            let ox = col as f32 * cell as f32 + cell as f32 / 2.0;
            let oy = g.index as f32 * cell as f32 + cell as f32 / 2.0;
            let t = Transform::from_row(scale, 0.0, 0.0, scale, ox - c.x * scale, oy - c.y * scale);
            scene.render(&mut pm, t);
        }
    }
    Some(pm)
}

/// Fence panels: columns are wear stages, rows are orientation (across a
/// left-right corridor, then across an up-down one).
fn fence_atlas(tile_px: f32) -> Pixmap {
    let cell = (FENCE_CELL_TILES * tile_px).round().max(1.0) as u32;
    let mut pm = Pixmap::new(cell * 3, cell * 2).expect("fence atlas size");
    let scale = tile_px / TILE;
    for row in 0..2u32 {
        for stage in 0..3u8 {
            let mut scene = Scene::new(0.0, 0.0);
            scene.set(scenery::G_FENCES, Element::group(0));
            let p = TilePos::new(0, 0);
            for (id, e) in scenery::fence_elements_at(p, row == 0, scenery::wear_of_stage(stage), 1) {
                scene.set(id, e);
            }
            let ox = stage as f32 * cell as f32 + cell as f32 / 2.0;
            let oy = row as f32 * cell as f32 + cell as f32 / 2.0;
            let c = p.center() * TILE;
            let t = Transform::from_row(scale, 0.0, 0.0, scale, ox - c.x * scale, oy - c.y * scale);
            scene.render(&mut pm, t);
        }
    }
    pm
}

#[cfg(test)]
mod tests;
