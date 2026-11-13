use std::io::{self, IsTerminal};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use ratatui::Frame;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Color;
use ratatui::symbols::Marker;
use ratatui::text::Line as TextLine;
use ratatui::widgets::canvas::{Canvas, Context, Line as CanvasLine};
use ratatui::widgets::{Block, Paragraph};

use clab_core::view::{Projection, Vec2, Vec3, ViewFrame};

use super::ProjectedBounds;

const BOUNDS_PADDING: f64 = 0.1;
const DEFAULT_PLAYBACK_FPS: u16 = 10;
const MIN_PLAYBACK_FPS: u16 = 1;
const MAX_PLAYBACK_FPS: u16 = 100;
const PLAYBACK_FPS_STEP: u16 = 10;

pub(crate) fn run_viewer(frames: Vec<ViewFrame>) -> Result<()> {
    if frames.is_empty() {
        bail!("solution has no time steps to view");
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("--view needs an interactive terminal");
    }
    let mut app = ViewerApp::new(frames);
    let mut terminal = ratatui::try_init()?;
    let result = run_app(&mut terminal, &mut app);
    ratatui::try_restore()?;
    result?;
    Ok(())
}

struct ViewerApp {
    frames: Vec<ViewFrame>,
    frame_index: usize,
    projection: Projection,
    viewports: [ProjectedBounds; 4],
    playing: bool,
    playback_fps: u16,
    loop_mode: LoopMode,
    playback_direction: PlaybackDirection,
    show_labels: bool,
    should_quit: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LoopMode {
    Restart,
    Bounce,
    Once,
}

impl LoopMode {
    fn name(self) -> &'static str {
        match self {
            Self::Restart => "restart",
            Self::Bounce => "bounce",
            Self::Once => "once",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlaybackDirection {
    Forward,
    Backward,
}

const PROJECTIONS: [Projection; 4] = [
    Projection::Xy,
    Projection::Xz,
    Projection::Yz,
    Projection::Isometric,
];

fn viewport_index(projection: Projection) -> usize {
    PROJECTIONS
        .iter()
        .position(|candidate| *candidate == projection)
        .expect("PROJECTIONS lists every projection")
}

fn bounds_for_frames(frames: &[ViewFrame], projection: Projection) -> ProjectedBounds {
    ProjectedBounds::for_frames(frames, projection).padded(BOUNDS_PADDING)
}

impl ViewerApp {
    fn new(frames: Vec<ViewFrame>) -> Self {
        let viewports = PROJECTIONS.map(|projection| bounds_for_frames(&frames, projection));
        Self {
            playing: frames.len() > 1,
            frames,
            frame_index: 0,
            projection: Projection::Xz,
            viewports,
            playback_fps: DEFAULT_PLAYBACK_FPS,
            loop_mode: LoopMode::Restart,
            playback_direction: PlaybackDirection::Forward,
            show_labels: false,
            should_quit: false,
        }
    }

    fn render(&self, frame: &mut Frame<'_>) {
        let [canvas_area, status_area] =
            Layout::vertical([Constraint::Min(3), Constraint::Length(3)]).areas(frame.area());

        frame.render_widget(self.canvas(canvas_area), canvas_area);
        frame.render_widget(self.status(), status_area);
    }

    fn canvas(&self, area: Rect) -> impl ratatui::widgets::Widget + '_ {
        let scene = self.current_frame();
        let bounds = self.bounds(area);
        let projection = self.projection;
        let show_labels = self.show_labels;

        Canvas::default()
            .block(Block::bordered().title(" clab "))
            .marker(Marker::Braille)
            .x_bounds([bounds.min()[0], bounds.max()[0]])
            .y_bounds([bounds.min()[1], bounds.max()[1]])
            .paint(move |context| {
                for body in &scene.bodies {
                    for [start, end] in &body.segments {
                        draw_segment(context, projection, *start, *end);
                    }
                    if show_labels {
                        print_label(context, projection.project(body.origin), &body.label);
                    }
                }
                for joint in &scene.joints {
                    draw_segment(context, projection, joint.i, joint.j);
                    if show_labels {
                        let middle =
                            std::array::from_fn(|axis| (joint.i[axis] + joint.j[axis]) / 2.0);
                        print_label(context, projection.project(middle), &joint.label);
                    }
                }
                draw_axis_triad(context, bounds, projection);
            })
    }

    fn status(&self) -> Paragraph<'_> {
        let status = format!(
            " {}/{}  t={:.3}  {}  {} fps  loop={}",
            self.frame_index + 1,
            self.frames.len(),
            self.current_frame().time,
            if self.playing { "playing" } else { "paused" },
            self.playback_fps,
            self.loop_mode.name(),
        );
        Paragraph::new(vec![
            TextLine::from(status),
            TextLine::from(" Space pause   ←/→ step   +/- speed   l loop   v labels"),
            TextLine::from(" f/F fit       1–4 view   q quit"),
        ])
    }

