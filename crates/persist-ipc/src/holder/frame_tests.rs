use super::*;

#[test]
fn accumulator_handles_fragmented_and_coalesced_frames() {
    let first = HolderFrame {
        message_type: HolderMessageType::Input,
        flags: 0,
        request_id: 1,
        generation: 3,
        payload: b"one".to_vec(),
    };
    let second = HolderFrame {
        message_type: HolderMessageType::Output,
        flags: 0,
        request_id: 2,
        generation: 4,
        payload: b"two".to_vec(),
    };
    let first_bytes = encode_frame(&first).unwrap();
    let second_bytes = encode_frame(&second).unwrap();
    let mut accumulator = HolderFrameAccumulator::new();
    accumulator.feed(&first_bytes[..7]).unwrap();
    assert_eq!(accumulator.try_read().unwrap(), None);
    let mut remainder = first_bytes[7..].to_vec();
    remainder.extend_from_slice(&second_bytes);
    accumulator.feed(&remainder).unwrap();
    assert_eq!(accumulator.try_read().unwrap(), Some(first));
    assert_eq!(accumulator.try_read().unwrap(), Some(second));
    assert_eq!(accumulator.try_read().unwrap(), None);
}

#[test]
fn accumulator_rejects_oversized_buffer_and_declared_payload() {
    let mut accumulator = HolderFrameAccumulator::new();
    assert_eq!(
        accumulator.feed(&vec![0; MAX_HOLDER_ACCUMULATOR + 1]),
        Err(HolderProtocolError::PayloadTooLarge)
    );

    let frame = HolderFrame {
        message_type: HolderMessageType::Inventory,
        flags: 0,
        request_id: 1,
        generation: 0,
        payload: Vec::new(),
    };
    let mut bytes = encode_frame(&frame).unwrap();
    bytes[8..12].copy_from_slice(&((MAX_HOLDER_CONTROL_FRAME + 1) as u32).to_be_bytes());
    let mut accumulator = HolderFrameAccumulator::new();
    accumulator.feed(&bytes).unwrap();
    assert_eq!(
        accumulator.try_read(),
        Err(HolderProtocolError::PayloadTooLarge)
    );
}
