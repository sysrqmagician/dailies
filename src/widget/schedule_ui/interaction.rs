use eframe::egui::{
  self, text::LayoutJob, CursorIcon, Label, LayerId, Rect, Response, Sense, Ui,
};

use crate::{
  event::Event,
  util::{on_the_same_day, reorder_times, DateTime},
};

use super::{
  layout::Layout, move_event, move_event_end, move_event_start, EventId,
  ScheduleUi,
};

#[derive(Clone, Copy, Debug)]
struct DraggingEventYOffset(f32);

#[derive(Clone, Debug)]
struct InteractingEvent {
  event: Event,
  state: FocusedEventState,
}

impl InteractingEvent {
  fn id() -> egui::Id {
    egui::Id::new("interacting_event")
  }

  fn get(ui: &Ui) -> Option<Self> {
    ui.memory().data.get_temp(Self::id())
  }

  fn set(ui: &Ui, event: Event, state: FocusedEventState) {
    let value = InteractingEvent { event, state };
    ui.memory().data.insert_temp(Self::id(), value)
  }

  fn save(self, ui: &Ui) {
    Self::set(ui, self.event.clone(), self.state)
  }

  fn discard(ui: &Ui) {
    debug_assert!(Self::get(ui).is_some());

    ui.memory().data.remove::<Self>(Self::id())
  }

  fn commit(self, ui: &Ui) {
    ui.memory().data.insert_temp(Self::id(), self.event);
    Self::discard(ui);
  }

  fn take_commited_event(ui: &Ui) -> Option<Event> {
    let event = ui.memory().data.get_temp(Self::id());
    ui.memory().data.remove::<Event>(Self::id());
    event
  }

  fn get_id(ui: &Ui, id: &EventId) -> Option<Self> {
    Self::get(ui).and_then(|value| (&value.event.id == id).then(|| value))
  }

  fn get_event(ui: &Ui) -> Option<Event> {
    Self::get(ui).map(|v| v.event)
  }
}

#[derive(Clone, Debug)]
struct DeletedEvent {
  event_id: EventId,
}

impl DeletedEvent {
  fn id() -> egui::Id {
    egui::Id::new("deleted_event")
  }

  fn set(ui: &Ui, event_id: &EventId) {
    ui.memory().data.insert_temp(
      Self::id(),
      Self {
        event_id: event_id.clone(),
      },
    );
  }

