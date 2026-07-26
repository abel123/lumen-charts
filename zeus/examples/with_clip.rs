//! Standalone test for `iced::widget::canvas::Frame::with_clip`.
//!
//! Draws a 200×200 clipped region at (20, 20), paints half-transparent
//! green inside it, plus a solid red box *inside* the clip and a solid
//! blue box *outside* the clip. If clip is implemented as a RAII
//! scope (it is), only the red box is visible; the blue box is hidden
//! because the clip is still active at the same scope level in Iced
//! 0.14's `Frame`.

use iced::widget::canvas::{Frame, Path, Program};
use iced::widget::{text, Canvas, Column};
use iced::{Color, Point, Rectangle, Settings, Size, Task, Theme};

/// Program that paints three primitive shapes and exercises the clip.
struct ClipExample;

// Iced 0.14's `Program` requires explicit `Message` and `Theme`
// generics. The associated `type State` is `()` because we have
// nothing to cache between frames.
impl Program<(), Theme> for ClipExample {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());

        // Clipped region: top-left (20, 20), 200 × 200
        let clip_area = Rectangle::new(Point::new(20.0, 20.0), Size::new(200.0, 200.0));

        frame.with_clip(clip_area, |clip_frame| {
            // Semi-transparent green tint to visualize the clip bounds.
            let clip_bg = Path::rectangle(clip_area.position(), clip_area.size());
            clip_frame.fill(&clip_bg, Color::from_rgba(0.0, 1.0, 0.0, 0.2));

            // Red box — fully inside the clip.
            let inner_box = Path::rectangle(Point::new(50.0, 50.0), Size::new(80.0, 80.0));
            clip_frame.fill(&inner_box, Color::from_rgb(0.94, 0.33, 0.31));
            // Blue box — outside the clip. With correct clip behavior this
            // should *not* appear (the clip is still active at the same
            // scope level in Iced 0.14's Frame).
            let outer_box = Path::rectangle(Point::new(200.0, 50.0), Size::new(80.0, 80.0));
            clip_frame.fill(&outer_box, Color::from_rgb(0.26, 0.52, 0.96));
        });

        vec![frame.into_geometry()]
    }
}

struct App;

impl App {
    fn new() -> (Self, Task<()>) {
        (App, Task::none())
    }
}

fn update(_state: &mut App, _message: ()) -> Task<()> {
    Task::none()
}

fn view(_state: &App) -> Column<'_, (), Theme, iced::Renderer> {
    Column::new()
        .push(text("with_clip 测试规则："))
        .push(text("✅ 正常：绿框 + 内部红方块，右侧蓝色方块消失"))
        .push(text("❌ 异常空白：仅绿色背景，红色方块消失"))
        .push(Canvas::new(ClipExample).width(400).height(300))
        .spacing(8)
        .padding(10)
}

fn main() -> iced::Result {
    iced::application(App::new, update, view)
        .settings(Settings::default())
        .title("Iced with_clip Example")
        .run()
}
