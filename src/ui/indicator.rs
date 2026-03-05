use gtk4::cairo;
use gtk4::glib;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

const DOT_COUNT: usize = 5;
const DOT_BASE_RADIUS: f64 = 4.0;
const DOT_MAX_RADIUS: f64 = 12.0;
const DOT_SPACING: f64 = 24.0;
const WINDOW_WIDTH: i32 = 200;
const WINDOW_HEIGHT: i32 = 60;
const FADEOUT_DELAY_MS: u64 = 2000;
const FADEOUT_STEP_MS: u64 = 33;
const FADEOUT_DECREMENT: f64 = 0.05;

#[derive(Clone, Debug)]
struct IndicatorState {
    current_state: String,
    audio_level: f32,
    phase: f64,
    visible: bool,
    fadeout_opacity: f64,
}

impl Default for IndicatorState {
    fn default() -> Self {
        Self {
            current_state: "Idle".to_string(),
            audio_level: 0.0,
            phase: 0.0,
            visible: false,
            fadeout_opacity: 1.0,
        }
    }
}

/// Compute the label text for a given application state.
pub fn label_for_state(state: &str) -> &'static str {
    match state {
        "Recording" => "Recording...",
        "Processing" => "Processing...",
        "Typing" => "Typing...",
        "Idle" => "Done",
        _ => "",
    }
}

/// Compute the dot radius for a given dot index, phase, and audio level.
///
/// Each dot oscillates with a phase offset, and the oscillation amplitude
/// is scaled by the audio level (0.0 .. 1.0).
pub fn dot_radius(index: usize, phase: f64, audio_level: f32) -> f64 {
    let phase_offset = index as f64 * 0.8;
    let wave = ((phase + phase_offset) * 2.0).sin().abs();
    let level_factor = audio_level as f64;
    DOT_BASE_RADIUS + (DOT_MAX_RADIUS - DOT_BASE_RADIUS) * wave * level_factor
}

/// Compute the x position for a dot at the given index, centered within the given total width.
pub fn dot_x_position(index: usize, total_width: f64) -> f64 {
    let dots_total_width = (DOT_COUNT - 1) as f64 * DOT_SPACING;
    let start_x = total_width / 2.0 - dots_total_width / 2.0;
    start_x + index as f64 * DOT_SPACING
}

/// Compute the color for a dot at the given index.
/// Returns (r, g, b) in 0.0..1.0 range. Gradient from cyan to blue.
pub fn dot_color(index: usize) -> (f64, f64, f64) {
    let hue = 0.5 + index as f64 * 0.05;
    (0.2, 0.6 + hue * 0.3, 1.0)
}

/// A floating indicator window that shows recording/processing state.
///
/// Displays animated dots whose sizes respond to audio levels, and a status
/// text label. The window is frameless, semi-transparent, and positioned at
/// the top center of the screen.
pub struct IndicatorWindow {
    window: gtk4::Window,
    state: Rc<RefCell<IndicatorState>>,
    _drawing_area: gtk4::DrawingArea,
}

