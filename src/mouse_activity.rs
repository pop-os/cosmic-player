//! Tracks pointer activity without turning every motion event into a message.
//!
//! Emitting a message per `CursorMoved` makes iced redraw the window, and for a GPU surface that
//! means committing the whole thing.
//! This widget keeps the "when did the pointer last move" state itself and only
//! speaks up on the transitions the application cares about: when the controls should appear, and
//! when they have been idle long enough to hide.

use cosmic::iced::{
    Event, Length, Rectangle, Size, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{Tree, tree},
    },
    mouse as iced_mouse, touch, window,
};
use std::time::{Duration, Instant};

use cosmic::{Element, Renderer, Theme};

pub struct MouseActivity<'a, Message> {
    content: Element<'a, Message>,
    active: bool,
    timeout: Duration,
    on_active: Option<Message>,
    on_idle: Option<Message>,
}

#[derive(Default)]
struct State {
    last_motion: Option<Instant>,
}

impl<'a, Message> MouseActivity<'a, Message> {
    pub fn new(content: impl Into<Element<'a, Message>>, active: bool, timeout: Duration) -> Self {
        Self {
            content: content.into(),
            active,
            timeout,
            on_active: None,
            on_idle: None,
        }
    }

    /// Emitted when the pointer moves while the application considers itself idle.
    pub fn on_active(mut self, message: Message) -> Self {
        self.on_active = Some(message);
        self
    }

    /// Emitted once the pointer has been still for the timeout.
    pub fn on_idle(mut self, message: Message) -> Self {
        self.on_idle = Some(message);
        self
    }
}

impl<Message: Clone> Widget<Message, Theme, Renderer> for MouseActivity<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );

        let state = tree.state.downcast_mut::<State>();

        match event {
            Event::Mouse(iced_mouse::Event::CursorMoved { .. })
            | Event::Touch(touch::Event::FingerMoved { .. }) => {
                let now = Instant::now();
                state.last_motion = Some(now);

                if self.active {
                    // Already visible, so no message, so no repaint.
                    // Just make sure we get woken once the pointer has been still long enough to hide again.
                    shell.request_redraw_at(window::RedrawRequest::At(now + self.timeout));
                } else if let Some(on_active) = self.on_active.clone() {
                    shell.publish(on_active);
                }
            }
            Event::Window(window::Event::RedrawRequested(now)) => {
                if self.active
                    && let Some(last_motion) = state.last_motion
                    && now.duration_since(last_motion) >= self.timeout
                    && let Some(on_idle) = self.on_idle.clone()
                {
                    state.last_motion = None;
                    shell.publish(on_idle);
                }
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn cosmic::iced::advanced::widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message: Clone + 'a> From<MouseActivity<'a, Message>> for Element<'a, Message> {
    fn from(widget: MouseActivity<'a, Message>) -> Self {
        Element::new(widget)
    }
}
