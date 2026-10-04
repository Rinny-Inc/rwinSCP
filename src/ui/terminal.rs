use crate::app::{Action, App};
use crate::theme;
use crate::ui::{keep, widgets};
use crate::vt::{Attrs, Cell};
use egui::{Event, Key, Modifiers, RichText, Ui};

pub fn show(app: &mut App, ui: &mut Ui) -> Option<Action> {
    let mut action = None;
    let session = app.session()?;

    if let Some(other) = ui.memory(|m| m.focused()) {
        ui.memory_mut(|m| m.surrender_focus(other));
    }

    let app_keys = session
        .terminal
        .as_ref()
        .is_some_and(|t| t.app_cursor_keys());

    let keystrokes = ui.input_mut(|i| {
        let bytes = encode_events(&i.events, app_keys);
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

    ui.add_space(theme::S1);

    let Some(terminal) = &session.terminal else {
        widgets::empty_state(ui, "No shell", "This session has no interactive shell");
        return action;
    };

    let frame = egui::Frame::default()
        .fill(theme::BG_SURFACE)
        .stroke(egui::Stroke::new(1.0, theme::ACCENT))
        .corner_radius(theme::R_MD)
        .inner_margin(theme::S1 as i8);

    frame.show(ui, |ui| {
        if terminal.is_empty() {
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

        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;

        let pane_height = ui.available_height();
        if advance > 0.0 && row_height > 0.0 {
            let cols = (ui.available_width() / advance).floor().max(20.0) as u32;
            let rows = (pane_height / row_height).floor().max(4.0) as u32;
            keep(&mut action, Some(Action::ShellResize(cols, rows)));
        }

        let pad = top_padding(
            pane_height,
            row_height,
            terminal.row_count(),
            terminal.is_alt_screen(),
        );
        if pad > 0.0 {
            ui.add_space(pad);
        }

        egui::ScrollArea::vertical()
            .id_salt("terminal_scroll")
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show_rows(ui, row_height, terminal.row_count(), |ui, rows| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                let caret = terminal.cursor();

                for index in rows {
                    let caret_col = caret.filter(|(row, _)| *row == index).map(|(_, col)| col);
                    draw_row(
                        ui,
                        terminal.row_cells(index),
                        caret_col,
                        &font,
                        row_height,
                        advance,
                    );
                }
            });
    });

    action
}

fn encode_events(events: &[Event], app_keys: bool) -> String {
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
                if let Some(seq) = encode_key(*key, *modifiers, app_keys) {
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

fn encode_key(key: Key, modifiers: Modifiers, app_keys: bool) -> Option<String> {
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
            if app_keys {
                format!("\x1bO{final_byte}")
            } else {
                format!("\x1b[{final_byte}")
            }
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

fn top_padding(pane_height: f32, row_height: f32, rows: usize, alt_screen: bool) -> f32 {
    if alt_screen || row_height <= 0.0 {
        return 0.0;
    }
    (pane_height - rows as f32 * row_height).max(0.0)
}

fn row_layout(cells: &[Cell], font: &egui::FontId) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();

    let end = cells
        .iter()
        .rposition(|c| c.ch != ' ' || c.attrs != Attrs::default())
        .map_or(0, |i| i + 1);
    let mut start = 0;
    while start < end {
        let attrs = cells[start].attrs.clone();
        let mut stop = start;
        let mut text = String::new();
        while stop < end && cells[stop].attrs == attrs {
            text.push(cells[stop].ch);
            stop += 1;
        }

        let mut fg = attrs.fg.unwrap_or(theme::TEXT);
        let mut bg = attrs.bg.unwrap_or(egui::Color32::TRANSPARENT);
        if attrs.inverse {
            if bg == egui::Color32::TRANSPARENT {
                bg = theme::BG_SURFACE;
            }
            std::mem::swap(&mut fg, &mut bg);
        }

        job.append(
            &text,
            0.0,
            egui::TextFormat {
                font_id: font.clone(),
                color: fg,
                background: bg,
                ..Default::default()
            },
        );
        start = stop;
    }

    job
}

fn draw_row(
    ui: &mut Ui,
    cells: &[Cell],
    caret_col: Option<usize>,
    font: &egui::FontId,
    row_height: f32,
    advance: f32,
) {
    let galley = ui.fonts_mut(|f| f.layout_job(row_layout(cells, font)));
    let cell_width = advance.max(2.0);
    let width = (galley.size().x + cell_width).max(ui.available_width());
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, row_height), egui::Sense::hover());
    ui.painter().galley(rect.min, galley, theme::TEXT);

    let Some(col) = caret_col else {
        return;
    };

    let caret = rect.min + egui::vec2(col as f32 * cell_width, 0.0);

    let time = ui.input(|i| i.time);
    if (time / CURSOR_BLINK) as i64 & 1 == 0 {
        ui.painter().rect_filled(
            egui::Rect::from_min_size(caret, egui::vec2(cell_width, row_height)),
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
        assert_eq!(encode_key(Key::C, ctrl(), false).as_deref(), Some("\u{3}"));
        assert_eq!(encode_key(Key::A, ctrl(), false).as_deref(), Some("\u{1}"));
        assert_eq!(encode_key(Key::D, ctrl(), false).as_deref(), Some("\u{4}"));
    }

    #[test]
    fn ctrl_delete_does_not_send_eot() {
        let encoded = encode_key(Key::Delete, ctrl(), false).expect("Delete is encodable");
        assert_ne!(encoded, "\u{4}", "Ctrl+Delete must never send EOT");
        assert!(encoded.starts_with('\u{1b}'), "expected an escape sequence");
    }

    #[test]
    fn ctrl_space_is_nul_not_xoff() {
        assert_eq!(encode_key(Key::Space, ctrl(), false).as_deref(), Some("\0"));
    }

    #[test]
    fn plain_cursor_keys_use_bare_sequences() {
        let plain = Modifiers::default();
        assert_eq!(
            encode_key(Key::ArrowUp, plain, false).as_deref(),
            Some("\u{1b}[A")
        );
        assert_eq!(
            encode_key(Key::ArrowLeft, plain, false).as_deref(),
            Some("\u{1b}[D")
        );
        assert_eq!(
            encode_key(Key::Home, plain, false).as_deref(),
            Some("\u{1b}[H")
        );
    }

    #[test]
    fn modified_cursor_keys_carry_the_modifier() {
        assert_eq!(
            encode_key(Key::ArrowLeft, ctrl(), false).as_deref(),
            Some("\u{1b}[1;5D")
        );
        assert_eq!(
            encode_key(Key::Delete, ctrl(), false).as_deref(),
            Some("\u{1b}[3;5~")
        );
    }

    #[test]
    fn application_cursor_keys_use_the_ss3_form() {
        let plain = Modifiers::default();
        assert_eq!(
            encode_key(Key::ArrowUp, plain, true).as_deref(),
            Some("\u{1b}OA"),
            "nano and vim ask for DECCKM and expect ESC O A"
        );
        assert_eq!(
            encode_key(Key::ArrowUp, plain, false).as_deref(),
            Some("\u{1b}[A")
        );
    }

    #[test]
    fn enter_and_backspace_match_terminal_convention() {
        let plain = Modifiers::default();
        assert_eq!(encode_key(Key::Enter, plain, false).as_deref(), Some("\r"));
        assert_eq!(
            encode_key(Key::Backspace, plain, false).as_deref(),
            Some("\u{7f}")
        );
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    use crate::vt::Terminal;

    #[test]
    fn rows_occupy_exactly_one_row_height_each() {
        const ROWS: usize = 10;
        let ctx = egui::Context::default();
        let mut terminal = Terminal::new(40, ROWS);
        for i in 0..ROWS {
            terminal.feed(&format!("row {i}\r\n"));
        }

        let mut measured = 0.0_f32;
        let mut row_height = 0.0_f32;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let font = egui::TextStyle::Monospace.resolve(ui.style());
            row_height = ui.fonts_mut(|f| f.row_height(&font));
            let advance = ui.fonts_mut(|f| f.glyph_width(&font, 'M'));

            let before = ui.cursor().top();
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            for index in 0..ROWS {
                draw_row(
                    ui,
                    terminal.row_cells(index),
                    None,
                    &font,
                    row_height,
                    advance,
                );
            }
            measured = ui.cursor().top() - before;
        });
        output.textures_delta.clear();

        assert!(row_height > 0.0);
        let expected = row_height * ROWS as f32;
        assert!(
            (measured - expected).abs() < 0.5,
            "{ROWS} rows took {measured}px but the PTY was sized for {expected}px"
        );
    }
}

#[cfg(test)]
mod scroll_geometry_tests {
    use super::*;
    use crate::vt::Terminal;

    #[test]
    fn the_last_row_ends_flush_with_the_bottom_of_the_viewport() {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 400.0),
            )),
            ..Default::default()
        };

        let mut terminal = Terminal::new(60, 20);
        for i in 0..200 {
            terminal.feed(&format!("line {i}\r\n"));
        }

        let mut gap = f32::NAN;
        for _ in 0..2 {
            let mut out = ctx.run_ui(input.clone(), |ui| {
                let font = egui::TextStyle::Monospace.resolve(ui.style());
                let row_height = ui.fonts_mut(|f| f.row_height(&font));
                let advance = ui.fonts_mut(|f| f.glyph_width(&font, 'M'));
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show_rows(ui, row_height, terminal.row_count(), |ui, rows| {
                        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                        let floor = ui.clip_rect().bottom();
                        for index in rows {
                            draw_row(
                                ui,
                                terminal.row_cells(index),
                                None,
                                &font,
                                row_height,
                                advance,
                            );
                        }
                        gap = floor - ui.cursor().top();
                    });
            });
            out.textures_delta.clear();
        }

        assert!(
            gap.abs() <= 1.0,
            "the newest line sits {gap}px above the floor of the pane"
        );
    }
}

