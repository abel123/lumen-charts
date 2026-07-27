use super::{CROSSHAIR_COLOR, TEXT_COLOR, WHITE};
use crate::chart::chart_state::ChartState;
use crate::chart::color::Palette;
use crate::widget::{snap_x, snap_y, IcedBackend};

pub fn draw_crosshair<'a>(b: &mut IcedBackend<'a>, state: &ChartState) {
    let plot = &state.layout.plot_area;
    let sf = state.layout.scale_factor;
    let x = snap_x(state.crosshair.x as f64, sf);
    let y = snap_y(state.crosshair.y as f64, sf);

    b.stroke_dashed_line(
        x,
        plot.y as f64,
        x,
        (plot.y + plot.height) as f64,
        CROSSHAIR_COLOR,
        1.0,
        4.0,
        4.0,
    );

    if let Some(active) = state.panes.get(state.active_pane) {
        let active_rect = &active.layout_rect;
        b.stroke_dashed_line(
            active_rect.x as f64,
            y,
            (active_rect.x + active_rect.width) as f64,
            y,
            CROSSHAIR_COLOR,
            1.0,
            4.0,
            4.0,
        );
    }

    if let Some(price) = state.crosshair.price {
        let label = format!("{:.2}", price);
        let label_x = (plot.x + plot.width + 2.0) as f64;
        let label_w = (state.layout.margins.right - 4.0) as f64;
        let label_h = 18.0;
        let label_y = y - label_h / 2.0;

        b.fill_rect(
            label_x,
            label_y,
            label_w,
            label_h,
            Palette::CrosshairLabelBg.color(),
        );
        b.draw_text(&label, label_x + 4.0, label_y + 13.0, 10.0, WHITE);
    }

    if let Some(idx) = state.crosshair.bar_index {
        if idx < state.data.bars.len() {
            let bar = &state.data.bars[idx];
            let info = format!(
                "O:{:.2}  H:{:.2}  L:{:.2}  C:{:.2}",
                bar.open, bar.high, bar.low, bar.close
            );

            let text_w = b.measure_text(&info, 10.0);
            let info_w = text_w + 16.0;
            let info_x = plot.x as f64 + 8.0;
            let info_y = plot.y as f64 + 4.0;
            b.fill_rect(
                info_x,
                info_y,
                info_w,
                20.0,
                Palette::CrosshairInfoBg.color(),
            );
            b.draw_text(&info, info_x + 8.0, info_y + 14.0, 10.0, TEXT_COLOR);
        }
    }
}

/// Draw the crosshair contribution that lives inside a single pane:
/// the vertical dashed line (full plot height) and, if this pane is
/// the active pane, the horizontal dashed line within its rect.
pub fn draw_crosshair_for_pane<'a>(pane_idx: usize, b: &mut IcedBackend<'a>, state: &ChartState) {
    let plot = &state.layout.plot_area;
    let sf = state.layout.scale_factor;
    let x = snap_x(state.crosshair.x as f64, sf);
    let y = snap_y(state.crosshair.y as f64, sf);

    b.stroke_dashed_line(
        x,
        plot.y as f64,
        x,
        (plot.y + plot.height) as f64,
        CROSSHAIR_COLOR,
        1.0,
        4.0,
        4.0,
    );

    if state.active_pane == pane_idx {
        if let Some(active) = state.panes.get(pane_idx) {
            let active_rect = &active.layout_rect;
            b.stroke_dashed_line(
                active_rect.x as f64,
                y,
                (active_rect.x + active_rect.width) as f64,
                y,
                CROSSHAIR_COLOR,
                1.0,
                4.0,
                4.0,
            );
        }
    }
}