impl IndicatorWindow {
    /// Create a new indicator window. Requires a running GTK4 main loop.
    pub fn new() -> Self {
        let window = gtk4::Window::new();
        window.set_decorated(false);
        window.set_resizable(false);
        window.set_default_size(WINDOW_WIDTH, WINDOW_HEIGHT);
        window.set_can_focus(false);
        window.set_focusable(false);

        // Make the window transparent using CSS
        let css_provider = gtk4::CssProvider::new();
        css_provider.load_from_data(
            "window.indicator-window { background-color: transparent; }",
        );
        gtk4::style_context_add_provider_for_display(
            &gtk4::gdk::Display::default().expect("display should exist"),
            &css_provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        window.add_css_class("indicator-window");

        let drawing_area = gtk4::DrawingArea::new();
        drawing_area.set_content_width(WINDOW_WIDTH);
        drawing_area.set_content_height(WINDOW_HEIGHT);

        let state = Rc::new(RefCell::new(IndicatorState::default()));

        // Set up the draw function
        let state_for_draw = state.clone();
        drawing_area.set_draw_func(move |_area, cr, width, height| {
            let s = state_for_draw.borrow();

            // Clear with transparent background
            cr.set_operator(cairo::Operator::Clear);
            let _ = cr.paint();
            cr.set_operator(cairo::Operator::Over);

            let w = width as f64;
            let h = height as f64;
            let corner_radius = 12.0;

            // Draw rounded rectangle background
            let bg_alpha = 0.85 * s.fadeout_opacity;
            cr.set_source_rgba(0.15, 0.15, 0.15, bg_alpha);
            draw_rounded_rect(cr, 4.0, 4.0, w - 8.0, h - 8.0, corner_radius);
            let _ = cr.fill();

            // Draw status text
            let label = label_for_state(&s.current_state);
            cr.set_source_rgba(1.0, 1.0, 1.0, s.fadeout_opacity);
            cr.set_font_size(12.0);
            if let Ok(extents) = cr.text_extents(label) {
                let text_x = (w - extents.width()) / 2.0 - extents.x_bearing();
                let text_y = h * 0.32;
                cr.move_to(text_x, text_y);
                let _ = cr.show_text(label);
            }

            // Draw animated dots
            let dots_y = h * 0.65;
            for i in 0..DOT_COUNT {
                let radius = dot_radius(i, s.phase, s.audio_level);
                let x = dot_x_position(i, w);
                let (r, g, b) = dot_color(i);
                cr.set_source_rgba(r, g, b, s.fadeout_opacity);
                cr.arc(x, dots_y, radius, 0.0, std::f64::consts::PI * 2.0);
                let _ = cr.fill();
            }
        });

        window.set_child(Some(&drawing_area));

        // Animation timer (~30fps)
        let state_for_timer = state.clone();
        let da_for_timer = drawing_area.clone();
        glib::timeout_add_local(Duration::from_millis(FADEOUT_STEP_MS), move || {
            let mut s = state_for_timer.borrow_mut();
            if s.visible {
                s.phase = (s.phase + 0.1) % (std::f64::consts::PI * 20.0);
                drop(s);
                da_for_timer.queue_draw();
            }
            glib::ControlFlow::Continue
        });

        Self {
            window,
            state,
            _drawing_area: drawing_area,
        }
    }

    /// Show the indicator for the given state. For "Idle", starts a fadeout
    /// and eventually hides the window.
    pub fn show_state(&self, state: &str) {
        let mut s = self.state.borrow_mut();
        s.current_state = state.to_string();
        match state {
            "Idle" => {
                // Start fadeout after a delay
                s.fadeout_opacity = 1.0;
                drop(s);
                self.start_fadeout();
            }
            _ => {
                s.visible = true;
                s.fadeout_opacity = 1.0;
                drop(s);
                self.window.set_visible(true);
            }
        }
    }

    /// Update the audio level (0.0 .. 1.0) used for dot animation amplitude.
    pub fn update_audio_level(&self, level: f32) {
        self.state.borrow_mut().audio_level = level.clamp(0.0, 1.0);
    }

    fn start_fadeout(&self) {
        let state = self.state.clone();
        let window = self.window.clone();

        // Wait for FADEOUT_DELAY_MS, then start decreasing opacity
        glib::timeout_add_local_once(Duration::from_millis(FADEOUT_DELAY_MS), move || {
            let state_fade = state.clone();
            let window_fade = window.clone();
            glib::timeout_add_local(Duration::from_millis(FADEOUT_STEP_MS), move || {
                let mut s = state_fade.borrow_mut();
                // If state changed away from Idle during fadeout, cancel
                if s.current_state != "Idle" {
                    return glib::ControlFlow::Break;
                }
                s.fadeout_opacity -= FADEOUT_DECREMENT;
                if s.fadeout_opacity <= 0.0 {
                    s.fadeout_opacity = 0.0;
                    s.visible = false;
                    drop(s);
                    window_fade.set_visible(false);
                    return glib::ControlFlow::Break;
                }
                glib::ControlFlow::Continue
            });
        });
    }
}

/// Draw a rounded rectangle path on the cairo context.
fn draw_rounded_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(
        x + w - r,
        y + h - r,
        r,
        0.0,
        std::f64::consts::FRAC_PI_2,
    );
    cr.arc(
        x + r,
        y + h - r,
        r,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    cr.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        3.0 * std::f64::consts::FRAC_PI_2,
    );
    cr.close_path();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_label_for_state() {
        assert_eq!(label_for_state("Recording"), "Recording...");
        assert_eq!(label_for_state("Processing"), "Processing...");
        assert_eq!(label_for_state("Typing"), "Typing...");
        assert_eq!(label_for_state("Idle"), "Done");
        assert_eq!(label_for_state("unknown"), "");
    }

    #[test]
    fn test_dot_radius_zero_audio() {
        // With zero audio level, all dots should have base radius
        for i in 0..DOT_COUNT {
            let r = dot_radius(i, 0.0, 0.0);
            assert!(
                (r - DOT_BASE_RADIUS).abs() < f64::EPSILON,
                "dot {} with zero audio should have base radius, got {}",
                i,
                r
            );
        }
    }

