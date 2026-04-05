use log::trace;
use path::Path;
use redwing_vfs::VfsINodeOps;
use rw_ulib_types::fcntl::FileType;

use crate::{
    arch::{ctx::Trapframe, memlayout::USER_BASE_VADDR, trap::user_trap_ret},
    elf::{ElfHeader, ProgramHeader, PT_LOAD},
    error::{KResult, SysErrorKind},
    fs::pathname,
    mmu::{
        pg_round_up,
        pgtable::{EntryPerm, PageTable},
        types::PageAlignedUsize,
        vm::{self, VM},
        PGSIZE,
    },
    params::TASK_USER_STACK_SIZE,
};

use super::{
    id::Tid,
    task::{self, Task, TASK_NAME_LEN},
};

const USER_ARGUMENTS_MAX_SIZE: usize = PGSIZE;
const ENV_VARS_MAX_SIZE: usize = PGSIZE * 3;

pub fn fork() -> KResult<Tid> {
    let cur_task = task::current_task_or_err()?;

    let (vm, name) = {
        let cur_task = cur_task.lock();
        (cur_task.vm.clone(), cur_task.name.clone())
    };

    // Creates a new task with the specified name.
    let new_task = Task::with_name(name)?;
    {
        let new_task = new_task.lock();
        let mut new_vm = new_task.vm.lock();
        new_vm.copy_user_content_from(&vm.lock())?;
        // Set return value for child task.
        new_vm.trapframe().set_return_val(0);
    }

    // start_with_parent() will copy open files and current inode from the specified
    // parent task.
    new_task.start_with_parent(Some(cur_task), || user_trap_ret())?;
    Ok(new_task.tid)
}

pub fn execve(pathname: &[u8], user_args: &[&str], env_vars: &[&str]) -> KResult<!> {
    let (new_vm, old_vm) = {
        let task = task::current_task_or_err()?;
        let task_name = task_name_from_path(pathname)?;

        let inode = pathname::lookup(pathname)?;
        check_executable(&*inode)?;

        let new_vm = VM::with_kernel_vm()?;

        trace!(target: "execve", "load '{task_name}'");

        let elfhdr = load_elf(&new_vm, &*inode)?;
        let sp = init_user_stack(&new_vm)?;
        let args_ptr = push_cstr_array(&new_vm, sp, user_args, USER_ARGUMENTS_MAX_SIZE)?;
        let env_ptrs = push_cstr_array(&new_vm, args_ptr, env_vars, ENV_VARS_MAX_SIZE)?;
        let sp = env_ptrs;

        let mut trapframe = Trapframe::new();
        trapframe.set_arg0(args_ptr as u64);
        trapframe.set_arg1(env_ptrs as u64);
        trapframe.set_sp(sp as u64);
        trapframe.set_ret_pc(elfhdr.entry);
        new_vm.lock().alloc_trapframe(trapframe)?;

        let old_vm = {
            let mut task = task.lock();
            task.set_task_name(task_name);
            core::mem::replace(&mut task.vm, new_vm.clone())
        };

        (new_vm, old_vm)
    };

    vm::switch_vm(&new_vm);

    drop(new_vm);
    drop(old_vm);

    user_trap_ret();
}

fn check_executable(inode: &dyn VfsINodeOps) -> KResult<()> {
    match inode.metadata()?.typ {
        FileType::RegularFile => Ok(()),
        FileType::Directory => Err(SysErrorKind::IsADirectory.into()),
        FileType::Device => Err(SysErrorKind::NoExec.into()),
        FileType::Symlink => panic!("unexpected"),
    }
}

fn task_name_from_path(pathname: &[u8]) -> KResult<heapless::String<TASK_NAME_LEN>> {
    let filename = Path::new(pathname).name().unwrap_or(b"user-task");
    Ok(heapless::String::try_from(str::from_utf8(filename)?)
        .map_err(|_| SysErrorKind::InvalidArgument)?)
}

/// Initialize the user stack and return the pointer to the stack top.
fn init_user_stack(vm: &VM) -> KResult<usize> {
    let mut vm = vm.lock();
    // A guard page for user stack.
    vm.uvgrow_page(EntryPerm::empty())?;

    // Lazily allocate user stack.
    let sz = vm.user_space_size() + TASK_USER_STACK_SIZE - PGSIZE * 4;
    vm.uvalloc(sz, EntryPerm::with_rw(), true)?;
    // Immediately allocate the 4 pages because we'll be ready to place user
    // arguments and envronments onto the stack.
    let sz = sz + PGSIZE * 4;
    vm.uvalloc(sz, EntryPerm::with_rw(), false)?;

    // A guard page for user stack.
    vm.uvgrow_page(EntryPerm::empty())?;

    Ok(USER_BASE_VADDR + sz)
}

