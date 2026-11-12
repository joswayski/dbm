use dbm_ui::{Rect, Ui};
use uuid::Uuid;

use crate::state::Workbench;

impl Workbench {
    pub(super) fn table_ui(&mut self, _ui: &mut Ui, _rect: Rect, _tab: u64, _profile: Uuid, _embedded: bool) {}
}
