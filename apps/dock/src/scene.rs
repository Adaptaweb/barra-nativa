//! Dock visual tree: shadow, glass, specular border, dividers, icons, indicators,
//! and labels. It is built once per configuration; only its elements move per frame.

use std::cell::Cell;

use anyhow::Result;
use muelle_core::{
    layout::{ItemKind, Layout, Rect},
    magnify::Magnified,
};
use windows::{
    UI::{
        Color,
        Composition::{
            CompositionBrush, CompositionColorSpace, CompositionRoundedRectangleGeometry,
            CompositionSurfaceBrush, ContainerVisual, ShapeVisual, SpriteVisual, Visual,
        },
    },
    core::Interface,
};
use windows_numerics::{Vector2, Vector3};

use crate::gfx::{Gfx, LabelBrush};

#[derive(Clone, Copy)]
pub struct Theme {
    pub light: bool,
    /// Blur what is behind the dock; without it the tint is drawn opaque.
    glass: bool,
    tint: Color,
    separator: Color,
    shadow: Color,
    indicator: Color,
}

impl Theme {
    /// `glass` is false while Windows has transparency off (or energy saver
    /// on): the dock is then solid gray, as macOS draws it with "Reduce
    /// transparency".
    pub fn for_system(light: bool, glass: bool) -> Self {
        if light {
            Self {
                light,
                glass,
                tint: if glass {
                    rgba(255, 255, 255, 46)
                } else {
                    rgba(226, 226, 228, 255)
                },
                separator: rgba(0, 0, 0, 56),
                shadow: rgba(0, 0, 0, 56),
                indicator: rgba(0, 0, 0, 170),
            }
        } else {
            Self {
                light,
                glass,
                tint: if glass {
                    rgba(28, 28, 30, 82)
                } else {
                    rgba(44, 44, 46, 255)
                },
                separator: rgba(255, 255, 255, 64),
                shadow: rgba(0, 0, 0, 96),
                indicator: rgba(255, 255, 255, 215),
            }
        }
    }

    /// Inside the frame around the icon a dragged app would join.
    fn merge_fill(&self) -> Color {
        if self.light {
            rgba(255, 255, 255, 120)
        } else {
            rgba(255, 255, 255, 46)
        }
    }

    /// Uses the context menu tint with the same rim and sheen as the window picker.
    pub(crate) fn for_popup(light: bool, glass: bool) -> Self {
        Self {
            tint: crate::menu::popup_tint(!light, glass),
            ..Self::for_system(light, glass)
        }
    }

    /// A fixed-size panel using the dock's backdrop, tint, sheen and rim.
    pub fn panel(
        &self,
        gfx: &Gfx,
        size: Vector2,
        radius: f32,
        scale: f32,
    ) -> Result<ContainerVisual> {
        let stroke = scale.max(1.0);
        let material = GlassSurface::build(gfx, *self, radius, stroke)?;
        material.glass.SetSize(size)?;
        material.outline.SetSize(size)?;
        material.rim.SetSize(size)?;
        material
            .rim_geometry
            .SetSize(v2(size.X - stroke, size.Y - stroke))?;
        let root = gfx.compositor.CreateContainerVisual()?;
        root.SetSize(size)?;
        root.Children()?.InsertAtTop(&material.glass)?;
        root.Children()?.InsertAtTop(&material.rim)?;
        Ok(root)
    }
}

/// The material is shared so dock and preview panels follow the same theme
/// and opaque fallback when Windows transparency is disabled.
struct GlassSurface {
    glass: ContainerVisual,
    outline: CompositionRoundedRectangleGeometry,
    rim: ShapeVisual,
    rim_geometry: CompositionRoundedRectangleGeometry,
}