  fn take(ui: &Ui) -> Option<EventId> {
    let deleted_event = ui.memory().data.get_temp(Self::id());
    ui.memory().data.remove::<Self>(Self::id());
    deleted_event.map(|x: Self| x.event_id)
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusedEventState {
  Editing,
  Dragging,
  DraggingEventStart,
  DraggingEventEnd,
  EventCloning,
}

impl ScheduleUi {
  fn interact_event_region(
    &self,
    ui: &mut Ui,
    resp: Response,
  ) -> Option<FocusedEventState> {
    use FocusedEventState::*;
    let event_rect = resp.rect;
    let [upper, lower] = self.event_resizer_regions(event_rect);

    let _lmb = egui::PointerButton::Primary;

    let interact_pos =
      resp.interact_pointer_pos().or_else(|| resp.hover_pos())?;

    if resp.clicked_by(egui::PointerButton::Primary) {
      return Some(Editing);
    }

    if upper.contains(interact_pos) {
      ui.output().cursor_icon = CursorIcon::ResizeVertical;
      if resp.drag_started() && resp.dragged_by(egui::PointerButton::Primary) {
        return Some(DraggingEventStart);
      }
      return None;
    }

    if lower.contains(interact_pos) {
      ui.output().cursor_icon = CursorIcon::ResizeVertical;
      if resp.drag_started() && resp.dragged_by(egui::PointerButton::Primary) {
        return Some(DraggingEventEnd);
      }
      return None;
    }

    if event_rect.contains(interact_pos) {
      ui.output().cursor_icon = CursorIcon::Grab;

      if resp.drag_started()
        && resp.dragged_by(egui::PointerButton::Primary)
        && ui.input().modifiers.ctrl
      {
        let offset = DraggingEventYOffset(event_rect.top() - interact_pos.y);
        ui.memory().data.insert_temp(egui::Id::null(), offset);
        return Some(EventCloning);
      }

      if resp.drag_started() && resp.dragged_by(egui::PointerButton::Primary) {
        let offset = DraggingEventYOffset(event_rect.top() - interact_pos.y);
        ui.memory().data.insert_temp(egui::Id::null(), offset);
        return Some(Dragging);
      }

      return None;
    }

    None
  }

  fn interact_event(
    &self,
    ui: &mut Ui,
    event_rect: Rect,
    state: FocusedEventState,
    event: &mut Event,
  ) -> (Response, Option<bool>) {
    let [upper, lower] = self.event_resizer_regions(event_rect);

    let resp = self.place_event_button(ui, event_rect, event);
    let commit = match state {
      FocusedEventState::DraggingEventStart => {
        self.handle_event_resizing(ui, upper, |time| {
          move_event_start(event, time, self.min_event_duration);
          event.start
        })
      }
      FocusedEventState::DraggingEventEnd => {
        self.handle_event_resizing(ui, lower, |time| {
          move_event_end(event, time, self.min_event_duration);
          event.end
        })
      }
      FocusedEventState::Dragging => {
        self.handle_event_dragging(ui, event_rect, |time| {
          move_event(event, time);
          (event.start, event.end)
        })
      }
      _ => unreachable!(),
    };

    (resp, commit)
  }

  fn handle_event_resizing(
    &self,
    ui: &mut Ui,
    rect: Rect,
    set_time: impl FnOnce(DateTime) -> DateTime,
  ) -> Option<bool> {
    if !ui.memory().is_anything_being_dragged() {
      return Some(true);
    }

    ui.output().cursor_icon = CursorIcon::ResizeVertical;

    let pointer_pos = self.relative_pointer_pos(ui).unwrap();

    if let Some(datetime) = self.pointer_to_datetime_auto(ui, pointer_pos) {
      let updated_time = set_time(datetime);
      self.show_resizer_hint(ui, rect, updated_time);
    }

    None
  }

  fn handle_event_dragging(
    &self,
    ui: &mut Ui,
    rect: Rect,
    set_time: impl FnOnce(DateTime) -> (DateTime, DateTime),
  ) -> Option<bool> {
    if !ui.memory().is_anything_being_dragged() {
      return Some(true);
    }

    ui.output().cursor_icon = CursorIcon::Grabbing;

    let mut pointer_pos = self.relative_pointer_pos(ui).unwrap();
    if let Some(offset_y) = ui
      .memory()
      .data
      .get_temp::<DraggingEventYOffset>(egui::Id::null())
    {
      pointer_pos.y += offset_y.0;
    }

    if let Some(datetime) = self.pointer_to_datetime_auto(ui, pointer_pos) {
      let (beg, end) = set_time(datetime);
      let [upper, lower] = self.event_resizer_regions(rect);
      self.show_resizer_hint(ui, upper, beg);
      self.show_resizer_hint(ui, lower, end);
    }

    None
  }

  pub(super) fn put_non_interacting_event_block(
    &self,
    ui: &mut Ui,
    layout: &Layout,
    event: &Event,
  ) -> Option<Response> {
    let event_rect = self.event_rect(ui, layout, event)?;

    let resp = self.place_event_button(ui, event_rect, event);
    match self.interact_event_region(ui, resp) {
      None => (),
      Some(FocusedEventState::EventCloning) => {
        let new_event = self.clone_to_new_event(event);
        InteractingEvent::set(ui, new_event, FocusedEventState::Dragging);
      }
      Some(state) => InteractingEvent::set(ui, event.clone(), state),
    }

    None
  }

  pub(super) fn put_interacting_event_block(
    &self,
    ui: &mut Ui,
    layout: &Layout,
  ) -> Option<Response> {
    use FocusedEventState::*;

    let mut ie = InteractingEvent::get(ui)?;
    let event_rect = self.event_rect(ui, layout, &ie.event)?;

    match ie.state {
      Editing => match self.place_event_editor(ui, event_rect, &mut ie.event) {
        None => ie.save(ui),
        Some(true) => ie.commit(ui),
        Some(false) => InteractingEvent::discard(ui),
      },
      _ => {
        let event_rect = self.event_rect(ui, layout, &ie.event)?;

        let (resp, commit) =
          self.interact_event(ui, event_rect, ie.state, &mut ie.event);

        match commit {
          None => {
            // two possibilities:
            // 1. a brief click
            // 2. really dragging something
            if let Some(new_state) = self.interact_event_region(ui, resp) {
              ie.state = state_override(ie.state, new_state);
            }
            ie.save(ui)
          }
          Some(true) => ie.commit(ui),
          Some(false) => InteractingEvent::discard(ui),
        }
      }
    }

    None
  }

  fn place_event_button(
    &self,
    ui: &mut Ui,
    rect: Rect,
    event: &Event,
  ) -> Response {
    let (layout, clipped) = self.shorten_event_label(ui, rect, &event.title);

    let button = egui::Button::new(layout).sense(Sense::click_and_drag());
    let resp = ui.put(rect, button);

    if clipped {
      // text is clipped, show a tooltip
      resp.clone().on_hover_text(event.title.clone());
    }

    resp.clone().context_menu(|ui| {
      if ui.button("Delete").clicked() {
        DeletedEvent::set(ui, &event.id);
        ui.close_menu();
      }
    });

    resp
  }

  fn shorten_event_label(
    &self,
    ui: &mut Ui,
    rect: Rect,
    label: &str,
  ) -> (impl Into<egui::WidgetText>, bool) {
    let text_style = egui::TextStyle::Button;
    let color = ui.visuals().text_color();

    let layout_job = |text| {
      let mut j = LayoutJob::simple_singleline(text, text_style, color);
      j.wrap_width = rect.shrink2(ui.spacing().button_padding).width();
      j
    };

    let job = layout_job(label.into());
    let line_height = job.font_height(ui.fonts());
    let mut galley = ui.fonts().layout_job(job);

    if galley.size().y <= line_height {
      // multiline
      return (galley, false);
    }

    for n in (0..(label.len() - 3)).rev() {
      let text = format!("{}..", &label[0..n]);
      galley = ui.fonts().layout_job(layout_job(text));
      if galley.size().y <= line_height {
        return (galley, true);
      }
    }

    (galley, false)
  }

  // Some(true) => commit change
  // Some(false) => discard change
  // None => still editing
  fn place_event_editor(
    &self,
    ui: &mut Ui,
    rect: Rect,
    event: &mut Event,
  ) -> Option<bool> {
    let editor = egui::TextEdit::singleline(&mut event.title);
    let resp = ui.put(rect, editor);

    // Anything dragging outside the textedit should be equivalent to
    // losing focus. Note: we still need to allow dragging within the
    // textedit widget to allow text selection, etc.
    let anything_else_dragging = ui.memory().is_anything_being_dragged()
      && !resp.dragged()
      && !resp.drag_released();

    // We cannot use key_released here, because it will be taken
    // precedence by resp.lost_focus() and commit the change.
    if ui.input().key_pressed(egui::Key::Escape) {
      return Some(false);
    }

    if resp.lost_focus() || resp.clicked_elsewhere() || anything_else_dragging {
      return Some(true);
    }

    resp.request_focus();
    None
  }

  fn show_resizer_hint(&self, ui: &mut Ui, rect: Rect, time: DateTime) {
    let layer_id = egui::Id::new("resizer_hint");
    let layer = LayerId::new(egui::Order::Tooltip, layer_id);

    let text = format!("{}", time.format(self.event_resizing_hint_format));
    let label = Label::new(egui::RichText::new(text).monospace());

    ui.with_layer_id(layer, |ui| ui.put(rect, label));
  }

  pub(super) fn handle_new_event(
    &self,
    ui: &mut Ui,
    response: &Response,
  ) -> Option<()> {
    use FocusedEventState::Editing;

    let id = response.id;

    if response.drag_started()
      && response.dragged_by(egui::PointerButton::Primary)
    {
      let mut event = self.new_event();
      let pointer_pos = self.relative_pointer_pos(ui)?;
      let init_time = self.pointer_to_datetime_auto(ui, pointer_pos)?;
      let new_state = self.assign_new_event_dates(ui, init_time, &mut event)?;

      ui.memory().data.insert_temp(id, event.id.clone());
      ui.memory().data.insert_temp(id, init_time);

      InteractingEvent::set(ui, event, new_state);

      return Some(());
    }

    if response.clicked_by(egui::PointerButton::Primary) {
      InteractingEvent::discard(ui);
      return Some(());
    }

    if response.drag_released() {
      let event_id = ui.memory().data.get_temp(id)?;
      let mut value = InteractingEvent::get_id(ui, &event_id)?;
      value.state = Editing;
      value.save(ui);
    }

    if response.dragged() && response.dragged_by(egui::PointerButton::Primary) {
      let event_id: String = ui.memory().data.get_temp(id)?;
      let init_time = ui.memory().data.get_temp(id)?;
      let mut value = InteractingEvent::get_id(ui, &event_id)?;
      let new_state =
        self.assign_new_event_dates(ui, init_time, &mut value.event)?;
      value.state = new_state;
      value.save(ui);
    }

    Some(())
  }

  fn assign_new_event_dates(
    &self,
    ui: &Ui,
    init_time: DateTime,
    event: &mut Event,
  ) -> Option<FocusedEventState> {
    use FocusedEventState::{DraggingEventEnd, DraggingEventStart};

    let pointer_pos = self.relative_pointer_pos(ui)?;
    let new_time = self.pointer_to_datetime_auto(ui, pointer_pos)?;

    let (mut start, mut end) = (init_time, new_time);
    let reordered = reorder_times(&mut start, &mut end);

    // the event crossed the day boundary, we need to pick a direction
    // based on the initial drag position
    if !on_the_same_day(start, end) {
      if self.day_progress(&init_time) < 0.5 {
        start = init_time;
        end = init_time + self.min_event_duration;
      } else {
        end = init_time;
        start = init_time - self.min_event_duration;
      }
    };

    event.start = start;
    event.end = end;

    if reordered {
      Some(DraggingEventStart)
    } else {
      Some(DraggingEventEnd)
    }
  }

  pub(super) fn get_interacting_event(&self, ui: &Ui) -> Option<Event> {
    InteractingEvent::get_event(ui)
  }

  pub(super) fn apply_interacting_events(&mut self, ui: &Ui) {
    if let Some(event) = InteractingEvent::take_commited_event(ui) {
      commit_updated_event(&mut self.events, event);
    }
    // commit deleted event
    if let Some(event_id) = DeletedEvent::take(ui) {
      remove_deleted_events(&mut self.events, event_id);
    }
  }
}

// Hack: allow editing to override existing drag state, because it
// seems that dragging always takes precedence.
fn state_override(
  old_state: FocusedEventState,
  new_state: FocusedEventState,
) -> FocusedEventState {
  if new_state == FocusedEventState::Editing {
    return new_state;
  }

  old_state
}

fn commit_updated_event(events: &mut Vec<Event>, mut commited_event: Event) {
  let mut updated = false;

  for event in events.iter_mut() {
    if event.id == commited_event.id {
      event.mark_changed();
      *event = commited_event.clone();
      updated = true;
    }
  }

  if !updated {
    commited_event.mark_changed();
    events.push(commited_event);
  }
}

fn remove_deleted_events(events: &mut Vec<Event>, deleted_event_id: EventId) {
  events.retain(|x| x.id != deleted_event_id)
}