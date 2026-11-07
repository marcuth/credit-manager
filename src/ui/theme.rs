//! Tema visual e componentes próprios, no estilo do PhotoCraft/ArtCraft (tema "Pro Medium").
//!
//! As cores, os raios e a geometria dos widgets seguem os valores publicados em
//! <https://github.com/storytold/photocraft> (licença MIT OR Apache-2.0). Só valores de design
//! foram usados; nenhum código foi copiado. Cores nunca devem ser escritas direto nas telas:
//! use as constantes deste módulo.
//!
//! Os tamanhos do PhotoCraft são compactos (texto de 12 px, botões de 28 px). Aqui todos são
//! multiplicados por [`SCALE`] para ficar mais confortável; ajuste a constante para mudar tudo.

use eframe::egui::{
    self, Color32, CornerRadius, FontId, Margin, Rect, Response, RichText, Sense, Shadow, Stroke,
    StrokeKind, TextStyle, Ui, Vec2, Widget, vec2,
};

/// Multiplicador dos tamanhos originais do PhotoCraft (1.0 = idêntico).
pub const SCALE: f32 = 1.2;

// ---- paleta "Pro Medium"
pub const CHROME: Color32 = Color32::from_rgb(83, 83, 83); // painéis
pub const DOCK: Color32 = Color32::from_rgb(66, 66, 66); // fundo da área de conteúdo
pub const CARD: Color32 = Color32::from_rgb(83, 83, 83);
pub const CARD_BORDER: Color32 = Color32::from_rgb(66, 66, 66);
pub const FIELD: Color32 = Color32::from_rgb(69, 69, 69);
pub const FIELD_BORDER: Color32 = Color32::from_rgb(104, 104, 104);
pub const HOVER: Color32 = Color32::from_rgb(98, 98, 98);
pub const PRESSED: Color32 = Color32::from_rgb(112, 112, 112);
pub const TEXT: Color32 = Color32::from_rgb(238, 238, 238);
pub const TEXT_DIM: Color32 = Color32::from_rgb(212, 212, 212);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(160, 160, 160);
pub const SEPARATOR: Color32 = Color32::from_rgb(62, 62, 62);
pub const ACCENT: Color32 = Color32::from_rgb(55, 142, 240);
pub const DANGER: Color32 = Color32::from_rgb(236, 91, 98);
pub const WARNING: Color32 = Color32::from_rgb(232, 176, 70);
/// Não existe no PhotoCraft; acrescentado para pagamentos e status "pago".
pub const SUCCESS: Color32 = Color32::from_rgb(96, 192, 120);
const SHADOW: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 150);
pub const SCRIM: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 110);

// ---- raios
const RADIUS_SM: u8 = 3;
const RADIUS: u8 = 4;
const RADIUS_LG: u8 = 6;

fn s(v: f32) -> f32 {
    v * SCALE
}

/// Aplica o tema ao egui (cores, formas, espaçamentos e tamanhos de texto).
pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        let v = &mut style.visuals;
        v.panel_fill = DOCK;
        v.window_fill = CARD;
        v.window_stroke = Stroke::new(1.0, CARD_BORDER);
        v.extreme_bg_color = FIELD;
        v.faint_bg_color = CARD;
        v.code_bg_color = FIELD;
        v.override_text_color = Some(TEXT);
        v.hyperlink_color = ACCENT;
        v.warn_fg_color = WARNING;
        v.error_fg_color = DANGER;
        v.window_corner_radius = CornerRadius::same(RADIUS_LG);
        v.menu_corner_radius = CornerRadius::same(RADIUS);
        v.window_shadow = Shadow { offset: [0, 10], blur: 32, spread: 0, color: SHADOW };
        v.popup_shadow = Shadow { offset: [0, 6], blur: 20, spread: 0, color: SHADOW };
        v.selection.bg_fill = ACCENT;
        v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        v.striped = false;

        let rounding = CornerRadius::same(RADIUS_SM);
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = CARD;
        w.noninteractive.weak_bg_fill = CARD;
        w.noninteractive.bg_stroke = Stroke::new(1.0, SEPARATOR);
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_DIM);
        w.inactive.bg_fill = FIELD;
        w.inactive.weak_bg_fill = FIELD;
        w.inactive.bg_stroke = Stroke::new(1.0, FIELD_BORDER);
        w.hovered.bg_fill = HOVER;
        w.hovered.weak_bg_fill = HOVER;
        w.hovered.bg_stroke = Stroke::new(1.0, FIELD_BORDER);
        w.active.bg_fill = PRESSED;
        w.active.weak_bg_fill = PRESSED;
        w.active.bg_stroke = Stroke::new(1.0, ACCENT);
        w.open.bg_fill = HOVER;
        w.open.weak_bg_fill = HOVER;
        w.open.bg_stroke = Stroke::new(1.0, FIELD_BORDER);
        for state in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
            state.corner_radius = rounding;
            state.expansion = 0.0;
        }
        for state in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
            state.fg_stroke = Stroke::new(1.0, TEXT);
        }

        let sp = &mut style.spacing;
        sp.item_spacing = vec2(s(8.0), s(6.0));
        sp.button_padding = vec2(s(10.0), s(4.0));
        sp.interact_size = vec2(s(24.0), s(24.0));
        sp.combo_width = 260.0;
        sp.menu_margin = Margin::same(6);
        sp.window_margin = Margin::same(16);
        sp.icon_width = s(14.0);

        for (ts, size) in [
            (TextStyle::Small, 10.5),
            (TextStyle::Body, 12.0),
            (TextStyle::Button, 12.0),
            (TextStyle::Heading, 15.0),
            (TextStyle::Monospace, 12.0),
        ] {
            let family = if ts == TextStyle::Monospace {
                egui::FontFamily::Monospace
            } else {
                egui::FontFamily::Proportional
            };
            style.text_styles.insert(ts, FontId::new(s(size), family));
        }
    });
}