impl GlassSurface {
    fn build(gfx: &Gfx, theme: Theme, radius: f32, stroke: f32) -> Result<Self> {
        let c = &gfx.compositor;
        let glass = c.CreateContainerVisual()?;
        let outline = c.CreateRoundedRectangleGeometry()?;
        outline.SetCornerRadius(v2(radius, radius))?;
        glass.SetClip(&c.CreateGeometricClipWithGeometry(&outline)?)?;

        let sheen = c.CreateLinearGradientBrush()?;
        sheen.SetStartPoint(v2(0.0, 0.0))?;
        sheen.SetEndPoint(v2(0.0, 1.0))?;
        let stops = sheen.ColorStops()?;
        stops
            .Append(&c.CreateColorGradientStopWithOffsetAndColor(0.0, rgba(255, 255, 255, 38))?)?;
        stops
            .Append(&c.CreateColorGradientStopWithOffsetAndColor(0.55, rgba(255, 255, 255, 0))?)?;

        let backdrop: Option<CompositionBrush> = if theme.glass {
            Some(c.CreateHostBackdropBrush()?.cast()?)
        } else {
            None
        };
        let fills = [
            c.CreateColorBrushWithColor(theme.tint)?.cast()?,
            sheen.cast()?,
        ];
        for brush in backdrop.iter().chain(&fills) {
            let fill = c.CreateSpriteVisual()?;
            fill.SetRelativeSizeAdjustment(v2(1.0, 1.0))?;
            fill.SetBrush(brush)?;
            glass.Children()?.InsertAtTop(&fill)?;
        }

        let rim_brush = c.CreateLinearGradientBrush()?;
        rim_brush.SetStartPoint(v2(0.0, 0.0))?;
        rim_brush.SetEndPoint(v2(1.0, 1.0))?;
        rim_brush.SetInterpolationSpace(CompositionColorSpace::RgbLinear)?;
        let stops = rim_brush.ColorStops()?;
        for (at, alpha) in [(0.0, 150), (0.35, 40), (0.65, 30), (1.0, 105)] {
            stops.Append(
                &c.CreateColorGradientStopWithOffsetAndColor(at, rgba(255, 255, 255, alpha))?,
            )?;
        }
        let rim_geometry = c.CreateRoundedRectangleGeometry()?;
        rim_geometry.SetOffset(v2(stroke / 2.0, stroke / 2.0))?;
        rim_geometry.SetCornerRadius(v2(radius - stroke / 2.0, radius - stroke / 2.0))?;
        let rim_shape = c.CreateSpriteShapeWithGeometry(&rim_geometry)?;
        rim_shape.SetStrokeBrush(&rim_brush)?;
        rim_shape.SetStrokeThickness(stroke)?;
        let rim = c.CreateShapeVisual()?;
        rim.Shapes()?.Append(&rim_shape)?;
        Ok(Self {
            glass,
            outline,
            rim,
            rim_geometry,
        })
    }
}

/// Outline of the icon a dragged app would join in a group.
const MERGE_ACCENT: Color = rgba(80, 150, 255, 230);

const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color {
        A: a,
        R: r,
        G: g,
        B: b,
    }
}

fn v2(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

fn v3(x: f32, y: f32) -> Vector3 {
    Vector3 { X: x, Y: y, Z: 0.0 }
}

#[derive(Clone, Copy, Default)]
pub struct SlotContent<'a> {
    pub icon: Option<&'a CompositionSurfaceBrush>,
    pub label: Option<&'a LabelBrush>,
    pub running: bool,
    /// App icon shown in the corner of a minimized-window thumbnail.
    pub badge: Option<&'a CompositionSurfaceBrush>,
}

pub struct SceneInput<'a> {
    pub layout: &'a Layout,
    pub contents: &'a [SlotContent<'a>],
    pub theme: Theme,
    pub scale: f32,
    pub origin: (f32, f32),
    /// Shown above an icon dragged out of the dock.
    pub remove_label: Option<&'a LabelBrush>,
}

/// App badge on a minimized-window thumbnail: side relative to the slot, and
/// how much of it lies inside the slot.
pub const BADGE_SIZE: f32 = 0.42;
pub const BADGE_INSIDE: f32 = 0.85;

/// An icon following the cursor while it is dragged.
#[derive(Clone, Copy)]
pub struct Drag {
    pub index: usize,
    /// Icon center, relative to the resting dock origin.
    pub center: (f32, f32),
    /// Released here, the icon would leave the dock.
    pub removing: bool,
    /// Taken out of a group: a window of its own follows the pointer (see
    /// `group_view::DragGhost`), and the slot only shows, faded, where the
    /// app will land.
    pub ghost: bool,
}

