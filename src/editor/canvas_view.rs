//! The canvas area: the canvas and the overlays over it (context menu, node picker,
//! comment title editor). The canvas itself draws and handles input; these are the
//! editor chrome Graphy leaves to the host.

use gpui::*;
use ui::input::TextInput;
use ui::{h_flex, v_flex, ActiveTheme};

use super::panel::{CanvasMenu, ShaderEditorPanel};

impl ShaderEditorPanel {
    /// The canvas of the document and the overlays over it.
    pub(crate) fn render_center(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let canvas = self.canvas.clone();
        div()
            .relative()
            .size_full()
            .on_action(cx.listener(|editor, _: &plugin_editor_api::Undo, _, cx| editor.undo(cx)))
            .on_action(cx.listener(|editor, _: &plugin_editor_api::Redo, _, cx| editor.redo(cx)))
            .child(div().size_full().child(canvas))
            .child(self.render_palette(window, cx))
            .child(self.render_context_menu(cx))
            .child(self.render_comment_editor(cx))
            .into_any_element()
    }

    fn render_context_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(menu) = self.menu.clone() else {
            return div().into_any_element();
        };
        let this = cx.entity();
        let mut list = div()
            .occlude()
            .w(px(220.0))
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().border)
            .shadow_lg()
            .rounded(px(6.0))
            .py(px(4.0));
        for item in menu.items.iter().cloned() {
            list = list.children(Self::menu_entry(item, &menu, &this, cx));
        }
        deferred(
            anchored()
                .position(menu.at)
                .snap_to_window_with_margin(px(8.0))
                .anchor(Corner::TopLeft)
                .child(list.on_mouse_down_out(move |_, _, cx| {
                    this.update(cx, |editor, cx| {
                        editor.menu = None;
                        cx.notify();
                    });
                })),
        )
        .with_priority(2)
        .into_any_element()
    }

    /// One menu entry (and its children, flattened below it).
    fn menu_entry(
        item: graphy_gpui::MenuItem,
        menu: &CanvasMenu,
        this: &Entity<Self>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut out: Vec<AnyElement> = Vec::new();
        if item.separator_before {
            out.push(
                div()
                    .my(px(4.0))
                    .mx(px(8.0))
                    .h(px(1.0))
                    .bg(cx.theme().border)
                    .into_any_element(),
            );
        }
        let foreground = if item.enabled {
            cx.theme().popover_foreground
        } else {
            cx.theme().muted_foreground
        };
        let id = item.id.clone();
        let mut row = div()
            .px(px(12.0))
            .py(px(6.0))
            .text_sm()
            .text_color(foreground)
            .child(item.label.clone());
        if item.enabled {
            let (this, menu) = (this.clone(), menu.clone());
            row = row
                .cursor_pointer()
                .hover(|s| s.bg(cx.theme().accent.opacity(0.12)))
                .on_mouse_down(MouseButton::Left, move |_, _window, cx| {
                    let (menu, id) = (menu.clone(), id.clone());
                    this.update(cx, |editor, cx| editor.run_menu_item(menu, &id, cx));
                });
        }
        out.push(row.into_any_element());
        for child in item.children {
            out.extend(Self::menu_entry(child, menu, this, cx));
        }
        out
    }

    fn render_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(palette) = &self.palette else {
            return div().into_any_element();
        };
        let query = self.palette_input.read(cx).text().to_string().to_lowercase();
        let mut list = v_flex().id("shader-palette-list").overflow_y_scroll().flex_1();
        let matches = palette
            .entries
            .iter()
            .filter(|entry| {
                query.is_empty()
                    || entry.title.to_lowercase().contains(&query)
                    || entry.category.to_lowercase().contains(&query)
                    || entry.keywords.iter().any(|k| k.to_lowercase().contains(&query))
            })
            .take(80);
        for (index, entry) in matches.enumerate() {
            let definition = entry.definition.clone();
            list = list.child(
                h_flex()
                    .id(("palette-entry", index))
                    .gap_2()
                    .px_2()
                    .py_1()
                    .cursor_pointer()
                    .hover(|row| row.bg(cx.theme().accent.opacity(0.12)))
                    .on_click(cx.listener(move |editor, _, _, cx| {
                        editor.place_picked(&definition, cx);
                    }))
                    .child(div().flex_1().text_sm().truncate().child(entry.title.clone()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(entry.category.clone()),
                    ),
            );
        }
        let at = palette.at;
        if palette.focus_pending {
            let handle = self.palette_input.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        }
        if let Some(palette) = &mut self.palette {
            palette.focus_pending = false;
        }
        let this = cx.entity();
        deferred(
            anchored()
                .position(at)
                .snap_to_window_with_margin(px(8.0))
                .anchor(Corner::TopLeft)
                .child(
                    v_flex()
                        .occlude()
                        .w(px(320.0))
                        .h(px(480.0))
                        .shadow_lg()
                        .rounded(px(6.0))
                        .overflow_hidden()
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().popover)
                        .child(div().p_1().child(TextInput::new(&self.palette_input).text_sm()))
                        .child(list)
                        .on_mouse_down_out(move |_, _, cx| {
                            this.update(cx, |editor, cx| editor.close_palette(cx));
                        }),
                ),
        )
        .with_priority(1)
        .into_any_element()
    }

    /// Title editor over the comment being renamed.
    fn render_comment_editor(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(index) = self.editing_comment else {
            return div().into_any_element();
        };
        let canvas = self.canvas.read(cx);
        let Some(comment) = canvas.graph().comments.get(index) else {
            return div().into_any_element();
        };
        let (_, zoom) = canvas.view();
        let title_h = (30.0 * zoom).clamp(18.0, 44.0);
        let title_w = (comment.size.0 as f32 * zoom - 24.0 * zoom).max(80.0);
        let origin = canvas.graph_to_window(graphy_render::Vec2::new(
            comment.position.x as f32,
            comment.position.y as f32,
        ));
        let position = Point::new(origin.x + px(12.0 * zoom), origin.y + px(6.0 * zoom));
        deferred(
            anchored()
                .position(position)
                .snap_to_window_with_margin(px(4.0))
                .anchor(Corner::TopLeft)
                .child(
                    div().occlude().w(px(title_w)).h(px(title_h * 0.68)).child(
                        TextInput::new(&self.comment_input)
                            .appearance(false)
                            .bordered(false)
                            .focus_bordered(false),
                    ),
                ),
        )
        .with_priority(2)
        .into_any_element()
    }
}

