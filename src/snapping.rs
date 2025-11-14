use x11rb::protocol::xproto::Window;

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    pub fn left(&self) -> i16 {
        self.x
    }
    
    pub fn right(&self) -> i16 {
        self.x + self.width as i16
    }
    
    pub fn top(&self) -> i16 {
        self.y
    }
    
    pub fn bottom(&self) -> i16 {
        self.y + self.height as i16
    }
}

#[derive(Debug)]
struct SnapCandidate {
    offset: i16,
    distance: i16,
}

/// Find the best snap position for a dragged thumbnail
/// Returns (x, y) if snapping should occur, None otherwise
pub fn find_snap_position(
    dragged: Rect,
    others: &[(Window, Rect)],
    threshold: u16,
) -> Option<(i16, i16)> {
    if threshold == 0 {
        return None; // Snapping disabled
    }
    
    let mut best_x: Option<SnapCandidate> = None;
    let mut best_y: Option<SnapCandidate> = None;
    let threshold = threshold as i16;
    
    for (_, other) in others {
        // Horizontal snapping (X-axis)
        // Snap left edge to right edge of other
        check_snap(&mut best_x, dragged.left(), other.right(), threshold);
        // Snap right edge to left edge of other
        check_snap(&mut best_x, dragged.right(), other.left(), threshold);
        // Align left edges
        check_snap(&mut best_x, dragged.left(), other.left(), threshold);
        // Align right edges
        check_snap(&mut best_x, dragged.right(), other.right(), threshold);
        
        // Vertical snapping (Y-axis)
        // Snap top edge to bottom edge of other
        check_snap(&mut best_y, dragged.top(), other.bottom(), threshold);
        // Snap bottom edge to top edge of other
        check_snap(&mut best_y, dragged.bottom(), other.top(), threshold);
        // Align top edges
        check_snap(&mut best_y, dragged.top(), other.top(), threshold);
        // Align bottom edges
        check_snap(&mut best_y, dragged.bottom(), other.bottom(), threshold);
    }
    
    // Apply snaps if found
    let snap_x = best_x.map(|s| dragged.x + s.offset);
    let snap_y = best_y.map(|s| dragged.y + s.offset);
    
    match (snap_x, snap_y) {
        (Some(x), Some(y)) => Some((x, y)),
        (Some(x), None) => Some((x, dragged.y)),
        (None, Some(y)) => Some((dragged.x, y)),
        (None, None) => None,
    }
}

fn check_snap(
    best: &mut Option<SnapCandidate>,
    edge: i16,
    target: i16,
    threshold: i16,
) {
    let distance = (edge - target).abs();
    if distance <= threshold {
        let candidate = SnapCandidate {
            offset: target - edge,
            distance,
        };
        
        // Keep this candidate if it's closer than the current best
        if best.as_ref().map_or(true, |b| candidate.distance < b.distance) {
            *best = Some(candidate);
        }
    }
}
