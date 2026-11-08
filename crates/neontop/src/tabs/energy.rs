use gpui::{prelude::*, AnyElement, Context, Window};

use crate::app::Root;
use crate::ui::kit;

#[derive(Default)]
pub struct EnergyView {}

pub fn render(_root: &mut Root, _window: &mut Window, cx: &mut Context<Root>) -> AnyElement {
    kit::empty("energy", cx).into_any_element()
}
