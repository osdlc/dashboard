//! A drawing target that is either the terminal frame or an off-screen buffer.
//! Tiles are drawn off-screen on a canvas taller than the terminal, then the
//! visible part is copied in, which is how the page scrolls.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

pub struct Canvas<'a>(pub &'a mut Buffer);

impl Canvas<'_> {
    pub fn render_widget<W: Widget>(&mut self, widget: W, area: Rect) {
        // Clip to the buffer: widgets assume their area is inside it.
        let area = area.intersection(self.0.area);
        widget.render(area, self.0);
    }

    pub fn buffer_mut(&mut self) -> &mut Buffer {
        self.0
    }
}
