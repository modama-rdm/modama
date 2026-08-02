use eframe::egui::{self, Align, Id, Response, Ui};

#[derive(Default)]
pub(crate) struct RovingListState {
    pub focused_index: Option<usize>,
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
        return output;
    }

    let active_index = state
        .focused_index
        .or(selected_index)
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
        if focus_on_entry && index == active_index && response.enabled() {
            response.request_focus();
            output.focus_requested = true;
        }
        responses.push(response);
    }

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
            output.selection_changed = Some(next_index);
            responses[next_index].request_focus();
            responses[next_index].scroll_to_me(Some(Align::Center));
        } else if consume_key(ui, egui::Key::Enter) || consume_key(ui, egui::Key::Space) {
            state.focused_index = Some(index);
            output.selection_changed = Some(index);
        }
    }

    output
}

fn consume_key(ui: &mut Ui, key: egui::Key) -> bool {
    ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, key))
}
