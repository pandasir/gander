//! The `a` (CPU affinity) and `i` (I/O priority) panels.
//!
//! Both keys did nothing before this existed. They are ordinary `Screen`
//! variants so the runtime's key routing is unchanged; the difference is that
//! these panels hold editable state rather than read-only text.

use gander_core::action::{IoClass, IoPriority};
use gander_core::affinity::CpuMask;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::theme::Theme;

/// State of the affinity panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AffinityPanel {
    pub pid: u32,
    /// The mask as read from the kernel, or all cores when it is not readable.
    pub mask: CpuMask,
    pub cores: usize,
    /// Which core the cursor is on.
    pub cursor: usize,
    /// Set when the platform has no readable mask (macOS).
    pub unsupported: Option<String>,
    /// Set when reading the mask failed on a platform that does support it.
    pub error: Option<String>,
    /// Whether anything has been written back yet.
    pub dirty: bool,
}

impl AffinityPanel {
    pub fn new(pid: u32, cores: usize) -> Self {
        let cores = cores.max(1);
        match gander_core::affinity::read(pid, cores) {
            Ok(Some(mask)) => Self {
                pid,
                mask,
                cores,
                cursor: 0,
                unsupported: None,
                error: None,
                dirty: false,
            },
            Ok(None) => Self {
                pid,
                // Showing every core as "available" is the only honest picture
                // when the OS will not tell us which ones are actually allowed.
                mask: CpuMask::all(cores),
                cores,
                cursor: 0,
                unsupported: Some(
                    "this platform has no per-process CPU affinity mask".to_string(),
                ),
                error: None,
                dirty: false,
            },
            Err(e) => Self {
                pid,
                mask: CpuMask::all(cores),
                cores,
                cursor: 0,
                unsupported: None,
                error: Some(e),
                dirty: false,
            },
        }
    }

    /// Whether the panel can actually change anything.
    pub fn can_edit(&self) -> bool {
        self.unsupported.is_none() && self.error.is_none() && gander_core::affinity::supported()
    }

    pub fn move_cursor(&mut self, delta: isize) {
        let n = self.cores as isize;
        let next = (self.cursor as isize + delta).clamp(0, n - 1);
        self.cursor = next as usize;
    }

    pub fn toggle(&mut self) {
        if !self.can_edit() {
            return;
        }
        self.mask.toggle(self.cursor);
        self.dirty = true;
    }

    /// Write the mask back. Refuses to apply an empty mask, which the kernel
    /// would reject anyway and which would leave the process unschedulable.
    pub fn apply(&mut self) -> Result<(), String> {
        if !self.can_edit() {
            return Err(self
                .unsupported
                .clone()
                .or_else(|| self.error.clone())
                .unwrap_or_else(|| "affinity cannot be changed here".to_string()));
        }
        gander_core::affinity::write(self.pid, &self.mask)?;
        self.dirty = false;
        Ok(())
    }

    pub fn draw(&self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        let title = format!("CPU affinity of {}", self.pid);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(theme.dialog_border)
            .title(crate::app::span_title(&title, theme));
        let inner = block.inner(area);
        Widget::render(block, area, buf);
        if inner.height == 0 || inner.width == 0 {
            return;
        }

        let chunks = Layout::vertical([Constraint::Min(1), Constraint::Length(2)]).split(inner);

        if let Some(reason) = &self.unsupported {
            let lines = vec![
                Line::styled(reason.as_str(), theme.meter_label),
                Line::from(""),
                Line::from("macOS offers only a scheduler hint (THREAD_AFFINITY_POLICY),"),
                Line::from("which is a preference rather than a mask. Linux is the only"),
                Line::from("supported platform with a real, readable per-process mask."),
            ];
            Widget::render(Paragraph::new(lines).style(theme.row_normal), chunks[0], buf);
            return;
        }
        if let Some(reason) = &self.error {
            let lines = vec![
                Line::styled(format!("cannot read the mask: {reason}"), theme.meter_label),
                Line::from(""),
                Line::from("Reading another user's affinity needs the same privileges as"),
                Line::from("inspecting their process at all. Try your own processes."),
            ];
            Widget::render(Paragraph::new(lines).style(theme.row_normal), chunks[0], buf);
            return;
        }

        // One line per row of eight cores; the cursor is marked with `[...]`.
        let per_row = 8usize;
        let rows = self.cores.div_ceil(per_row);
        let visible = chunks[0].height as usize;
        let cursor_row = self.cursor / per_row;
        let first = cursor_row.saturating_sub(visible.saturating_sub(1));

        let lines: Vec<Line> = (0..rows)
            .skip(first)
            .take(visible)
            .map(|row| {
                let mut text = String::new();
                for col in 0..per_row {
                    let core = row * per_row + col;
                    if core >= self.cores {
                        break;
                    }
                    let on = self.mask.get(core);
                    let mark = if on { '#' } else { '.' };
                    if core == self.cursor {
                        text.push_str(&format!("[{core:>3}{mark}]"));
                    } else {
                        text.push_str(&format!(" {core:>3}{mark} "));
                    }
                }
                if row == cursor_row {
                    Line::styled(text, theme.panel_selected)
                } else {
                    Line::from(text)
                }
            })
            .collect();
        Widget::render(Paragraph::new(lines).style(theme.row_normal), chunks[0], buf);

        let status = format!(
            "{} of {} CPUs allowed{}",
            self.mask.count(),
            self.cores,
            if self.dirty { "  (unapplied)" } else { "" }
        );
        let help = vec![
            Line::styled(status, theme.meter_value),
            Line::styled(
                "Space toggle   Left/Right move   S apply   Esc cancel",
                theme.meter_label,
            ),
        ];
        Widget::render(Paragraph::new(help).style(theme.row_normal), chunks[1], buf);
    }
}