/// Pushes an array of C-style null-terminated strings onto the virtual
/// machine's stack.
///
/// The stack is assumed to grow downwards (from higher to lower addresses).
/// The data layout on the stack after this operation will generally be:
///
/// ```text
/// +-----------------------+ <- new_sp
/// |  String N bytes       | (e.g., "argN\0")
/// |  ...                  |
/// |  String 1 bytes       | (e.g., "arg1\0")
/// |  Number of arguments  | (usize, little-endian)
/// +-----------------------+
/// ```
fn push_cstr_array(vm: &VM, mut sp: usize, cstrs: &[&str], max_len: usize) -> KResult<usize> {
    let cstrs_layout_sz: usize = crate::mmu::align_to_word(
        size_of::<usize>() + cstrs.iter().map(|arg| arg.len() + 1).sum::<usize>(),
    );

    if cstrs_layout_sz > max_len {
        return Err(SysErrorKind::InvalidArgument.into());
    }

    let new_sp = sp - cstrs_layout_sz;
    sp = new_sp;

    let vm = vm.lock();

    // Writes the length of arguments.
    sp = vm.write(sp, &cstrs.len().to_le_bytes()[..])?;

    for arg in cstrs {
        let bytes = arg.as_bytes();
        sp = vm.write(sp, bytes)?;
        // Write null-terminated byte.
        sp = vm.write(sp, &[0])?;
    }

    Ok(new_sp)
}

fn load_elf(vm: &VM, inode: &dyn VfsINodeOps) -> KResult<ElfHeader> {
    let elfhdr = ElfHeader::load_from(inode)?;
    elfhdr.check_valid()?;

    for i in 0..elfhdr.phnum {
        let proghdr = ProgramHeader::load_from(inode, &elfhdr, i as usize)?;
        if proghdr.typ != PT_LOAD {
            continue;
        }
        proghdr.check_valid()?;
        load_segment(inode, &proghdr, vm)?;
    }

    Ok(elfhdr)
}

fn load_segment(inode: &dyn VfsINodeOps, phdr: &ProgramHeader, vm: &VM) -> KResult<()> {
    let perm = flags_to_entry_perm(phdr.flags);
    let vaddr = PageAlignedUsize::new(phdr.vaddr as usize).ok_or(SysErrorKind::InvalidArgument)?;
    let memsz = pg_round_up(phdr.memsz as usize);

    let mut vm = vm.lock();

    // Load user-accessiable virtual memory for the segment.
    vm.alloc_segment(vaddr, memsz, perm | EntryPerm::USER)?;

    let mut phys_ptrs = vm
        .pg_table
        .range_of_entries(vaddr, vaddr + memsz)
        .map(|(_va, entry)| entry.physical_addr() as *mut u8);
    let mut file_offset = 0;

    while file_offset < phdr.filesz {
        let to_read = usize::min((phdr.filesz - file_offset) as usize, PGSIZE);

        let buf = unsafe {
            // SAFETY: the physical address is mapped to a valid page frame that
            // can be directly accessed from kernel.
            core::slice::from_raw_parts_mut(phys_ptrs.next().unwrap(), PGSIZE)
        };

        let len = inode.read(phdr.offset + file_offset, &mut buf[..to_read])?;
        if len as usize != to_read {
            buf[len as usize..to_read].fill(0);
        }

        file_offset += len;
    }

    Ok(())
}

fn flags_to_entry_perm(flags: u32) -> EntryPerm {
    let mut perm = EntryPerm::empty();
    if flags & 0x1 != 0 {
        perm |= EntryPerm::EXECUTABLE;
    }
    if flags & 0x2 != 0 {
        perm |= EntryPerm::WRITABLE;
    }
    if flags & 0x4 != 0 {
        perm |= EntryPerm::READABLE;
    }
    perm
}

#[cfg(test)]
mod tests {
    use crate::{error::SysErrorKind, proc::task};

    #[test_case]
    pub fn test_exec_dev() {
        let tid = task::spawn(|| {
            let err = super::execve(b"/dev/tty", &[], &[]).unwrap_err();
            assert_eq!(err.kind, SysErrorKind::NoExec);
        });

        task::wait(Some(tid)).unwrap();
    }

    #[test_case]
    pub fn test_exec_dir() {
        let tid = task::spawn(|| {
            let err = super::execve(b"/home", &[], &[]).unwrap_err();
            assert_eq!(err.kind, SysErrorKind::IsADirectory);
        });

        task::wait(Some(tid)).unwrap();
    }
}