/// Everything that changes from one frame to the next.
pub struct Frame<'a> {
    pub state: &'a Magnified,
    pub hovered: Option<usize>,
    pub lifts: &'a [f32],
    pub drag: Option<Drag>,
    /// The icon the dragged app would join in a group if released now.
    pub target: Option<usize>,
    /// Vertical offset of the whole dock (auto-hide), in pixels.
    pub slide: f32,
    /// Align to whole pixels; only at rest, where sharpness matters.
    pub snap: bool,
}

struct Label {
    visual: SpriteVisual,
    width: f32,
    height: f32,
}

impl Label {
    fn new(c: &windows::UI::Composition::Compositor, brush: &LabelBrush) -> Result<Self> {
        let visual = c.CreateSpriteVisual()?;
        visual.SetSize(v2(brush.width, brush.height))?;
        visual.SetBrush(&brush.brush)?;
        visual.SetIsVisible(false)?;
        Ok(Self {
            visual,
            width: brush.width,
            height: brush.height,
        })
    }
}

pub struct Scene {
    pub root: ContainerVisual,
    origin: (f32, f32),
    height: f32,
    stroke: f32,
    label_gap: f32,
    indicator_size: f32,
    indicator_y: f32,
    frames: [Visual; 3],
    outline: CompositionRoundedRectangleGeometry,
    rim_geometry: CompositionRoundedRectangleGeometry,
    content: ContainerVisual,
    /// Frame drawn around the icon a dragged app would join.
    merge: ShapeVisual,
    merge_geometry: CompositionRoundedRectangleGeometry,
    merge_stroke: f32,
    /// The slot brought to the front for the current drag, so it passes
    /// over the icons it is dropped on.
    raised: Cell<Option<usize>>,
    slots: Vec<SpriteVisual>,
    badges: Vec<Option<SpriteVisual>>,
    indicators: Vec<Option<ShapeVisual>>,
    labels: Vec<Option<Label>>,
    remove_label: Option<Label>,
}

