//! State for the opt-in three-finger zoom recognizer. Coordinates are normalized
//! trackpad positions, not cursor movement. Contact transitions rebase the gesture.
#[derive(Default)]
pub(crate) struct TrackpadZoom {
    previous: Option<f64>,
    pub(crate) suppress_scroll: bool,
    touching: bool,
}
impl TrackpadZoom {
    /// Returns a magnification delta; positive vertical movement zooms in.
    pub(crate) fn update(&mut self, positions: &[f64], rebase: bool) -> Option<f32> {
        if !self.touching && !positions.is_empty() {
            self.suppress_scroll = false;
        }
        self.touching = !positions.is_empty();
        if positions.len() != 3 || positions.iter().any(|y| !y.is_finite()) {
            self.previous = None;
            return None;
        }
        self.suppress_scroll = true;
        let center = positions.iter().sum::<f64>() / 3.;
        let previous = self.previous.replace(center);
        if rebase {
            return None;
        }
        previous.map(|last| ((center - last).clamp(-0.2, 0.2) * 5.).exp() as f32 - 1.)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_three_contacts_zoom_and_transitions_do_not_jump() {
        let mut z = TrackpadZoom::default();
        assert_eq!(z.update(&[0.2, 0.3], true), None);
        assert_eq!(z.update(&[0.2, 0.3, 0.4], true), None);
        let delta = z.update(&[0.3, 0.4, 0.5], false).unwrap();
        assert!((delta - (0.5f32.exp() - 1.)).abs() < 1e-6);
        assert!(z.suppress_scroll);
        assert_eq!(z.update(&[0.3, 0.4], true), None);
        assert_eq!(z.update(&[0.7, 0.8, 0.9], true), None);
        assert!(z.update(&[0.6, 0.7, 0.8], false).unwrap() < 0.);
        assert_eq!(z.update(&[], true), None);
        // Do not turn the tail of a three-finger scroll into a pan.
        assert!(z.suppress_scroll);
        z.update(&[0.2, 0.3], true);
        assert!(!z.suppress_scroll);
    }
    #[test]
    fn replaced_contacts_and_invalid_coordinates_rebase() {
        let mut z = TrackpadZoom::default();
        z.update(&[0.1, 0.2, 0.3], true);
        assert_eq!(z.update(&[0.5, 0.6, 0.7], true), None);
        assert_eq!(z.update(&[f64::NAN, 0.6, 0.7], false), None);
        assert_eq!(z.update(&[0.5, 0.6, 0.7], false), None);
        assert_eq!(z.update(&[0.5, 0.6, 0.7, 0.8], false), None);
    }
}
