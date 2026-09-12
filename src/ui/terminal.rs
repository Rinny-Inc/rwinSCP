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
        if terminal.lines.is_empty() && terminal.current.is_empty() {
            ui.label(
                RichText::new("Waiting for the remote shell \u{2026}")
                    .color(theme::TEXT_FAINT)
                    .monospace()
                    .small(),
            );
            return;
        }
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        let row_height = ui.fonts_mut(|f| f.row_height(&font));

        let advance = ui.fonts_mut(|f| f.glyph_width(&font, 'M'));
        if advance > 0.0 && row_height > 0.0 {
            let cols = (ui.available_width() / advance).floor().max(20.0) as u32;
            let rows = (ui.available_height() / row_height).floor().max(4.0) as u32;
            keep(&mut action, Some(Action::ShellResize(cols, rows)));
        }

        egui::ScrollArea::vertical()
            .id_salt("terminal_scroll")
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show_rows(ui, row_height, terminal.row_count(), |ui, rows| {
                ui.set_width(ui.available_width());
                let last = terminal.row_count().saturating_sub(1);

                for index in rows {
                    if index == last {
                        draw_row_with_cursor(ui, terminal.row(index), font.clone(), row_height);
                    } else {
                        ui.label(
                            RichText::new(terminal.row(index))
                                .color(theme::TEXT)
                                .monospace(),
                        );
                    }
                }
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

fn draw_row_with_cursor(ui: &mut Ui, text: &str, font: egui::FontId, row_height: f32) {
    let galley = ui.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), font.clone(), theme::TEXT));
    let advance = ui.fonts_mut(|f| f.glyph_width(&font, ' '));
    let width = galley.size().x + advance.max(2.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, row_height), egui::Sense::hover());
    let caret = rect.min + egui::vec2(galley.size().x, 0.0);
    ui.painter().galley(rect.min, galley, theme::TEXT);

    let time = ui.input(|i| i.time);
    if (time / CURSOR_BLINK) as i64 & 1 == 0 {
        ui.painter().rect_filled(
            egui::Rect::from_min_size(caret, egui::vec2(advance.max(2.0), row_height)),
            1,
            theme::tint(theme::ACCENT, 200),
        );
    }

    ui.ctx()
        .request_repaint_after(std::time::Duration::from_secs_f64(CURSOR_BLINK));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctrl() -> Modifiers {
        Modifiers {
            ctrl: true,
            ..Default::default()
        }
    }

    #[test]
    fn ctrl_letter_maps_to_control_codes() {
        assert_eq!(encode_key(Key::C, ctrl()).as_deref(), Some("\u{3}"));
        assert_eq!(encode_key(Key::A, ctrl()).as_deref(), Some("\u{1}"));
        assert_eq!(encode_key(Key::D, ctrl()).as_deref(), Some("\u{4}"));
    }

    #[test]
    fn ctrl_delete_does_not_send_eot() {
        let encoded = encode_key(Key::Delete, ctrl()).expect("Delete is encodable");
        assert_ne!(encoded, "\u{4}", "Ctrl+Delete must never send EOT");
        assert!(encoded.starts_with('\u{1b}'), "expected an escape sequence");
    }

    #[test]
    fn ctrl_space_is_nul_not_xoff() {
        assert_eq!(encode_key(Key::Space, ctrl()).as_deref(), Some("\0"));
    }

    #[test]
    fn plain_cursor_keys_use_bare_sequences() {
        let plain = Modifiers::default();
        assert_eq!(encode_key(Key::ArrowUp, plain).as_deref(), Some("\u{1b}[A"));
        assert_eq!(
            encode_key(Key::ArrowLeft, plain).as_deref(),
            Some("\u{1b}[D")
        );
        assert_eq!(encode_key(Key::Home, plain).as_deref(), Some("\u{1b}[H"));
    }

    #[test]
    fn modified_cursor_keys_carry_the_modifier() {
        assert_eq!(
            encode_key(Key::ArrowLeft, ctrl()).as_deref(),
            Some("\u{1b}[1;5D")
        );
        assert_eq!(
            encode_key(Key::Delete, ctrl()).as_deref(),
            Some("\u{1b}[3;5~")
        );
    }

    #[test]
    fn enter_and_backspace_match_terminal_convention() {
        let plain = Modifiers::default();
        assert_eq!(encode_key(Key::Enter, plain).as_deref(), Some("\r"));
        assert_eq!(encode_key(Key::Backspace, plain).as_deref(), Some("\u{7f}"));
    }
}
