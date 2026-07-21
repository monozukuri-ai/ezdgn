use crate::DgnError;

/// A small bounded cursor used by binary decoders.
#[derive(Debug, Clone)]
pub(crate) struct ByteCursor<'a> {
    input: &'a [u8],
    position: usize,
}

impl<'a> ByteCursor<'a> {
    pub(crate) const fn new(input: &'a [u8]) -> Self {
        Self { input, position: 0 }
    }

    pub(crate) const fn position(&self) -> usize {
        self.position
    }

    pub(crate) const fn remaining(&self) -> usize {
        self.input.len() - self.position
    }

    pub(crate) fn peek_exact(
        &self,
        length: usize,
        context: &'static str,
    ) -> Result<&'a [u8], DgnError> {
        let remaining = self.remaining();
        let end = self
            .position
            .checked_add(length)
            .ok_or(DgnError::UnexpectedEof {
                offset: self.position,
                needed: length,
                remaining,
                context,
            })?;
        self.input
            .get(self.position..end)
            .ok_or(DgnError::UnexpectedEof {
                offset: self.position,
                needed: length,
                remaining,
                context,
            })
    }

    pub(crate) fn read_exact(
        &mut self,
        length: usize,
        context: &'static str,
    ) -> Result<&'a [u8], DgnError> {
        let bytes = self.peek_exact(length, context)?;
        self.position += length;
        Ok(bytes)
    }

    pub(crate) fn skip(&mut self, length: usize, context: &'static str) -> Result<(), DgnError> {
        self.read_exact(length, context).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_without_crossing_bounds() {
        let mut cursor = ByteCursor::new(&[1, 2, 3, 4]);
        assert_eq!(cursor.read_exact(2, "test").unwrap(), &[1, 2]);
        assert_eq!(cursor.position(), 2);
        assert_eq!(cursor.peek_exact(2, "test").unwrap(), &[3, 4]);
        assert_eq!(cursor.position(), 2);
        cursor.skip(2, "test").unwrap();
        assert_eq!(cursor.remaining(), 0);
    }

    #[test]
    fn reports_the_failed_read_boundary() {
        let mut cursor = ByteCursor::new(&[1, 2, 3]);
        cursor.skip(2, "prefix").unwrap();
        assert!(matches!(
            cursor.read_exact(2, "value"),
            Err(DgnError::UnexpectedEof {
                offset: 2,
                needed: 2,
                remaining: 1,
                context: "value"
            })
        ));
    }
}
