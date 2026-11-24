//! Free-form drawing for widgets: text, color emoji, rounded rectangles and
//! arcs on a surface of any size.

use super::{Gfx, LabelBrush, rgb, text_font};
use anyhow::Result;
use windows::{
    Win32::Graphics::{
        Direct2D::{
            Common::{D2D_RECT_F, D2D_SIZE_F, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN},
            D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE, D2D1_ARC_SIZE_SMALL, D2D1_CAP_STYLE_ROUND,
            D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, D2D1_DRAW_TEXT_OPTIONS_NONE,
            D2D1_LINE_JOIN_ROUND, D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES,
            D2D1_SWEEP_DIRECTION_CLOCKWISE, ID2D1DeviceContext, ID2D1SolidColorBrush,
        },
        DirectWrite::{
            DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT,
            DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_TEXT_METRICS, DWRITE_WORD_WRAPPING_NO_WRAP, IDWriteFactory, IDWriteTextLayout,
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
}

impl Weight {
    fn dwrite(self) -> DWRITE_FONT_WEIGHT {
        match self {
            Self::Normal => DWRITE_FONT_WEIGHT_NORMAL,
            Self::Medium => DWRITE_FONT_WEIGHT_MEDIUM,
            Self::Bold => DWRITE_FONT_WEIGHT_SEMI_BOLD,
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

    /// Width and height of one line of interface text, in physical pixels.
    pub fn measure(&self, text: &str, size: f32, scale: f32, weight: Weight) -> Result<(f32, f32)> {
        let (_, width, height) = one_line(&self.dwrite, text, text_font(), size * scale, weight)?;
        Ok((width, height))
    }

    /// Width and height of a color emoji, in physical pixels.
    pub fn measure_emoji(&self, text: &str, size: f32, scale: f32) -> Result<(f32, f32)> {
        let (_, width, height) =
            one_line(&self.dwrite, text, EMOJI_FONT, size * scale, Weight::Normal)?;
        Ok((width, height))
    }
}
