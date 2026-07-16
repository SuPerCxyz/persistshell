use super::{CollectionStatus, Completeness};

pub(super) fn encode_completeness(value: Completeness) -> u8 {
    match value {
        Completeness::Complete => 0,
        Completeness::Partial => 1,
        Completeness::Stale => 2,
        Completeness::Unavailable => 3,
    }
}

pub(super) fn decode_completeness(value: u8) -> Option<Completeness> {
    match value {
        0 => Some(Completeness::Complete),
        1 => Some(Completeness::Partial),
        2 => Some(Completeness::Stale),
        3 => Some(Completeness::Unavailable),
        _ => None,
    }
}

pub(super) fn encode_collection_status(value: CollectionStatus) -> u8 {
    match value {
        CollectionStatus::Complete => 0,
        CollectionStatus::Partial => 1,
        CollectionStatus::Unavailable => 2,
    }
}

pub(super) fn decode_collection_status(value: u8) -> Option<CollectionStatus> {
    match value {
        0 => Some(CollectionStatus::Complete),
        1 => Some(CollectionStatus::Partial),
        2 => Some(CollectionStatus::Unavailable),
        _ => None,
    }
}

pub(super) fn put_u8(output: &mut Vec<u8>, value: u8) {
    output.push(value);
}

pub(super) fn put_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_be_bytes());
}

pub(super) fn put_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_be_bytes());
}

pub(super) fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_be_bytes());
}

pub(super) struct Reader<'a> {
    payload: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    pub(super) fn new(payload: &'a [u8]) -> Self {
        Self { payload, offset: 0 }
    }

    pub(super) fn u8(&mut self) -> Option<u8> {
        let value = *self.payload.get(self.offset)?;
        self.offset += 1;
        Some(value)
    }

    pub(super) fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.take::<2>()?))
    }

    pub(super) fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take::<4>()?))
    }

    pub(super) fn u64(&mut self) -> Option<u64> {
        Some(u64::from_be_bytes(self.take::<8>()?))
    }

    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let end = self.offset.checked_add(N)?;
        let value = self.payload.get(self.offset..end)?.try_into().ok()?;
        self.offset = end;
        Some(value)
    }

    pub(super) fn finish(&self) -> bool {
        self.offset == self.payload.len()
    }
}
