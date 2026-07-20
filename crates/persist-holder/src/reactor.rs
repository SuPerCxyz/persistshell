use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io;
use std::os::fd::RawFd;

use persist_core::{PersistError, Result};

const BASE_EVENTS: u32 = (libc::EPOLLIN | libc::EPOLLRDHUP | libc::EPOLLERR) as u32;

pub(crate) struct Reactor {
    fd: RawFd,
    next_token: Cell<u64>,
    tokens: RefCell<HashMap<RawFd, u64>>,
    fds: RefCell<HashMap<u64, RawFd>>,
}

impl Reactor {
    pub(crate) fn new() -> Result<Self> {
        let fd = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };
        if fd < 0 {
            return Err(io_error("create holder epoll", io::Error::last_os_error()));
        }
        Ok(Self {
            fd,
            next_token: Cell::new(1),
            tokens: RefCell::new(HashMap::new()),
            fds: RefCell::new(HashMap::new()),
        })
    }

    pub(crate) fn add(&self, fd: RawFd, writable: bool) -> Result<()> {
        let token = self.next_token.get();
        self.next_token.set(token.wrapping_add(1).max(1));
        self.control(libc::EPOLL_CTL_ADD, fd, token, writable)?;
        self.tokens.borrow_mut().insert(fd, token);
        self.fds.borrow_mut().insert(token, fd);
        Ok(())
    }

    pub(crate) fn modify(&self, fd: RawFd, writable: bool) -> Result<()> {
        let token =
            *self.tokens.borrow().get(&fd).ok_or_else(|| {
                PersistError::invalid_argument("holder epoll fd is not registered")
            })?;
        self.control(libc::EPOLL_CTL_MOD, fd, token, writable)
    }

    pub(crate) fn remove(&self, fd: RawFd) {
        if let Some(token) = self.tokens.borrow_mut().remove(&fd) {
            self.fds.borrow_mut().remove(&token);
        }
        unsafe {
            libc::epoll_ctl(self.fd, libc::EPOLL_CTL_DEL, fd, std::ptr::null_mut());
        }
    }

    pub(crate) fn fd_for_token(&self, token: u64) -> Option<RawFd> {
        self.fds.borrow().get(&token).copied()
    }

    pub(crate) fn wait(&self, events: &mut [libc::epoll_event]) -> Result<usize> {
        loop {
            let count =
                unsafe { libc::epoll_wait(self.fd, events.as_mut_ptr(), events.len() as i32, -1) };
            if count >= 0 {
                return Ok(count as usize);
            }
            let source = io::Error::last_os_error();
            if source.kind() != io::ErrorKind::Interrupted {
                return Err(io_error("wait for holder epoll", source));
            }
        }
    }

    fn control(&self, operation: i32, fd: RawFd, token: u64, writable: bool) -> Result<()> {
        let mut event = libc::epoll_event {
            events: BASE_EVENTS | if writable { libc::EPOLLOUT as u32 } else { 0 },
            u64: token,
        };
        if unsafe { libc::epoll_ctl(self.fd, operation, fd, &mut event) } != 0 {
            return Err(io_error(
                "configure holder epoll fd",
                io::Error::last_os_error(),
            ));
        }
        Ok(())
    }
}

impl Drop for Reactor {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd) };
    }
}

fn io_error(operation: &'static str, source: io::Error) -> PersistError {
    PersistError::Io { operation, source }
}
