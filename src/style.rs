//! Colours, text sizes and the few custom widgets the app is built from.
//!
//! Everything is big and labelled in words: this app is meant for people who are nervous around
//! computers, often running it in a dark theatre.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame, Margin, Response,
    RichText, Sense, Stroke, Ui, Vec2, pos2, vec2,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct Palette {
    pub bg: Color32,
    pub panel: Color32,
    pub card: Color32,
    pub card_hi: Color32,
    pub line: Color32,
    pub line_hi: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub wave: Color32,
    pub accent: Color32,
    pub go: Color32,
    pub go_hover: Color32,
    pub go_ink: Color32,
    pub warn: Color32,
    pub warn_ink: Color32,
    pub stop: Color32,
    pub stop_hover: Color32,
    pub stop_ink: Color32,
    pub music: Color32,
    pub good: Color32,
}

const DARK: Palette = Palette {
    bg: Color32::from_rgb(12, 20, 26),
    panel: Color32::from_rgb(17, 28, 35),
    card: Color32::from_rgb(23, 38, 47),
    card_hi: Color32::from_rgb(31, 50, 61),
    line: Color32::from_rgb(42, 64, 75),
    line_hi: Color32::from_rgb(70, 100, 114),
    text: Color32::from_rgb(234, 242, 245),
    muted: Color32::from_rgb(160, 182, 191),
    faint: Color32::from_rgb(104, 128, 138),
    wave: Color32::from_rgb(66, 98, 110),
    accent: Color32::from_rgb(45, 212, 191),
    go: Color32::from_rgb(34, 197, 94),
    go_hover: Color32::from_rgb(74, 222, 128),
    go_ink: Color32::from_rgb(3, 34, 16),
    warn: Color32::from_rgb(251, 191, 36),
    warn_ink: Color32::from_rgb(46, 31, 0),
    stop: Color32::from_rgb(239, 68, 68),
    stop_hover: Color32::from_rgb(248, 113, 113),
    stop_ink: Color32::WHITE,
    music: Color32::from_rgb(167, 139, 250),
    good: Color32::from_rgb(74, 222, 128),
};

const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(244, 247, 249),
    panel: Color32::from_rgb(233, 239, 243),
    card: Color32::WHITE,
    card_hi: Color32::from_rgb(226, 244, 241),
    line: Color32::from_rgb(200, 212, 219),
    line_hi: Color32::from_rgb(140, 160, 170),
    text: Color32::from_rgb(17, 29, 36),
    muted: Color32::from_rgb(72, 92, 102),
    faint: Color32::from_rgb(120, 138, 147),
    wave: Color32::from_rgb(170, 190, 199),
    accent: Color32::from_rgb(13, 148, 136),
    go: Color32::from_rgb(22, 163, 74),
    go_hover: Color32::from_rgb(34, 197, 94),
    go_ink: Color32::WHITE,
    warn: Color32::from_rgb(217, 119, 6),
    warn_ink: Color32::WHITE,
    stop: Color32::from_rgb(220, 38, 38),
    stop_hover: Color32::from_rgb(239, 68, 68),
    stop_ink: Color32::WHITE,
    music: Color32::from_rgb(124, 58, 237),
    good: Color32::from_rgb(22, 163, 74),
};

static LIGHT_MODE: AtomicBool = AtomicBool::new(false);

/// The colours in use right now.
pub fn p() -> &'static Palette {
    if LIGHT_MODE.load(Ordering::Relaxed) { &LIGHT } else { &DARK }
}

pub fn bold() -> FontFamily {
    FontFamily::Name("bold".into())
}

pub fn strong(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text).family(bold()).size(size).color(p().text)
}

pub fn muted(text: impl Into<String>) -> RichText {
    RichText::new(text).color(p().muted)
}

/// Small capitals over a section, e.g. NEXT SOUND.
pub fn caps(text: &str, color: Color32) -> RichText {
    RichText::new(text.to_uppercase()).family(bold()).size(12.0).color(color)
}

/// A muted explanation under a control.
pub fn hint(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(14.0).color(p().muted));
}

