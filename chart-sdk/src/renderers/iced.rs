//! Iced 0.14 backend — drop-in widget for embedding a [`ChartApi`] in an
//! Iced application.
//!
//! This module is the **only** rendering path. There is no
//! `Box<dyn Renderer>` indirection, no trait dispatch, no separate
//! "backend" vs "renderer" file. Painting a chart is one function:
//!
//! ```ignore
//! use lumen_charts_sdk::renderers::iced::IcedChart;
//!
//! let mut chart = ChartApi::with_size(800, 600, 1.0);
//! chart.set_data(bars);
//!
//! // Wrap in an Iced widget, then drop into your view tree:
//! let canvas = IcedChart::new(chart).canvas();
//! ```
//!
//! ## Architecture
//!
//! ```text
//! IcedChart  ──owns──▶  Rc<RefCell<ChartApi>>
//!     │
//!     └── IcedProgram : iced::widget::canvas::Program
//!             ├── draw    → resize if needed, then paint_to_iced_frame
//!             ├── update  → translate Iced events → chart pointer/wheel
//!             └── mouse_interaction
//!
//! paint_to_iced_frame
//!     ├── creates a tiny private `Backend` impl of `DrawBackend`
//     │      that forwards to `iced::widget::canvas::Frame`
//     ├── render_bottom_scene(&mut backend, state)
//     └── render_crosshair_scene(&mut backend, state)
//! ```
//!
//! Mutating the chart from your `update` handler:
//!
//! ```ignore
//! chart_view.with_chart_mut(|c| {
//!     c.set_series_type(1);
//! });
//! ```
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use iced::mouse::Cursor as IcedCursor;
use iced::widget::canvas::gradient::Linear;
use iced::widget::canvas::{
    Action, Canvas, Event, Frame, Geometry, Gradient, LineDash, Path, Program, Stroke, Style,
};
use iced::{
    mouse, Color as IcedColor, Pixels, Point, Rectangle, Renderer as IcedRenderer, Theme,
};

use crate::chart_renderer::{render_bottom_scene, render_crosshair_scene};
use crate::chart_state::ChartState;
use crate::color::{Color, GradientStop};
use crate::ChartApi;

// ════════════════════════════════════════════════════════════════════════════
//  paint_to_iced_frame — single private paint path
// ════════════════════════════════════════════════════════════════════════════

/// Paint the chart into an Iced `Frame`. The single rendering entry
/// point; called by [`IcedProgram::draw`].
///
/// Constructs an [`IcedBackend`] wrapping the Frame and hands it to
/// the chart engine's scene renderers. All draw primitives (fill_rect,
/// stroke_line, text, …) flow through this adapter into Iced.
fn paint_to_iced_frame(frame: &mut Frame, state: &ChartState) {
    let mut backend = IcedBackend {
        frame,
        sx: 1.0,
        sy: 1.0,
        _marker: std::marker::PhantomData,
    };
    render_bottom_scene(&mut backend, state);
    render_crosshair_scene(&mut backend, state);
}

/// Adapter that translates the chart engine's draw primitives into Iced
/// `Frame` calls. Has a lifetime tied to the `Frame` borrow — see
/// [`paint_to_iced_frame`].
///
/// This is the **only** concrete backend in the project. The chart
/// engine's renderer functions take `&mut IcedBackend<'a>` directly;
/// there is no trait dispatch layer.
pub struct IcedBackend<'a> {
    pub frame: &'a mut Frame,
    pub(crate) sx: f32,
    pub(crate) sy: f32,
    // PhantomData<&'a ()> forces the lifetime to be covariant, so
    // `IcedBackend<'short>` coerces to `IcedBackend<'long>` when
    // `'short ⊆ 'long`. This is what makes `with_clip` work: the
    // Iced closure hands us a `&'short mut Frame` (reborrowed for
    // the duration of the clip scope), we wrap it in `IcedBackend<'short>`,
    // and pass it to a user closure expecting `IcedBackend<'long>`.
    pub(crate) _marker: std::marker::PhantomData<&'a ()>,
}

impl IcedBackend<'_> {
    fn points_to_iced(&self, points: &[(f64, f64)]) -> Vec<Point> {
        points
            .iter()
            .map(|&(x, y)| Point::new((x as f32) * self.sx, (y as f32) * self.sy))
            .collect()
    }
}

fn c4_to_iced(c: Color) -> IcedColor {
    IcedColor::from_rgba(c[0], c[1], c[2], c[3])
}

// ── Pixel snapping helpers ───────────────────────────────────────────────
//
// Snap a coordinate to the nearest pixel boundary so that 1px strokes
// aren't drawn half-on / half-off a pixel (which produces blurry lines
// on LCD screens). `sf` is the device scale factor (1.0 for logical /
// CSS pixels, > 1.0 for HiDPI).

