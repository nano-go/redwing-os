use core::mem;

use alloc::{collections::vec_deque::VecDeque, sync::Arc};

use crate::devices::terminal::TextScreen;

pub enum Key {
    Eof,
    Down,
    Up,
    Left,
    Right,
    Backspace,
    CtrlU,
    CtrlD,
    Home,
    End,
    Ignored,
    Normal(u8),
}

impl Key {
    #[must_use]
    pub fn from_escape(escape_chars: [u8; 2]) -> Key {
        match escape_chars {
            [b'[', b'D'] => Key::Left,
            [b'[', b'C'] => Key::Right,
            [b'[', b'A'] => Key::Up,
            [b'[', b'B'] => Key::Down,
            [b'[', b'H'] => Key::Home,
            [b'[', b'F'] => Key::End,
            _ => Key::Ignored,
        }
    }
}

pub struct LineDiscipline {
    completed_lines: VecDeque<VecDeque<u8>>,

    line_buffer: VecDeque<u8>,
    edit: usize,

    read_escape_idx: Option<usize>,
    escape_chars: [u8; 2],

    echo: bool,
    canonical: bool,

    text_screen: Arc<dyn TextScreen>,
}

impl LineDiscipline {
    #[must_use]
    pub fn new(text_screen: Arc<dyn TextScreen>) -> Self {
        Self {
            completed_lines: VecDeque::new(),
            line_buffer: VecDeque::new(),
            edit: 0,
            read_escape_idx: None,
            escape_chars: [0; 2],
            echo: true,
            canonical: true,
            text_screen,
        }
    }

    #[must_use]
    pub fn is_canonical(&self) -> bool {
        self.canonical
    }

    pub fn completed_line(&mut self) -> Option<VecDeque<u8>> {
        self.completed_lines.pop_front()
    }

    fn read_key(&mut self, ch: u8) -> Key {
        if let Some(read_escape_idx) = &mut self.read_escape_idx {
            // We are reading an escape char.
            self.escape_chars[*read_escape_idx] = ch;
            *read_escape_idx += 1;
            if *read_escape_idx == 2 {
                self.read_escape_idx = None;
                return Key::from_escape(self.escape_chars);
            }
            return Key::Ignored;
        }

        match ch {
            0x0 => Key::Eof,

            // ESC
            0x1B => {
                self.read_escape_idx = Some(0);
                Key::Ignored
            }

            // Backspace and Delete
            0x8 | 0x7F => Key::Backspace,

            0x15 => Key::CtrlU,
            0x04 => Key::CtrlD,

            _ => Key::Normal(ch),
        }
    }

    pub fn input_byte(&mut self, byte: u8) -> bool {
        let key = self.read_key(byte);

        let lines = self.completed_lines.len();

        match key {
            Key::Ignored | Key::Down | Key::Up => (),

            Key::Backspace => self.delete_char(),

            Key::CtrlU => {
                self.clear_line();
            }

            Key::CtrlD => {
                if self.line_buffer.is_empty() {
                    self.insert_char_to_buffer(0);
                }
            }

            Key::Eof => {
                self.insert_char_to_buffer(0);
            }

            Key::Home => {
                if self.echo {
                    self.text_screen.move_cursor_left(self.edit);
                }
                self.edit = 0;
            }

            Key::End => {
                if self.echo {
                    self.text_screen
                        .move_cursor_rigth(self.line_buffer.len() - self.edit);
                }
                self.edit = self.line_buffer.len();
            }

            Key::Left => {
                if self.edit != 0 {
                    self.edit -= 1;
                    if self.echo {
                        self.text_screen.move_cursor_left(1);
                    }
                }
            }

            Key::Right => {
                if self.edit < self.line_buffer.len() {
                    self.edit += 1;
                    if self.echo {
                        self.text_screen.move_cursor_rigth(1);
                    }
                }
            }

            Key::Normal(b'\t') => {
                let mut allow = true;
                for ch in &self.line_buffer {
                    if !ch.is_ascii_whitespace() {
                        allow = false;
                    }
                }
                if allow {
                    self.insert_char_to_buffer(b' ');
                }
            }

            Key::Normal(mut ch) => {
                if ch == b'\r' {
                    ch = b'\n';
                }
                self.insert_char_to_buffer(ch);
            }
        }

        lines != self.completed_lines.len()
    }

    fn insert_char_to_buffer(&mut self, ch: u8) {
        if ch == b'\n' || ch == 0 {
            self.edit = 0;
            self.line_buffer.push_back(ch);
            let line_buffer = mem::take(&mut self.line_buffer);
            self.completed_lines.push_back(line_buffer);
            self.echo_byte(ch);
        } else {
            self.line_buffer.insert(self.edit, ch);
            self.edit += 1;

            if self.echo && self.edit != self.line_buffer.len() {
                self.text_screen.insert_whitespace_at_cursor();
            }

            self.echo_byte(ch);
        }
    }

    fn delete_char(&mut self) {
        if self.edit == 0 {
            return;
        }

        if self.echo {
            self.text_screen.delete_char_before_cursor();
        }

        self.line_buffer.remove(self.edit - 1);
        self.edit -= 1;
    }

    fn clear_line(&mut self) {
        if self.echo {
            self.text_screen.move_cursor_left(self.edit);
            self.text_screen.clear_to_end_of_line();
        }

        self.line_buffer.clear();
        self.edit = 0;
    }

    #[inline]
    fn echo_byte(&self, byte: u8) {
        if self.echo {
            let _ = self.text_screen.write(&[byte]);
        }
    }
}
