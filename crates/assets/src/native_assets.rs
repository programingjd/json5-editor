use anyhow::anyhow;
use gpui::{AssetSource, Result, SharedString};
use std::borrow::Cow;

include!(concat!(env!("OUT_DIR"), "/default_assets.rs"));

pub use Assets as AllAssets;

macro_rules! impl_asset_source {
    ($name:ident) => {
        impl $name {
            pub fn new(_endpoint: impl Into<SharedString>) -> Self {
                Self
            }
        }

        impl AssetSource for $name {
            fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
                if path.is_empty() {
                    return Ok(None);
                }
                Self::get(path)
                    .map(|file| Some(file.data))
                    .ok_or_else(|| anyhow!("could not find asset at path \"{}\"", path))
            }

            fn list(&self, path: &str) -> Result<Vec<SharedString>> {
                Ok(Self::iter()
                    .filter_map(|name| name.starts_with(path).then(|| name.into()))
                    .collect())
            }
        }
    };
}

impl_asset_source!(Assets);
