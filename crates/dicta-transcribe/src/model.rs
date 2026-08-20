use std::path::{Path, PathBuf};

pub const COMPACT_FILENAME: &str = "ggml-base-q5_1.bin";
pub const LARGE_V3_TURBO_FILENAME: &str = "ggml-large-v3-turbo-q5_0.bin";
pub const LARGE_V3_TURBO_LEGACY_FILENAME: &str = "ggml-large-v3-turbo.bin";
pub const LARGE_V3_TURBO_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin";
pub const LARGE_V3_TURBO_SHA1: &str = "e050f7970618a659205450ad97eb95a18d69c9ee";
/// Existing UI download estimate; use the SHA-1 digest for exact integrity.
pub const LARGE_V3_TURBO_DOWNLOAD_BYTES: u64 = 547 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ModelKind {
    Compact,
    LargeV3Turbo,
}

impl ModelKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact · base",
            Self::LargeV3Turbo => "High quality · large-v3-turbo",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum ModelSelection {
    #[default]
    Auto,
    Kind(ModelKind),
    Path(PathBuf),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntegrityAlgorithm {
    Sha1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegritySpec {
    pub algorithm: IntegrityAlgorithm,
    pub digest: String,
    pub minimum_size_bytes: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedModel {
    pub kind: ModelKind,
    pub path: PathBuf,
}

impl PreparedModel {
    #[must_use]
    pub fn new(kind: ModelKind, path: PathBuf) -> Self {
        Self { kind, path }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelPreparationStage {
    Locating,
    Downloading,
    Verifying,
    Ready,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelPreparation {
    pub stage: ModelPreparationStage,
    pub completed_bytes: u64,
    pub total_bytes: Option<u64>,
    pub message: String,
}

impl ModelPreparation {
    #[must_use]
    pub fn new(
        stage: ModelPreparationStage,
        completed_bytes: u64,
        total_bytes: Option<u64>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            stage,
            completed_bytes,
            total_bytes,
            message: message.into(),
        }
    }
}

/// Path and download metadata compatible with Dicta 0.8's model layout.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelCatalog {
    models_dir: PathBuf,
    bundled_compact: Option<PathBuf>,
    override_path: Option<PathBuf>,
}

impl ModelCatalog {
    #[must_use]
    pub fn new(models_dir: PathBuf) -> Self {
        Self {
            models_dir,
            bundled_compact: None,
            override_path: None,
        }
    }

    #[must_use]
    pub fn with_bundled_compact(mut self, path: PathBuf) -> Self {
        self.bundled_compact = Some(path);
        self
    }

    /// Sets the value supplied by the legacy `DICTA_WHISPER_MODEL` setting.
    #[must_use]
    pub fn with_override_path(mut self, path: PathBuf) -> Self {
        self.override_path = Some(path);
        self
    }

    #[must_use]
    pub fn models_dir(&self) -> &Path {
        &self.models_dir
    }

    #[must_use]
    pub fn managed_path(&self, kind: ModelKind) -> PathBuf {
        self.models_dir.join(match kind {
            ModelKind::Compact => COMPACT_FILENAME,
            ModelKind::LargeV3Turbo => LARGE_V3_TURBO_FILENAME,
        })
    }

    #[must_use]
    pub fn candidates(&self, selection: &ModelSelection) -> Vec<PreparedModel> {
        match selection {
            ModelSelection::Path(path) => vec![PreparedModel::new(
                infer_kind(path).unwrap_or(ModelKind::Compact),
                path.clone(),
            )],
            ModelSelection::Kind(ModelKind::Compact) => self.compact_candidates(),
            ModelSelection::Kind(ModelKind::LargeV3Turbo) => self.large_candidates(),
            ModelSelection::Auto => {
                let mut candidates = Vec::new();
                if let Some(path) = &self.override_path {
                    candidates.push(PreparedModel::new(
                        infer_kind(path).unwrap_or(ModelKind::Compact),
                        path.clone(),
                    ));
                }
                // Dicta 0.8 preferred a user-provided unquantized turbo model
                // before its managed q5 model. Preserve that migration order.
                candidates.push(PreparedModel::new(
                    ModelKind::LargeV3Turbo,
                    self.models_dir.join(LARGE_V3_TURBO_LEGACY_FILENAME),
                ));
                candidates.push(PreparedModel::new(
                    ModelKind::LargeV3Turbo,
                    self.managed_path(ModelKind::LargeV3Turbo),
                ));
                candidates.extend(self.compact_candidates());
                deduplicate(candidates)
            }
        }
    }

    #[must_use]
    pub fn integrity(&self, kind: ModelKind) -> Option<IntegritySpec> {
        match kind {
            ModelKind::Compact => None,
            ModelKind::LargeV3Turbo => Some(IntegritySpec {
                algorithm: IntegrityAlgorithm::Sha1,
                digest: LARGE_V3_TURBO_SHA1.to_owned(),
                minimum_size_bytes: Some(500 * 1024 * 1024),
            }),
        }
    }

    #[must_use]
    pub const fn download_url(&self, kind: ModelKind) -> Option<&'static str> {
        match kind {
            ModelKind::Compact => None,
            ModelKind::LargeV3Turbo => Some(LARGE_V3_TURBO_URL),
        }
    }

    fn compact_candidates(&self) -> Vec<PreparedModel> {
        let mut candidates = Vec::new();
        if let Some(path) = &self.bundled_compact {
            candidates.push(PreparedModel::new(ModelKind::Compact, path.clone()));
        }
        candidates.push(PreparedModel::new(
            ModelKind::Compact,
            self.managed_path(ModelKind::Compact),
        ));
        deduplicate(candidates)
    }

    fn large_candidates(&self) -> Vec<PreparedModel> {
        deduplicate(vec![
            PreparedModel::new(
                ModelKind::LargeV3Turbo,
                self.managed_path(ModelKind::LargeV3Turbo),
            ),
            PreparedModel::new(
                ModelKind::LargeV3Turbo,
                self.models_dir.join(LARGE_V3_TURBO_LEGACY_FILENAME),
            ),
        ])
    }
}

fn infer_kind(path: &Path) -> Option<ModelKind> {
    let filename = path.file_name()?.to_str()?;
    if filename.contains("large-v3-turbo") {
        Some(ModelKind::LargeV3Turbo)
    } else {
        Some(ModelKind::Compact)
    }
}

fn deduplicate(candidates: Vec<PreparedModel>) -> Vec<PreparedModel> {
    let mut unique = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if !unique
            .iter()
            .any(|current: &PreparedModel| current.path == candidate.path)
        {
            unique.push(candidate);
        }
    }
    unique
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_preserves_legacy_lookup_order() {
        let catalog = ModelCatalog::new(PathBuf::from("/data/models"))
            .with_bundled_compact(PathBuf::from("/app/resources/ggml-base-q5_1.bin"))
            .with_override_path(PathBuf::from("/custom/ggml-base.bin"));
        let paths = catalog
            .candidates(&ModelSelection::Auto)
            .into_iter()
            .map(|candidate| candidate.path)
            .collect::<Vec<_>>();
        assert_eq!(paths[0], PathBuf::from("/custom/ggml-base.bin"));
        assert_eq!(
            paths[1],
            PathBuf::from("/data/models/ggml-large-v3-turbo.bin")
        );
        assert_eq!(
            paths[2],
            PathBuf::from("/data/models/ggml-large-v3-turbo-q5_0.bin")
        );
        assert_eq!(paths[3], PathBuf::from("/app/resources/ggml-base-q5_1.bin"));
    }

    #[test]
    fn quality_metadata_matches_existing_install_contract() {
        let catalog = ModelCatalog::new(PathBuf::from("models"));
        let integrity = catalog.integrity(ModelKind::LargeV3Turbo).unwrap();
        assert_eq!(integrity.algorithm, IntegrityAlgorithm::Sha1);
        assert_eq!(integrity.digest, LARGE_V3_TURBO_SHA1);
        assert_eq!(
            catalog.download_url(ModelKind::LargeV3Turbo),
            Some(LARGE_V3_TURBO_URL)
        );
    }

    #[test]
    fn explicit_paths_infer_the_compatible_model_kind() {
        let catalog = ModelCatalog::new(PathBuf::from("models"));
        let candidate = catalog
            .candidates(&ModelSelection::Path(PathBuf::from(
                "/tmp/ggml-large-v3-turbo.bin",
            )))
            .pop()
            .unwrap();
        assert_eq!(candidate.kind, ModelKind::LargeV3Turbo);
    }
}
