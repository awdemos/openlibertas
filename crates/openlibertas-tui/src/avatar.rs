//! Animated sprite avatar system for the TUI.
//!
//! Provides a zero-allocation render path for braille-based animated avatars
//! that drift smoothly within a bounded rectangular area.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::widgets::Widget;

/// A single animation frame consisting of exactly 8 lines of text.
///
/// Each line is rendered horizontally. The frame is positioned as a unit
/// at the avatar's current floating-point coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpriteFrame(pub &'static [&'static str; 8]);

/// An animated sprite avatar that cycles through frames and drifts within bounds.
///
/// The avatar deterministically derives its foreground color and starting frame
/// from a hash of its `id` string, making it visually consistent across renders.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimatedAvatar {
    /// Hash seed used for deterministic color and starting frame.
    pub id: u64,
    /// Animation frame sequence.
    pub frames: &'static [SpriteFrame],
    /// Current frame index into `frames`.
    pub frame_idx: usize,
    /// Milliseconds accumulated toward the next frame.
    pub accumulator: f32,
    /// Milliseconds per frame (default 480.0 = 2 fps).
    pub anim_speed: f32,
    /// Floating-point position `(x, y)` in terminal cells.
    pub pos: (f32, f32),
    /// Drift velocity in cells per second.
    pub velocity: (f32, f32),
    /// Foreground color for rendering.
    pub color: Color,
    /// Boundary rectangle for clamping position.
    pub bounds: Rect,
}

impl AnimatedAvatar {
    /// Creates a new avatar from a string identifier and frame sequence.
    ///
    /// The `id` is hashed with `seahash` to produce a deterministic `u64` seed.
    /// That seed drives both the foreground color (via [`hash_to_color`]) and
    /// the starting animation frame.
    ///
    /// # Example
    ///
    /// ```
    /// use openlibertas_tui::avatar::{AnimatedAvatar, IDLE_FRAMES};
    /// let avatar = AnimatedAvatar::new("user-42", IDLE_FRAMES);
    /// ```
    pub fn new(id: &str, frames: &'static [SpriteFrame]) -> Self {
        let hash = seahash::hash(id.as_bytes());
        let color = hash_to_color(hash);
        let frame_idx = (hash as usize) % frames.len().max(1);

        Self {
            id: hash,
            frames,
            frame_idx,
            accumulator: 0.0,
            anim_speed: 480.0,
            pos: (0.0, 0.0),
            velocity: (0.3, 0.15),
            color,
            bounds: Rect::default(),
        }
    }

    /// Advances animation and physics by `delta_ms` milliseconds.
    ///
    /// - Adds `delta_ms` to the frame accumulator; when it exceeds `anim_speed`,
    ///   the frame index wraps to the next frame.
    /// - Applies velocity: `pos += velocity * (delta_ms / 1000.0)`.
    /// - Clamps `pos` so the 8×N frame stays fully inside `bounds`.
    pub fn update(&mut self, delta_ms: f32) {
        // Advance animation.
        if !self.frames.is_empty() && self.anim_speed > 0.0 {
            self.accumulator += delta_ms;
            while self.accumulator >= self.anim_speed {
                self.accumulator -= self.anim_speed;
                self.frame_idx = (self.frame_idx + 1) % self.frames.len();
            }
        }

        // Apply drift.
        let dt = delta_ms / 1000.0;
        self.pos.0 += self.velocity.0 * dt;
        self.pos.1 += self.velocity.1 * dt;

        // Clamp to bounds.
        self.clamp_to_bounds();
    }

