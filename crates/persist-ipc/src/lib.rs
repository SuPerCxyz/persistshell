//! Unix socket protocol boundary for PersistShell.

pub mod dashboard;
pub mod holder;
pub mod protocol;
pub mod socket;

pub use dashboard::{
    decode_summary_request, decode_summary_response, decode_trend_request, decode_trend_response,
    encode_summary_request, encode_summary_response, encode_trend_request, encode_trend_response,
    CollectionStatus, Completeness, DaemonMetrics, DashboardSummaryRequest,
    DashboardSummaryResponse, DashboardTrendRequest, DashboardTrendResponse, SessionMetrics,
    TrendPoint, TrendRange, TrendScope, MAX_SUMMARY_PAGE, MAX_TREND_POINTS,
};
pub use protocol::{
    decode_attach, decode_attach_resp, decode_detach, decode_hello, decode_hello_ack,
    decode_list_sessions_resp, decode_lock, decode_new_session_resp, decode_note,
    decode_note_get_resp, decode_op_resp, decode_pin, decode_process_stats_resp,
    decode_process_tree_resp, decode_rename, decode_resize, decode_session_exited, decode_signal,
    decode_tag, decode_tag_list_resp, decode_writer_control, encode_attach, encode_attach_resp,
    encode_attach_with_context, encode_detach, encode_hello, encode_hello_ack,
    encode_list_sessions_resp, encode_lock, encode_new_session_resp, encode_note,
    encode_note_get_resp, encode_op_resp, encode_pin, encode_process_stats_resp,
    encode_process_tree_resp, encode_rename, encode_resize, encode_session_exited, encode_signal,
    encode_tag, encode_tag_list_resp, encode_writer_control, read_frame, write_frame,
    AttachPayload, AttachRespPayload, ConnectionEnvironment, DecodedAttachPayload, DetachPayload,
    Frame, FrameAccumulator, HelloAckPayload, HelloPayload, HelloStatus, ListSessionsRespPayload,
    LockPayload, MessageType, NewSessionRespPayload, NotePayload, OpRespPayload, PinPayload,
    ProcessStatsRespPayload, ProcessTreeNode, ProcessTreeRespPayload, RenamePayload, ResizePayload,
    SessionEntry, SessionExitedPayload, SignalPayload, TagListRespPayload, TagPayload,
    WriterControlPayload, ATTACH_CONTEXT_PROTOCOL_MINOR, HEADER_SIZE, MAX_CONTROL_FRAME,
    MAX_IO_FRAME,
};
pub use socket::{
    check_socket_path, cleanup_stale_socket, ClientSocket, DaemonConnection, DaemonSocket,
};