/// Seconds as 1:05.
pub fn mmss(secs: f32) -> String {
    let s = secs.max(0.0) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

/// A length in words: "2.3 seconds", "1 minute 13 seconds".
pub fn length(secs: f32) -> String {
    if secs < 60.0 {
        format!("{secs:.1} seconds")
    } else {
        let s = secs.round() as u32;
        let (m, s) = (s / 60, s % 60);
        let mins = if m == 1 { "1 minute".to_string() } else { format!("{m} minutes") };
        if s == 0 { mins } else { format!("{mins} {s} seconds") }
    }
}

/// A rounded panel to group things in.
pub fn card(fill: Color32, stroke: Color32) -> Frame {
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, stroke))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(14))
}

/// A big painted button with a title and an optional line under it. It fills `size`.
pub fn big_button(
    ui: &mut Ui,
    title: &str,
    sub: Option<&str>,
    (fill, hover, ink): (Color32, Color32, Color32),
    size: Vec2,
    title_size: f32,
    enabled: bool,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let fill = if !enabled {
        p().card
    } else if response.is_pointer_button_down_on() {
        fill.gamma_multiply(0.85)
    } else if response.hovered() {
        hover
    } else {
        fill
    };
    let ink = if enabled { ink } else { p().faint };
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(14),
        fill,
        Stroke::new(1.0, if enabled { fill } else { p().line }),
        egui::StrokeKind::Inside,
    );
    // Title and subtitle are centred as one block.
    const SUB: f32 = 15.0;
    let block = if sub.is_some() { title_size + 6.0 + SUB } else { title_size };
    let title_pos = rect.center() - vec2(0.0, block / 2.0 - title_size / 2.0);
    painter.text(title_pos, Align2::CENTER_CENTER, title, FontId::new(title_size, bold()), ink);
    if let Some(sub) = sub {
        let sub_pos = rect.center() + vec2(0.0, block / 2.0 - SUB / 2.0);
        painter.text(sub_pos, Align2::CENTER_CENTER, sub, FontId::proportional(SUB), ink.gamma_multiply(0.85));
    }
    if enabled { response.on_hover_cursor(egui::CursorIcon::PointingHand) } else { response }
}

/// An ordinary button, a bit larger than egui's, with an optional colour.
pub fn button(ui: &mut Ui, text: &str, color: Option<Color32>) -> Response {
    let mut label = RichText::new(text).size(16.0);
    if let Some(c) = color {
        label = label.color(c);
    }
    ui.add(egui::Button::new(label).min_size(vec2(0.0, 34.0)).corner_radius(CornerRadius::same(8)))
}

/// A filled button for the main action in a place.
pub fn primary(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let pal = p();
    let label = RichText::new(text).size(17.0).family(bold()).color(if enabled { pal.go_ink } else { pal.faint });
    let b = egui::Button::new(label)
        .fill(if enabled { pal.go } else { pal.card })
        .min_size(vec2(0.0, 42.0))
        .corner_radius(CornerRadius::same(10));
    ui.add_enabled(enabled, b)
}

pub fn danger(ui: &mut Ui, text: &str) -> Response {
    let pal = p();
    let b = egui::Button::new(RichText::new(text).size(16.0).color(pal.stop))
        .stroke(Stroke::new(1.0, pal.stop.gamma_multiply(0.6)))
        .min_size(vec2(0.0, 34.0))
        .corner_radius(CornerRadius::same(8));
    ui.add(b)
}

/// A thin progress bar.
pub fn bar(ui: &mut Ui, frac: f32, color: Color32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let radius = CornerRadius::same((height / 2.0) as u8);
    ui.painter().rect_filled(rect, radius, p().card_hi);
    let mut done = rect;
    done.set_width(rect.width() * frac.clamp(0.0, 1.0));
    ui.painter().rect_filled(done, radius, color);
}

