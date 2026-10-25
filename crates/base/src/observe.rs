//! Registers native elements without accepting a second description of their state.
use gpui::{Element, Hitbox, InteractiveElement};

#[doc(hidden)]
#[cfg(feature = "test-support")]
pub type ObservedElement<E> = crate::test_support::Observed<E>;
#[doc(hidden)]
#[cfg(not(feature = "test-support"))]
pub type ObservedElement<E> = E;

pub trait TestSupportExt:
    Element<PrepaintState = Option<Hitbox>> + InteractiveElement + Sized
{
    fn test_support(self) -> ObservedElement<Self> {
        #[cfg(feature = "test-support")]
        {
            crate::test_support::Observed::new(self)
        }
        #[cfg(not(feature = "test-support"))]
        {
            self
        }
    }
}
impl<E: Element<PrepaintState = Option<Hitbox>> + InteractiveElement> TestSupportExt for E {}
