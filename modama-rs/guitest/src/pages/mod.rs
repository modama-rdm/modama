use eframe::egui;

use crate::{actions::Action, pages::{page_a::StateA, page_b::StateB, page_c::StateC}};

pub mod page_a;
pub mod page_b;
pub mod page_c;

pub enum Page {
    A { inner: StateA },
    B { inner: StateB },
    C { inner: StateC },
}

impl Page {
    pub fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) -> Action {
        match self {
            Page::A { inner } => inner.ui(ui, frame),
            Page::B { inner } => inner.ui(ui, frame),
            Page::C { inner } => inner.ui(ui, frame),
        }
    }

    pub fn poll(&mut self) -> Action {
        match self {
            Page::A { inner: _ } => Action::None,
            Page::B { inner } => inner.poll(),
            Page::C { inner: _ } => Action::None,
        }
    }
}