#[cfg(test)]
mod anchor_tests {
    use super::*;

    const PANE: f32 = 400.0;
    const ROW: f32 = 16.0;

    #[test]
    fn a_short_session_sits_on_the_floor_of_the_pane() {
        let pad = top_padding(PANE, ROW, 3, false);
        assert_eq!(pad, PANE - 3.0 * ROW);
        assert_eq!(
            pad + 3.0 * ROW,
            PANE,
            "the last row must end flush with the bottom of the pane"
        );
    }

    #[test]
    fn a_full_screen_is_not_padded() {
        assert_eq!(top_padding(PANE, ROW, 25, false), 0.0);
    }

    #[test]
    fn scrollback_longer_than_the_pane_is_not_padded() {
        assert_eq!(top_padding(PANE, ROW, 400, false), 0.0);
    }

    #[test]
    fn the_alternate_screen_is_never_padded() {
        assert_eq!(
            top_padding(PANE, ROW, 3, true),
            0.0,
            "padding nano would push its shortcut bars off the bottom"
        );
    }

    #[test]
    fn output_arriving_shrinks_the_padding_one_row_at_a_time() {
        let before = top_padding(PANE, ROW, 5, false);
        let after = top_padding(PANE, ROW, 6, false);
        assert_eq!(
            before - after,
            ROW,
            "each new line consumes exactly one row"
        );
    }
}