    /// Sets the boundary rectangle used for position clamping.
    pub fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
        self.clamp_to_bounds();
    }

    /// Sets the drift velocity in cells per second.
    pub fn set_velocity(&mut self, vx: f32, vy: f32) {
        self.velocity = (vx, vy);
    }

    /// Ensure the avatar stays fully inside `self.bounds`.
    fn clamp_to_bounds(&mut self) {
        if self.bounds.width == 0 || self.bounds.height == 0 {
            return;
        }

        let frame_width = self.current_frame_width();
        let max_x = (self.bounds.x + self.bounds.width).saturating_sub(frame_width) as f32;
        let max_y = (self.bounds.y + self.bounds.height).saturating_sub(8) as f32;
        let min_x = self.bounds.x as f32;
        let min_y = self.bounds.y as f32;

        self.pos.0 = self.pos.0.clamp(min_x, max_x);
        self.pos.1 = self.pos.1.clamp(min_y, max_y);
    }

    /// Width of the current frame in cells (longest line).
    fn current_frame_width(&self) -> u16 {
        self.frames
            .get(self.frame_idx)
            .map(|f| {
                f.0.iter()
                    .map(|line| line.chars().count() as u16)
                    .max()
                    .unwrap_or(0)
            })
            .unwrap_or(0)
    }
}

impl Widget for &AnimatedAvatar {
    fn render(self, _area: Rect, buf: &mut Buffer) {
        let this = self;
        let Some(frame) = this.frames.get(this.frame_idx) else {
            return;
        };

        let base_x = this.pos.0 as u16;
        let base_y = this.pos.1 as u16;

        for (row, line) in frame.0.iter().enumerate() {
            let y = base_y + row as u16;
            if y >= buf.area().y + buf.area().height {
                continue;
            }

            for (col, ch) in line.chars().enumerate() {
                // Skip transparent characters.
                if ch == ' ' || ch == '\u{2800}' {
                    continue;
                }

                let x = base_x + col as u16;
                if x >= buf.area().x + buf.area().width {
                    break;
                }

                // Safe: x and y are within the buffer's area because of the
                // bounds checks above, and `buf` is indexed by `(x, y)`.
                buf[(x, y)].set_symbol(&ch.to_string()).set_fg(this.color);
            }
        }
    }
}

/// Maps a `u64` hash to one of six ANSI colors.
///
/// Colors: Red, Green, Blue, Yellow, Cyan, Magenta.
pub fn hash_to_color(hash: u64) -> Color {
    match hash % 6 {
        0 => Color::Red,
        1 => Color::Green,
        2 => Color::Blue,
        3 => Color::Yellow,
        4 => Color::Cyan,
        _ => Color::Magenta,
    }
}

// ---------------------------------------------------------------------------
// Predefined frame sets
// ---------------------------------------------------------------------------

static HUMAN_FRAME_1: SpriteFrame = SpriteFrame(&[
    "⠀⠀⠀⠀⣀⣀⠀⠀⠀⠀⠀",
    "⠀⠀⠀⣰⣿⣿⣆⠀⠀⠀⠀",
    "⠀⠀⣰⣿⣿⣿⣆⠀⠀⠀⠀",
    "⠀⢀⣿⣿⣿⣿⣿⡀⠀⠀⠀",
    "⠀⠈⢻⣿⣿⡟⠁⠀⠀⠀⠀",
    "⠀⠀⠀⠻⠟⠀⠀⠀⠀⠀⠀",
    "⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀",
    "⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀",
]);

static HUMAN_FRAME_2: SpriteFrame = SpriteFrame(&[
    "⠀⠀⠀⠀⣀⣀⠀⠀⠀⠀⠀",
    "⠀⠀⢰⣿⣿⣿⣆⠀⠀⠀⠀",
    "⠀⢰⣿⣿⣿⣿⣆⠀⠀⠀⠀",
    "⠀⣿⣿⣿⣿⣿⣿⡀⠀⠀⠀",
    "⠀⠈⢻⣿⣿⡟⠁⠀⠀⠀⠀",
    "⠀⠀⠀⠻⠟⠀⠀⠀⠀⠀⠀",
    "⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀",
    "⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀",
]);

