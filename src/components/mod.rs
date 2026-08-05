use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::Rect;

pub mod create_form;
pub mod list;
pub mod progress;

pub fn fit_tail(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count <= width {
        return text.to_string();
    }
    let tail: String = text.chars().skip(count - width.saturating_sub(1)).collect();
    format!("…{tail}")
}

pub enum KeyEventResponse {
    Consumed,
    Ignored,
}

pub trait Component {
    fn render(&mut self, frame: &mut Frame, area: Rect);
    fn handle_event_key(&mut self, key_event: KeyEvent) -> KeyEventResponse;
}

#[cfg(test)]
pub fn buffer_to_string(buf: &ratatui::buffer::Buffer) -> String {
    buf.content.iter().map(|cell| cell.symbol()).collect()
}

#[cfg(test)]
pub fn key(code: crossterm::event::KeyCode) -> KeyEvent {
    KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
}
