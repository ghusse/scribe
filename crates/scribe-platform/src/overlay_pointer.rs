//! Pointer state over the overlay, fed by native mouse events: macOS sends none to the overlay's web view while
//! another app is active, so the hover state, the hovered button and the pointing-hand cursor are decided here.

/// An area in overlay coordinates (CSS pixels, origin top-left): `[x, y, width, height]`.
pub type HitArea = [f64; 4];

/// The overlay's layout after a render.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct OverlayAreas {
    /// The visible pill or toast (`None` while idle): the rest of the window is transparent and does not count
    /// as being over the overlay.
    pub surface: Option<HitArea>,
    /// Its buttons, in DOM order.
    pub buttons: Vec<HitArea>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PointerState {
    /// The pointer is over the visible surface.
    pub inside: bool,
    /// Index of the button under the pointer, in the order the overlay sent them.
    pub over: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cursor {
    Arrow,
    PointingHand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerChange {
    pub state: PointerState,
    /// Set only when the pointer enters or leaves a button: elsewhere the cursor belongs to the app underneath.
    pub cursor: Option<Cursor>,
}

#[derive(Debug)]
pub struct PointerTracker {
    areas: OverlayAreas,
    /// Last position while over the overlay window.
    at: Option<(f64, f64)>,
    state: PointerState,
}

fn contains(a: &HitArea, (x, y): (f64, f64)) -> bool {
    x >= a[0] && x < a[0] + a[2] && y >= a[1] && y < a[1] + a[3]
}

impl Default for PointerTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl PointerTracker {
    pub const fn new() -> Self {
        Self {
            areas: OverlayAreas { surface: None, buttons: Vec::new() },
            at: None,
            state: PointerState { inside: false, over: None },
        }
    }

    pub fn state(&self) -> PointerState {
        self.state
    }

    pub fn moved(&mut self, x: f64, y: f64) -> Option<PointerChange> {
        self.at = Some((x, y));
        self.update()
    }

    /// The pointer left the window, or the window was hidden (no exit event then): its last position is forgotten.
    pub fn left(&mut self) -> Option<PointerChange> {
        self.at = None;
        self.update()
    }

    /// The overlay re-rendered: its surface and buttons moved, appeared or went away under a still pointer.
    pub fn set_areas(&mut self, areas: OverlayAreas) -> Option<PointerChange> {
        self.areas = areas;
        self.update()
    }

    fn update(&mut self) -> Option<PointerChange> {
        let at = self.at.filter(|&p| self.areas.surface.is_some_and(|s| contains(&s, p)));
        let now = PointerState {
            inside: at.is_some(),
            over: at.and_then(|p| self.areas.buttons.iter().position(|a| contains(a, p))),
        };
        if now == self.state {
            return None;
        }
        let cursor = (now.over.is_some() != self.state.over.is_some()).then_some(if now.over.is_some() {
            Cursor::PointingHand
        } else {
            Cursor::Arrow
        });
        self.state = now;
        Some(PointerChange { state: now, cursor })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SURFACE: HitArea = [0.0, 0.0, 220.0, 44.0];
    const COPY: HitArea = [100.0, 10.0, 50.0, 24.0];
    const VIEW: HitArea = [156.0, 10.0, 40.0, 24.0];

    fn areas(buttons: &[HitArea]) -> OverlayAreas {
        OverlayAreas { surface: Some(SURFACE), buttons: buttons.to_vec() }
    }

    fn change(inside: bool, over: Option<usize>, cursor: Option<Cursor>) -> Option<PointerChange> {
        Some(PointerChange { state: PointerState { inside, over }, cursor })
    }

    #[test]
    fn entering_moving_across_buttons_and_leaving() {
        let mut t = PointerTracker::default();
        t.set_areas(areas(&[COPY, VIEW]));
        assert_eq!(t.moved(10.0, 10.0), change(true, None, None));
        assert_eq!(t.moved(20.0, 15.0), None);
        assert_eq!(t.moved(100.0, 10.0), change(true, Some(0), Some(Cursor::PointingHand)));
        assert_eq!(t.state(), PointerState { inside: true, over: Some(0) });
        assert_eq!(t.moved(149.0, 33.0), None);
        assert_eq!(t.moved(150.0, 20.0), change(true, None, Some(Cursor::Arrow)));
        assert_eq!(t.moved(160.0, 20.0), change(true, Some(1), Some(Cursor::PointingHand)));
        assert_eq!(t.moved(170.0, 20.0), None);
        assert_eq!(t.left(), change(false, None, Some(Cursor::Arrow)));
        assert_eq!(t.left(), None);
    }

    #[test]
    fn entering_straight_onto_a_button() {
        let mut t = PointerTracker::default();
        t.set_areas(areas(&[[0.0, 0.0, 10.0, 10.0], COPY]));
        assert_eq!(t.moved(110.0, 20.0), change(true, Some(1), Some(Cursor::PointingHand)));
    }

    #[test]
    fn the_transparent_part_of_the_window_is_not_the_overlay() {
        let mut t = PointerTracker::default();
        t.set_areas(areas(&[COPY]));
        assert_eq!(t.moved(300.0, 20.0), None);
        assert_eq!(t.moved(10.0, 60.0), None);
        assert_eq!(t.moved(10.0, 10.0), change(true, None, None));
        assert_eq!(t.moved(10.0, 60.0), change(false, None, None));
        t.set_areas(OverlayAreas { surface: None, buttons: vec![COPY] });
        assert_eq!(t.moved(110.0, 20.0), None);
    }

    #[test]
    fn areas_changing_under_a_still_pointer() {
        let mut t = PointerTracker::default();
        assert_eq!(t.set_areas(areas(&[COPY])), None);
        t.moved(120.0, 20.0);
        assert_eq!(t.set_areas(areas(&[])), change(true, None, Some(Cursor::Arrow)));
        assert_eq!(t.set_areas(areas(&[VIEW, COPY])), change(true, Some(1), Some(Cursor::PointingHand)));
        assert_eq!(t.set_areas(OverlayAreas::default()), change(false, None, Some(Cursor::Arrow)));
        t.left();
        assert_eq!(t.set_areas(areas(&[COPY])), None);
    }

    #[test]
    fn a_hidden_overlay_forgets_the_pointer_so_the_next_layout_is_not_hovered() {
        let mut t = PointerTracker::default();
        t.set_areas(areas(&[COPY, VIEW]));
        t.moved(160.0, 20.0);
        assert_eq!(t.left(), change(false, None, Some(Cursor::Arrow)));
        assert_eq!(t.set_areas(OverlayAreas::default()), None);
        assert_eq!(t.set_areas(areas(&[COPY, VIEW])), None);
        assert_eq!(t.state(), PointerState::default());
    }
}