/// A sound's outline, with the part that plays picked out and a playhead if it's playing.
pub fn waveform(ui: &mut Ui, peaks: &[f32], size: Vec2, plays: (f32, f32), playhead: Option<f32>) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(6), p().panel);
    let n = peaks.len().max(1) as f32;
    let bar_w = rect.width() / n;
    let half = rect.height() / 2.0 - 3.0;
    for (k, &peak) in peaks.iter().enumerate() {
        let at = (k as f32 + 0.5) / n;
        let x = rect.left() + at * rect.width();
        let h = (peak * half).max(0.75);
        let inside = at >= plays.0 && at <= plays.1;
        let played = playhead.is_some_and(|ph| at <= ph) && inside;
        let color = if played {
            p().accent
        } else if inside {
            p().line_hi
        } else {
            p().wave.gamma_multiply(0.45)
        };
        painter.line_segment(
            [pos2(x, rect.center().y - h), pos2(x, rect.center().y + h)],
            Stroke::new((bar_w * 0.62).max(1.0), color),
        );
    }
    if let Some(ph) = playhead {
        let x = rect.left() + ph.clamp(0.0, 1.0) * rect.width();
        painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(2.0, p().text));
    }
    response
}

/// A colour for each speaker's name, the same every time.
pub fn who_color(who: &str) -> Color32 {
    const COLORS: [Color32; 8] = [
        Color32::from_rgb(96, 165, 250),
        Color32::from_rgb(244, 114, 182),
        Color32::from_rgb(45, 212, 191),
        Color32::from_rgb(251, 146, 60),
        Color32::from_rgb(192, 132, 252),
        Color32::from_rgb(250, 204, 21),
        Color32::from_rgb(163, 230, 53),
        Color32::from_rgb(248, 113, 113),
    ];
    let key = who.trim().to_lowercase();
    if key.is_empty() {
        return p().muted;
    }
    let hash = key.bytes().fold(5381u32, |h, b| h.wrapping_mul(33) ^ b as u32);
    let c = COLORS[hash as usize % COLORS.len()];
    if LIGHT_MODE.load(Ordering::Relaxed) {
        // Darker versions read better on white.
        Color32::from_rgb((c.r() as f32 * 0.62) as u8, (c.g() as f32 * 0.62) as u8, (c.b() as f32 * 0.62) as u8)
    } else {
        c
    }
}

/// Text sizes, spacing and colours for the whole window.
pub fn apply(ctx: &egui::Context, light: bool, zoom: f32) {
    LIGHT_MODE.store(light, Ordering::Relaxed);
    ctx.set_theme(if light { egui::ThemePreference::Light } else { egui::ThemePreference::Dark });
    ctx.set_zoom_factor(zoom);
    let pal = p();
    ctx.all_styles_mut(|style| {
        use egui::TextStyle as T;
        style.text_styles = [
            (T::Small, FontId::proportional(13.0)),
            (T::Body, FontId::proportional(16.0)),
            (T::Button, FontId::proportional(16.0)),
            (T::Heading, FontId::new(24.0, bold())),
            (T::Monospace, FontId::monospace(14.0)),
        ]
        .into();
        style.spacing.item_spacing = vec2(8.0, 7.0);
        style.spacing.button_padding = vec2(12.0, 5.0);
        style.spacing.interact_size = vec2(40.0, 30.0);
        style.spacing.slider_width = 200.0;
        style.spacing.text_edit_width = 320.0;
        style.spacing.combo_width = 260.0;
        style.spacing.icon_width = 20.0;
        style.spacing.icon_width_inner = 12.0;
        // Labels must not swallow clicks meant for the cards they sit in.
        style.interaction.selectable_labels = false;
        style.interaction.tooltip_delay = 0.25;

        let v = &mut style.visuals;
        *v = if light { egui::Visuals::light() } else { egui::Visuals::dark() };
        v.panel_fill = pal.bg;
        v.window_fill = pal.panel;
        v.window_stroke = Stroke::new(1.0, pal.line);
        v.window_corner_radius = CornerRadius::same(14);
        v.extreme_bg_color = if light { Color32::WHITE } else { Color32::from_rgb(8, 14, 18) };
        v.faint_bg_color = pal.panel;
        v.selection.bg_fill = pal.accent.gamma_multiply(0.45);
        v.selection.stroke = Stroke::new(1.0, pal.accent);
        v.hyperlink_color = pal.accent;
        v.slider_trailing_fill = true;
        v.widgets.noninteractive.fg_stroke.color = pal.text;
        v.widgets.noninteractive.bg_stroke.color = pal.line;
        for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.corner_radius = CornerRadius::same(8);
        }
        // Slider rails and tick boxes use bg_fill, buttons use weak_bg_fill: rails must stand out.
        v.widgets.inactive.bg_fill = pal.line;
        v.widgets.inactive.weak_bg_fill = pal.card;
        v.widgets.inactive.fg_stroke.color = pal.text;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, pal.line);
        v.widgets.hovered.bg_fill = pal.card_hi;
        v.widgets.hovered.weak_bg_fill = pal.card_hi;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, pal.line_hi);
        v.widgets.active.bg_fill = pal.card_hi;
        v.widgets.active.weak_bg_fill = pal.card_hi;
    });
}

