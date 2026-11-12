//! Bundled editor icons shared by GPUI Kit presentation and behavior layers.
//!
//! [`IconName`] and [`IconNamed`] do not depend on GPUI Component. Register
//! [`Assets`] for the default component bundle, or [`AllAssets`] for the full
//! catalog. Applications can provide their own source for additional icons.

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
compile_error!("The JSON5 editor crates require wasm32-unknown-unknown");

mod icon;
pub use icon::{IconName, IconNamed};

mod wasm_assets;
pub use wasm_assets::Assets;
pub use wasm_assets::Assets as AllAssets;

// Public only so exported macros can resolve dependencies in downstream crates,
// including applications that depend solely on the gpui-kit umbrella crate.
#[doc(hidden)]
pub mod __private {
    pub use crate::icon::embedded::*;
    pub use gpui::{AssetSource, Result, SharedString};
}

#[macro_export]
macro_rules! icon_assets {
    ($vis:vis $name:ident, [$($icon:ident),* $(,)?]) => {
        #[derive(Clone, Copy, Debug, Default)]
        $vis struct $name;

        impl $crate::__private::AssetSource for $name {
            fn load(&self, path: &str) -> $crate::__private::Result<Option<::std::borrow::Cow<'static, [u8]>>> {
                $(if path == $crate::__private::$icon.0 {
                    return Ok(Some(::std::borrow::Cow::Borrowed($crate::__private::$icon.1)));
                })*
                let _ = path;
                Ok(None)
            }

            fn list(&self, path: &str) -> $crate::__private::Result<::std::vec::Vec<$crate::__private::SharedString>> {
                let paths: &[&str] = &[$($crate::__private::$icon.0),*];
                Ok(paths.iter().copied().filter(|name| name.starts_with(path)).map(Into::into).collect())
            }
        }
    };
}
