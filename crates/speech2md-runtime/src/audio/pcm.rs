use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use memmap2::{Mmap, MmapOptions};
use tempfile::TempDir;

use super::resample::OUTPUT_SAMPLE_RATE;
use crate::RuntimeError;

/// Normalized, file-backed PCM samples.
///
/// The mapping is declared before its backing resources so it is dropped first.
pub struct DecodedPcm {
    mapping: Mmap,
    _file: File,
    _directory: TempDir,
}

impl DecodedPcm {
    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        OUTPUT_SAMPLE_RATE
    }

    #[must_use]
    pub const fn channels(&self) -> u16 {
        1
    }

    #[must_use]
    pub fn samples(&self) -> &[f32] {
        bytemuck::cast_slice(&self.mapping)
    }
}

pub(super) fn store_pcm(samples: &[f32], temp_root: &Path) -> Result<DecodedPcm, RuntimeError> {
    if samples.is_empty() {
        return Err(RuntimeError::InvalidPcm);
    }
    let directory = tempfile::Builder::new()
        .prefix("speech2md-pcm-")
        .tempdir_in(temp_root)?;
    let path = directory.path().join("audio.f32le");
    {
        let mut writer = BufWriter::new(File::create(&path)?);
        for sample in samples {
            writer.write_all(&sample.to_le_bytes())?;
        }
        writer.flush()?;
    }
    let file = File::open(path)?;
    let mapping = map_read_only(&file)?;
    bytemuck::try_cast_slice::<u8, f32>(&mapping).map_err(|_| RuntimeError::InvalidPcm)?;
    Ok(DecodedPcm {
        mapping,
        _file: file,
        _directory: directory,
    })
}

#[allow(
    unsafe_code,
    reason = "memmap2 requires unsafe because external file mutation can invalidate a mapping; the file is private in an owned temporary directory and is never exposed or mutated after mapping"
)]
fn map_read_only(file: &File) -> Result<Mmap, RuntimeError> {
    // SAFETY: `file` is a private file created in an owned temporary directory. Neither
    // the path nor a writable handle escapes, and `DecodedPcm` retains the file and
    // mapping until it removes the directory after the mapping is dropped.
    unsafe { MmapOptions::new().map(file).map_err(RuntimeError::Io) }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use tempfile::TempDir;

    use super::store_pcm;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn file_backed_pcm_preserves_every_sample_bit(
            samples in prop::collection::vec(any::<f32>().prop_filter("finite", |value| value.is_finite()), 1..2_048)
        ) {
            let root = TempDir::new().expect("temporary root");
            let stored = store_pcm(&samples, root.path()).expect("nonempty PCM can be stored");

            prop_assert_eq!(
                stored.samples().iter().map(|sample| sample.to_bits()).collect::<Vec<_>>(),
                samples.iter().map(|sample| sample.to_bits()).collect::<Vec<_>>()
            );
        }
    }
}
