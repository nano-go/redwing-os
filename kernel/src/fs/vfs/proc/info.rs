use core::fmt::Write;

use alloc::{borrow::Cow, format, string::String, vec::Vec};
use redwing_ram::file::FileContentProvider;

use crate::{
    mmu::{
        buddy,
        slab::{self, SLAB_ALLOCATORS_TABLE},
    },
    proc::cpu::CPU_INFOS,
    sync::spin::Once,
};

pub(super) fn cpuinfo() -> &'static str {
    static CPU_INFO: Once<String> = Once::new();
    CPU_INFO.get_or_init(|| {
        CPU_INFOS
            .iter()
            .filter_map(|info| info.get())
            .map(|info| format!("{}", info))
            .collect::<Vec<_>>()
            .join("\n\n")
    })
}

pub(super) struct BuddyInfo {}

impl FileContentProvider for BuddyInfo {
    fn provide_content(&self) -> Cow<'static, str> {
        format!("{}", buddy::BuddyAllocatorState::current()).into()
    }
}

pub(super) struct SlabInfo {}

impl FileContentProvider for SlabInfo {
    fn provide_content(&self) -> Cow<'static, str> {
        let mut vec: heapless::Vec<slab::SlabInfo, 32> = heapless::Vec::new();
        for slab in SLAB_ALLOCATORS_TABLE.lock().iter() {
            vec.push(slab.info()).unwrap();
        }
        let mut info = String::new();
        let name_w = vec
            .iter()
            .map(|slabinfo| slabinfo.name.len().max(4))
            .reduce(|acc, e| acc.max(e))
            .unwrap();

        let _ = info.write_fmt(format_args!(
            "{:<name_w$}  {}  {}  {}  {}  {}  {}  {}\n\n",
            "name",
            "active objs",
            "num objs",
            "obj size",
            "objs per slab",
            "pages per slab",
            "num slabs",
            "batchcount"
        ));

        for slab_info in vec {
            let _ = info.write_fmt(format_args!(
                "{:<name_w$}  {:>11}  {:>8}  {:>8}  {:>13}  {:>14}  {:>9}  {:>10}\n",
                slab_info.name,
                slab_info.active_objs,
                slab_info.num_objs,
                slab_info.object_size,
                slab_info.objs_per_slab,
                slab_info.pages_per_slab(),
                slab_info.num_slabs,
                slab_info.batchcount
            ));
        }
        info.into()
    }
}
