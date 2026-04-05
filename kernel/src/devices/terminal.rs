use core::any::Any;

use alloc::{
    collections::linked_list::LinkedList,
    sync::{self},
};

use crate::{error::KResult, sync::spin::Spinlock};

pub trait TextScreen: Any + Sync + Send {
    fn write(&self, buf: &[u8]) -> KResult<usize>;
    fn clear_screen(&self) -> KResult<()>;

    fn insert_whitespace_at_cursor(&self);

    /// Delete the character before cursor and move cursor left.
    fn delete_char_before_cursor(&self);

    /// Move cursor left n times
    fn move_cursor_left(&self, n: usize);

    /// Move cursor right n times
    fn move_cursor_rigth(&self, n: usize);

    /// Clear chars after cursor in the line.
    fn clear_to_end_of_line(&self);
}

pub trait InputReceiver: Any + Sync + Send {
    fn receive_input(&self, buf: &[u8]);
}

pub struct InputReciverList {
    pub list: Spinlock<LinkedList<sync::Weak<dyn InputReceiver>>>,
}

impl Default for InputReciverList {
    fn default() -> Self {
        Self::new()
    }
}

impl InputReciverList {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            list: Spinlock::new("input_clients", LinkedList::new()),
        }
    }

    pub fn register(&self, input_client: sync::Weak<dyn InputReceiver>) {
        self.list.lock_irq_save().push_back(input_client);
    }

    pub fn receive_input(&self, input: &[u8]) {
        let mut list = self.list.lock_irq_save();
        let mut cursor = list.cursor_front_mut();

        while let Some(client) = cursor.current() {
            if let Some(client) = client.upgrade() {
                client.receive_input(input);
                cursor.move_next();
            } else {
                cursor.remove_current();
            }
        }
    }
}