impl Scene {
    pub fn build(gfx: &Gfx, input: &SceneInput<'_>) -> Result<Self> {
        let c = &gfx.compositor;
        let SceneInput {
            layout,
            theme,
            scale,
            origin,
            ..
        } = *input;
        let radius = layout.height * 0.36;

        let root = c.CreateContainerVisual()?;
        root.SetRelativeSizeAdjustment(v2(1.0, 1.0))?;
        let layers = root.Children()?;

        let shadow = c.CreateDropShadow()?;
        let mask = c.CreateNineGridBrush()?;
        mask.SetSource(&gfx.rounded_mask(radius)?)?;
        mask.SetInsets(radius.ceil() + 1.0)?;
        shadow.SetMask(&mask)?;
        shadow.SetBlurRadius(22.0 * scale)?;
        shadow.SetOffset(v3(0.0, 8.0 * scale))?;
        shadow.SetColor(theme.shadow)?;
        let shadow_host = c.CreateSpriteVisual()?;
        shadow_host.SetShadow(&shadow)?;
        layers.InsertAtTop(&shadow_host)?;

        let stroke = scale.max(1.0);
        let GlassSurface {
            glass,
            outline,
            rim,
            rim_geometry,
        } = GlassSurface::build(gfx, theme, radius, stroke)?;
        layers.InsertAtTop(&glass)?;
        layers.InsertAtTop(&rim)?;

        let content = c.CreateContainerVisual()?;
        content.SetOffset(v3(origin.0, origin.1))?;
        let children = content.Children()?;
        let indicator_size = (4.0 * scale).round();
        let dot = c.CreateEllipseGeometry()?;
        dot.SetCenter(v2(indicator_size / 2.0, indicator_size / 2.0))?;
        dot.SetRadius(v2(indicator_size / 2.0, indicator_size / 2.0))?;
        let dot_brush = c.CreateColorBrushWithColor(theme.indicator)?;

        // Below the icons: it frames the one under the dragged app.
        let merge_stroke = (2.0 * scale).max(1.0);
        let merge_geometry = c.CreateRoundedRectangleGeometry()?;
        merge_geometry.SetOffset(v2(merge_stroke / 2.0, merge_stroke / 2.0))?;
        let merge_shape = c.CreateSpriteShapeWithGeometry(&merge_geometry)?;
        merge_shape.SetFillBrush(&c.CreateColorBrushWithColor(theme.merge_fill())?)?;
        merge_shape.SetStrokeBrush(&c.CreateColorBrushWithColor(MERGE_ACCENT)?)?;
        merge_shape.SetStrokeThickness(merge_stroke)?;
        let merge = c.CreateShapeVisual()?;
        merge.Shapes()?.Append(&merge_shape)?;
        merge.SetIsVisible(false)?;
        children.InsertAtTop(&merge)?;

        let mut slots = Vec::with_capacity(layout.slots.len());
        let mut badges = Vec::with_capacity(layout.slots.len());
        let mut indicators = Vec::with_capacity(layout.slots.len());
        let mut labels = Vec::with_capacity(layout.slots.len());
        for (slot, item) in layout.slots.iter().zip(input.contents) {
            let visual = c.CreateSpriteVisual()?;
            match (slot.kind, item.icon) {
                (ItemKind::Separator, _) => {
                    visual.SetBrush(&c.CreateColorBrushWithColor(theme.separator)?)?;
                }
                (ItemKind::Icon, Some(icon)) => visual.SetBrush(icon)?,
                (ItemKind::Icon, None) => {}
            }
            children.InsertAtTop(&visual)?;
            slots.push(visual);
            badges.push(match item.badge {
                Some(brush) => {
                    let badge = c.CreateSpriteVisual()?;
                    badge.SetBrush(brush)?;
                    children.InsertAtTop(&badge)?;
                    Some(badge)
                }
                None => None,
            });

            indicators.push(if item.running {
                let shape = c.CreateSpriteShapeWithGeometry(&dot)?;
                shape.SetFillBrush(&dot_brush)?;
                let visual = c.CreateShapeVisual()?;
                visual.SetSize(v2(indicator_size, indicator_size))?;
                visual.Shapes()?.Append(&shape)?;
                children.InsertAtTop(&visual)?;
                Some(visual)
            } else {
                None
            });

            labels.push(item.label.map(|label| Label::new(c, label)).transpose()?);
        }

        let remove_label = input
            .remove_label
            .map(|label| Label::new(c, label))
            .transpose()?;
        for label in labels.iter().flatten().chain(&remove_label) {
            children.InsertAtTop(&label.visual)?;
        }
        layers.InsertAtTop(&content)?;

        Ok(Self {
            root,
            origin,
            height: layout.height,
            stroke,
            label_gap: 8.0 * scale,
            indicator_size,
            indicator_y: layout.height - indicator_size - (2.0 * scale).round(),
            frames: [shadow_host.cast()?, glass.cast()?, rim.cast()?],
            outline,
            rim_geometry,
            content,
            merge,
            merge_geometry,
            merge_stroke,
            raised: Cell::new(None),
            slots,
            badges,
            indicators,
            labels,
            remove_label,
        })
    }

