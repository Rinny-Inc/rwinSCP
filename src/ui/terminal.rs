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
                        draw_output(ui, terminal.output.trim_start_matches('\n'));
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
    if modifiers.ctrl && !modifiers.shift {
        let name = key.name();
        let mut chars = name.chars();
        if let (Some(c), None) = (chars.next(), chars.next()) {
            let upper = c.to_ascii_uppercase();
            if upper.is_ascii_uppercase() {
                return Some(char::from(upper as u8 - b'A' + 1).to_string());
            }
        }
        if key == Key::Space {
            return Some("\0".to_owned());
        }
    }

    let modifier =
        1 + u8::from(modifiers.shift) + 2 * u8::from(modifiers.alt) + 4 * u8::from(modifiers.ctrl);

    let cursor_final = match key {
        Key::ArrowUp => Some('A'),
        Key::ArrowDown => Some('B'),
        Key::ArrowRight => Some('C'),
        Key::ArrowLeft => Some('D'),
        Key::Home => Some('H'),
        Key::End => Some('F'),
        _ => None,
    };

    let modifier_is_one = modifier == 1;

    if let Some(final_byte) = cursor_final {
        return Some(if modifier_is_one {
            format!("\x1b[{final_byte}")
        } else {
            format!("\x1b[1;{modifier}{final_byte}")
        });
    }

    if key == Key::Delete {
        return Some(if modifier_is_one {
            "\u{1B}[3~".to_owned()
        } else {
            format!("\x1b[3;{modifier}~")
        });
    }

    let seq = match key {
        Key::Enter => "\r",
        Key::Backspace => "\u{7F}",
        Key::Tab => "\t",
        Key::Escape => "\u{1B}",
        Key::PageUp => "\u{1B}[5~",
        Key::PageDown => "\u{1B}[6~",
        _ => return None,
    };

    Some(seq.to_owned())
}

const CURSOR_BLINK: f64 = 0.53;

fn draw_output(ui: &mut Ui, text: &str) {
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let row_height = ui.fonts_mut(|f| f.row_height(&font));
    let wrap_width = ui.available_width();

    let galley = ui.fonts_mut(|f| f.layout(text.to_owned(), font.clone(), theme::TEXT, wrap_width));

    let size = egui::vec2(galley.size().x.max(1.0), galley.size().y + row_height);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());

    let end = galley.end();
    let caret = galley.pos_from_cursor(end).min;
    ui.painter().galley(rect.min, galley, theme::TEXT);

    let time = ui.input(|i| i.time);
    let visible = (time / CURSOR_BLINK) as i64 & 1 == 0;
    if visible {
        let advance = ui.fonts_mut(|f| f.glyph_width(&font, ' '));
        let cursor_rect = egui::Rect::from_min_size(
            rect.min + caret.to_vec2(),
            egui::vec2(advance.max(2.0), row_height),
        );
        ui.painter()
            .rect_filled(cursor_rect, 1, theme::tint(theme::ACCENT, 200));
    }

    ui.ctx()
        .request_repaint_after(std::time::Duration::from_secs_f64(CURSOR_BLINK));
}