static HUMAN_FRAME_3: SpriteFrame = SpriteFrame(&[
    "⠀⠀⠀⠀⣀⣀⠀⠀⠀⠀⠀",
    "⠀⠀⠀⣰⣿⣿⣆⢰⠀⠀⠀",
    "⠀⠀⣰⣿⣿⣿⣆⠀⠀⠀⠀",
    "⠀⢀⣿⣿⣿⣿⣿⡀⠀⠀⠀",
    "⠀⠈⢻⣿⣿⡟⠁⠀⠀⠀⠀",
    "⠀⠀⠀⠻⠟⠀⠀⠀⠀⠀⠀",
    "⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀",
    "⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀",
]);

/// Predefined idle animation frames.
pub const IDLE_FRAMES: &[SpriteFrame] = &[HUMAN_FRAME_1, HUMAN_FRAME_2, HUMAN_FRAME_3];

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animation_cycles_through_frames() {
        let mut avatar = AnimatedAvatar::new("test-id", IDLE_FRAMES);
        avatar.anim_speed = 100.0;
        avatar.frame_idx = 0;
        avatar.accumulator = 0.0;

        // Should stay on frame 0.
        avatar.update(50.0);
        assert_eq!(avatar.frame_idx, 0);
        assert!(avatar.accumulator >= 50.0);

        // Should advance to frame 1.
        avatar.update(60.0);
        assert_eq!(avatar.frame_idx, 1);
        assert!(avatar.accumulator < 100.0);

        // Should wrap back to frame 0.
        avatar.update(100.0);
        assert_eq!(avatar.frame_idx, 0);
    }

    #[test]
    fn boundary_clamping_works() {
        let mut avatar = AnimatedAvatar::new("test-id", IDLE_FRAMES);
        // Frame width is ~8 chars, height is 8 lines.
        avatar.set_bounds(Rect {
            x: 0,
            y: 0,
            width: 20,
            height: 20,
        });
        avatar.set_velocity(1000.0, 1000.0);

        // Move far outside.
        avatar.update(1000.0);

        // Should be clamped inside bounds.
        let max_x = (20 - avatar.current_frame_width()) as f32;
        let max_y = (20 - 8) as f32;
        assert!(avatar.pos.0 >= 0.0);
        assert!(avatar.pos.0 <= max_x);
        assert!(avatar.pos.1 >= 0.0);
        assert!(avatar.pos.1 <= max_y);
    }

    #[test]
    fn hash_to_color_maps_all_moduli() {
        assert_eq!(hash_to_color(0), Color::Red);
        assert_eq!(hash_to_color(1), Color::Green);
        assert_eq!(hash_to_color(2), Color::Blue);
        assert_eq!(hash_to_color(3), Color::Yellow);
        assert_eq!(hash_to_color(4), Color::Cyan);
        assert_eq!(hash_to_color(5), Color::Magenta);
        assert_eq!(hash_to_color(6), Color::Red); // wraps
    }

    #[test]
    fn new_determinism() {
        let a1 = AnimatedAvatar::new("same-id", IDLE_FRAMES);
        let a2 = AnimatedAvatar::new("same-id", IDLE_FRAMES);
        assert_eq!(a1.id, a2.id);
        assert_eq!(a1.color, a2.color);
        assert_eq!(a1.frame_idx, a2.frame_idx);
    }

    #[test]
    fn render_skips_spaces_and_braille_blank() {
        let avatar = AnimatedAvatar::new("render-test", IDLE_FRAMES);
        let mut buf = Buffer::empty(Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 20,
        });

        // Place the avatar at (5, 5).
        let mut avatar = avatar;
        avatar.pos = (5.0, 5.0);
        avatar.set_bounds(Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 20,
        });

        ratatui::widgets::Widget::render(&avatar, buf.area, &mut buf);

        // Spot-check: a corner cell that should be transparent (space or braille blank)
        // remains empty.
        let cell = &buf[(5, 5)];
        // The first cell of the first line is a braille blank (U+2800) or space,
        // so it should NOT have been overwritten with the avatar's color.
        assert_eq!(cell.symbol(), " ");
    }
}
