use iced::widget::canvas::gradient::Linear;
use iced::widget::canvas::{Frame, Gradient, LineDash, Path, Stroke, Style};
use iced::{Pixels, Point};

use crate::chart::chart_state::ChartState;
use crate::chart::color::{Color, GradientStop};
use crate::render::{
    render_bottom_scene, render_crosshair_for_pane, render_crosshair_scene, render_pane,
    render_pane_axes,
};

/// Paint the chart into an Iced `Frame`. The single rendering entry
/// point; called by [`IcedProgram::draw`].
///
/// Constructs an [`IcedBackend`] wrapping the Frame and hands it to
/// the chart engine's scene renderers. All draw primitives (fill_rect,
/// stroke_line, text, …) flow through this adapter into Iced.
pub(crate) fn paint_to_iced_frame(frame: &mut Frame, state: &ChartState) {
    let mut backend = IcedBackend {
        frame,
        sx: 1.0,
        sy: 1.0,
        _marker: std::marker::PhantomData,
    };
    render_bottom_scene(&mut backend, state);
    render_crosshair_scene(&mut backend, state);
}

/// Paint a single pane's content into the given frame.
///
/// The frame is the bounds of a single Canvas widget in the multi-
/// canvas widget tree produced by [`ChartWithSeparators::view`]. The
/// chart's overall layout uses absolute coordinates, so we shift the
/// frame by `-pane.layout_rect.y` so that this pane's content lands at
/// the top-left of its canvas.
pub(crate) fn paint_pane_to_iced_frame(pane_index: usize, frame: &mut Frame, state: &ChartState) {
    let (pane_y, pane_h) = state
        .pane_size(pane_index as u32)
        .map(|(_, y, _, h)| (y, h))
        .unwrap_or((0.0, 0.0));

    frame.translate(iced::Vector::new(0.0, -pane_y));
    let mut backend = IcedBackend {
        frame,
        sx: 1.0,
        sy: 1.0,
        _marker: std::marker::PhantomData,
    };
    render_pane(pane_index, &mut backend, state);
    render_crosshair_for_pane(pane_index, &mut backend, state);
    render_pane_axes(pane_index, &mut backend, state);
    let _ = pane_h;
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

pub(crate) fn c4_to_iced(c: Color) -> iced::Color {
    iced::Color::from_rgba(c[0], c[1], c[2], c[3])
}

// ── Pixel snapping helpers ───────────────────────────────────────────────

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
            self.fill_path(points, stops[stops.len() - 1].0);
        }
    }

    pub fn fill_circle(&mut self, cx: f64, cy: f64, radius: f64, color: Color) {
        let path = Path::circle(Point::new(cx as f32, cy as f32), radius as f32);
        self.frame.fill(&path, c4_to_iced(color));
    }

    pub fn draw_text(&mut self, text: &str, x: f64, y: f64, font_size: f64, color: Color) {
        let approx_baseline_to_top = (font_size as f32) * 0.8;
        let mut t = iced::widget::canvas::Text::default();
        t.content = text.to_string();
        t.position = Point::new(x as f32, (y as f32) - approx_baseline_to_top);
        t.color = c4_to_iced(color);
        t.size = Pixels(font_size as f32);
        self.frame.fill_text(t);
    }

    pub fn measure_text(&self, text: &str, font_size: f64) -> f64 {
        text.chars().count() as f64 * font_size * 0.6
    }
}