    #[test]
    fn test_dot_radius_full_audio() {
        // With full audio level, radius should be between base and max
        for i in 0..DOT_COUNT {
            let r = dot_radius(i, 1.0, 1.0);
            assert!(
                r >= DOT_BASE_RADIUS && r <= DOT_MAX_RADIUS,
                "dot {} radius {} should be between {} and {}",
                i,
                r,
                DOT_BASE_RADIUS,
                DOT_MAX_RADIUS
            );
        }
    }

    #[test]
    fn test_dot_radius_half_audio() {
        // With half audio level, the maximum possible radius should be
        // at most halfway between base and max
        for i in 0..DOT_COUNT {
            let r = dot_radius(i, 0.0, 0.5);
            let max_possible = DOT_BASE_RADIUS + (DOT_MAX_RADIUS - DOT_BASE_RADIUS) * 0.5;
            assert!(
                r >= DOT_BASE_RADIUS && r <= max_possible + f64::EPSILON,
                "dot {} radius {} should be between {} and {}",
                i,
                r,
                DOT_BASE_RADIUS,
                max_possible
            );
        }
    }

    #[test]
    fn test_dot_x_positions_centered() {
        let total_width = 200.0;
        let positions: Vec<f64> = (0..DOT_COUNT)
            .map(|i| dot_x_position(i, total_width))
            .collect();

        // Dots should be centered around total_width / 2
        let center = total_width / 2.0;
        let midpoint = (positions.first().unwrap() + positions.last().unwrap()) / 2.0;
        assert!(
            (midpoint - center).abs() < f64::EPSILON,
            "dots midpoint {} should be at center {}",
            midpoint,
            center
        );
    }

    #[test]
    fn test_dot_x_positions_evenly_spaced() {
        let total_width = 200.0;
        let positions: Vec<f64> = (0..DOT_COUNT)
            .map(|i| dot_x_position(i, total_width))
            .collect();

        for i in 1..DOT_COUNT {
            let spacing = positions[i] - positions[i - 1];
            assert!(
                (spacing - DOT_SPACING).abs() < f64::EPSILON,
                "spacing between dot {} and {} should be {}, got {}",
                i - 1,
                i,
                DOT_SPACING,
                spacing
            );
        }
    }

    #[test]
    fn test_dot_color_values_in_range() {
        for i in 0..DOT_COUNT {
            let (r, g, b) = dot_color(i);
            assert!(r >= 0.0 && r <= 1.0, "red out of range for dot {}", i);
            assert!(g >= 0.0 && g <= 1.0, "green out of range for dot {}", i);
            assert!(b >= 0.0 && b <= 1.0, "blue out of range for dot {}", i);
        }
    }

    #[test]
    fn test_dot_colors_form_gradient() {
        // Each successive dot should have a slightly different green component
        let colors: Vec<(f64, f64, f64)> = (0..DOT_COUNT).map(|i| dot_color(i)).collect();
        for i in 1..DOT_COUNT {
            assert!(
                colors[i].1 > colors[i - 1].1,
                "green should increase from dot {} to {}",
                i - 1,
                i
            );
        }
    }

    #[test]
    fn test_indicator_state_default() {
        let state = IndicatorState::default();
        assert_eq!(state.current_state, "Idle");
        assert_eq!(state.audio_level, 0.0);
        assert_eq!(state.phase, 0.0);
        assert!(!state.visible);
        assert_eq!(state.fadeout_opacity, 1.0);
    }

    // GTK-dependent tests are marked #[ignore] since they require a display
    #[test]
    #[ignore]
    fn test_indicator_window_creation() {
        gtk4::init().expect("GTK init failed");
        let _indicator = IndicatorWindow::new();
    }

    #[test]
    #[ignore]
    fn test_indicator_show_recording() {
        gtk4::init().expect("GTK init failed");
        let indicator = IndicatorWindow::new();
        indicator.show_state("Recording");
        assert!(indicator.state.borrow().visible);
        assert_eq!(indicator.state.borrow().current_state, "Recording");
    }

    #[test]
    #[ignore]
    fn test_indicator_update_audio_level() {
        gtk4::init().expect("GTK init failed");
        let indicator = IndicatorWindow::new();
        indicator.update_audio_level(0.75);
        assert_eq!(indicator.state.borrow().audio_level, 0.75);
    }

    #[test]
    #[ignore]
    fn test_indicator_audio_level_clamped() {
        gtk4::init().expect("GTK init failed");
        let indicator = IndicatorWindow::new();
        indicator.update_audio_level(1.5);
        assert_eq!(indicator.state.borrow().audio_level, 1.0);
        indicator.update_audio_level(-0.5);
        assert_eq!(indicator.state.borrow().audio_level, 0.0);
    }
}
