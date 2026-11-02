//! 宠物的运动逻辑：沿着各显示器工作区（任务栏上沿 / 屏幕底边）行走、睡觉、被拖拽后落地、
//! 从屏幕边缘走出去或走进来。所有坐标都是物理像素（与显示器 DPI 无关），速度按所在显示器的
//! 缩放比例换算，所以在 100% 和 200% 缩放的屏幕上看起来一样快。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
}

/// 一块显示器：工作区（去掉任务栏 / Dock 的区域）和缩放比例。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Screen {
    pub work: Rect,
    pub scale: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub fn opposite(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }
}

/// 宠物动作。前端按它切换精灵动画。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anim {
    Idle,
    Walk,
    Sleep,
    /// 开心（任务完成）。
    Happy,
    /// 举手（等我回复 / 批准）。
    RaiseHand,
    /// 着急（出错）。
    Anxious,
    /// 看日历（日程提醒）。
    Calendar,
    Drag,
    Fall,
}

#[derive(Debug, Clone, PartialEq)]
enum Mode {
    Idle { until: i64 },
    Walk { target: f64, until: i64 },
    Sleep { until: i64 },
    React { anim: Anim, until: i64 },
    Drag,
    Fall { vy: f64 },
    Exit { side: Side },
    Enter { target: f64 },
    Hidden,
}

/// 每帧输出。
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// 窗口左上角（物理像素）。
    pub x: i32,
    pub y: i32,
    pub anim: Anim,
    pub facing: Side,
    pub visible: bool,
    pub event: Option<BrainEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrainEvent {
    /// 已完全走出这一侧的屏幕边缘（应当交接给邻居机器）。
    Exited(Side),
    /// 走进来已完成。
    Entered,
}

/// 运动参数。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Tuning {
    /// 行走速度（逻辑像素 / 秒）。
    pub walk_speed: f64,
    /// 宠物精灵在窗口里的水平位置（相对窗口宽度的比例：左、右）。
    pub body_left: f64,
    pub body_right: f64,
    /// 自己去串门的概率（每次决定去散步时），0 表示只在被拖过去或手动要求时串门。
    pub wander_exit_chance: f64,
    /// 待机多久后开始犯困（毫秒）。
    pub sleep_after_ms: i64,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning {
            walk_speed: 45.0,
            body_left: 0.30,
            body_right: 0.70,
            wander_exit_chance: 0.0,
            sleep_after_ms: 5 * 60_000,
        }
    }
}

/// 宠物的大脑。
#[derive(Debug, Clone)]
pub struct PetBrain {
    pub x: f64,
    pub y: f64,
    pub facing: Side,
    mode: Mode,
    rng: u64,
    last_active_ms: i64,
    pub tuning: Tuning,
}

