use const_default::ConstDefault;
use redwing_vfs::VfsINodeOps;
use syserr::sys_err;

use crate::{
    arch::memlayout::{USER_ELF_BASE_VADDR, USER_ELF_END_VADDR, USER_ELF_SIZE},
    error::{KResult, SysErrorKind},
    mmu::PGSIZE,
};

const MAGIC: &[u8; 4] = b"\x7FELF";

#[repr(C)]
#[derive(Default, ConstDefault, Debug, Clone, Copy)]
pub struct ElfHeader {
    pub ident: [u8; 16],
    pub typ: u16,
    pub machine: u16,
    pub version: u32,
    pub entry: u64,
    pub phoff: u64,
    pub shoff: u64,
    pub flags: u32,
    pub ehsize: u16,
    pub phentsize: u16,
    pub phnum: u16,
    pub shentsize: u16,
    pub shnum: u16,
    pub shstrndx: u16,
}

impl ElfHeader {
    #[inline]
    pub fn load_from(inode: &dyn VfsINodeOps) -> KResult<Self> {
        let mut dst = Self::DEFAULT;
        unsafe { inode.read_struct(0, &mut dst) }.map_err(|_| SysErrorKind::ExecFormat)?;
        Ok(dst)
    }

    pub fn check_valid(&self) -> KResult<()> {
        if &self.ident[..4] != MAGIC {
            return Err(sys_err!(SysErrorKind::ExecFormat, "bad elf magic"));
        }
        Ok(())
    }
}

pub const PT_LOAD: u32 = 1;
#[repr(C)]
#[derive(Default, ConstDefault, Debug, Clone, Copy)]
pub struct ProgramHeader {
    pub typ: u32,
    pub flags: u32,
    pub offset: u64,
    pub vaddr: u64,
    pub paddr: u64,
    pub filesz: u64,
    pub memsz: u64,
    pub align: u64,
}

impl ProgramHeader {
    #[inline]
    pub fn load_from(inode: &dyn VfsINodeOps, hdr: &ElfHeader, index: usize) -> KResult<Self> {
        let offset = hdr.phoff + hdr.phentsize as u64 * index as u64;
        let mut dst = Self::DEFAULT;
        unsafe { inode.read_struct(offset, &mut dst) }.map_err(|_| SysErrorKind::ExecFormat)?;
        Ok(dst)
    }

    pub fn check_valid(&self) -> KResult<()> {
        if self.vaddr % PGSIZE as u64 != 0 {
            return Err(sys_err!(
                SysErrorKind::ExecFormat,
                "phdr: vaddr is not page-aligned"
            ));
        }

        if self.vaddr.checked_add(self.memsz).is_none() {
            return Err(sys_err!(
                SysErrorKind::ExecFormat,
                "phdr: invalid vaddr or memsz"
            ));
        }

        if self.filesz > self.memsz {
            return Err(sys_err!(SysErrorKind::ExecFormat, "phdr: filesz > memsz"));
        }

        if self.memsz > USER_ELF_SIZE as u64 {
            return Err(sys_err!(SysErrorKind::ExecFormat, "phdr: too large memsz"));
        }

        if self.vaddr < USER_ELF_BASE_VADDR as u64
            || self.vaddr + self.memsz > USER_ELF_END_VADDR as u64
        {
            return Err(sys_err!(
                SysErrorKind::ExecFormat,
                "phdr: vaddr..+=memsz is invalid"
            ));
        }
        Ok(())
    }
}
