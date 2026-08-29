use crate::app::{Action, App};
use crate::theme;
use crate::ui::{keep, widgets};
use egui::{Event, Key, Modifiers, RichText, Ui};

pub fn show(app: &mut App, ui: &mut Ui) -> Option<Action> {
    let mut action = None;
    let session = app.session()?;

    if let Some(other) = ui.memory(|m| m.focused()) {
        ui.memory_mut(|m| m.surrender_focus(other));
    }

    let keystrokes = ui.input_mut(|i| {
        let bytes = encode_events(&i.events);
        if !bytes.is_empty() {
            i.events.retain(|event| {
                !matches!(
                    event,
                    Event::Text(_) | Event::Key { .. } | Event::Paste(_) | Event::Copy | Event::Cut
                )
            });
        }
        bytes
    });

    if !keystrokes.is_empty() {
        keep(&mut action, Some(Action::ShellBytes(keystrokes)));
    }

    ui.add_space(theme::S3);

    let Some(terminal) = &session.terminal else {
        widgets::empty_state(ui, "No shell", "This session has no interactive shell");
        return action;
    };

    let frame = egui::Frame::default()
        .fill(theme::BG_SURFACE)
        .stroke(egui::Stroke::new(1.0, theme::ACCENT))
        .corner_radius(theme::R_MD)
        .inner_margin(theme::S2 as i8);

    frame.show(ui, |ui| {
        egui::ScrollArea::vertical()
            .id_salt("terminal_scroll")
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    if terminal.output.is_empty() {
                        ui.label(
                            RichText::new("Waiting for the remote shell\u{2026}")
                                .color(theme::TEXT_FAINT)
                                .monospace()
                                .small(),
                        );
                    } else {
                        ui.label(
                            RichText::new(terminal.output.trim_start_matches('\n'))
                                .color(theme::TEXT)
                                .monospace(),
                        );
                    }
                });
            });
    });

    action
}

fn encode_events(events: &[Event]) -> String {
    let mut out = String::new();

    for event in events {
        match event {
            Event::Text(text) => out.push_str(text),

            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                if let Some(seq) = encode_key(*key, *modifiers) {
                    out.push_str(&seq);
                }
            }

            Event::Paste(text) => out.push_str(text),

            // egui turns Ctrl+C into a Copy event before it ever reaches
            // the key handler. In a terminal that chord means interrupt;
            // copying is Ctrl+Shift+C.
            Event::Copy => out.push('\u{3}'),

            // Ctrl+X likewise.
            Event::Cut => out.push('\u{18}'),

            _ => {}
        }
    }

    out
}

fn encode_key(key: Key, modifiers: Modifiers) -> Option<String> {
    // Ctrl+letter maps onto the C0 control codes: Ctrl+C is 0x03, and so on
    if modifiers.ctrl
        && !modifiers.shift
        && let Some(name) = key.name().chars().next()
    {
        let upper = name.to_ascii_uppercase();
        if upper.is_ascii_alphabetic() {
            return Some(char::from(upper as u8 - b'A' + 1).to_string());
        }
    }

    let seq = match key {
        Key::Enter => "\r",
        Key::Backspace => "\u{7F}",
        Key::Tab => "\t",
        Key::Escape => "\u{1B}",
        Key::ArrowUp => "\u{1B}[A",
        Key::ArrowDown => "\u{1B}[B",
        Key::ArrowRight => "\u{1B}[C",
        Key::ArrowLeft => "\u{1B}[D",
        Key::Home => "\u{1B}[H",
        Key::End => "\u{1B}[F",
        Key::Delete => "\u{1B}[3~",
        Key::PageUp => "\u{1B}[5~",
        Key::PageDown => "\u{1B}[6~",
        _ => return None,
    };

    Some(seq.to_owned())
}