    fn bounds(&self, area: Rect) -> ProjectedBounds {
        self.viewports[viewport_index(self.projection)].fit_aspect(
            area.width.saturating_sub(2).saturating_mul(2),
            area.height.saturating_sub(2).saturating_mul(4),
        )
    }

    fn current_frame(&self) -> &ViewFrame {
        &self.frames[self.frame_index]
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Char('f') => self.fit(false),
            KeyCode::Char('F') => self.fit(true),
            KeyCode::Char('v') => self.show_labels = !self.show_labels,
            KeyCode::Char(' ') if self.frames.len() > 1 => {
                if !self.playing
                    && self.loop_mode == LoopMode::Once
                    && self.frame_index + 1 == self.frames.len()
                {
                    self.frame_index = 0;
                    self.playback_direction = PlaybackDirection::Forward;
                }
                self.playing = !self.playing;
            }
            KeyCode::Left => {
                self.playing = false;
                self.frame_index = self
                    .frame_index
                    .checked_sub(1)
                    .unwrap_or(self.frames.len() - 1);
            }
            KeyCode::Right => {
                self.playing = false;
                self.frame_index = (self.frame_index + 1).min(self.frames.len() - 1);
                self.playback_direction = PlaybackDirection::Forward;
            }
            KeyCode::Char('1') => self.projection = Projection::Xy,
            KeyCode::Char('2') => self.projection = Projection::Xz,
            KeyCode::Char('3') => self.projection = Projection::Yz,
            KeyCode::Char('4') => self.projection = Projection::Isometric,
            KeyCode::Char('+' | '=') | KeyCode::Up => {
                self.playback_fps = (self.playback_fps + PLAYBACK_FPS_STEP).min(MAX_PLAYBACK_FPS);
            }
            KeyCode::Char('-') | KeyCode::Down => {
                self.playback_fps = self
                    .playback_fps
                    .saturating_sub(PLAYBACK_FPS_STEP)
                    .max(MIN_PLAYBACK_FPS);
            }
            KeyCode::Char('l') => {
                self.loop_mode = match self.loop_mode {
                    LoopMode::Restart => LoopMode::Bounce,
                    LoopMode::Bounce => LoopMode::Once,
                    LoopMode::Once => LoopMode::Restart,
                };
                self.playback_direction = PlaybackDirection::Forward;
            }
            _ => {}
        }
    }

    fn tick(&mut self) {
        if !self.playing {
            return;
        }

        match self.playback_direction {
            PlaybackDirection::Forward if self.frame_index + 1 < self.frames.len() => {
                self.frame_index += 1;
            }
            PlaybackDirection::Forward => match self.loop_mode {
                LoopMode::Restart => self.frame_index = 0,
                LoopMode::Bounce if self.frames.len() > 1 => {
                    self.playback_direction = PlaybackDirection::Backward;
                    self.frame_index -= 1;
                }
                LoopMode::Bounce | LoopMode::Once => self.playing = false,
            },
            PlaybackDirection::Backward if self.frame_index > 0 => self.frame_index -= 1,
            PlaybackDirection::Backward => {
                self.playback_direction = PlaybackDirection::Forward;
                if self.frames.len() > 1 {
                    self.frame_index = 1;
                }
            }
        }
    }

    fn playback_interval(&self) -> Duration {
        Duration::from_secs_f64(1.0 / f64::from(self.playback_fps))
    }

    fn fit(&mut self, all_frames: bool) {
        let frames = if all_frames {
            &self.frames[..]
        } else {
            std::slice::from_ref(&self.frames[self.frame_index])
        };
        let bounds = bounds_for_frames(frames, self.projection);
        self.viewports[viewport_index(self.projection)] = bounds;
    }
}

