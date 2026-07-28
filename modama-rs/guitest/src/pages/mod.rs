use crate::pages::{page_a::StateA, page_b::StateB, page_c::StateC};

pub mod page_a;
pub mod page_b;
pub mod page_c;

pub enum Page {
    A { inner: StateA },
    B { inner: StateB },
    C { inner: StateC },
}

