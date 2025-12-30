//! A simple resizable container with a bottom-right drag handle.
//!
//! Intended for centered modals (Rules / Indicators / Rule Log).

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{Operation, Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell};
use iced::event::Event;
use iced::mouse;
use iced::{Color, Element, Length, Point, Rectangle, Size, Theme};
use iced::advanced::renderer::Quad;

// Bigger hitbox makes the UX much more discoverable, especially with UI scaling.
const HANDLE_SIZE: f32 = 28.0;

#[derive(Debug, Clone)]
enum Action {
    Idle,
    Resizing { origin: Point, start: Size<f32> },
}

#[allow(missing_debug_implementations)]
pub struct ResizeBox<'a, Message, Renderer = iced::Renderer>
where
    Renderer: renderer::Renderer,
{
    content: Element<'a, Message, Theme, Renderer>,
    width: f32,
    height: f32,
    min: Size<f32>,
    max: Size<f32>,
    on_resize: Box<dyn Fn(f32, f32) -> Message + 'a>,
}

impl<'a, Message, Renderer> ResizeBox<'a, Message, Renderer>
where
    Renderer: renderer::Renderer,
{
    pub fn new(
        content: impl Into<Element<'a, Message, Theme, Renderer>>,
        width: f32,
        height: f32,
        on_resize: impl Fn(f32, f32) -> Message + 'a,
    ) -> Self {
        Self {
            content: content.into(),
            width,
            height,
            min: Size::new(280.0, 220.0),
            max: Size::new(1200.0, 1000.0),
            on_resize: Box::new(on_resize),
        }
    }

    pub fn min_size(mut self, w: f32, h: f32) -> Self {
        self.min = Size::new(w, h);
        self
    }

    pub fn max_size(mut self, w: f32, h: f32) -> Self {
        self.max = Size::new(w, h);
        self
    }
}

impl<'a, Message> From<ResizeBox<'a, Message>> for Element<'a, Message>
where
    Message: 'a,
{
    fn from(widget: ResizeBox<'a, Message>) -> Self {
        Element::new(widget)
    }
}

impl<Message, Renderer> Widget<Message, Theme, Renderer> for ResizeBox<'_, Message, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Action>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Action::Idle)
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(self.width), Length::Fixed(self.height))
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let w = self.width.clamp(self.min.width, self.max.width);
        let h = self.height.clamp(self.min.height, self.max.height);

        let limits = limits
            .clone()
            .width(Length::Fixed(w))
            .height(Length::Fixed(h));

        let child = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, &limits);

        layout::Node::with_children(
            Size::new(w, h),
            vec![child.move_to(Point::new(0.0, 0.0))],
        )
    }

    fn operate(
        &mut self,
        _tree: &mut Tree,
        layout: Layout<'_>,
        _renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds());
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let action = tree.state.downcast_mut::<Action>();
        let bounds = layout.bounds();

        let handle_bounds = Rectangle {
            x: bounds.x + bounds.width - HANDLE_SIZE,
            y: bounds.y + bounds.height - HANDLE_SIZE,
            width: HANDLE_SIZE,
            height: HANDLE_SIZE,
        };

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if cursor.position_in(handle_bounds).is_some()
                    && let Some(pos) = cursor.position()
                {
                    *action = Action::Resizing {
                        origin: pos,
                        start: Size::new(self.width, self.height),
                    };
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let Action::Resizing { origin, start } = *action
                    && let Some(pos) = cursor.position()
                {
                    let dx = pos.x - origin.x;
                    let dy = pos.y - origin.y;
                    let nw = (start.width + dx).clamp(self.min.width, self.max.width);
                    let nh = (start.height + dy).clamp(self.min.height, self.max.height);
                    shell.publish((self.on_resize)(nw, nh));
                    shell.request_redraw();
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if matches!(action, Action::Resizing { .. }) {
                    *action = Action::Idle;
                    shell.capture_event();
                }
            }
            _ => {}
        }

        // forward events to content
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout.children().next().unwrap(),
            cursor,
            _renderer,
            _clipboard,
            shell,
            _viewport,
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
        let action = tree.state.downcast_ref::<Action>();
        let bounds = layout.bounds();
        let handle_bounds = Rectangle {
            x: bounds.x + bounds.width - HANDLE_SIZE,
            y: bounds.y + bounds.height - HANDLE_SIZE,
            width: HANDLE_SIZE,
            height: HANDLE_SIZE,
        };

        if matches!(action, Action::Resizing { .. }) {
            return mouse::Interaction::ResizingDiagonallyUp;
        }

        if cursor.position_in(handle_bounds).is_some() {
            return mouse::Interaction::ResizingDiagonallyUp;
        }

        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout.children().next().unwrap(),
            cursor,
            viewport,
            renderer,
        )
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
            layout.children().next().unwrap(),
            cursor,
            viewport,
        );
        // Draw a subtle "grip" in the bottom-right so resizing is discoverable.
        let bounds = layout.bounds();
        let grip = Rectangle {
            x: bounds.x + bounds.width - HANDLE_SIZE,
            y: bounds.y + bounds.height - HANDLE_SIZE,
            width: HANDLE_SIZE,
            height: HANDLE_SIZE,
        };

        let color = theme.extended_palette().background.strong.color;
        renderer.fill_quad(
            Quad {
                bounds: grip,
                border: iced::Border::default(),
                shadow: iced::Shadow::default(),
                snap: true,
            },
            Color {
                a: 0.08,
                ..color
            },
        );
    }
}