fn run_app(terminal: &mut ratatui::DefaultTerminal, app: &mut ViewerApp) -> io::Result<()> {
    let mut last_tick = Instant::now();

    while !app.should_quit {
        terminal.draw(|frame| app.render(frame))?;

        let interval = app.playback_interval();
        if event::poll(interval.saturating_sub(last_tick.elapsed()))?
            && let Event::Key(key) = event::read()?
        {
            app.handle_key(key);
        }
        if last_tick.elapsed() >= interval {
            app.tick();
            last_tick = Instant::now();
        }
    }
    Ok(())
}

fn draw_segment(context: &mut Context<'_>, projection: Projection, start: Vec3, end: Vec3) {
    draw_projected_line(context, projection.project(start), projection.project(end));
}

fn draw_projected_line(context: &mut Context<'_>, [x1, y1]: Vec2, [x2, y2]: Vec2) {
    context.draw(&CanvasLine {
        x1,
        y1,
        x2,
        y2,
        color: Color::Reset,
    });
}

fn draw_axis_triad(context: &mut Context<'_>, bounds: ProjectedBounds, projection: Projection) {
    let length = bounds.width().min(bounds.height()) * 0.08;
    let origin = [
        bounds.min()[0] + bounds.width() * 0.12,
        bounds.min()[1] + bounds.height() * 0.12,
    ];
    for (axis, label) in [
        ([1.0, 0.0, 0.0], "x"),
        ([0.0, 1.0, 0.0], "y"),
        ([0.0, 0.0, 1.0], "z"),
    ] {
        let [dx, dy] = projection.project(axis);
        let norm = dx.hypot(dy);
        if norm == 0.0 {
            continue;
        }
        let end = [
            origin[0] + dx / norm * length,
            origin[1] + dy / norm * length,
        ];
        draw_projected_line(context, origin, end);
        context.print(end[0], end[1], label.to_owned());
    }
}

fn print_label(context: &mut Context<'_>, [x, y]: Vec2, label: &str) {
    context.print(x, y, label.to_owned());
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::KeyModifiers;

    use super::*;

    fn frame(x: f64) -> ViewFrame {
        ViewFrame {
            time: x,
            bodies: vec![clab_core::view::SceneBody {
                label: "Link".to_owned(),
                origin: [x, 0.0, 0.0],
                segments: vec![[[x, 0.0, 0.0], [x, 0.0, 1.0]]],
            }],
            joints: vec![],
        }
    }

    fn press(app: &mut ViewerApp, key: KeyCode) {
        app.handle_key(KeyEvent::new(key, KeyModifiers::NONE));
    }

    #[test]
    fn handles_projection_speed_step_and_quit_keys() {
        let mut app = ViewerApp::new(vec![frame(0.0), frame(1.0)]);
        assert_eq!(app.projection, Projection::Xz);
        press(&mut app, KeyCode::Char('4'));
        assert_eq!(app.projection, Projection::Isometric);
        press(&mut app, KeyCode::Char('+'));
        assert_eq!(app.playback_fps, DEFAULT_PLAYBACK_FPS + PLAYBACK_FPS_STEP);
        press(&mut app, KeyCode::Right);
        assert_eq!((app.frame_index, app.playing), (1, false));
        press(&mut app, KeyCode::Esc);
        assert!(app.should_quit);
    }

    #[test]
    fn bounces_at_both_ends() {
        let mut app = ViewerApp::new(vec![frame(0.0), frame(1.0), frame(2.0)]);
        app.loop_mode = LoopMode::Bounce;
        let indices: Vec<_> = (0..5)
            .map(|_| {
                app.tick();
                app.frame_index
            })
            .collect();
        assert_eq!(indices, [1, 2, 1, 0, 1]);
    }

    #[test]
    fn once_mode_stops_and_restarts() {
        let mut app = ViewerApp::new(vec![frame(0.0), frame(1.0)]);
        app.loop_mode = LoopMode::Once;
        app.tick();
        app.tick();
        assert_eq!((app.frame_index, app.playing), (1, false));
        press(&mut app, KeyCode::Char(' '));
        assert_eq!((app.frame_index, app.playing), (0, true));
    }

    #[test]
    fn renders_canvas_and_status() {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let app = ViewerApp::new(vec![frame(0.0)]);
        terminal.draw(|f| app.render(f)).unwrap();
        let rendered: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(rendered.contains("clab") && rendered.contains("q quit"));
        assert!(
            rendered
                .chars()
                .any(|c| ('\u{2800}'..='\u{28ff}').contains(&c))
        );
    }
}
