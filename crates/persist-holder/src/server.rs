use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::process::ExitCode;

use persist_core::{load_config, ConfigLoadOptions, PersistError, Result};
use persist_ipc::holder::*;

use crate::connection::{Connection, ConnectionRole};
use crate::lifecycle::{prepare_runtime_dir, PidLock, SignalFd};
use crate::log_worker::LogWorker;
use crate::reactor::Reactor;
use crate::runtime::{OutputBatch, Runtime};
use crate::socket::HolderSocket;

const MAX_EVENTS: usize = 128;

pub fn run<I>(args: I) -> ExitCode
where
    I: IntoIterator<Item = String>,
{
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    ExitCode::from(run_with_io(args, &mut stdout, &mut stderr))
}

fn run_with_io<I, W, E>(args: I, stdout: &mut W, stderr: &mut E) -> u8
where
    I: IntoIterator<Item = String>,
    W: Write,
    E: Write,
{
    let command = args.into_iter().nth(1);
    let result = match command.as_deref() {
        Some("foreground") => run_foreground(),
        Some("help" | "-h" | "--help") | None => writeln!(
            stdout,
            "persist-holder is an internal PersistShell component"
        )
        .map_err(|source| PersistError::Io {
            operation: "write holder help",
            source,
        }),
        Some(other) => Err(PersistError::invalid_argument(format!(
            "unknown persist-holder command: {other}"
        ))),
    };
    match result {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(stderr, "persist-holder: {error}");
            2
        }
    }
}

fn run_foreground() -> Result<()> {
    let config = load_config(&ConfigLoadOptions::from_environment()?)?;
    prepare_runtime_dir(&config.paths.runtime_dir)?;
    let _pid_lock = PidLock::acquire(config.paths.runtime_dir.join("holder.pid"))?;
    let socket = HolderSocket::bind(config.paths.holder_socket_path)?;
    let signals = SignalFd::create()?;
    HolderServer::new(
        socket,
        signals,
        random_instance_id()?,
        config.paths.runtime_dir.join("session-state"),
        config.logging.max_file_size.bytes(),
        config.logging.max_files,
    )?
    .run()
}

#[derive(Clone, Copy)]
struct Controller {
    fd: RawFd,
    pid: u32,
    nonce: [u8; 16],
    protocol_minor: u16,
    capabilities: u64,
}

struct HolderServer {
    socket: HolderSocket,
    signals: SignalFd,
    logs: LogWorker,
    reactor: Reactor,
    runtime: Runtime,
    connections: HashMap<RawFd, Connection>,
    writers: HashMap<u32, RawFd>,
    controller: Option<Controller>,
    instance_id: [u8; 16],
    shutdown_fd: Option<RawFd>,
}

impl HolderServer {
    fn new(
        socket: HolderSocket,
        signals: SignalFd,
        instance_id: [u8; 16],
        state_dir: std::path::PathBuf,
        log_max_bytes: u64,
        log_max_files: u32,
    ) -> Result<Self> {
        let logs = LogWorker::start(log_max_bytes, log_max_files)?;
        let reactor = Reactor::new()?;
        reactor.add(socket.as_raw_fd(), false)?;
        reactor.add(signals.as_raw_fd(), false)?;
        reactor.add(logs.as_raw_fd(), false)?;
        Ok(Self {
            socket,
            signals,
            logs,
            reactor,
            runtime: Runtime::new(state_dir),
            connections: HashMap::new(),
            writers: HashMap::new(),
            controller: None,
            instance_id,
            shutdown_fd: None,
        })
    }

