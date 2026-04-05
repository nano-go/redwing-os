use crate::{error, fs, proc::task};

pub const DEFAULT_ENVRON: [&str; 3] = ["PATH=/bin", "HOME=/home", "PWD=/"];

pub fn setup_init_task() {
    let init_task_tid = task::spawn(|| init_main());
    task::set_init_task(init_task_tid);
}

fn init_main() -> ! {
    fs::fs_init();

    task::set_name_for_kernel_task("init");
    task::set_sid().unwrap();

    #[cfg(test)]
    crate::spawn_test_task();
    #[cfg(not(test))]
    setup_kernel_tasks();

    loop {
        let mut _status = 0;
        if let Err(err) = task::wait_with_xstatus(None, &mut _status) {
            if err.kind != error::SysErrorKind::Interrupted {
                log::warn!("init wait error: {err}");
            }
        }
    }
}

#[cfg(not(test))]
fn setup_kernel_tasks() {
    use crate::printkln;

    printkln!("Now the OS kernel is initialized");

    open_stdio_fds();

    // Test sleep.
    task::spawn(|| loop {
        task::set_name_for_kernel_task("test_sleep");
        // log::info!("now: {}ms", arch::timer::timer_now().as_millis());
        let _ = task::sleep(core::time::Duration::from_secs(3));
    });

    task::spawn(|| fs_sync_events());
    task::spawn(|| shell_main());
}

#[cfg(not(test))]
fn shell_main() {
    use crate::arch;
    use crate::printkln;
    use crate::proc;

    task::set_name_for_kernel_task("wait_shell");
    task::set_sid().unwrap();

    let tid = task::spawn(|| {
        {
            task::set_sid().unwrap();
            fs::ioctl::tcsetpgrp(0, proc::id::Tid::zero()).unwrap();
        }
        let err = proc::exec::execve(b"/bin/sh", &[], &DEFAULT_ENVRON).unwrap_err();
        printkln!("couldn't start shell: {err} ('/bin/sh')");
    });

    task::wait(Some(tid));
    arch::cpu::exit_in_qemu();
}

#[cfg(not(test))]
fn fs_sync_events() {
    task::set_sid().unwrap();
    task::set_name_for_kernel_task("fs_sync");
    loop {
        if let Err(err) = fs::fcntl::sync() {
            log::error!("sync error: {}", err);
        }
        log::trace!("fs: sync");
        let _ = task::sleep(core::time::Duration::from_secs(3));
    }
}

#[cfg(not(test))]
fn open_stdio_fds() {
    use crate::arch;
    use crate::printkln;
    use error::KResult;
    use fs::fcntl;
    use rw_ulib_types::fcntl::OpenFlags;

    fn _open_stdio_fds() -> KResult<()> {
        let _stdin = fcntl::open(b"/dev/tty", OpenFlags::RDONLY)?;
        let _stdout = fcntl::open(b"/dev/tty", OpenFlags::WRONLY)?;
        Ok(())
    }

    if let Err(err) = _open_stdio_fds() {
        printkln!("couldn't open the stdio fds: {err}");
        arch::cpu::exit_in_qemu();
    }
}
