//!
//! This module contains overlay elements that render on top of the main graph:
//! - Selection box during drag-select operations
//! - Graph controls (zoom level, etc.)

//! Overlay rendering - selection box, graph controls
use super::graph::NodeGraphRenderer;
use crate::editor::workspace_panels::GraphCanvasPanel;
use gpui::*;
use ui::{
    button::{Button, ButtonVariants},
    h_flex, v_flex, ActiveTheme, IconName, PixelsExt, Sizable, StyledExt,
};

/// Render the selection box during drag-select
pub fn render_selection_box(
    panel: &crate::editor::workspace_panels::GraphCanvasPanel,
    _view_id: &str,
    cx: &mut Context<crate::editor::workspace_panels::GraphCanvasPanel>,
) -> impl IntoElement {
    if false {
        return div().into_any_element();
    }

    if let (Some(start), Some(end)) = (panel.selection_start, panel.selection_end) {
        // Convert selection bounds to screen coordinates
        let start_screen = NodeGraphRenderer::graph_to_screen_pos(start, &panel.graph);
        let end_screen = NodeGraphRenderer::graph_to_screen_pos(end, &panel.graph);

        let left = start_screen.x.min(end_screen.x);
        let top = start_screen.y.min(end_screen.y);
        let width = (end_screen.x - start_screen.x).abs();
        let height = (end_screen.y - start_screen.y).abs();

        div()
            .absolute()
            .inset_0()
            .child(
                div()
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(width))
                    .h(px(height))
                    .border_1()
                    .border_color(gpui::Hsla {
                        h: 0.58,
                        s: 0.7,
                        l: 0.6,
                        a: 0.7,
                    })
                    .bg(gpui::Hsla {
                        h: 0.58,
                        s: 0.5,
                        l: 0.5,
                        a: 0.08,
                    })
                    .rounded(px(3.0)),
            )
            .into_any_element()
    } else {
        div().into_any_element()
    }
}

/// Graph controls overlay (zoom indicator, fit-to-view, close button)
pub fn render_graph_controls(
    panel: &crate::editor::workspace_panels::GraphCanvasPanel,
    cx: &mut Context<crate::editor::workspace_panels::GraphCanvasPanel>,
) -> impl IntoElement {
    div().absolute().bottom_4().right_4().w(px(280.0)).child(
        v_flex().gap_2().items_end().w(px(280.0)).child(
            h_flex()
                .gap_2()
                .p_2()
                .w_full()
                .bg(cx.theme().background.opacity(0.9))
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border)
                .justify_between()
                .items_center()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("Zoom: {:.0}%", panel.graph.zoom_level * 100.0)),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("zoom_fit")
                                .icon(IconName::BadgeCheck)
                                .tooltip("Fit to View")
                                .on_click(cx.listener(|panel, _, _window, cx| {
                                    let graph = &mut panel.graph;
                                    graph.zoom_level = 1.0;
                                    graph.pan_offset = Point::new(0.0, 0.0);
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("close_graph_controls")
                                .icon(IconName::X)
                                .ghost()
                                .xsmall()
                                .on_click(cx.listener(|panel, _, _, cx| {
                                    panel.show_graph_controls = false;
                                    cx.notify();
                                })),
                        ),
                ),
        ),
    )
}