    /// - `lifts`: how far each slot rises above its resting position (bounce).
    /// - `snap`: whether to align to pixels. Used at rest for sharpness; fractional
    ///   positions are retained during movement for smooth animation.
    pub fn apply(&self, frame: &Frame<'_>) -> Result<()> {
        let Frame {
            state,
            hovered,
            lifts,
            drag,
            target,
            slide,
            snap,
        } = *frame;
        let fit = |value: f32| if snap { value.round() } else { value };
        self.root.SetOffset(v3(0.0, slide))?;
        self.raise(drag.map(|drag| drag.index))?;
        self.place_merge(target.and_then(|index| {
            Some((
                *state.rects.get(index)?,
                lifts.get(index).copied().unwrap_or(0.0),
            ))
        }))?;

        let size = v2(fit(state.right - state.left), self.height);
        let offset = v3(self.origin.0 + fit(state.left), self.origin.1);
        for frame in &self.frames {
            frame.SetSize(size)?;
            frame.SetOffset(offset)?;
        }
        self.outline.SetSize(size)?;
        self.rim_geometry
            .SetSize(v2(size.X - self.stroke, size.Y - self.stroke))?;

        for (index, rect) in state.rects.iter().enumerate() {
            let Rect {
                x,
                y,
                width,
                height,
            } = *rect;
            let dragged = drag.filter(|drag| drag.index == index);
            let (x, y) = match dragged.filter(|drag| !drag.ghost) {
                Some(drag) => (drag.center.0 - width / 2.0, drag.center.1 - height / 2.0),
                None => (x, y - lifts.get(index).copied().unwrap_or(0.0)),
            };

            if let Some(visual) = self.slots.get(index) {
                visual.SetOffset(v3(fit(x), fit(y)))?;
                visual.SetSize(v2(fit(width), fit(height)))?;
                let opacity = match dragged {
                    Some(drag) if drag.removing => 0.55,
                    Some(drag) if drag.ghost => 0.35,
                    _ => 1.0,
                };
                visual.SetOpacity(opacity)?;
            }
            if let Some(Some(badge)) = self.badges.get(index) {
                let side = width * BADGE_SIZE;
                badge.SetSize(v2(fit(side), fit(side)))?;
                badge.SetOffset(v3(
                    fit(x + width - side * BADGE_INSIDE),
                    fit(y + height - side * BADGE_INSIDE),
                ))?;
            }
            if let Some(Some(indicator)) = self.indicators.get(index) {
                indicator.SetIsVisible(dragged.is_none())?;
                indicator.SetOffset(v3(
                    fit(x + (width - self.indicator_size) / 2.0),
                    self.indicator_y,
                ))?;
            }
            if let Some(Some(label)) = self.labels.get(index) {
                let shown = hovered == Some(index) && drag.is_none();
                label.visual.SetIsVisible(shown)?;
                if shown {
                    self.place_label(label, x, y, width)?;
                }
            }
            if let (Some(label), Some(drag)) = (&self.remove_label, dragged) {
                label.visual.SetIsVisible(drag.removing)?;
                if drag.removing {
                    self.place_label(label, x, y, width)?;
                }
            }
        }
        if drag.is_none()
            && let Some(label) = &self.remove_label
        {
            label.visual.SetIsVisible(false)?;
        }
        Ok(())
    }

    /// Brings the dragged slot above the others, once per drag (the scene
    /// is rebuilt whenever the order changes).
    fn raise(&self, index: Option<usize>) -> Result<()> {
        if index.is_none() || self.raised.get() == index {
            return Ok(());
        }
        if let Some(visual) = index.and_then(|index| self.slots.get(index)) {
            let children = self.content.Children()?;
            children.Remove(visual)?;
            children.InsertAtTop(visual)?;
        }
        self.raised.set(index);
        Ok(())
    }

    /// Frames `target` (its rectangle and how far it is lifted), or hides
    /// the frame.
    fn place_merge(&self, target: Option<(Rect, f32)>) -> Result<()> {
        let Some((rect, lift)) = target else {
            self.merge.SetIsVisible(false)?;
            return Ok(());
        };
        let pad = (rect.width * 0.1).round();
        let size = v2(rect.width + 2.0 * pad, rect.height + 2.0 * pad);
        self.merge
            .SetOffset(v3(rect.x - pad, rect.y - pad - lift))?;
        self.merge.SetSize(size)?;
        self.merge_geometry
            .SetSize(v2(size.X - self.merge_stroke, size.Y - self.merge_stroke))?;
        let radius = size.X * 0.26;
        self.merge_geometry.SetCornerRadius(v2(radius, radius))?;
        self.merge.SetIsVisible(true)?;
        Ok(())
    }

    fn place_label(&self, label: &Label, x: f32, y: f32, width: f32) -> Result<()> {
        label.visual.SetOffset(v3(
            (x + (width - label.width) / 2.0).round(),
            (y - self.label_gap - label.height).round(),
        ))?;
        Ok(())
    }
}