// ---------------------------------------------------------------- texto

/// Título de seção/diálogo (peso semibold no PhotoCraft; aqui, negrito).
pub fn title(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).size(s(15.0)).strong()
}

pub fn dim(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).color(TEXT_DIM)
}

pub fn faint(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).color(TEXT_FAINT)
}

// ---------------------------------------------------------------- botões

#[derive(Clone, Copy, PartialEq, Eq)]
enum Variant {
    Primary,
    Secondary,
    Danger,
}

/// Botão em pílula: primário (azul), secundário (só contorno) ou perigo (vermelho).
pub struct Btn {
    text: String,
    variant: Variant,
    compact: bool,
    min_width: f32,
}

impl Btn {
    fn new(variant: Variant, text: impl Into<String>) -> Self {
        Self { text: text.into(), variant, compact: false, min_width: 0.0 }
    }
    pub fn primary(text: impl Into<String>) -> Self {
        Self::new(Variant::Primary, text)
    }
    pub fn secondary(text: impl Into<String>) -> Self {
        Self::new(Variant::Secondary, text)
    }
    pub fn danger(text: impl Into<String>) -> Self {
        Self::new(Variant::Danger, text)
    }
    /// Sem o multiplicador [`SCALE`] (tamanho original do PhotoCraft).
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }
    pub fn min_width(mut self, w: f32) -> Self {
        self.min_width = w;
        self
    }
}

impl Widget for Btn {
    fn ui(self, ui: &mut Ui) -> Response {
        let k = if self.compact { 1.0 } else { SCALE };
        let enabled = ui.is_enabled();
        let height = 28.0 * k;
        let text_color = match self.variant {
            Variant::Primary | Variant::Danger => Color32::WHITE,
            Variant::Secondary => TEXT,
        };
        let galley = ui.painter().layout_no_wrap(self.text.clone(), FontId::proportional(13.0 * k), text_color);
        let width = (galley.size().x + 28.0 * k).max(self.min_width);
        let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Button, enabled, self.text.clone())
        });
        if !ui.is_rect_visible(rect) {
            return response;
        }
        let fade = if enabled { 1.0 } else { ui.visuals().disabled_alpha() };
        let (hovered, pressed) = (response.hovered(), response.is_pointer_button_down_on());
        let radius = CornerRadius::same((height / 2.0) as u8);
        let painter = ui.painter();
        match self.variant {
            Variant::Primary | Variant::Danger => {
                let base = if self.variant == Variant::Primary { ACCENT } else { DANGER };
                let factor = if pressed { 0.8 } else if hovered { 0.9 } else { 1.0 };
                painter.rect_filled(rect, radius, base.gamma_multiply(factor * fade));
            }
            Variant::Secondary => {
                if pressed || hovered {
                    painter.rect_filled(rect, radius, HOVER.gamma_multiply(fade));
                }
                let line = if hovered { TEXT } else { TEXT_DIM };
                painter.rect_stroke(rect, radius, Stroke::new(1.5, line.gamma_multiply(fade)), StrokeKind::Inside);
            }
        }
        if response.has_focus() {
            painter.rect_stroke(
                rect.expand(2.0),
                CornerRadius::same((height / 2.0 + 2.0) as u8),
                Stroke::new(2.0, ACCENT),
                StrokeKind::Inside,
            );
        }
        let pos = rect.center() - galley.size() / 2.0;
        painter.galley(pos, galley, text_color.gamma_multiply(fade));
        if enabled {
            response.on_hover_cursor(egui::CursorIcon::PointingHand)
        } else {
            response
        }
    }
}

/// Aba em pílula (seleção entre poucas opções, no lugar de rádios).
pub fn pill_tab(ui: &mut Ui, selected: bool, label: impl Into<String>) -> Response {
    let label = label.into();
    let color = if selected { TEXT } else { TEXT_DIM };
    let galley = ui.painter().layout_no_wrap(label.clone(), FontId::proportional(s(12.5)), color);
    let size = vec2(galley.size().x + s(20.0), s(24.0));
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, selected, label.clone()));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let radius = CornerRadius::same(RADIUS_SM);
        if selected {
            painter.rect_filled(rect, radius, HOVER);
            painter.rect_stroke(rect, radius, Stroke::new(1.0, FIELD_BORDER), StrokeKind::Inside);
        } else if response.hovered() {
            painter.rect_filled(rect, radius, HOVER.gamma_multiply(0.6));
        }
        painter.galley(rect.center() - galley.size() / 2.0, galley, color);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Item do sidebar: largura total, alinhado à esquerda, no tamanho original (sem [`SCALE`]).