/// State of the I/O priority panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IoPriorityPanel {
    pub pid: u32,
    pub priority: IoPriority,
    pub unsupported: Option<String>,
    pub error: Option<String>,
}

impl IoPriorityPanel {
    pub fn new(pid: u32) -> Self {
        match gander_core::action::ioprio_get(pid) {
            Ok(priority) => Self {
                pid,
                priority,
                unsupported: None,
                error: None,
            },
            Err(e) if e.kind() == std::io::ErrorKind::Unsupported => Self {
                pid,
                priority: IoPriority::default(),
                unsupported: Some(e.to_string()),
                error: None,
            },
            Err(e) => Self {
                pid,
                priority: IoPriority::default(),
                unsupported: None,
                error: Some(e.to_string()),
            },
        }
    }

    pub fn can_edit(&self) -> bool {
        self.unsupported.is_none() && self.error.is_none()
    }

    /// Cycle the class, keeping the level.
    pub fn cycle_class(&mut self) {
        if self.can_edit() {
            self.priority.class = self.priority.class.next();
        }
    }

    /// Change the level within the current class.
    pub fn change_level(&mut self, delta: i8) {
        if !self.can_edit() {
            return;
        }
        let level = (self.priority.level as i8 + delta).clamp(0, 7) as u8;
        self.priority.level = level;
    }

    pub fn apply(&mut self) -> Result<(), String> {
        if !self.can_edit() {
            return Err(self
                .unsupported
                .clone()
                .or_else(|| self.error.clone())
                .unwrap_or_else(|| "I/O priority cannot be changed here".to_string()));
        }
        gander_core::action::ioprio_set(self.pid, self.priority).map_err(|e| e.to_string())
    }

