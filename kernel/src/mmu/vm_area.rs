use alloc::vec::Vec;

use super::{pgtable::EntryPerm, types::PageAlignedUsize};

pub type Section = (PageAlignedUsize, PageAlignedUsize);

#[derive(Debug, Clone, Copy)]
pub struct VmArea {
    pub va: PageAlignedUsize,
    pub size: PageAlignedUsize,
    pub perm: EntryPerm,
}

impl VmArea {
    #[must_use]
    #[inline]
    pub fn start_va(&self) -> PageAlignedUsize {
        self.va
    }

    #[must_use]
    #[inline]
    pub fn end_va(&self) -> PageAlignedUsize {
        self.va + self.size
    }

    #[must_use]
    #[inline]
    pub fn contains(&self, va: usize) -> bool {
        va >= *self.va && va < *self.end_va()
    }
}

#[derive(Default)]
pub struct VmAreaList {
    list: Vec<VmArea>,
}

impl VmAreaList {
    pub const fn new() -> Self {
        Self { list: Vec::new() }
    }

    pub fn add(&mut self, vm_area: VmArea) -> Result<(), &'static str> {
        if self.is_overlap(vm_area.va, vm_area.size) {
            Err("couldn't add a new vm area.")
        } else {
            self.list.push(vm_area);
            Ok(())
        }
    }

    pub fn is_overlap(&self, start_va: PageAlignedUsize, sz: PageAlignedUsize) -> bool {
        let Some(end_va) = start_va.checked_add(*sz) else {
            return false;
        };

        for area in &self.list {
            if *start_va < *area.end_va() && *area.start_va() < end_va {
                return true; // Overlap detected
            }
        }
        false
    }

    /// Remove by start va.
    pub fn remove_by_va(&mut self, va: PageAlignedUsize) -> Option<VmArea> {
        if let Some(pos) = self.list.iter().position(|area| area.contains(*va)) {
            Some(self.list.remove(pos))
        } else {
            None
        }
    }

    pub fn get_by_va(&self, va: PageAlignedUsize) -> Option<VmArea> {
        self.list
            .iter()
            .position(|area| area.contains(*va))
            .map(|pos| self.list[pos])
    }

    pub fn iter(&self) -> impl Iterator<Item = &VmArea> {
        self.list.iter()
    }
}

impl IntoIterator for VmAreaList {
    type Item = VmArea;

    type IntoIter = alloc::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.list.into_iter()
    }
}
