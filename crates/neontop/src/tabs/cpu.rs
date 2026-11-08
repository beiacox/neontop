use gpui::{prelude::*, AnyElement, Context, Window};

use crate::app::Root;
use crate::ui::kit;

pub fn render(_root: &mut Root, _window: &mut Window, cx: &mut Context<Root>) -> AnyElement {
    kit::empty("cpu", cx).into_any_element()
}
