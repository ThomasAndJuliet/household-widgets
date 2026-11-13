// Forward only the hooks named by each wrapper; interception stays in its impl.
macro_rules! forward_widget {
    ($message:ty, $theme:ty, $renderer:ty; $($hook:ident),* $(,)?) => {
        $(forward_widget!(@hook $message, $theme, $renderer; $hook);)*
    };
    (@hook $message:ty, $theme:ty, $renderer:ty; children) => {
        fn children(&self) -> Vec<Tree> {
                vec![Tree::new(&self.content)]
            }
    };
    (@hook $message:ty, $theme:ty, $renderer:ty; diff) => {
        fn diff(&mut self, tree: &mut Tree) {
                tree.diff_children(std::slice::from_mut(&mut self.content));
            }
    };
    (@hook $message:ty, $theme:ty, $renderer:ty; size) => {
        fn size(&self) -> Size<Length> {
                self.content.as_widget().size()
            }
    };
    (@hook $message:ty, $theme:ty, $renderer:ty; layout) => {
        fn layout(&mut self, tree: &mut Tree, renderer: &$renderer, limits: &layout::Limits) -> layout::Node {
                self.content.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
            }
    };
    (@hook $message:ty, $theme:ty, $renderer:ty; operate) => {
        fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &$renderer, operation: &mut dyn Operation) {
                self.content.as_widget_mut().operate(&mut tree.children[0], layout, renderer, operation);
            }
    };
    (@hook $message:ty, $theme:ty, $renderer:ty; size_hint) => {
        fn size_hint(&self) -> Size<Length> {
            self.content.as_widget().size_hint()
        }
    };
    (@hook $message:ty, $theme:ty, $renderer:ty; a11y_nodes) => {
        #[cfg(feature = "a11y")]
        fn a11y_nodes(
            &self,
            layout: Layout<'_>,
            tree: &Tree,
            cursor: mouse::Cursor,
        ) -> iced_accessibility::A11yTree {
            self.content.as_widget().a11y_nodes(layout, &tree.children[0], cursor)
        }
    };
    (@hook $message:ty, $theme:ty, $renderer:ty; update) => {
        fn update(
                &mut self,
                tree: &mut Tree,
                event: &Event,
                layout: Layout<'_>,
                cursor: mouse::Cursor,
                renderer: &$renderer,
                clipboard: &mut dyn Clipboard,
                shell: &mut Shell<'_, $message>,
                viewport: &Rectangle,
            ) {
                self.content.as_widget_mut().update(&mut tree.children[0], event, layout, cursor, renderer, clipboard, shell, viewport);
            }
    };
    (@hook $message:ty, $theme:ty, $renderer:ty; draw) => {
        fn draw(
                &self,
                tree: &Tree,
                renderer: &mut $renderer,
                theme: &$theme,
                style: &renderer::Style,
                layout: Layout<'_>,
                cursor: mouse::Cursor,
                viewport: &Rectangle,
            ) {
                self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
            }
    };
    (@hook $message:ty, $theme:ty, $renderer:ty; mouse_interaction) => {
        fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &$renderer) -> mouse::Interaction {
                self.content.as_widget().mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
            }
    };
    (@hook $message:ty, $theme:ty, $renderer:ty; overlay) => {
        fn overlay<'b>(
                &'b mut self,
                tree: &'b mut Tree,
                layout: Layout<'b>,
                renderer: &$renderer,
                viewport: &Rectangle,
                translation: Vector,
            ) -> Option<overlay::Element<'b, $message, $theme, $renderer>> {
                self.content.as_widget_mut().overlay(&mut tree.children[0], layout, renderer, viewport, translation)
            }
    };
    (@hook $message:ty, $theme:ty, $renderer:ty; drag_destinations) => {
        fn drag_destinations(&self, tree: &Tree, layout: Layout<'_>, renderer: &$renderer, dnd_rectangles: &mut cosmic::iced::advanced::clipboard::DndDestinationRectangles) {
                self.content.as_widget().drag_destinations(&tree.children[0], layout, renderer, dnd_rectangles);
            }
    };
}