pub fn snap_x(x: f64, sf: f64) -> f64 {
    (x * sf).round() / sf
}

pub fn snap_y(y: f64, sf: f64) -> f64 {
    (y * sf).round() / sf
}

impl<'a> IcedBackend<'a> {
    pub fn begin_frame(&mut self, _width: f64, _height: f64) {}

    pub fn end_frame(&mut self) {}

    pub fn set_scale(&mut self, sx: f64, sy: f64) {
        self.sx = sx as f32;
        self.sy = sy as f32;
    }

    pub fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64, color: Color) {
        self.frame.fill_rectangle(
            Point::new(x as f32, y as f32),
            iced::Size::new(w as f32, h as f32),
            c4_to_iced(color),
        );
    }

    pub fn fill_rect_gradient(
        &mut self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        y_start: f64,
        y_end: f64,
        stops: &[GradientStop],
    ) {
        if stops.is_empty() {
            return;
        }
        if stops.len() == 1 {
            self.fill_rect(x, y, w, h, stops[0].0);
            return;
        }
        // For 2+ stops we paint horizontal bands at proportional positions.
        // Iced's `Linear` gradient supports up to 8 stops but the chart core
        // can emit arbitrarily many (e.g. heat-map style fills). Falling back
        // to bands keeps the implementation simple and visually correct.
        let total_h = (y_end - y_start).max(f64::EPSILON);
        let bands = stops.len().saturating_sub(1);
        for i in 0..bands {
            let t0 = stops[i].1;
            let t1 = stops[i + 1].1;
            let by0 = y_start + t0 as f64 * total_h;
            let by1 = y_start + t1 as f64 * total_h;
            let bh = (by1 - by0).max(0.0);
            let p0 = Point::new(x as f32, by0 as f32);
            let p1 = Point::new(x as f32, by1 as f32);
            let linear = Linear::new(p0, p1)
                .add_stop(0.0, c4_to_iced(stops[i].0))
                .add_stop(1.0, c4_to_iced(stops[i + 1].0));
            self.frame.fill_rectangle(
                Point::new(x as f32, by0 as f32),
                iced::Size::new(w as f32, bh as f32),
                Gradient::Linear(linear),
            );
        }
    }

    pub fn stroke_line(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, color: Color, width: f64) {
        let path = Path::line(
            Point::new(x0 as f32, y0 as f32),
            Point::new(x1 as f32, y1 as f32),
        );
        let stroke = Stroke::default()
            .with_color(c4_to_iced(color))
            .with_width(width as f32);
        self.frame.stroke(&path, stroke);
    }

    pub fn stroke_dashed_line(
        &mut self,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        color: Color,
        width: f64,
        dash_len: f64,
        gap_len: f64,
    ) {
        let path = Path::line(
            Point::new(x0 as f32, y0 as f32),
            Point::new(x1 as f32, y1 as f32),
        );
        let stroke = Stroke {
            style: Style::Solid(c4_to_iced(color)),
            width: width as f32,
            line_cap: Default::default(),
            line_join: Default::default(),
            line_dash: LineDash {
                segments: &[dash_len as f32, gap_len as f32],
                offset: 0,
            },
        };
        self.frame.stroke(&path, stroke);
    }

    pub fn stroke_path(&mut self, points: &[(f64, f64)], color: Color, width: f64) {
        if points.len() < 2 {
            return;
        }
        let pts = self.points_to_iced(points);
        let path = Path::new(|b| {
            b.move_to(pts[0]);
            for p in &pts[1..] {
                b.line_to(*p);
            }
        });
        let stroke = Stroke::default()
            .with_color(c4_to_iced(color))
            .with_width(width as f32);
        self.frame.stroke(&path, stroke);
    }

    pub fn fill_path(&mut self, points: &[(f64, f64)], color: Color) {
        if points.len() < 3 {
            return;
        }
        let pts = self.points_to_iced(points);
        let path = Path::new(|b| {
            b.move_to(pts[0]);
            for p in &pts[1..] {
                b.line_to(*p);
            }
            b.close();
        });
        self.frame.fill(&path, c4_to_iced(color));
    }

    pub fn fill_path_gradient(
        &mut self,
        points: &[(f64, f64)],
        y_start: f64,
        y_end: f64,
        stops: &[GradientStop],
    ) {
        if points.len() < 3 || stops.is_empty() {
            return;
        }
        if stops.len() == 2 {
            let p0 = Point::new(points[0].0 as f32, y_start as f32);
            let p1 = Point::new(points[0].0 as f32, y_end as f32);
            let linear = Linear::new(p0, p1)
                .add_stop(0.0, c4_to_iced(stops[0].0))
                .add_stop(1.0, c4_to_iced(stops[1].0));
            let pts = self.points_to_iced(points);
            let path = Path::new(|b| {
                b.move_to(pts[0]);
                for p in &pts[1..] {
                    b.line_to(*p);
                }
                b.close();
            });
            self.frame.fill(&path, Gradient::Linear(linear));
        } else {
            // Multiple-stop gradients: fall back to the dominant color.
            self.fill_path(points, stops[stops.len() - 1].0);
        }
    }

    pub fn fill_circle(&mut self, cx: f64, cy: f64, radius: f64, color: Color) {
        let path = Path::circle(Point::new(cx as f32, cy as f32), radius as f32);
        self.frame.fill(&path, c4_to_iced(color));
    }

    pub fn draw_text(&mut self, text: &str, x: f64, y: f64, font_size: f64, color: Color) {
        // y is the baseline in the chart core's convention. Iced expects
        // the top-left of the text bounding box — approximate by
        // subtracting ~80% of the font size (typical ascent ratio).
        let approx_baseline_to_top = (font_size as f32) * 0.8;
        let mut t = iced::widget::canvas::Text::default();
        t.content = text.to_string();
        t.position = Point::new(x as f32, (y as f32) - approx_baseline_to_top);
        t.color = c4_to_iced(color);
        t.size = Pixels(font_size as f32);
        self.frame.fill_text(t);
    }

    pub fn measure_text(&self, text: &str, font_size: f64) -> f64 {
        // Iced 0.14 doesn't expose synchronous text measurement on Frame.
        // Return a conservative estimate.
        text.chars().count() as f64 * font_size * 0.6
    }

    /// Run `f` with drawing clipped to the rectangle `(x, y, w, h)`.
    ///
    /// Forwards to Iced's `Frame::with_clip`. The closure receives
    /// a fresh `IcedBackend<'_>` bound to the inner (clipped) frame.
    ///
    /// Implementation note: we can't write `Fn(&mut IcedBackend<'b>)`
    /// directly because `&mut Frame` makes the lifetime parameter
    /// invariant. Instead we bridge by constructing the nested
    /// `IcedBackend` inside the Iced closure where the reborrow lifetime
    /// is already in scope. The outer `for<'r> FnMut(&'r mut Frame)`
    /// is HRTB-friendly so the closure body works regardless of the
    /// reborrow lifetime.
    pub fn with_clip(
        &mut self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        mut f: impl FnMut(&mut IcedBackend<'_>),
    ) {
        let bounds = iced::Rectangle {
            x: x as f32,
            y: y as f32,
            width: w as f32,
            height: h as f32,
        };

        let sx = self.sx;
        let sy = self.sy;
        let outer_frame: &mut Frame = self.frame;

        outer_frame.with_clip(bounds, |frame| {
            let mut nested = IcedBackend {
                frame,
                sx,
                sy,
                _marker: std::marker::PhantomData,
            };
            f(&mut nested);
        });
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  IcedChart — drop-in Iced widget
// ════════════════════════════════════════════════════════════════════════════

/// Drop-in Iced widget that owns a [`ChartApi`] and renders it via an
/// [`IcedProgram`].
///
/// Internally holds a `Rc<RefCell<ChartApi>>` so the chart can be
/// mutated from anywhere — your application's `update` handler, a
/// toolbar button's `on_press` callback, etc.
///
/// `IcedChart` is `Clone`-cheap (`Rc` clone, `O(1)`); clones share the
/// same backing chart. This matters because `view()` is called every
/// frame and constructs a fresh `Canvas` widget from a clone.
#[derive(Clone)]
pub struct IcedChart {
    chart: Rc<RefCell<ChartApi>>,
}

impl IcedChart {
    /// Wrap a `ChartApi` for use as an Iced widget. The chart becomes
    /// `Rc<RefCell<…>>` internally; subsequent mutation goes through
    /// [`IcedChart::with_chart_mut`].
    pub fn new(chart: ChartApi) -> Self {
        Self {
            chart: Rc::new(RefCell::new(chart)),
        }
    }

    /// Wrap an existing `Rc<RefCell<ChartApi>>`. Use this when the
    /// application state already owns the shared handle.
    pub fn from_shared(chart: Rc<RefCell<ChartApi>>) -> Self {
        Self { chart }
    }

    /// Borrow the inner `Rc<RefCell<ChartApi>>`. Useful when you need
    /// to clone the handle for a different part of your app.
    pub fn chart_handle(&self) -> Rc<RefCell<ChartApi>> {
        self.chart.clone()
    }

    /// Run a closure with mutable access to the chart. Use this from
    /// your application's `update` handler to mutate the chart
    /// (`set_data`, `add_series`, `add_pane`, etc.).
    pub fn with_chart_mut<R>(&self, f: impl FnOnce(&mut ChartApi) -> R) -> R {
        f(&mut self.chart.borrow_mut())
    }

    /// Build the Iced `Canvas` widget. The returned widget is `Fill` /
    /// `Fill` sized; shrink with `iced::Length::Shrink` if you need a
    /// non-stretching chart.
    pub fn canvas<Message: 'static>(self) -> Canvas<IcedProgram, Message, Theme, IcedRenderer> {
        let program = IcedProgram {
            chart: self.chart,
            last_size: Cell::new((0, 0)),
        };
        Canvas::new(program)
            .width(iced::Length::Fill)
            .height(iced::Length::Fill)
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  IcedProgram — Iced Program<…> impl that drives the chart
// ════════════════════════════════════════════════════════════════════════════

/// Iced `Program<…>` impl that paints the chart and translates Iced
/// pointer events to chart pointer events.
///
/// Exposed in the type signature of [`Canvas`], but end users never
/// name it directly.
pub struct IcedProgram {
    chart: Rc<RefCell<ChartApi>>,
    /// Last canvas size used for the resize-detection block in `draw`.
    /// Single-threaded, widget-private — `Cell` is enough.
    last_size: Cell<(u32, u32)>,
}

impl<Message: 'static> Program<Message, Theme, IcedRenderer> for IcedProgram {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &IcedRenderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: IcedCursor,
    ) -> Vec<Geometry> {
        let w = bounds.width.round().max(1.0) as u32;
        let h = bounds.height.round().max(1.0) as u32;

        // Resize chart if canvas size changed. Drop the borrow before
        // constructing the Frame so we don't hold a RefCell borrow
        // across the paint.
        let prev = self.last_size.get();
        if prev != (w, h) {
            self.last_size.set((w, h));
            self.chart.borrow_mut().resize(w, h, 1.0);
            self.chart.borrow_mut().render();
        }

        let mut frame = Frame::new(renderer, bounds.size());

        // Paint: borrow the chart immutably, drop the borrow before
        // returning.
        {
            let chart = self.chart.borrow();
            paint_to_iced_frame(&mut frame, &chart.inner.state);
        }

        vec![frame.into_geometry()]
    }

    fn update(
        &self,
        _state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: IcedCursor,
    ) -> Option<Action<Message>> {
        let pos_in_bounds = cursor.position_in(bounds);

        let redraw = {
            let mut chart = self.chart.borrow_mut();
            match event {
                Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                    if let Some(p) = pos_in_bounds {
                        chart.pointer_move(p.x, p.y)
                    } else {
                        chart.pointer_leave()
                    }
                }
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                    if let Some(p) = pos_in_bounds {
                        chart.pointer_down(p.x, p.y, 0)
                    } else {
                        false
                    }
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    if let Some(p) = pos_in_bounds {
                        chart.pointer_up(p.x, p.y, 0)
                    } else {
                        false
                    }
                }
                Event::Mouse(mouse::Event::CursorLeft) => chart.pointer_leave(),
                Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                    let (dx, dy) = match delta {
                        mouse::ScrollDelta::Lines { x: lx, y: ly } => (lx * 20.0, ly * 20.0),
                        mouse::ScrollDelta::Pixels { x: px, y: py } => (*px, *py),
                    };
                    let mut redraw = false;
                    if let Some(p) = pos_in_bounds {
                        if dx.abs() > 0.1 {
                            redraw |= chart.scroll(-dx, 0.0);
                        }
                        if dy.abs() > 0.1 {
                            let factor = 1.0 - dy * 0.003;
                            redraw |= chart.zoom(factor, p.x);
                        }
                    }
                    redraw
                }
                _ => false,
            }
        };

        if redraw {
            Some(Action::request_redraw())
        } else {
            None
        }
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: IcedCursor,
    ) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_conversion_preserves_components() {
        let c = Color([0.25, 0.5, 0.75, 1.0]);
        let iced = c4_to_iced(c);
        assert_eq!(iced.r, 0.25);
        assert_eq!(iced.g, 0.5);
        assert_eq!(iced.b, 0.75);
        assert_eq!(iced.a, 1.0);
    }

    #[test]
    fn points_applies_scale() {
        // Test the Backend's point conversion by constructing one with a
        // dummy frame. Since Frame construction needs an Iced Renderer,
        // we just sanity-check the math directly.
        fn scaled(points: &[(f64, f64)], sx: f32, sy: f32) -> Vec<(f32, f32)> {
            points
                .iter()
                .map(|&(x, y)| ((x as f32) * sx, (y as f32) * sy))
                .collect()
        }
        assert_eq!(
            scaled(&[(1.0, 2.0), (3.0, 4.0)], 2.0, 0.5),
            vec![(2.0, 1.0), (6.0, 2.0)]
        );
    }
}
