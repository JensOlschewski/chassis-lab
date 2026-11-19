use std::collections::{BTreeMap, btree_map::Entry};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use clab_core::canonical_hdf5::{CanonicalFile, CanonicalHdf5Error};

pub(crate) mod display;
pub(crate) mod knc;
pub(crate) mod svc;

/// Canonical HDF5 files opened so far. Opening validates every signal, which
/// takes long on large files, so one run opens each file once and shares it.
#[derive(Default)]
pub(crate) struct OpenFiles(BTreeMap<PathBuf, Rc<CanonicalFile>>);

impl OpenFiles {
    pub(crate) fn open(&mut self, path: &Path) -> Result<Rc<CanonicalFile>, CanonicalHdf5Error> {
        match self.0.entry(path.to_path_buf()) {
            Entry::Occupied(entry) => Ok(entry.get().clone()),
            Entry::Vacant(entry) => Ok(entry.insert(Rc::new(CanonicalFile::open(path)?)).clone()),
        }
    }
}
