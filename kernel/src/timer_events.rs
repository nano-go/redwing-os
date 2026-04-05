use core::time::Duration;

use alloc::{collections::binary_heap::BinaryHeap, sync::Arc};
use lazy_static::lazy_static;

use crate::{
    arch::{cpu::cpuid, timer::timer_now},
    sync::{percpu::PerCpu, spin::Spinlock},
    utils::id::{IdAllocator, SelfIncIdAllocator},
};

pub type TimerEventFn = Arc<dyn Fn() + Sync + Send>;

lazy_static! {
    static ref TIMER_EVENT_LIST: PerCpu<Spinlock<TimerEventList>> =
        PerCpu::from_fn(|_| Spinlock::new("timer_list", TimerEventList::new()));
}

lazy_static! {
    static ref GLOBAL_ID_ALLOCATOR: SelfIncIdAllocator = SelfIncIdAllocator::new();
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimerEventId {
    cpuid: usize,
    id: usize,
}

struct TimerEvent {
    id: TimerEventId,
    callback: TimerEventFn,
    expired_time: Duration,
}

impl Ord for TimerEvent {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.expired_time.cmp(&other.expired_time).reverse()
    }
}

impl PartialOrd for TimerEvent {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for TimerEvent {}
impl PartialEq for TimerEvent {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

#[derive(Default)]
pub struct TimerEventList {
    lists: BinaryHeap<TimerEvent>,
}

impl TimerEventList {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            lists: BinaryHeap::new(),
        }
    }

    pub fn add_event<F>(&mut self, time: Duration, event: F) -> TimerEventId
    where
        F: Fn() + Sync + Send + 'static,
    {
        let id = TimerEventId {
            cpuid: cpuid(),
            id: GLOBAL_ID_ALLOCATOR.allocate_id(),
        };
        self.lists.push(TimerEvent {
            id: id.clone(),
            callback: Arc::new(event),
            expired_time: timer_now() + time,
        });
        id
    }

    pub fn remove_event(&mut self, id: TimerEventId) {
        self.lists.retain(|event| event.id != id);
    }

    pub fn tick(&mut self, jiffies: Duration) {
        while let Some(event) = self.lists.peek() {
            if event.expired_time > jiffies {
                break;
            }
            let event = self.lists.pop().unwrap();
            (event.callback)();
        }
    }
}

pub fn add_event<F>(time: Duration, event: F) -> TimerEventId
where
    F: Fn() + Sync + Send + 'static,
{
    TIMER_EVENT_LIST
        .lock_irq_save()
        .lock()
        .add_event(time, event)
}

pub fn remove_event(id: TimerEventId) {
    let timer_list = unsafe {
        // SAFETY: we use spinlock to ensure the synchronization.
        TIMER_EVENT_LIST.get_by_cpuid(id.cpuid)
    };
    timer_list.lock_irq().remove_event(id);
}

pub fn tick(jiffies: Duration) {
    TIMER_EVENT_LIST.lock_irq_save().lock().tick(jiffies);
}

#[cfg(test)]
mod tests {
    use core::time::Duration;

    use alloc::{collections::binary_heap::BinaryHeap, sync::Arc};

    use crate::timer_events::TimerEventId;

    use super::TimerEvent;

    #[test_case]
    pub fn test_cmp() {
        let mut heap = BinaryHeap::new();
        heap.push(TimerEvent {
            id: TimerEventId { cpuid: 0, id: 0 },
            expired_time: Duration::from_secs(1),
            callback: Arc::new(|| {}),
        });

        heap.push(TimerEvent {
            id: TimerEventId { cpuid: 0, id: 1 },
            expired_time: Duration::from_secs(2),
            callback: Arc::new(|| {}),
        });

        heap.push(TimerEvent {
            id: TimerEventId { cpuid: 0, id: 2 },
            expired_time: Duration::from_millis(500),
            callback: Arc::new(|| {}),
        });

        heap.push(TimerEvent {
            id: TimerEventId { cpuid: 0, id: 3 },
            expired_time: Duration::from_secs(1),
            callback: Arc::new(|| {}),
        });

        assert_eq!(heap.pop().unwrap().expired_time, Duration::from_millis(500));
        assert_eq!(heap.pop().unwrap().expired_time, Duration::from_secs(1));
        assert_eq!(heap.pop().unwrap().expired_time, Duration::from_secs(1));
        assert_eq!(heap.pop().unwrap().expired_time, Duration::from_secs(2));
    }
}
