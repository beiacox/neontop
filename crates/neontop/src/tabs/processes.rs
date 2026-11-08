use gpui::{prelude::*, AnyElement, Context, Entity, Window};

use crate::app::Root;
use crate::ui::input::TextInput;
use crate::ui::kit;

pub struct ProcsView {
    pub search: Entity<TextInput>,
}

impl ProcsView {
    pub fn new(_window: &mut Window, cx: &mut Context<Root>) -> Self {
        ProcsView { search: cx.new(|cx| TextInput::new(cx, "Buscar proceso…")) }
    }
}

pub fn render(_root: &mut Root, _window: &mut Window, cx: &mut Context<Root>) -> AnyElement {
    kit::empty("procs", cx).into_any_element()
}
