//! Free-form drawing for widgets: text, color emoji, rounded rectangles and
//! arcs on a surface of any size.

use super::{Gfx, LabelBrush, draw_image_in, icon_font, rgb, text_font};
use muelle_shell::icons::IconImage;
use anyhow::Result;
use windows::{
    Win32::Graphics::{
        Direct2D::{
            Common::{
                D2D_RECT_F, D2D_SIZE_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW,
                D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN, D2D1_GRADIENT_STOP,
            },
            D2D1_COMBINE_MODE_EXCLUDE, D2D1_COMBINE_MODE_INTERSECT, D2D1_ELLIPSE,
            D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE, D2D1_ARC_SIZE_SMALL,
            D2D1_BUFFER_PRECISION_8BPC_UNORM, D2D1_CAP_STYLE_ROUND,
            D2D1_COLOR_INTERPOLATION_MODE_PREMULTIPLIED, D2D1_COLOR_SPACE_SRGB,
            D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, D2D1_DRAW_TEXT_OPTIONS_NONE,
            D2D1_EXTEND_MODE_CLAMP, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
            D2D1_LINE_JOIN_ROUND, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_ROUNDED_RECT,
            D2D1_STROKE_STYLE_PROPERTIES,
            D2D1_SWEEP_DIRECTION_CLOCKWISE, ID2D1DeviceContext, ID2D1SolidColorBrush,
        },
        DirectWrite::{
            DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT,
            DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_METRICS, DWRITE_WORD_WRAPPING_NO_WRAP, IDWriteFactory, IDWriteTextLayout,
        },
    },
    core::{PCWSTR, w},
};
use windows_numerics::Vector2;

/// Text weight for [`Pen`] and [`Gfx::measure`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    Normal,
    Medium,
    Bold,
    Heavy,
}

impl Weight {
    fn dwrite(self) -> DWRITE_FONT_WEIGHT {
        match self {
            Self::Normal => DWRITE_FONT_WEIGHT_NORMAL,
            Self::Medium => DWRITE_FONT_WEIGHT_MEDIUM,
            Self::Bold => DWRITE_FONT_WEIGHT_SEMI_BOLD,
            Self::Heavy => DWRITE_FONT_WEIGHT_BOLD,
        }
    }
}

/// An RGBA color, components from 0 to 1.
pub type Rgba = [f32; 4];

const EMOJI_FONT: PCWSTR = w!("Segoe UI Emoji");

fn one_line(
    dwrite: &IDWriteFactory,
    text: &str,
    font: PCWSTR,
    size: f32,
    weight: Weight,
) -> Result<(IDWriteTextLayout, f32, f32)> {
    let utf16: Vec<u16> = text.encode_utf16().collect();
    // SAFETY: The strings and structures live for each call.
    unsafe {
        let format = dwrite.CreateTextFormat(
            font,
            None,
            weight.dwrite(),
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            size,
            w!(""),
        )?;
        let layout = dwrite.CreateTextLayout(&utf16, &format, 4096.0, 512.0)?;
        layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
        let mut metrics = DWRITE_TEXT_METRICS::default();
        layout.GetMetrics(&mut metrics)?;
        Ok((layout, metrics.width, metrics.height))
    }
}

/// A wrapped paragraph `width` pixels wide: its layout and height.
fn paragraph(
    dwrite: &IDWriteFactory,
    text: &str,
    size: f32,
    weight: Weight,
    width: f32,
) -> Result<(IDWriteTextLayout, f32)> {
    let utf16: Vec<u16> = text.encode_utf16().collect();
    // SAFETY: The strings and structures live for each call.
    unsafe {
        let format = dwrite.CreateTextFormat(
            text_font(),
            None,
            weight.dwrite(),
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            size,
            w!(""),
        )?;
        let layout = dwrite.CreateTextLayout(&utf16, &format, width.max(1.0), 4096.0)?;
        let mut metrics = DWRITE_TEXT_METRICS::default();
        layout.GetMetrics(&mut metrics)?;
        Ok((layout, metrics.height))
    }
}