    fn run(mut self) -> Result<()> {
        let mut events = vec![libc::epoll_event { events: 0, u64: 0 }; MAX_EVENTS];
        loop {
            let ready = self.reactor.wait(&mut events)?;
            let listener_fd = self.socket.as_raw_fd();
            let accept_ready = events
                .iter()
                .take(ready)
                .filter_map(|event| self.reactor.fd_for_token(event.u64))
                .any(|fd| fd == listener_fd);
            for event in events.iter().take(ready) {
                let Some(fd) = self.reactor.fd_for_token(event.u64) else {
                    continue;
                };
                if fd == listener_fd {
                    continue;
                } else if fd == self.signals.as_raw_fd() {
                    if self.handle_signal()? {
                        return Ok(());
                    }
                } else if fd == self.logs.as_raw_fd() {
                    self.handle_log_failures();
                } else if self.runtime.session_for_pty(fd).is_some() {
                    self.handle_pty(fd, event.events)?;
                } else if self.connections.contains_key(&fd) {
                    self.handle_connection(fd, event.events)?;
                }
            }
            if accept_ready {
                self.accept_connections()?;
            }
            if self.shutdown_fd.is_some_and(|fd| {
                !self
                    .connections
                    .get(&fd)
                    .is_some_and(Connection::wants_write)
            }) {
                return Ok(());
            }
        }
    }

    fn accept_connections(&mut self) -> Result<()> {
        while let Some(peer) = self.socket.accept()? {
            let connection = Connection::new(peer)?;
            let fd = connection.fd();
            self.reactor.add(fd, false)?;
            self.connections.insert(fd, connection);
        }
        Ok(())
    }