fn xorshift(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

/// 在若干显示器上找到 `cx` 所在的那一块。
fn screen_at(screens: &[Screen], cx: f64) -> Option<&Screen> {
    screens
        .iter()
        .find(|s| cx >= s.work.x as f64 && cx < s.work.right() as f64)
}

/// 所有显示器拼起来的左右边界。
pub fn desktop_bounds(screens: &[Screen]) -> (i32, i32) {
    let left = screens.iter().map(|s| s.work.x).min().unwrap_or(0);
    let right = screens.iter().map(|s| s.work.right()).max().unwrap_or(1920);
    (left, right)
}

impl PetBrain {
    pub fn new(seed: u64) -> Self {
        PetBrain {
            x: 0.0,
            y: 0.0,
            facing: Side::Right,
            mode: Mode::Idle { until: 0 },
            rng: seed | 1,
            last_active_ms: 0,
            tuning: Tuning::default(),
        }
    }

    fn rand01(&mut self) -> f64 {
        (xorshift(&mut self.rng) % 10_000) as f64 / 10_000.0
    }

    fn rand_range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (xorshift(&mut self.rng) % ((hi - lo).max(1) as u64)) as i64
    }

    /// 窗口中宠物身体的中心横坐标。
    fn body_center(&self, win_w: f64) -> f64 {
        self.x + win_w * (self.tuning.body_left + self.tuning.body_right) / 2.0
    }

    /// 某个横坐标下的地面：窗口应放的 y（窗口底边贴着工作区底边）。
    fn ground_y(&self, screens: &[Screen], cx: f64, win_h: f64) -> f64 {
        let s = screen_at(screens, cx).or_else(|| {
            // 不在任何显示器上（比如两块屏之间的缝）：用最近的一块。
            screens.iter().min_by_key(|s| {
                let mid = s.work.x as f64 + s.work.w as f64 / 2.0;
                (mid - cx).abs() as i64
            })
        });
        match s {
            Some(s) => s.work.bottom() as f64 - win_h,
            None => 0.0,
        }
    }

    fn scale_at(&self, screens: &[Screen], cx: f64) -> f64 {
        screen_at(screens, cx).map(|s| s.scale).unwrap_or(1.0)
    }

    /// 放到第一块显示器（或指定横坐标）的地面上。
    pub fn place(&mut self, screens: &[Screen], cx: Option<f64>, win_w: f64, win_h: f64) {
        let target = cx.unwrap_or_else(|| {
            let s = screens.first().map(|s| s.work).unwrap_or(Rect {
                x: 0,
                y: 0,
                w: 1920,
                h: 1080,
            });
            s.right() as f64 - win_w * 1.5
        });
        self.x = target - win_w / 2.0;
        self.y = self.ground_y(screens, target, win_h);
        self.mode = Mode::Idle { until: 0 };
    }

    pub fn current_anim(&self) -> Anim {
        match &self.mode {
            Mode::Idle { .. } => Anim::Idle,
            Mode::Walk { .. } | Mode::Exit { .. } | Mode::Enter { .. } => Anim::Walk,
            Mode::Sleep { .. } => Anim::Sleep,
            Mode::React { anim, .. } => *anim,
            Mode::Drag => Anim::Drag,
            Mode::Fall { .. } => Anim::Fall,
            Mode::Hidden => Anim::Idle,
        }
    }

    pub fn is_hidden(&self) -> bool {
        matches!(self.mode, Mode::Hidden)
    }

    pub fn is_busy_moving(&self) -> bool {
        matches!(
            self.mode,
            Mode::Drag | Mode::Fall { .. } | Mode::Exit { .. } | Mode::Enter { .. } | Mode::Hidden
        )
    }

    /// 做出一个反应动作（开心 / 举手 / 着急 / 看日历），持续 `duration_ms`。
    /// 拖拽、交接途中不打断。
    pub fn react(&mut self, anim: Anim, now: i64, duration_ms: i64) {
        if self.is_busy_moving() {
            return;
        }
        self.last_active_ms = now;
        self.mode = Mode::React {
            anim,
            until: now + duration_ms,
        };
    }

    /// 结束当前的反应动作（例如等待状态解除了）。
    pub fn clear_reaction(&mut self, anim: Anim, now: i64) {
        if let Mode::React { anim: a, .. } = self.mode {
            if a == anim {
                self.mode = Mode::Idle { until: now + 2_000 };
            }
        }
    }

    /// 被点了一下：醒过来。
    pub fn poke(&mut self, now: i64) {
        self.last_active_ms = now;
        if matches!(self.mode, Mode::Sleep { .. }) {
            self.mode = Mode::Idle { until: now + 3_000 };
        }
    }

    pub fn start_drag(&mut self, now: i64) {
        if self.is_hidden() {
            return;
        }
        self.last_active_ms = now;
        self.mode = Mode::Drag;
    }

    /// 拖拽中：窗口左上角跟随鼠标。
    pub fn drag_to(&mut self, x: f64, y: f64) {
        if matches!(self.mode, Mode::Drag) {
            self.x = x;
            self.y = y;
        }
    }

    /// 松手。如果拖到了允许串门的边缘之外，直接走出去；否则落地。
    pub fn end_drag(&mut self, screens: &[Screen], win_w: f64, allow_exit: impl Fn(Side) -> bool) {
        if !matches!(self.mode, Mode::Drag) {
            return;
        }
        let (left, right) = desktop_bounds(screens);
        let bl = self.x + win_w * self.tuning.body_left;
        let br = self.x + win_w * self.tuning.body_right;
        if bl < left as f64 + 4.0 && allow_exit(Side::Left) {
            self.facing = Side::Left;
            self.mode = Mode::Exit { side: Side::Left };
            return;
        }
        if br > right as f64 - 4.0 && allow_exit(Side::Right) {
            self.facing = Side::Right;
            self.mode = Mode::Exit { side: Side::Right };
            return;
        }
        self.clamp_inside(screens, win_w);
        self.mode = Mode::Fall { vy: 0.0 };
    }

    fn clamp_inside(&mut self, screens: &[Screen], win_w: f64) {
        let (left, right) = desktop_bounds(screens);
        let min_x = left as f64 - win_w * self.tuning.body_left;
        let max_x = right as f64 - win_w * self.tuning.body_right;
        self.x = self.x.clamp(min_x, max_x.max(min_x));
    }

    /// 要求宠物从某一侧走出去（串门 / 回家）。
    pub fn request_exit(&mut self, side: Side) {
        if matches!(self.mode, Mode::Drag | Mode::Hidden | Mode::Exit { .. }) {
            return;
        }
        self.facing = side;
        self.mode = Mode::Exit { side };
    }

    /// 从某一侧走进来（来访 / 回家）。
    pub fn enter_from(&mut self, side: Side, screens: &[Screen], win_w: f64, win_h: f64) {
        let (left, right) = desktop_bounds(screens);
        let (start_x, target) = match side {
            Side::Left => (left as f64 - win_w, left as f64 + win_w * 0.8),
            Side::Right => (right as f64, right as f64 - win_w * 1.8),
        };
        self.x = start_x;
        let edge_cx = match side {
            Side::Left => left as f64 + 1.0,
            Side::Right => right as f64 - 1.0,
        };
        self.y = self.ground_y(screens, edge_cx, win_h);
        self.facing = side.opposite();
        self.mode = Mode::Enter { target };
    }

    /// 立即隐藏（交接给别的机器后）。
    pub fn hide(&mut self) {
        self.mode = Mode::Hidden;
    }

    /// 从隐藏状态直接出现在地面上（例如交接超时后收回）。
    pub fn reappear(&mut self, screens: &[Screen], win_w: f64, win_h: f64) {
        self.place(screens, None, win_w, win_h);
    }

    fn pick_next(
        &mut self,
        now: i64,
        screens: &[Screen],
        win_w: f64,
        allow_exit: &dyn Fn(Side) -> bool,
    ) {
        let idle_for = now - self.last_active_ms;
        let r = self.rand01();
        if idle_for > self.tuning.sleep_after_ms && r < 0.5 {
            let d = self.rand_range(30_000, 120_000);
            self.mode = Mode::Sleep { until: now + d };
            return;
        }
        if r < 0.55 {
            let (left, right) = desktop_bounds(screens);
            let cx = self.body_center(win_w);
            // 偶尔自己去串门。
            if self.tuning.wander_exit_chance > 0.0
                && self.rand01() < self.tuning.wander_exit_chance
            {
                let side = if self.rand01() < 0.5 {
                    Side::Left
                } else {
                    Side::Right
                };
                if allow_exit(side) {
                    self.request_exit(side);
                    return;
                }
            }
            // 在当前显示器附近随便走走。
            let span = (self.rand_range(80, 400) as f64) * self.scale_at(screens, cx);
            let dir = if self.rand01() < 0.5 { -1.0 } else { 1.0 };
            let margin = win_w * 0.5;
            let target = (cx + dir * span).clamp(left as f64 + margin, right as f64 - margin);
            self.mode = Mode::Walk {
                target,
                until: now + 30_000,
            };
        } else {
            let d = self.rand_range(2_500, 8_000);
            self.mode = Mode::Idle { until: now + d };
        }
    }

    /// 推进一帧。
    pub fn tick(
        &mut self,
        now: i64,
        dt_ms: i64,
        screens: &[Screen],
        win_w: f64,
        win_h: f64,
        allow_exit: &dyn Fn(Side) -> bool,
    ) -> Frame {
        let dt = (dt_ms.clamp(0, 200)) as f64 / 1000.0;
        if self.last_active_ms == 0 {
            self.last_active_ms = now;
        }
        let mut event = None;
        let cx = self.body_center(win_w);
        let scale = self.scale_at(screens, cx);
        let speed = self.tuning.walk_speed * scale;
        let mode = self.mode.clone();
        match mode {
            Mode::Idle { until } => {
                if now >= until {
                    self.pick_next(now, screens, win_w, allow_exit);
                }
                self.y = self.ground_y(screens, cx, win_h);
            }
            Mode::Sleep { until } => {
                if now >= until {
                    self.last_active_ms = now;
                    self.mode = Mode::Idle { until: now + 2_000 };
                }
                self.y = self.ground_y(screens, cx, win_h);
            }
            Mode::React { until, .. } => {
                if now >= until {
                    self.mode = Mode::Idle { until: now + 2_000 };
                }
                self.y = self.ground_y(screens, cx, win_h);
            }
            Mode::Walk { target, until } => {
                let dir = if target > cx { 1.0 } else { -1.0 };
                self.facing = if dir > 0.0 { Side::Right } else { Side::Left };
                let step = speed * dt;
                if (target - cx).abs() <= step || now >= until {
                    self.x += target - cx;
                    let d = self.rand_range(2_000, 6_000);
                    self.mode = Mode::Idle { until: now + d };
                } else {
                    self.x += dir * step;
                }
                let ncx = self.body_center(win_w);
                // 走到两块屏之间的缝或桌面边缘：转身。
                if screen_at(screens, ncx).is_none() {
                    self.x -= dir * step;
                    self.mode = Mode::Idle { until: now + 1_500 };
                }
                self.y = self.ground_y(screens, self.body_center(win_w), win_h);
            }
            Mode::Exit { side } => {
                let (left, right) = desktop_bounds(screens);
                let dir = if side == Side::Left { -1.0 } else { 1.0 };
                self.facing = side;
                self.x += dir * speed * dt;
                // 地面高度按边缘那块屏计算，走出去时不会掉下来。
                let edge_cx = if side == Side::Left {
                    left as f64 + 1.0
                } else {
                    right as f64 - 1.0
                };
                self.y = self.ground_y(screens, edge_cx, win_h);
                let gone = match side {
                    Side::Left => self.x + win_w * self.tuning.body_right <= left as f64,
                    Side::Right => self.x + win_w * self.tuning.body_left >= right as f64,
                };
                if gone {
                    self.mode = Mode::Hidden;
                    event = Some(BrainEvent::Exited(side));
                }
            }
            Mode::Enter { target } => {
                let dir = if target > cx { 1.0 } else { -1.0 };
                self.facing = if dir > 0.0 { Side::Right } else { Side::Left };
                let step = speed * dt;
                if (target - cx).abs() <= step {
                    self.x += target - cx;
                    self.mode = Mode::Idle { until: now + 3_000 };
                    self.last_active_ms = now;
                    event = Some(BrainEvent::Entered);
                } else {
                    self.x += dir * step;
                }
                let (left, right) = desktop_bounds(screens);
                let probe = self
                    .body_center(win_w)
                    .clamp(left as f64 + 1.0, right as f64 - 1.0);
                self.y = self.ground_y(screens, probe, win_h);
            }
            Mode::Fall { vy } => {
                let g = 2400.0 * scale;
                let nvy = vy + g * dt;
                self.y += nvy * dt;
                let ground = self.ground_y(screens, cx, win_h);
                if self.y >= ground {
                    self.y = ground;
                    self.mode = Mode::Idle { until: now + 1_500 };
                } else {
                    self.mode = Mode::Fall { vy: nvy };
                }
            }
            Mode::Drag | Mode::Hidden => {}
        }
        Frame {
            x: self.x.round() as i32,
            y: self.y.round() as i32,
            anim: self.current_anim(),
            facing: self.facing,
            visible: !self.is_hidden(),
            event,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f64 = 200.0;
    const H: f64 = 200.0;

    fn two_screens() -> Vec<Screen> {
        vec![
            // 主屏 1920x1080，任务栏 48 像素。
            Screen {
                work: Rect {
                    x: 0,
                    y: 0,
                    w: 1920,
                    h: 1032,
                },
                scale: 1.0,
            },
            // 右边一块 4K 200% 缩放，位置更低。
            Screen {
                work: Rect {
                    x: 1920,
                    y: 200,
                    w: 3840,
                    h: 2100,
                },
                scale: 2.0,
            },
        ]
    }

    fn run(
        b: &mut PetBrain,
        screens: &[Screen],
        ms: i64,
        allow: &dyn Fn(Side) -> bool,
    ) -> Vec<Frame> {
        let mut frames = Vec::new();
        let mut now = 1_000_000;
        for _ in 0..(ms / 33) {
            now += 33;
            frames.push(b.tick(now, 33, screens, W, H, allow));
        }
        frames
    }

    #[test]
    fn stands_on_taskbar() {
        let s = two_screens();
        let mut b = PetBrain::new(7);
        b.place(&s, Some(500.0), W, H);
        let f = b.tick(1, 33, &s, W, H, &|_| false);
        assert_eq!(f.y, 1032 - 200);
        // 放到第二块屏上，地面随之改变。
        b.place(&s, Some(3000.0), W, H);
        let f = b.tick(2, 33, &s, W, H, &|_| false);
        assert_eq!(f.y, 2300 - 200);
    }

    #[test]
    fn never_leaves_without_permission() {
        let s = two_screens();
        let mut b = PetBrain::new(42);
        b.tuning.wander_exit_chance = 1.0;
        b.place(&s, Some(100.0), W, H);
        let frames = run(&mut b, &s, 120_000, &|_| false);
        let (left, right) = desktop_bounds(&s);
        for f in &frames {
            assert!(f.visible);
            let cx = f.x as f64 + W / 2.0;
            assert!(cx >= left as f64 && cx <= right as f64, "{cx}");
            assert!(f.event.is_none());
        }
        // 确实走动过。
        assert!(frames.iter().any(|f| f.anim == Anim::Walk));
    }

    #[test]
    fn exits_when_requested_and_allowed() {
        let s = two_screens();
        let mut b = PetBrain::new(3);
        b.place(&s, Some(300.0), W, H);
        b.request_exit(Side::Left);
        let frames = run(&mut b, &s, 20_000, &|side| side == Side::Left);
        let exit = frames
            .iter()
            .position(|f| f.event == Some(BrainEvent::Exited(Side::Left)));
        assert!(exit.is_some());
        assert!(!frames.last().unwrap().visible);
        assert!(b.is_hidden());
    }

    #[test]
    fn drag_past_edge_exits_only_if_allowed() {
        let s = two_screens();
        let mut b = PetBrain::new(5);
        b.place(&s, Some(1000.0), W, H);
        b.start_drag(0);
        b.drag_to(5000.0, 300.0);
        b.end_drag(&s, W, |_| false);
        // 不允许：夹回屏幕内并下落到第二块屏的地面。
        let frames = run(&mut b, &s, 3_000, &|_| false);
        let last = frames.last().unwrap();
        assert!(last.visible);
        assert_eq!(last.y, 2300 - 200);
        assert!(last.x as f64 + W * 0.7 <= 5760.0 + 0.5);

        b.start_drag(0);
        b.drag_to(5700.0, 300.0);
        b.end_drag(&s, W, |side| side == Side::Right);
        let frames = run(&mut b, &s, 5_000, &|side| side == Side::Right);
        assert!(frames
            .iter()
            .any(|f| f.event == Some(BrainEvent::Exited(Side::Right))));
    }

    #[test]
    fn enter_then_idle() {
        let s = two_screens();
        let mut b = PetBrain::new(9);
        b.hide();
        b.enter_from(Side::Right, &s, W, H);
        assert_eq!(b.facing, Side::Left);
        let frames = run(&mut b, &s, 20_000, &|_| false);
        assert!(frames.iter().any(|f| f.event == Some(BrainEvent::Entered)));
        let entered = frames
            .iter()
            .position(|f| f.event == Some(BrainEvent::Entered))
            .unwrap();
        assert!(frames[entered].visible);
        // 走进来的地方在第二块屏的地面上。
        assert_eq!(frames[entered].y, 2300 - 200);
    }

    #[test]
    fn reactions_and_sleep() {
        let s = two_screens();
        let mut b = PetBrain::new(11);
        b.place(&s, Some(500.0), W, H);
        b.react(Anim::Happy, 0, 1_000);
        assert_eq!(b.current_anim(), Anim::Happy);
        b.tick(2_000, 33, &s, W, H, &|_| false);
        assert_eq!(b.current_anim(), Anim::Idle);
        b.react(Anim::RaiseHand, 3_000, 60_000);
        b.clear_reaction(Anim::RaiseHand, 3_500);
        assert_eq!(b.current_anim(), Anim::Idle);
        // 拖拽中不被打断。
        b.start_drag(4_000);
        b.react(Anim::Anxious, 4_000, 5_000);
        assert_eq!(b.current_anim(), Anim::Drag);
        // 长时间没动静会睡着。
        let mut b = PetBrain::new(13);
        b.tuning.sleep_after_ms = 1_000;
        b.place(&s, Some(500.0), W, H);
        let frames = run(&mut b, &s, 120_000, &|_| false);
        assert!(frames.iter().any(|f| f.anim == Anim::Sleep));
    }
}