/// Draws in physical pixels; text sizes are logical points, multiplied by `scale`.
pub struct Pen<'a> {
    dc: &'a ID2D1DeviceContext,
    dwrite: &'a IDWriteFactory,
    pub scale: f32,
}

impl Pen<'_> {
    fn brush(&self, color: Rgba) -> Result<ID2D1SolidColorBrush> {
        // SAFETY: Plain resource creation on the live context.
        Ok(unsafe {
            self.dc
                .CreateSolidColorBrush(&rgb(color[0], color[1], color[2], color[3]), None)?
        })
    }

    /// One line of text with its top left at (`x`, `y`). Returns its width.
    pub fn text(
        &self,
        text: &str,
        (x, y): (f32, f32),
        size: f32,
        weight: Weight,
        color: Rgba,
    ) -> Result<f32> {
        self.run(text, text_font(), (x, y), size, weight, color, false)
    }

    /// Like [`Pen::text`], but only the part between `clip_left` and
    /// `clip_right` (x) shows. Returns the text's full width.
    pub fn text_clipped(
        &self,
        text: &str,
        (x, y): (f32, f32),
        size: f32,
        weight: Weight,
        color: Rgba,
        (clip_left, clip_right): (f32, f32),
    ) -> Result<f32> {
        // SAFETY: The clip is popped below, always, so the stack stays balanced.
        unsafe {
            self.dc.PushAxisAlignedClip(
                &D2D_RECT_F { left: clip_left, top: -1e4, right: clip_right, bottom: 1e4 },
                D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            );
        }
        let drawn = self.text(text, (x, y), size, weight, color);
        // SAFETY: Pairs the push above.
        unsafe { self.dc.PopAxisAlignedClip() };
        drawn
    }

    /// Text wrapped to `width`, top left at (`x`, `y`). Returns its height.
    pub fn paragraph(
        &self,
        text: &str,
        (x, y): (f32, f32),
        width: f32,
        size: f32,
        color: Rgba,
    ) -> Result<f32> {
        let (layout, height) = paragraph(self.dwrite, text, size * self.scale, Weight::Normal, width)?;
        let ink = self.brush(color)?;
        // SAFETY: The layout and brush live for the call.
        unsafe {
            self.dc.DrawTextLayout(
                Vector2 { X: x, Y: y },
                &layout,
                &ink,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }
        Ok(height)
    }

    /// A symbol from the system icon font (Segoe Fluent Icons).
    pub fn icon(&self, glyph: char, (x, y): (f32, f32), size: f32, color: Rgba) -> Result<f32> {
        let mut buffer = [0u8; 4];
        self.run(glyph.encode_utf8(&mut buffer), icon_font(), (x, y), size, Weight::Normal, color, false)
    }

    /// A color emoji (weather symbols) with its top left at (`x`, `y`).
    pub fn emoji(&self, text: &str, (x, y): (f32, f32), size: f32) -> Result<f32> {
        self.run(text, EMOJI_FONT, (x, y), size, Weight::Normal, [1.0; 4], true)
    }

    #[expect(clippy::too_many_arguments, reason = "One text run and where it goes")]
    fn run(
        &self,
        text: &str,
        font: PCWSTR,
        (x, y): (f32, f32),
        size: f32,
        weight: Weight,
        color: Rgba,
        colored: bool,
    ) -> Result<f32> {
        let (layout, width, _) = one_line(self.dwrite, text, font, size * self.scale, weight)?;
        let ink = self.brush(color)?;
        let options = if colored {
            D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT
        } else {
            D2D1_DRAW_TEXT_OPTIONS_NONE
        };
        // SAFETY: The layout and brush live for the call.
        unsafe {
            self.dc
                .DrawTextLayout(Vector2 { X: x, Y: y }, &layout, &ink, options);
        }
        Ok(width)
    }

    /// An image (a logo) stretched over the given box.
    pub fn image(&self, image: &IconImage, (x, y): (f32, f32), (width, height): (f32, f32)) -> Result<()> {
        draw_image_in(
            self.dc,
            image,
            D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            },
            D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
        )
    }

    pub fn round_rect(
        &self,
        (x, y): (f32, f32),
        (width, height): (f32, f32),
        radius: f32,
        color: Rgba,
    ) -> Result<()> {
        let fill = self.brush(color)?;
        // SAFETY: The brush lives for the call.
        unsafe {
            self.dc.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: x,
                        top: y,
                        right: x + width,
                        bottom: y + height,
                    },
                    radiusX: radius,
                    radiusY: radius,
                },
                &fill,
            );
        }
        Ok(())
    }

    /// A rounded rectangle filled with a linear gradient from `from` to `to`
    /// (absolute coordinates), with (position, colour) stops.
    pub fn gradient_rect(
        &self,
        (x, y): (f32, f32),
        (width, height): (f32, f32),
        radius: f32,
        from: (f32, f32),
        to: (f32, f32),
        stops: &[(f32, Rgba)],
    ) -> Result<()> {
        let stops: Vec<D2D1_GRADIENT_STOP> = stops
            .iter()
            .map(|&(position, color)| D2D1_GRADIENT_STOP {
                position,
                color: rgb(color[0], color[1], color[2], color[3]),
            })
            .collect();
        // SAFETY: The stop collection and brush live for the drawing call.
        unsafe {
            let collection = self.dc.CreateGradientStopCollection(
                &stops,
                D2D1_COLOR_SPACE_SRGB,
                D2D1_COLOR_SPACE_SRGB,
                D2D1_BUFFER_PRECISION_8BPC_UNORM,
                D2D1_EXTEND_MODE_CLAMP,
                D2D1_COLOR_INTERPOLATION_MODE_PREMULTIPLIED,
            )?;
            let gradient = self.dc.CreateLinearGradientBrush(
                &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                    startPoint: Vector2 { X: from.0, Y: from.1 },
                    endPoint: Vector2 { X: to.0, Y: to.1 },
                },
                None,
                &collection,
            )?;
            self.dc.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: x,
                        top: y,
                        right: x + width,
                        bottom: y + height,
                    },
                    radiusX: radius,
                    radiusY: radius,
                },
                &gradient,
            );
        }
        Ok(())
    }

    /// An arc with round ends around `center`. Angles in degrees, clockwise
    /// from 12 o'clock.
    pub fn arc(
        &self,
        center: (f32, f32),
        radius: f32,
        stroke: f32,
        (from, sweep): (f32, f32),
        color: Rgba,
    ) -> Result<()> {
        if sweep <= 0.05 {
            return Ok(());
        }
        let sweep = sweep.min(359.9);
        let point = |degrees: f32| {
            let r = (degrees - 90.0).to_radians();
            Vector2 {
                X: center.0 + radius * r.cos(),
                Y: center.1 + radius * r.sin(),
            }
        };
        let ink = self.brush(color)?;
        // SAFETY: Geometry, sink and stroke style live for the drawing call.
        unsafe {
            let factory = self.dc.GetFactory()?;
            let path = factory.CreatePathGeometry()?;
            let sink = path.Open()?;
            sink.BeginFigure(point(from), D2D1_FIGURE_BEGIN_HOLLOW);
            sink.AddArc(&D2D1_ARC_SEGMENT {
                point: point(from + sweep),
                size: D2D_SIZE_F {
                    width: radius,
                    height: radius,
                },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: if sweep > 180.0 {
                    D2D1_ARC_SIZE_LARGE
                } else {
                    D2D1_ARC_SIZE_SMALL
                },
            });
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            sink.Close()?;
            let style = factory.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    dashCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    miterLimit: 10.0,
                    ..Default::default()
                },
                None,
            )?;
            self.dc.DrawGeometry(&path, &ink, stroke, &style);
        }
        Ok(())
    }

    /// The outline of a rounded rectangle, `stroke` wide, centred on its edge.
    pub fn stroke_round_rect(&self, (x, y): (f32, f32), (width, height): (f32, f32), radius: f32, stroke: f32, color: Rgba) -> Result<()> {
        let ink = self.brush(color)?;
        // SAFETY: The brush lives for the call.
        unsafe {
            self.dc.DrawRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: x, top: y, right: x + width, bottom: y + height },
                    radiusX: radius,
                    radiusY: radius,
                },
                &ink,
                stroke,
                None,
            );
        }
        Ok(())
    }

    /// A straight line with round ends.
    pub fn line(&self, from: (f32, f32), to: (f32, f32), stroke: f32, color: Rgba) -> Result<()> {
        let ink = self.brush(color)?;
        // SAFETY: The brush and stroke style live for the drawing call.
        unsafe {
            let style = self.dc.GetFactory()?.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    dashCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    miterLimit: 10.0,
                    ..Default::default()
                },
                None,
            )?;
            self.dc.DrawLine(
                Vector2 { X: from.0, Y: from.1 },
                Vector2 { X: to.0, Y: to.1 },
                &ink,
                stroke,
                &style,
            );
        }
        Ok(())
    }

    /// A crescent moon: a disc of `radius` at `center` with a disc of the
    /// same size, moved by `bite`, cut out of it.
    pub fn crescent(&self, center: (f32, f32), radius: f32, bite: (f32, f32), color: Rgba) -> Result<()> {
        let fill = self.brush(color)?;
        // SAFETY: Geometries, sink and brush live for the drawing call.
        unsafe {
            let factory = self.dc.GetFactory()?;
            let disc = |cx: f32, cy: f32| {
                factory.CreateEllipseGeometry(&D2D1_ELLIPSE {
                    point: Vector2 { X: cx, Y: cy },
                    radiusX: radius,
                    radiusY: radius,
                })
            };
            let moon = disc(center.0, center.1)?;
            let shadow = disc(center.0 + bite.0, center.1 + bite.1)?;
            let shape = factory.CreatePathGeometry()?;
            let sink = shape.Open()?;
            moon.CombineWithGeometry(&shadow, D2D1_COMBINE_MODE_EXCLUDE, None, 0.25, &sink)?;
            sink.Close()?;
            self.dc.FillGeometry(&shape, &fill, None);
        }
        Ok(())
    }

    /// Liquid inside a rounded box: everything under a sine wave whose rest
    /// line is `surface` (absolute y), `amplitude` high and `wavelength`
    /// long, shifted by `phase` (radians), cut to the box's rounded shape and
    /// filled top to bottom from `top` to `bottom`.
    pub fn wave(
        &self,
        (x, y): (f32, f32),
        (width, height): (f32, f32),
        radius: f32,
        surface: f32,
        (amplitude, wavelength, phase): (f32, f32, f32),
        (top, bottom): (Rgba, Rgba),
    ) -> Result<()> {
        // SAFETY: Geometries, sinks and brushes live for the drawing call.
        unsafe {
            let factory = self.dc.GetFactory()?;
            let wave = factory.CreatePathGeometry()?;
            let sink = wave.Open()?;
            let at = |px: f32| surface + amplitude * (std::f32::consts::TAU * (px - x) / wavelength.max(1.0) + phase).sin();
            sink.BeginFigure(Vector2 { X: x, Y: at(x) }, D2D1_FIGURE_BEGIN_FILLED);
            let step = 2.0_f32.max(width / 48.0);
            let mut px = x + step;
            while px < x + width {
                sink.AddLine(Vector2 { X: px, Y: at(px) });
                px += step;
            }
            sink.AddLine(Vector2 { X: x + width, Y: at(x + width) });
            sink.AddLine(Vector2 { X: x + width, Y: y + height });
            sink.AddLine(Vector2 { X: x, Y: y + height });
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            sink.Close()?;
            let shape = factory.CreateRoundedRectangleGeometry(&D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: x, top: y, right: x + width, bottom: y + height },
                radiusX: radius,
                radiusY: radius,
            })?;
            let liquid = factory.CreatePathGeometry()?;
            let inside = liquid.Open()?;
            wave.CombineWithGeometry(&shape, D2D1_COMBINE_MODE_INTERSECT, None, 0.25, &inside)?;
            inside.Close()?;
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgb(top[0], top[1], top[2], top[3]) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgb(bottom[0], bottom[1], bottom[2], bottom[3]) },
            ];
            let collection = self.dc.CreateGradientStopCollection(
                &stops,
                D2D1_COLOR_SPACE_SRGB,
                D2D1_COLOR_SPACE_SRGB,
                D2D1_BUFFER_PRECISION_8BPC_UNORM,
                D2D1_EXTEND_MODE_CLAMP,
                D2D1_COLOR_INTERPOLATION_MODE_PREMULTIPLIED,
            )?;
            let fill = self.dc.CreateLinearGradientBrush(
                &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                    startPoint: Vector2 { X: x, Y: surface - amplitude },
                    endPoint: Vector2 { X: x, Y: y + height },
                },
                None,
                &collection,
            )?;
            self.dc.FillGeometry(&liquid, &fill, None);
        }
        Ok(())
    }
}

