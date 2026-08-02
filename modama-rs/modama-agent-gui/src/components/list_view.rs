use eframe::egui::{self, Align, Id, Response, Ui};

#[derive(Default)]
pub(crate) struct RovingListState {
    pub focused_index: Option<usize>,
    pending_focus: Option<usize>,
    last_focused_index: Option<usize>,
    last_focused_id: Option<Id>,
    was_focused: bool,
}

#[derive(Default)]
pub(crate) struct RovingListOutput {
    pub selection_changed: Option<usize>,
    pub activated: Option<usize>,
    pub focus_requested: bool,
}

/// Render a single-select list whose active item is the only row in the Tab order.
///
/// The caller owns domain selection state. This helper owns only the keyboard focus
/// index and reports selection/activation events back to the caller.
pub(crate) fn show_roving_list<T>(
    ui: &mut Ui,
    id: Id,
    items: &[T],
    selected_index: Option<usize>,
    state: &mut RovingListState,
    focus_on_entry: bool,
    mut render_row: impl FnMut(&mut Ui, usize, &T, bool, bool) -> Response,
) -> RovingListOutput {
    let mut output = RovingListOutput::default();
    if items.is_empty() {
        state.focused_index = None;
        state.pending_focus = None;
        state.last_focused_index = None;
        state.last_focused_id = None;
        state.was_focused = false;
        return output;
    }

    let active_index = if !state.was_focused {
        selected_index.or(state.focused_index)
    } else {
        state.focused_index.or(selected_index)
    }
    .unwrap_or(0)
    .min(items.len() - 1);
    let mut responses = Vec::with_capacity(items.len());

    for (index, item) in items.iter().enumerate() {
        let response = ui
            .push_id(id.with(index), |ui| {
                render_row(
                    ui,
                    index,
                    item,
                    selected_index == Some(index),
                    index == active_index,
                )
            })
            .inner;
        let should_request_focus =
            (focus_on_entry || state.pending_focus == Some(index)) && index == active_index;
        if should_request_focus && response.enabled() {
            response.request_focus();
            output.focus_requested = true;
            state.pending_focus = None;
        }
        responses.push(response);
    }

    let mut list_has_focus = false;
    for (index, response) in responses.iter().enumerate() {
        if response.clicked() {
            state.focused_index = Some(index);
            output.selection_changed = Some(index);
            if response.double_clicked() {
                output.activated = Some(index);
            }
        }

        if !response.has_focus() {
            continue;
        }
        list_has_focus = true;
        state.was_focused = true;

        if state.last_focused_index != Some(index) {
            response.scroll_to_me(Some(Align::Center));
        }
        state.last_focused_index = Some(index);
        state.last_focused_id = Some(response.id);

        let next_index = if consume_key(ui, egui::Key::ArrowUp) {
            Some(if index == 0 {
                items.len() - 1
            } else {
                index - 1
            })
        } else if consume_key(ui, egui::Key::ArrowDown) {
            Some((index + 1) % items.len())
        } else {
            None
        };

        if let Some(next_index) = next_index {
            state.focused_index = Some(next_index);
            state.pending_focus = Some(next_index);
            output.selection_changed = Some(next_index);
            responses[next_index].request_focus();
            responses[next_index].scroll_to_me(Some(Align::Center));
        } else if consume_key(ui, egui::Key::Enter) || consume_key(ui, egui::Key::Space) {
            state.focused_index = Some(index);
            output.selection_changed = Some(index);
        }
    }

    if !list_has_focus {
        let focused_id = ui.memory(|memory| memory.focused());
        let focus_is_elsewhere =
            focused_id.is_some_and(|focused_id| state.last_focused_id != Some(focused_id));
        let tab_pressed = ui.input(|input| input.key_pressed(egui::Key::Tab));
        state.pending_focus = if state.was_focused && !focus_is_elsewhere && !tab_pressed {
            state.last_focused_index
        } else {
            None
        };
        state.was_focused = false;
    }

    output
}

fn consume_key(ui: &mut Ui, key: egui::Key) -> bool {
    ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, key))
}