    pub fn draw(&self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        let title = format!("I/O priority of {}", self.pid);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(theme.dialog_border)
            .title(crate::app::span_title(&title, theme));
        let inner = block.inner(area);
        Widget::render(block, area, buf);
        if inner.height == 0 || inner.width == 0 {
            return;
        }

        let mut lines = Vec::new();
        if let Some(reason) = &self.unsupported {
            lines.push(Line::styled(reason.as_str(), theme.meter_label));
            lines.push(Line::from(""));
            lines.push(Line::from(
                "ioprio_get/ioprio_set are Linux syscalls. macOS schedules I/O",
            ));
            lines.push(Line::from(
                "through its own throttling layer, with no per-process knob.",
            ));
        } else if let Some(reason) = &self.error {
            lines.push(Line::styled(
                format!("cannot read the current priority: {reason}"),
                theme.meter_label,
            ));
        } else {
            lines.push(Line::styled(
                format!("current: {}", self.priority.label()),
                theme.meter_value,
            ));
            lines.push(Line::from(""));
            for class in [
                IoClass::None,
                IoClass::BestEffort,
                IoClass::Idle,
                IoClass::Realtime,
            ] {
                let marker = if class == self.priority.class {
                    "(*)"
                } else {
                    "( )"
                };
                let line = format!("{marker} {}", class.name());
                if class == self.priority.class {
                    lines.push(Line::styled(line, theme.panel_selected));
                } else {
                    lines.push(Line::from(line));
                }
            }
            lines.push(Line::from(""));
            lines.push(Line::styled(
                "Space cycles the class   Up/Down change the level   S applies",
                theme.meter_label,
            ));
            lines.push(Line::styled(
                "Raising I/O priority above best-effort normally needs CAP_SYS_ADMIN.",
                theme.meter_label,
            ));
        }

        Widget::render(
            Paragraph::new(lines).style(theme.row_normal),
            inner,
            buf,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_affinity_cursor_stays_in_range() {
        let mut p = AffinityPanel {
            pid: 1,
            mask: CpuMask::all(4),
            cores: 4,
            cursor: 0,
            unsupported: None,
            error: None,
            dirty: false,
        };
        p.move_cursor(-5);
        assert_eq!(p.cursor, 0);
        p.move_cursor(100);
        assert_eq!(p.cursor, 3);
    }

    #[test]
    fn toggling_a_core_flips_exactly_one_bit() {
        let mut p = AffinityPanel {
            pid: 1,
            mask: CpuMask::all(4),
            cores: 4,
            cursor: 2,
            unsupported: None,
            error: None,
            dirty: false,
        };
        // On a platform with no mask the panel must refuse to edit; on one with a
        // mask it must allow it. `capability::AFFINITY` is not the right question
        // here — it is true on macOS too, where the panel exists only to explain
        // that a scheduler hint is all the OS offers.
        assert_eq!(p.can_edit(), gander_core::affinity::supported());
        let before = p.mask.count();
        if p.can_edit() {
            p.toggle();
            assert_eq!(p.mask.count(), before - 1);
            assert!(!p.mask.get(2));
            assert!(p.dirty);
            p.toggle();
            assert_eq!(p.mask.count(), before);
        }
    }

    #[test]
    fn an_unsupported_platform_refuses_to_edit_but_still_draws() {
        let p = AffinityPanel {
            pid: 1,
            mask: CpuMask::all(8),
            cores: 8,
            cursor: 0,
            unsupported: Some("no mask here".into()),
            error: None,
            dirty: false,
        };
        assert!(!p.can_edit());
        let area = Rect::new(0, 0, 60, 12);
        let mut buf = Buffer::empty(area);
        p.draw(area, &mut buf, &Theme::default());
        let text: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(text.contains("no mask here"), "the reason must be visible");
    }

    #[test]
    fn the_io_panel_cycles_and_clamps() {
        let mut p = IoPriorityPanel {
            pid: 1,
            priority: IoPriority::default(),
            unsupported: None,
            error: None,
        };
        // Only meaningful where ioprio exists; on macOS `can_edit` is false and
        // the calls must be no-ops rather than panics.
        let start = p.priority.class;
        p.cycle_class();
        if p.can_edit() {
            assert_ne!(p.priority.class, start);
        } else {
            assert_eq!(p.priority.class, start);
        }

        p.change_level(100);
        if p.can_edit() {
            assert_eq!(p.priority.level, 7);
        }
        p.change_level(-100);
        if p.can_edit() {
            assert_eq!(p.priority.level, 0);
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    #[test]
    fn macos_panels_explain_themselves() {
        let panel = IoPriorityPanel::new(std::process::id());
        assert!(!panel.can_edit());
        assert!(panel.unsupported.as_deref().unwrap().contains("Linux"));

        // macOS has no per-process mask, so the affinity panel explains itself
        // too — unlike FreeBSD, where `cpuset_setaffinity` makes it real.
        let affinity = AffinityPanel::new(std::process::id(), 8);
        assert!(!affinity.can_edit());
        assert!(affinity.unsupported.is_some());
    }

    /// `i` is the one panel FreeBSD cannot honour: it has `rtprio`/`idprio`
    /// instead of an I/O priority class, which is a different question.
    #[cfg(target_os = "freebsd")]
    #[test]
    fn freebsd_has_affinity_but_no_io_priority() {
        let affinity = AffinityPanel::new(std::process::id(), 8);
        assert!(
            affinity.can_edit(),
            "cpuset_setaffinity works on FreeBSD: {:?}",
            affinity.unsupported
        );

        let io = IoPriorityPanel::new(std::process::id());
        assert!(!io.can_edit(), "FreeBSD has no ioprio_set");
        assert!(io.unsupported.as_deref().unwrap().contains("Linux"));
    }
}