/// Use the system's own UI font (and its bold) when there is one, so names in any alphabet show
/// up; egui's built-in fonts fill in symbols and emoji.
pub fn load_fonts(ctx: &egui::Context) {
    const PAIRS: [(&str, &str); 6] = [
        (r"C:\Windows\Fonts\segoeui.ttf", r"C:\Windows\Fonts\segoeuib.ttf"),
        ("/System/Library/Fonts/Supplemental/Arial.ttf", "/System/Library/Fonts/Supplemental/Arial Bold.ttf"),
        ("/Library/Fonts/Arial.ttf", "/Library/Fonts/Arial Bold.ttf"),
        ("/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf", "/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf"),
        ("/usr/share/fonts/noto/NotoSans-Regular.ttf", "/usr/share/fonts/noto/NotoSans-Bold.ttf"),
        ("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"),
    ];
    let mut fonts = FontDefinitions::default();
    let mut bold_stack = fonts.families[&FontFamily::Proportional].clone();
    if let Some((regular, bold_font)) =
        PAIRS.iter().find_map(|(r, b)| std::fs::read(r).ok().map(|reg| (reg, std::fs::read(b).ok())))
    {
        fonts.font_data.insert("ui".into(), Arc::new(FontData::from_owned(regular)));
        if let Some(stack) = fonts.families.get_mut(&FontFamily::Proportional) {
            stack.insert(0, "ui".into());
        }
        bold_stack.insert(0, "ui".into());
        if let Some(bold_font) = bold_font {
            fonts.font_data.insert("ui-bold".into(), Arc::new(FontData::from_owned(bold_font)));
            bold_stack.insert(0, "ui-bold".into());
        }
    }
    fonts.families.insert(bold(), bold_stack);
    ctx.set_fonts(fonts);
}

/// The app's logo, drawn rather than loaded: a teal rounded square with a play arrow and sound waves.
pub fn logo(ui: &mut Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let s = size / 512.0;
    let at = |x: f32, y: f32| rect.min + vec2(x * s, y * s);
    painter.rect_filled(
        egui::Rect::from_min_max(at(24.0, 24.0), at(488.0, 488.0)),
        CornerRadius::same((104.0 * s) as u8),
        Color32::from_rgb(20, 170, 160),
    );
    let tri = vec![at(118.0, 160.0), at(118.0, 352.0), at(272.0, 256.0)];
    painter.add(egui::Shape::convex_polygon(tri, Color32::WHITE, Stroke::new(26.0 * s, Color32::WHITE)));
    for (r, w) in [(86.0, 30.0), (146.0, 30.0)] {
        let pts: Vec<egui::Pos2> = (0..=16)
            .map(|k| {
                let a = (-42.0 + 84.0 * k as f32 / 16.0f32).to_radians();
                at(256.0 + r * a.cos(), 256.0 + r * a.sin())
            })
            .collect();
        painter.add(egui::Shape::line(pts, Stroke::new(w * s, Color32::WHITE)));
    }
}