    fn handle_signal(&mut self) -> Result<bool> {
        match self.signals.consume()? {
            libc::SIGINT | libc::SIGTERM => Ok(true),
            libc::SIGCHLD => {
                for fd in self.runtime.pty_fds() {
                    let batch = self.runtime.drain_output(fd, &self.logs)?;
                    self.publish_output(batch)?;
                }
                for exited in self.runtime.reap_exited()? {
                    self.reactor.remove(exited.pty_fd);
                    self.release_writer(exited.session_id);
                    let legacy_payload = encode_session_exited_event(&SessionExitedEvent {
                        session_id: exited.session_id,
                        exit_code: exited.exit_code,
                        cwd: exited.cwd.clone(),
                    })
                    .map_err(|error| protocol_error("SessionExited", error))?;
                    let v2_payload = encode_session_exited_event_v2(&SessionExitedEventV2 {
                        session_id: exited.session_id,
                        exit_code: exited.exit_code,
                        cwd: exited.cwd.clone(),
                        environment: exited.environment.clone(),
                    })
                    .map_err(|error| protocol_error("SessionExited v2", error))?;
                    let targets = self
                        .connections
                        .iter()
                        .filter_map(|(fd, connection)| {
                            (connection.attached_session == Some(exited.session_id)).then_some(*fd)
                        })
                        .collect::<Vec<_>>();
                    for target in targets {
                        let payload = if self.connections[&target].protocol_minor()
                            == HOLDER_PROTOCOL_MINOR
                        {
                            v2_payload.clone()
                        } else {
                            legacy_payload.clone()
                        };
                        self.queue_or_disconnect(
                            target,
                            HolderFrame {
                                message_type: HolderMessageType::SessionExited,
                                flags: 0,
                                request_id: 0,
                                generation: self.runtime.generation(),
                                payload,
                            },
                        );
                    }
                    if exited.closing {
                        self.detach_session(exited.session_id);
                    }
                    if let Some(controller) = self.controller {
                        let payload = if controller.protocol_minor == HOLDER_PROTOCOL_MINOR {
                            v2_payload
                        } else {
                            legacy_payload
                        };
                        self.queue_or_disconnect(
                            controller.fd,
                            HolderFrame {
                                message_type: HolderMessageType::SessionExited,
                                flags: 0,
                                request_id: 0,
                                generation: self.runtime.generation(),
                                payload,
                            },
                        );
                    }
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    fn handle_log_failures(&mut self) {
        for (session_id, dropped_bytes) in self.logs.take_failures() {
            self.runtime.mark_log_failed(session_id);
            self.send_control(HolderFrame {
                message_type: HolderMessageType::LogDegraded,
                flags: 0,
                request_id: 0,
                generation: self.runtime.generation(),
                payload: encode_log_degraded(&LogDegradedEvent {
                    session_id,
                    dropped_bytes,
                }),
            });
        }
    }

    fn handle_pty(&mut self, fd: RawFd, events: u32) -> Result<()> {
        if events & (libc::EPOLLIN | libc::EPOLLHUP | libc::EPOLLERR) as u32 != 0 {
            let batch = self.runtime.drain_output(fd, &self.logs)?;
            self.publish_output(batch)?;
        }
        if events & libc::EPOLLOUT as u32 != 0 {
            let session_id = self.runtime.session_for_pty(fd).unwrap();
            let pending = self.runtime.flush_input(session_id)?;
            self.reactor.modify(fd, pending)?;
        }
        Ok(())
    }

    fn publish_output(&mut self, batch: OutputBatch) -> Result<()> {
        let targets = self
            .connections
            .iter()
            .filter_map(|(fd, connection)| {
                (connection.attached_session == Some(batch.session_id)).then_some(*fd)
            })
            .collect::<Vec<_>>();
        for chunk in batch.chunks {
            let frame = HolderFrame {
                message_type: HolderMessageType::Output,
                flags: 0,
                request_id: 0,
                generation: self.runtime.generation(),
                payload: chunk,
            };
            for fd in &targets {
                self.queue_or_disconnect(*fd, frame.clone());
            }
        }
        if batch.log_dropped > 0 {
            self.send_control(HolderFrame {
                message_type: HolderMessageType::LogDegraded,
                flags: 0,
                request_id: 0,
                generation: self.runtime.generation(),
                payload: encode_log_degraded(&LogDegradedEvent {
                    session_id: batch.session_id,
                    dropped_bytes: batch.log_dropped,
                }),
            });
        }
        Ok(())
    }

    fn handle_connection(&mut self, fd: RawFd, events: u32) -> Result<()> {
        let mut closed = events & (libc::EPOLLERR | libc::EPOLLHUP | libc::EPOLLRDHUP) as u32 != 0;
        let frames = if events & libc::EPOLLIN as u32 != 0 {
            match self.connections.get_mut(&fd).unwrap().read_frames() {
                Ok((frames, peer_closed)) => {
                    closed |= peer_closed;
                    frames
                }
                Err(_) => {
                    self.disconnect(fd);
                    return Ok(());
                }
            }
        } else {
            Vec::new()
        };
        for (minor, frame) in frames {
            if self.process_frame(fd, minor, frame).is_err() {
                self.disconnect(fd);
                return Ok(());
            }
        }
        if closed {
            self.disconnect(fd);
            return Ok(());
        }
        if events & libc::EPOLLOUT as u32 != 0 && !self.connections.get_mut(&fd).unwrap().flush()? {
            self.disconnect(fd);
            return Ok(());
        }
        if let Some(connection) = self.connections.get(&fd) {
            self.reactor.modify(fd, connection.wants_write())?;
        }
        Ok(())
    }

    fn process_frame(&mut self, fd: RawFd, minor: u16, frame: HolderFrame) -> Result<()> {
        match self.connections[&fd].role {
            ConnectionRole::Pending => self.handle_hello(fd, minor, frame),
            ConnectionRole::Control => self.handle_control(fd, minor, frame),
            ConnectionRole::Data => {
                if minor != self.connections[&fd].protocol_minor() {
                    return Err(PersistError::invalid_argument(
                        "holder data protocol minor changed",
                    ));
                }
                self.handle_data(fd, frame)
            }
        }
    }
}

include!("server_handlers.rs");

fn random_instance_id() -> Result<[u8; 16]> {
    let mut file = File::open("/dev/urandom").map_err(|source| PersistError::Io {
        operation: "open urandom for holder instance id",
        source,
    })?;
    let mut value = [0; 16];
    file.read_exact(&mut value)
        .map_err(|source| PersistError::Io {
            operation: "read holder instance id",
            source,
        })?;
    if value == [0; 16] {
        return Err(PersistError::invalid_argument("zero holder instance id"));
    }
    Ok(value)
}
