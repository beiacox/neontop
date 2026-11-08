use gpui::{div, prelude::*, Context, WeakEntity, Window};

use crate::app::Root;

pub struct SettingsView {
    pub root: WeakEntity<Root>,
}

impl SettingsView {
    pub fn new(root: WeakEntity<Root>, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        SettingsView { root }
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