impl Gfx {
    /// A `width` × `height` (physical pixels) surface painted through a [`Pen`].
    pub fn paint(
        &self,
        (width, height): (f32, f32),
        scale: f32,
        paint: impl FnOnce(&Pen<'_>) -> Result<()>,
    ) -> Result<LabelBrush> {
        let (width, height) = (width.ceil().max(1.0), height.ceil().max(1.0));
        let brush = self.draw(width as u32, height as u32, |dc| {
            paint(&Pen {
                dc,
                dwrite: &self.dwrite,
                scale,
            })
        })?;
        Ok(LabelBrush {
            brush,
            width,
            height,
        })
    }

    /// Like [`Gfx::paint`], but draws again on `reuse`'s surface when it has
    /// this size, so a repaint keeps its surface and brush (no new GPU
    /// allocation, nothing for the visual to swap). Another size, or none,
    /// gets a new surface.
    pub fn paint_into(
        &self,
        reuse: Option<&windows::UI::Composition::CompositionSurfaceBrush>,
        (width, height): (f32, f32),
        scale: f32,
        paint: impl FnOnce(&Pen<'_>) -> Result<()>,
    ) -> Result<LabelBrush> {
        let (w, h) = (width.ceil().max(1.0), height.ceil().max(1.0));
        let surface = reuse.and_then(|brush| {
            let surface: windows::UI::Composition::CompositionDrawingSurface =
                windows::core::Interface::cast(&brush.Surface().ok()?).ok()?;
            let size = surface.SizeInt32().ok()?;
            (size.Width == w as i32 && size.Height == h as i32).then_some(surface)
        });
        if let (Some(brush), Some(surface)) = (reuse, surface) {
            self.redraw_surface(&surface, |dc| {
                paint(&Pen {
                    dc,
                    dwrite: &self.dwrite,
                    scale,
                })
            })?;
            return Ok(LabelBrush {
                brush: brush.clone(),
                width: w,
                height: h,
            });
        }
        self.paint((width, height), scale, paint)
    }

    /// Width and height of one line of interface text, in physical pixels.
    pub fn measure(&self, text: &str, size: f32, scale: f32, weight: Weight) -> Result<(f32, f32)> {
        let (_, width, height) = one_line(&self.dwrite, text, text_font(), size * scale, weight)?;
        Ok((width, height))
    }

    /// Height of text wrapped to `width` physical pixels.
    pub fn measure_paragraph(&self, text: &str, size: f32, scale: f32, width: f32) -> Result<f32> {
        Ok(paragraph(&self.dwrite, text, size * scale, Weight::Normal, width)?.1)
    }

    /// Width and height of a system icon font symbol, in physical pixels.
    pub fn measure_icon(&self, glyph: char, size: f32, scale: f32) -> Result<(f32, f32)> {
        let mut buffer = [0u8; 4];
        let (_, width, height) =
            one_line(&self.dwrite, glyph.encode_utf8(&mut buffer), icon_font(), size * scale, Weight::Normal)?;
        Ok((width, height))
    }

    /// Width and height of a color emoji, in physical pixels.
    pub fn measure_emoji(&self, text: &str, size: f32, scale: f32) -> Result<(f32, f32)> {
        let (_, width, height) =
            one_line(&self.dwrite, text, EMOJI_FONT, size * scale, Weight::Normal)?;
        Ok((width, height))
    }
}
