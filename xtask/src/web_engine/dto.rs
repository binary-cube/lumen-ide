//! What the engine's packages say, as the app reads it: a package's description
//! (`<archive>.json`, beside the archive), the published list (`web-engine.json`) and its
//! entries, and the versions they're compared by. The app's own types, so the two can't
//! drift apart.

pub use web_engine_protocol::{
    PackageDescription as Description, PackageEntry as Package, PackageList as List,
};
pub use web_engine_versions::Version;
