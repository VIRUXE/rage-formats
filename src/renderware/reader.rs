use crate::{Mat4, Vec3};
use anyhow::{ensure, Context, Result};

pub(crate) struct Reader<'a> {
    pub data: &'a [u8],
    pub at: usize,
}
impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }
    pub fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(count)
            .context("binary length overflow")?;
        let bytes = self.data.get(self.at..end).with_context(|| {
            format!(
                "truncated binary record at offset {} (needs {count} bytes)",
                self.at
            )
        })?;
        self.at = end;
        Ok(bytes)
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn f32(&mut self) -> Result<f32> {
        let value = f32::from_le_bytes(self.take(4)?.try_into().unwrap());
        ensure!(
            value.is_finite(),
            "non-finite float at offset {}",
            self.at - 4
        );
        Ok(value)
    }
    pub fn raw_f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn vec3(&mut self) -> Result<Vec3> {
        Ok(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }
    pub fn rw_matrix(&mut self) -> Result<(Mat4, [u32; 4])> {
        let mut matrix = [0.; 16];
        let mut padding = [0; 4];
        matrix[15] = 1.;
        for group in 0..4 {
            for axis in 0..3 {
                matrix[group * 4 + axis] = self.f32()?;
            }
            padding[group] = self.u32()?;
        }
        Ok((Mat4(matrix), padding))
    }
    pub fn tail(&self) -> &'a [u8] {
        &self.data[self.at..]
    }
}
pub(crate) fn put_vec3(out: &mut Vec<u8>, v: Vec3) {
    for f in [v.x, v.y, v.z] {
        out.extend_from_slice(&f.to_le_bytes());
    }
}
pub(crate) fn put_f32(out: &mut Vec<u8>, f: f32) {
    out.extend_from_slice(&f.to_le_bytes());
}
pub(crate) fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
