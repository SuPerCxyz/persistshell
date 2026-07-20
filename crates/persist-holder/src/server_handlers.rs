impl HolderServer {
    fn handle_hello(&mut self, fd: RawFd, minor: u16, frame: HolderFrame) -> Result<()> {
        match frame.message_type {
            HolderMessageType::ControlHello => self.handle_control_hello(fd, minor, frame),
            HolderMessageType::DataHello => self.handle_data_hello(fd, minor, frame),
            _ => Err(PersistError::invalid_argument("expected holder hello")),
        }
    }

    fn handle_control_hello(
        &mut self,
        fd: RawFd,
        minor: u16,
        frame: HolderFrame,
    ) -> Result<()> {
        if minor != HOLDER_PROTOCOL_BASELINE_MINOR {
            return Err(PersistError::invalid_argument(
                "ControlHello must use baseline minor",
            ));
        }
        if self.controller.is_some_and(|controller| {
            self.connections
                .get(&controller.fd)
                .is_some_and(|connection| connection.peer.closes_within(20))
        }) {
            self.disconnect(self.controller.unwrap().fd);
        }
        let hello = decode_control_hello(&frame.payload)
            .map_err(|error| protocol_error("ControlHello", error))?;
        let peer = &self.connections[&fd].peer;
        let status = if hello.uid != peer.uid || hello.daemon_pid != peer.pid {
            HelloStatus::PermissionDenied
        } else if self.controller.is_some() {
            HelloStatus::Busy
        } else {
            HelloStatus::Accepted
        };
        self.queue_frame(
            fd,
            HolderFrame {
                message_type: HolderMessageType::ControlHelloAck,
                flags: 0,
                request_id: frame.request_id,
                generation: self.runtime.generation(),
                payload: encode_control_hello_ack(&ControlHelloAck {
                    holder_pid: std::process::id(),
                    instance_id: self.instance_id,
                    nonce: hello.nonce,
                    status,
                }),
            },
        )?;
        if status == HelloStatus::Accepted {
            self.connections.get_mut(&fd).unwrap().role = ConnectionRole::Control;
            self.controller = Some(Controller {
                fd,
                pid: hello.daemon_pid,
                nonce: hello.nonce,
                protocol_minor: HOLDER_PROTOCOL_BASELINE_MINOR,
                capabilities: 0,
            });
        } else {
            self.connections.get_mut(&fd).unwrap().close_after_flush();
        }
        Ok(())
    }

    fn handle_data_hello(&mut self, fd: RawFd, minor: u16, frame: HolderFrame) -> Result<()> {
        let hello = decode_data_hello(&frame.payload)
            .map_err(|error| protocol_error("DataHello", error))?;
        let peer_pid = self.connections[&fd].peer.pid;
        let accepted = self.controller.is_some_and(|controller| {
            hello.daemon_pid == peer_pid
                && hello.daemon_pid == controller.pid
                && hello.instance_id == self.instance_id
                && hello.nonce == controller.nonce
                && minor == controller.protocol_minor
        });
        let status = if accepted {
            HelloStatus::Accepted
        } else {
            HelloStatus::PermissionDenied
        };
        if accepted {
            self.connections
                .get_mut(&fd)
                .unwrap()
                .set_protocol_minor(minor);
        }
        self.queue_frame(
            fd,
            HolderFrame {
                message_type: HolderMessageType::DataHelloAck,
                flags: 0,
                request_id: frame.request_id,
                generation: self.runtime.generation(),
                payload: encode_data_hello_ack(&DataHelloAck {
                    instance_id: self.instance_id,
                    nonce: hello.nonce,
                    status,
                }),
            },
        )?;
        if accepted {
            self.connections.get_mut(&fd).unwrap().role = ConnectionRole::Data;
        } else {
            self.connections.get_mut(&fd).unwrap().close_after_flush();
        }
        Ok(())
    }

    fn handle_control(&mut self, fd: RawFd, minor: u16, frame: HolderFrame) -> Result<()> {
        if frame.message_type == HolderMessageType::Capability {
            return self.handle_capability(fd, minor, frame);
        }
        if minor != self.connections[&fd].protocol_minor() {
            return Err(PersistError::invalid_argument(
                "holder control protocol minor changed",
            ));
        }
        match frame.message_type {
            HolderMessageType::Inventory => {
                let request = decode_inventory_request(&frame.payload)
                    .map_err(|error| protocol_error("Inventory", error))?;
                let mut inventory = self.runtime.inventory(request);
                for entry in &mut inventory.entries {
                    entry.writer_active = self.writers.contains_key(&entry.session_id);
                }
                let payload = encode_inventory_response(&inventory)
                    .map_err(|error| protocol_error("InventoryResp", error))?;
                self.respond(fd, &frame, HolderMessageType::InventoryResp, payload)
            }
            HolderMessageType::Create => {
                let request = if minor == HOLDER_PROTOCOL_MINOR {
                    decode_create_request_v2(&frame.payload)
                } else {
                    decode_create_request(&frame.payload)
                }
                    .map_err(|error| protocol_error("Create", error))?;
                let (response, pty_fd) = self.runtime.create(request)?;
                if let Some(pty_fd) = pty_fd {
                    self.reactor.add(pty_fd, false)?;
                }
                self.operation_response(fd, &frame, HolderMessageType::CreateResp, response)
            }
            HolderMessageType::Close | HolderMessageType::Kill => {
                let request = decode_operation_request(&frame.payload)
                    .map_err(|error| protocol_error("operation", error))?;
                let (kind, response) = if frame.message_type == HolderMessageType::Close {
                    (
                        HolderMessageType::CloseResp,
                        self.runtime.close(request.session_id),
                    )
                } else {
                    (
                        HolderMessageType::KillResp,
                        self.runtime.kill(request.session_id),
                    )
                };
                self.operation_response(fd, &frame, kind, response)
            }
            HolderMessageType::GetExitContext => {
                let request = decode_operation_request(&frame.payload)
                    .map_err(|error| protocol_error("GetExitContext", error))?;
                let payload = if minor == HOLDER_PROTOCOL_MINOR {
                    encode_exit_context_response_v2(
                        &self.runtime.exit_context_response_v2(request.session_id),
                    )
                } else {
                    encode_exit_context_response(
                        &self.runtime.exit_context_response(request.session_id),
                    )
                }
                .map_err(|error| protocol_error("GetExitContextResp", error))?;
                self.respond(
                    fd,
                    &frame,
                    HolderMessageType::GetExitContextResp,
                    payload,
                )
            }
            HolderMessageType::RetireExited => {
                let request = decode_operation_request(&frame.payload)
                    .map_err(|error| protocol_error("RetireExited", error))?;
                let response = self.runtime.retire_exited(request.session_id);
                self.operation_response(
                    fd,
                    &frame,
                    HolderMessageType::RetireExitedResp,
                    response,
                )
            }
            HolderMessageType::ShutdownAll if frame.payload.is_empty() => {
                self.respond(
                    fd,
                    &frame,
                    HolderMessageType::ShutdownAllResp,
                    Vec::new(),
                )?;
                self.shutdown_fd = Some(fd);
                Ok(())
            }
            _ => Err(PersistError::invalid_argument("invalid holder control request")),
        }
    }

    fn handle_capability(&mut self, fd: RawFd, minor: u16, frame: HolderFrame) -> Result<()> {
        if minor != HOLDER_PROTOCOL_MINOR
            || self.connections[&fd].protocol_minor() != HOLDER_PROTOCOL_BASELINE_MINOR
        {
            return Err(PersistError::invalid_argument(
                "invalid holder capability negotiation state",
            ));
        }
        let request = decode_capability_request(&frame.payload)
            .map_err(|error| protocol_error("Capability", error))?;
        if request.max_minor != HOLDER_PROTOCOL_MINOR {
            return Err(PersistError::invalid_argument(
                "unsupported holder capability minor",
            ));
        }
        let (selected_minor, capabilities) = {
            let controller = self
                .controller
                .as_mut()
                .filter(|controller| controller.fd == fd)
                .ok_or_else(|| PersistError::invalid_argument("missing holder controller"))?;
            if request.instance_id != self.instance_id || request.nonce != controller.nonce {
                return Err(PersistError::invalid_argument(
                    "holder capability identity mismatch",
                ));
            }
            controller.protocol_minor = HOLDER_PROTOCOL_MINOR;
            controller.capabilities = HOLDER_CAPABILITY_ENVIRONMENT_CONTEXT;
            (controller.protocol_minor, controller.capabilities)
        };
        self.connections
            .get_mut(&fd)
            .unwrap()
            .set_protocol_minor(selected_minor);
        self.queue_frame(
            fd,
            HolderFrame {
                message_type: HolderMessageType::CapabilityResp,
                flags: 0,
                request_id: frame.request_id,
                generation: self.runtime.generation(),
                payload: encode_capability_response(&CapabilityResponse {
                    instance_id: self.instance_id,
                    nonce: request.nonce,
                    selected_minor,
                    capabilities,
                }),
            },
        )
    }

    fn handle_data(&mut self, fd: RawFd, frame: HolderFrame) -> Result<()> {
        match frame.message_type {
            HolderMessageType::Attach => self.attach(fd, frame),
            HolderMessageType::Detach if frame.payload.is_empty() => {
                self.detach_connection(fd);
                Ok(())
            }
            HolderMessageType::Input => {
                let session_id = self.require_writer(fd)?;
                let pending = self.runtime.queue_input(session_id, frame.payload)?;
                if let Some(pty_fd) = self.runtime.pty_fd(session_id) {
                    self.reactor.modify(pty_fd, pending)?;
                }
                Ok(())
            }
            HolderMessageType::Resize => {
                let session_id = self.require_writer(fd)?;
                let request = decode_resize_request(&frame.payload)
                    .map_err(|error| protocol_error("Resize", error))?;
                self.runtime.resize(session_id, request)
            }
            HolderMessageType::Signal => {
                let session_id = self.require_writer(fd)?;
                let request = decode_signal_request(&frame.payload)
                    .map_err(|error| protocol_error("Signal", error))?;
                self.runtime.signal(session_id, request)
            }
            _ => Err(PersistError::invalid_argument("invalid holder data request")),
        }
    }

    fn attach(&mut self, fd: RawFd, frame: HolderFrame) -> Result<()> {
        let request = decode_attach_request(&frame.payload)
            .map_err(|error| protocol_error("Attach", error))?;
        if self.runtime.replay(request.session_id, 0).is_none() {
            return self.operation_response(
                fd,
                &frame,
                HolderMessageType::AttachResp,
                operation(request.session_id, OperationStatus::NotFound, "not found"),
            );
        }
        self.detach_connection(fd);
        if request.mode == HolderAttachMode::ReadWrite {
            if let Some(previous) = self.writers.insert(request.session_id, fd) {
                self.queue_or_disconnect(previous, writer_frame(
                    HolderMessageType::WriteRevoked,
                    request.session_id,
                    self.runtime.generation(),
                ));
            }
        }
        self.connections.get_mut(&fd).unwrap().attached_session = Some(request.session_id);
        self.operation_response(
            fd,
            &frame,
            HolderMessageType::AttachResp,
            operation(request.session_id, OperationStatus::Ok, ""),
        )?;
        for chunk in self
            .runtime
            .replay(request.session_id, request.replay_bytes as usize)
            .unwrap()
            .chunks(MAX_HOLDER_IO_FRAME)
        {
            self.queue_frame(
                fd,
                HolderFrame {
                    message_type: HolderMessageType::Output,
                    flags: 0,
                    request_id: 0,
                    generation: self.runtime.generation(),
                    payload: chunk.to_vec(),
                },
            )?;
        }
        if request.mode == HolderAttachMode::ReadWrite {
            self.queue_frame(
                fd,
                writer_frame(
                    HolderMessageType::WriteGranted,
                    request.session_id,
                    self.runtime.generation(),
                ),
            )?;
        }
        Ok(())
    }

    fn require_writer(&self, fd: RawFd) -> Result<u32> {
        let session_id = self.connections[&fd]
            .attached_session
            .ok_or_else(|| PersistError::invalid_argument("holder data client not attached"))?;
        if self.writers.get(&session_id) != Some(&fd) || !self.runtime.is_running(session_id) {
            return Err(PersistError::invalid_argument("holder data client is not writer"));
        }
        Ok(session_id)
    }

    fn operation_response(
        &mut self,
        fd: RawFd,
        request: &HolderFrame,
        kind: HolderMessageType,
        response: OperationResponse,
    ) -> Result<()> {
        let payload = encode_operation_response(&response)
            .map_err(|error| protocol_error("operation response", error))?;
        self.respond(fd, request, kind, payload)
    }

    fn respond(
        &mut self,
        fd: RawFd,
        request: &HolderFrame,
        kind: HolderMessageType,
        payload: Vec<u8>,
    ) -> Result<()> {
        self.queue_frame(
            fd,
            HolderFrame {
                message_type: kind,
                flags: 0,
                request_id: request.request_id,
                generation: self.runtime.generation(),
                payload,
            },
        )
    }

    fn send_control(&mut self, frame: HolderFrame) {
        if let Some(controller) = self.controller {
            self.queue_or_disconnect(controller.fd, frame);
        }
    }

    fn queue_frame(&mut self, fd: RawFd, frame: HolderFrame) -> Result<()> {
        self.connections
            .get_mut(&fd)
            .ok_or_else(|| PersistError::invalid_argument("holder connection disappeared"))?
            .queue(frame)?;
        self.reactor.modify(fd, true)
    }

    fn queue_or_disconnect(&mut self, fd: RawFd, frame: HolderFrame) {
        if self.queue_frame(fd, frame).is_err() {
            self.disconnect(fd);
        }
    }

    fn detach_connection(&mut self, fd: RawFd) {
        let session = self
            .connections
            .get_mut(&fd)
            .and_then(|connection| connection.attached_session.take());
        if session.is_some_and(|id| self.writers.get(&id) == Some(&fd)) {
            self.writers.remove(&session.unwrap());
        }
    }

    fn detach_session(&mut self, session_id: u32) {
        self.writers.remove(&session_id);
        for connection in self.connections.values_mut() {
            if connection.attached_session == Some(session_id) {
                connection.attached_session = None;
            }
        }
    }

    fn release_writer(&mut self, session_id: u32) {
        if let Some(fd) = self.writers.remove(&session_id) {
            self.queue_or_disconnect(
                fd,
                writer_frame(
                    HolderMessageType::WriteRevoked,
                    session_id,
                    self.runtime.generation(),
                ),
            );
        }
    }

    fn disconnect(&mut self, fd: RawFd) {
        self.reactor.remove(fd);
        self.detach_connection(fd);
        self.connections.remove(&fd);
        if self.controller.is_some_and(|controller| controller.fd == fd) {
            self.controller = None;
            let data_fds = self
                .connections
                .iter()
                .filter_map(|(fd, connection)| {
                    (connection.role == ConnectionRole::Data).then_some(*fd)
                })
                .collect::<Vec<_>>();
            for data_fd in data_fds {
                self.disconnect(data_fd);
            }
        }
    }
}

fn writer_frame(kind: HolderMessageType, session_id: u32, generation: u64) -> HolderFrame {
    HolderFrame {
        message_type: kind,
        flags: 0,
        request_id: 0,
        generation,
        payload: encode_operation_request(&OperationRequest { session_id }),
    }
}

fn operation(session_id: u32, status: OperationStatus, message: &str) -> OperationResponse {
    OperationResponse {
        session_id,
        status,
        message: message.into(),
    }
}

fn protocol_error(context: &str, error: HolderProtocolError) -> PersistError {
    PersistError::invalid_argument(format!("invalid holder {context}: {error:?}"))
}
