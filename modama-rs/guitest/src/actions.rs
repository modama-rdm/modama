#[derive(PartialEq, Eq, Debug)]
pub enum Action {
    /// UI should stay at the current state and view/page
    None,

    /// Navigate to the "enter name" view
    GoToEnterName,

    /// `name` is verified, go to name confirmation page
    ConfirmName { name: String },
}

impl Action {
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}



