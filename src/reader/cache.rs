use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use byte_slice_cast::{FromByteVec, IntoVecOf};

use crate::idx::{Chunk, Dataset};

pub struct DatasetReader<'a> {
    ds: &'a Dataset,
    fd: File,
    cache: HashMap<Chunk, Vec<u8>>,
}

impl<'a> DatasetReader<'a> {
    pub fn with_dataset<P>(ds: &'a Dataset, p: P) -> Result<DatasetReader, anyhow::Error>
    where
        P: AsRef<Path>,
    {
        let fd = File::open(p)?;

        Ok(DatasetReader {
            ds,
            fd,
            cache: HashMap::with_capacity(ds.chunks.len()),
        })
    }

    pub fn read(
        &mut self,
        indices: Option<&[u64]>,
        counts: Option<&[u64]>,
    ) -> Result<Vec<u8>, anyhow::Error> {
        let counts: &[u64] = counts.unwrap_or(self.ds.shape.as_slice());

        let dsz = self.ds.dtype.size() as u64;
        let vsz = counts.iter().product::<u64>() * dsz;
        let mut buf = Vec::with_capacity(vsz as usize);
        unsafe {
            buf.set_len(vsz as usize);
        }
        let mut buf_slice = &mut buf[..];

        for (c, start, end) in self.ds.chunk_slices(indices, Some(&counts)) {
            let start = (start * dsz) as usize;
            let end = (end * dsz) as usize;
            let slice_sz = end - start;

            if let Some(cache) = self.cache.get(c) {
                // println!("cache: {} start: {} -> end: {}", c.addr, start, end);
                buf_slice[..slice_sz].copy_from_slice(&cache[start..end]);
            } else {
                // println!("read: {} start: {} -> end: {} (sz: {})", c.addr, start, end, c.size);
                let mut cache: Vec<u8> = Vec::with_capacity((c.size * dsz) as usize);
                unsafe {
                    cache.set_len((c.size * dsz) as usize);
                }

                let mut cache = if self.ds.shuffle {
                    filters::shuffle::unshuffle_sized(&cache, dsz as usize)
                } else {
                    cache
                };

                self.fd.seek(SeekFrom::Start(c.addr))?;
                self.fd.read_exact(&mut cache); // TODO: handle error

                buf_slice[..slice_sz].copy_from_slice(&cache[start..end]);
                self.cache.insert(c.clone(), cache);
            }

            buf_slice = &mut buf_slice[slice_sz..];
        }

        Ok(buf)
    }

    pub fn values<T>(
        &mut self,
        indices: Option<&[u64]>,
        counts: Option<&[u64]>,
    ) -> Result<Vec<T>, anyhow::Error>
    where
        T: FromByteVec,
    {
        // TODO: BE, LE conversion
        // TODO: use as_slice_of() to avoid copy, or possible values_to(&mut buf) so that
        //       caller keeps ownership of slice too.
        Ok(self.read(indices, counts)?.into_vec_of::<T>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::idx::Index;

    #[test]
    fn read_t_float32() {
        let i = Index::index("tests/data/t_float.h5").unwrap();
        let mut r = DatasetReader::with_dataset(i.dataset("d32_1").unwrap(), i.path()).unwrap();

        let vs = r.values::<f32>(None, None).unwrap();

        let h = hdf5::File::open(i.path()).unwrap();
        let hvs = h.dataset("d32_1").unwrap().read_raw::<f32>().unwrap();

        assert_eq!(vs, hvs);
    }

    #[test]
    fn read_chunked_1d() {
        let i = Index::index("tests/data/chunked_oneD.h5").unwrap();
        let mut r =
            DatasetReader::with_dataset(i.dataset("d_4_chunks").unwrap(), i.path()).unwrap();

        let vs = r.values::<f32>(None, None).unwrap();

        let h = hdf5::File::open(i.path()).unwrap();
        let hvs = h.dataset("d_4_chunks").unwrap().read_raw::<f32>().unwrap();

        assert_eq!(vs, hvs);
    }

    #[test]
    fn read_chunked_2d() {
        let i = Index::index("tests/data/chunked_twoD.h5").unwrap();
        let mut r =
            DatasetReader::with_dataset(i.dataset("d_4_chunks").unwrap(), i.path()).unwrap();

        let vs = r.values::<f32>(None, None).unwrap();

        let h = hdf5::File::open(i.path()).unwrap();
        let hvs = h.dataset("d_4_chunks").unwrap().read_raw::<f32>().unwrap();

        assert_eq!(vs, hvs);
    }

    #[test]
    fn read_chunked_shuffled_2d() {
        let i = Index::index("tests/data/dmrpp/chunked/chunked_shuffled_twoD.h5").unwrap();
        let mut r =
            DatasetReader::with_dataset(i.dataset("d_4_shuffled_chunks").unwrap(), i.path()).unwrap();

        let mut vs = r.values::<f32>(None, None).unwrap();
        use byteorder::{BigEndian, LittleEndian, ByteOrder};
        BigEndian::from_slice_f32(&mut vs);

        let h = hdf5::File::open(i.path()).unwrap();
        let mut hvs = h.dataset("d_4_shuffled_chunks").unwrap().read_raw::<f32>().unwrap();

        assert_eq!(vs, hvs);
    }
}