pub fn nav_item(ui: &mut Ui, selected: bool, label: impl Into<String>) -> Response {
    let label = label.into();
    let color = if selected { TEXT } else { TEXT_DIM };
    let galley = ui.painter().layout_no_wrap(label.clone(), FontId::proportional(13.0), color);
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, selected, label.clone()));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let radius = CornerRadius::same(RADIUS_SM);
        if selected {
            painter.rect_filled(rect, radius, HOVER);
            painter.rect_stroke(rect, radius, Stroke::new(1.0, FIELD_BORDER), StrokeKind::Inside);
        } else if response.hovered() {
            painter.rect_filled(rect, radius, HOVER.gamma_multiply(0.6));
        }
        painter.galley(egui::pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0), galley, color);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

// ---------------------------------------------------------------- checkbox

/// Caixa de seleção de 14 px com marca branca sobre fundo azul quando ligada.
pub fn checkbox(ui: &mut Ui, on: &mut bool, label: impl Into<String>) -> Response {
    let label = label.into();
    let boxed = s(14.0);
    let gap = 6.0;
    let galley = ui.painter().layout_no_wrap(label.clone(), FontId::proportional(s(12.0)), TEXT_DIM);
    let size = vec2(boxed + gap + galley.size().x, s(24.0));
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let checked = *on;
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, checked, label.clone()));
    if ui.is_rect_visible(rect) {
        let b = Rect::from_min_size(egui::pos2(rect.left(), rect.center().y - boxed / 2.0), Vec2::splat(boxed));
        let pressed = response.is_pointer_button_down_on();
        let painter = ui.painter();
        let radius = CornerRadius::same(2);
        if checked {
            painter.rect_filled(b, radius, if pressed { ACCENT.gamma_multiply(0.75) } else { ACCENT });
            let pts = vec![
                b.left_center() + vec2(s(3.0), 0.5),
                b.center_bottom() + vec2(-1.0, -s(3.5)),
                b.right_top() + vec2(-s(3.0), s(3.5)),
            ];
            painter.add(egui::Shape::line(pts, Stroke::new(b.width() * 0.125, Color32::WHITE)));
        } else {
            painter.rect_filled(b, radius, if pressed { FIELD.lerp_to_gamma(Color32::BLACK, 0.18) } else { FIELD });
            let line = if response.hovered() || pressed { TEXT_DIM } else { TEXT_FAINT };
            painter.rect_stroke(b, radius, Stroke::new(1.5, line), StrokeKind::Inside);
        }
        painter.galley(
            egui::pos2(rect.left() + boxed + gap, rect.center().y - galley.size().y / 2.0),
            galley,
            TEXT_DIM,
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

// ---------------------------------------------------------------- cartão, badge, diálogo

/// Cartão: fundo `CARD`, borda de 1 px e raio grande.
pub fn card<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, CARD_BORDER))
        .corner_radius(CornerRadius::same(RADIUS_LG))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add_contents(ui)
        })
}

/// Moldura dos diálogos modais.
pub fn modal_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, CARD_BORDER))
        .corner_radius(CornerRadius::same(RADIUS_LG))
        .inner_margin(Margin::same(18))
        .shadow(Shadow { offset: [0, 10], blur: 32, spread: 0, color: SHADOW })
}

/// Moldura do conteúdo principal.
pub fn content_frame() -> egui::Frame {
    egui::Frame::new().fill(DOCK).inner_margin(Margin::same(18))
}

/// Moldura do sidebar.
pub fn sidebar_frame() -> egui::Frame {
    egui::Frame::new().fill(CHROME).inner_margin(Margin::same(10))
}

/// Moldura da barra de status.
pub fn status_frame() -> egui::Frame {
    egui::Frame::new().fill(DOCK).inner_margin(Margin::symmetric(14, 6))
}

#[derive(Clone, Copy)]
pub enum Tone {
    Neutral,
    Success,
    Warning,
    Danger,
}

impl Tone {
    fn color(self) -> Color32 {
        match self {
            Tone::Neutral => TEXT_DIM,
            Tone::Success => SUCCESS,
            Tone::Warning => WARNING,
            Tone::Danger => DANGER,
        }
    }
}

/// Selo em pílula para status ("Pago", "Aguardando", "Conectado"...).
pub fn badge(ui: &mut Ui, text: impl Into<String>, tone: Tone) -> Response {
    let c = tone.color();
    egui::Frame::new()
        .fill(Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 48))
        .corner_radius(CornerRadius::same(255))
        .inner_margin(Margin::symmetric(9, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text.into()).size(s(11.5)).color(c).strong());
        })
        .response
}
