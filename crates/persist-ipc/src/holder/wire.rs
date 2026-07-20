use super::HolderProtocolError;

pub(super) struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    pub(super) fn u8(&mut self) -> Result<u8, HolderProtocolError> {
        Ok(self.take(1)?[0])
    }

    pub(super) fn u16(&mut self) -> Result<u16, HolderProtocolError> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }

    pub(super) fn u32(&mut self) -> Result<u32, HolderProtocolError> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub(super) fn i32(&mut self) -> Result<i32, HolderProtocolError> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub(super) fn u64(&mut self) -> Result<u64, HolderProtocolError> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }

    pub(super) fn fixed<const N: usize>(&mut self) -> Result<[u8; N], HolderProtocolError> {
        Ok(self.take(N)?.try_into().unwrap())
    }

    pub(super) fn string(&mut self, max: usize) -> Result<String, HolderProtocolError> {
        let len = self.u16()? as usize;
        if len > max {
            return Err(HolderProtocolError::PayloadTooLarge);
        }
        let value =
            std::str::from_utf8(self.take(len)?).map_err(|_| HolderProtocolError::InvalidField)?;
        if value.as_bytes().contains(&0) {
            return Err(HolderProtocolError::InvalidField);
        }
        Ok(value.to_owned())
    }

    pub(super) fn optional_string(
        &mut self,
        max: usize,
    ) -> Result<Option<String>, HolderProtocolError> {
        match self.u8()? {
            0 => Ok(None),
            1 => self.string(max).map(Some),
            _ => Err(HolderProtocolError::InvalidField),
        }
    }

    pub(super) fn optional_bytes_u32(
        &mut self,
        max: usize,
    ) -> Result<Option<Vec<u8>>, HolderProtocolError> {
        match self.u8()? {
            0 => Ok(None),
            1 => {
                let len = self.u32()? as usize;
                if len > max {
                    return Err(HolderProtocolError::PayloadTooLarge);
                }
                Ok(Some(self.take(len)?.to_vec()))
            }
            _ => Err(HolderProtocolError::InvalidField),
        }
    }

    pub(super) fn finish(self) -> Result<(), HolderProtocolError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(HolderProtocolError::Trailing)
        }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], HolderProtocolError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or(HolderProtocolError::PayloadTooLarge)?;
        if end > self.bytes.len() {
            return Err(HolderProtocolError::Truncated);
        }
        let value = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(value)
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

pub(super) fn put_i32(output: &mut Vec<u8>, value: i32) {
    output.extend_from_slice(&value.to_be_bytes());
}

pub(super) fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_be_bytes());
}

pub(super) fn put_optional_bytes_u32(
    output: &mut Vec<u8>,
    value: Option<&[u8]>,
    max: usize,
) -> Result<(), HolderProtocolError> {
    match value {
        Some(value) if value.len() <= max => {
            put_u8(output, 1);
            put_u32(output, value.len() as u32);
            output.extend_from_slice(value);
            Ok(())
        }
        Some(_) => Err(HolderProtocolError::PayloadTooLarge),
        None => {
            put_u8(output, 0);
            Ok(())
        }
    }
}

pub(super) fn put_string(
    output: &mut Vec<u8>,
    value: &str,
    max: usize,
) -> Result<(), HolderProtocolError> {
    if value.is_empty() || value.len() > max || value.len() > u16::MAX as usize {
        return Err(if value.len() > max {
            HolderProtocolError::PayloadTooLarge
        } else {
            HolderProtocolError::InvalidField
        });
    }
    if value.as_bytes().contains(&0) {
        return Err(HolderProtocolError::InvalidField);
    }
    put_u16(output, value.len() as u16);
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

pub(super) fn put_string_allow_empty(
    output: &mut Vec<u8>,
    value: &str,
    max: usize,
) -> Result<(), HolderProtocolError> {
    if value.len() > max || value.len() > u16::MAX as usize {
        return Err(HolderProtocolError::PayloadTooLarge);
    }
    if value.as_bytes().contains(&0) {
        return Err(HolderProtocolError::InvalidField);
    }
    put_u16(output, value.len() as u16);
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

pub(super) fn put_optional_string(
    output: &mut Vec<u8>,
    value: Option<&str>,
    max: usize,
) -> Result<(), HolderProtocolError> {
    match value {
        None => put_u8(output, 0),
        Some(value) => {
            put_u8(output, 1);
            put_string(output, value, max)?;
        }
    }
    Ok(())
}
