mod paste;
mod store;
mod terminal;
mod text_input;

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use gpui::{
    App, Application, Bounds, ClickEvent, ClipboardItem, Context, Entity, FocusHandle, Focusable, FontWeight,
    Hsla, KeyBinding, PathPromptOptions, ScrollStrategy, SharedString, Subscription,
    TitlebarOptions, UniformListScrollHandle, Window, WindowBounds, WindowOptions, actions, div,
    prelude::*, px, rgb, rgba, size, uniform_list,
};

use store::{Directory, Store};
use text_input::{InputColors, TextInput};

const BG: u32 = 0x0F1011;
const SURFACE: u32 = 0x151618;
const BORDER: u32 = 0x23252A;
const TEXT: u32 = 0xF7F8F8;
const MUTED: u32 = 0x8A8F98;
const SUBTLE: u32 = 0x62666D;
const ACCENT: u32 = 0x5E6AD2;
const SELECTED: u32 = 0x1C1D21;
const HOVER: u32 = 0x18191B;
const DANGER: u32 = 0xEB5757;
const FONT: &str = "Inter";
const ROW_HEIGHT: f32 = 44.;

actions!(
    directory_saver,
    [
        SelectPrev, SelectNext, Confirm, OpenTerminal, CopyPath, Cancel, AddFolder, RemoveSelected,
        DialogConfirm, DialogCancel, Quit,
    ]
);

const CONTEXT: &str = "DirectorySaver";
const DIALOG_CONTEXT: &str = "AddDialog";

enum Notice {
    Error(SharedString),
    Info(SharedString),
}

#[derive(Clone, Copy, PartialEq)]
enum OpenWith {
    Files,
    Terminal,
}

#[derive(Clone, Copy, PartialEq)]
enum RowAction {
    Open(OpenWith),
    CopyPath,
    Remove,
}

const ROW_ACTIONS: [(RowAction, &str); 4] = [
    (RowAction::Open(OpenWith::Terminal), "Terminal"),
    (RowAction::Open(OpenWith::Files), "Files"),
    (RowAction::CopyPath, "Copy path"),
    (RowAction::Remove, "Remove"),
];

enum PathCheck {
    Empty,
    Ready(PathBuf),
    Invalid(&'static str),
}

fn check_path(raw: &str, home: Option<&Path>) -> PathCheck {
    if raw.trim().is_empty() {
        return PathCheck::Empty;
    }
    match paste::parse_pasted_path(raw, home) {
        None => PathCheck::Invalid("Not an absolute path"),
        Some(path) if path.is_dir() => PathCheck::Ready(path),
        Some(path) if path.exists() => PathCheck::Invalid("Not a folder"),
        Some(_) => PathCheck::Invalid("Folder does not exist"),
    }
}

/// What the dialog's validation line says.
enum DialogStatus {
    Check(PathCheck),
    /// A submit or the folder picker failed; stays until the field changes.
    Error(SharedString),
}

struct AddDialog {
    input: Entity<TextInput>,
    status: DialogStatus,
    _changed: Subscription,
}

/// Which row should be selected once results are reloaded.
enum Keep {
    First,
    Id(i64),
    Index,
}

struct DirectorySaver {
    store: Store,
    input: Entity<TextInput>,
    add_dialog: Option<AddDialog>,
    results: Vec<Directory>,
    selected: Option<usize>,
    hovered: Option<usize>,
    hovered_action: Option<(usize, RowAction)>,
    notice: Option<Notice>,
    scroll: UniformListScrollHandle,
    home: Option<PathBuf>,
    _input_changed: Subscription,
}

fn input_colors() -> InputColors {
    InputColors {
        text: rgb(TEXT).into(),
        placeholder: rgb(SUBTLE).into(),
        cursor: rgb(ACCENT).into(),
        selection: rgba(0x5E6AD266).into(),
    }
}

impl DirectorySaver {
    fn new(store: Store, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextInput::new("Search directories…", input_colors(), cx));
        let _input_changed = cx.subscribe(&input, |this, _, _: &text_input::Changed, cx| {
            this.notice = None;
            this.refresh(Keep::First, cx);
        });
        let mut this = Self {
            store,
            input,
            add_dialog: None,
            results: Vec::new(),
            selected: None,
            hovered: None,
            hovered_action: None,
            notice: None,
            scroll: UniformListScrollHandle::new(),
            home: dirs::home_dir(),
            _input_changed,
        };
        this.refresh(Keep::First, cx);
        this
    }

    fn refresh(&mut self, keep: Keep, cx: &mut Context<Self>) {
        match self.store.search(self.input.read(cx).content()) {
            Ok(results) => self.results = results,
            Err(err) => {
                self.results.clear();
                self.notice = Some(Notice::Error(format!("Search failed: {err:#}").into()));
            }
        }
        let last = self.results.len().checked_sub(1);
        self.selected = match keep {
            Keep::First => last.map(|_| 0),
            Keep::Id(id) => self
                .results
                .iter()
                .position(|d| d.id == id)
                .or(last.map(|_| 0)),
            Keep::Index => last.map(|last| self.selected.unwrap_or(0).min(last)),
        };
        if let Some(ix) = self.selected {
            self.scroll.scroll_to_item(ix, ScrollStrategy::Top);
        }
        cx.notify();
    }

    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.notice = None;
        self.selected = Some(ix);
        self.scroll.scroll_to_item(ix, ScrollStrategy::Top);
        cx.notify();
    }

    fn select_prev(&mut self, _: &SelectPrev, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.selected {
            self.select(ix.saturating_sub(1), cx);
        }
    }

    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.selected {
            self.select((ix + 1).min(self.results.len() - 1), cx);
        }
    }

    fn confirm(&mut self, _: &Confirm, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.selected {
            self.open_dir(ix, OpenWith::Files, cx);
        }
    }

    fn open_terminal(&mut self, _: &OpenTerminal, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.selected {
            self.open_dir(ix, OpenWith::Terminal, cx);
        }
    }

    fn copy_selected_path(&mut self, _: &CopyPath, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.selected {
            self.copy_path(ix, cx);
        }
    }

    fn copy_path(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(dir) = self.results.get(ix) else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(dir.path.display().to_string()));
        self.notice = Some(Notice::Info(format!("Copied {}", self.display_path(&dir.path)).into()));
        self.selected = Some(ix);
        cx.notify();
    }

    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        let keep = if self.input.read(cx).content().is_empty() {
            Keep::Index
        } else {
            Keep::First
        };
        self.notice = None;
        self.input.update(cx, |input, cx| input.set_content("", cx));
        self.refresh(keep, cx);
    }

    fn open_dir(&mut self, ix: usize, with: OpenWith, cx: &mut Context<Self>) {
        let Some(dir) = self.results.get(ix).cloned() else {
            return;
        };
        self.selected = Some(ix);
        if !dir.path.is_dir() {
            self.notice = Some(Notice::Error(
                format!("{} no longer exists", self.display_path(&dir.path)).into(),
            ));
            cx.notify();
            return;
        }
        let opened = match with {
            OpenWith::Files => {
                cx.open_with_system(&dir.path);
                format!("Opened {} in Files", dir.name)
            }
            OpenWith::Terminal => match terminal::open(&dir.path) {
                Ok(()) => format!("Opened terminal in {}", dir.name),
                Err(err) => {
                    self.notice = Some(Notice::Error(format!("{err:#}").into()));
                    cx.notify();
                    return;
                }
            },
        };
        self.notice = Some(match self.store.mark_opened(dir.id) {
            Ok(()) => Notice::Info(opened.into()),
            Err(err) => Notice::Error(format!("{opened}, but could not record it: {err:#}").into()),
        });
        self.refresh(Keep::Id(dir.id), cx);
    }

    fn add_folder(&mut self, _: &AddFolder, window: &mut Window, cx: &mut Context<Self>) {
        if self.add_dialog.is_some() {
            return;
        }
        let input = cx.new(|cx| TextInput::new("Paste a folder path…", input_colors(), cx));
        let _changed = cx.subscribe(&input, |this, _, _: &text_input::Changed, cx| {
            this.recheck_dialog(cx);
        });
        window.focus(&input.focus_handle(cx));
        self.notice = None;
        self.add_dialog = Some(AddDialog {
            input,
            status: DialogStatus::Check(PathCheck::Empty),
            _changed,
        });
        cx.notify();
    }

    fn recheck_dialog(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = &mut self.add_dialog else {
            return;
        };
        let check = check_path(dialog.input.read(cx).content(), self.home.as_deref());
        dialog.status = DialogStatus::Check(check);
        cx.notify();
    }

    fn close_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.add_dialog = None;
        window.focus(&self.input.focus_handle(cx));
        cx.notify();
    }

    fn dialog_cancel(&mut self, _: &DialogCancel, window: &mut Window, cx: &mut Context<Self>) {
        self.close_dialog(window, cx);
    }

    fn dialog_confirm(&mut self, _: &DialogConfirm, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = &mut self.add_dialog else {
            return;
        };
        // Checked again here because the folder may have changed since the field was edited.
        let path = match check_path(dialog.input.read(cx).content(), self.home.as_deref()) {
            PathCheck::Empty => return,
            PathCheck::Invalid(reason) => {
                dialog.status = DialogStatus::Error(reason.into());
                cx.notify();
                return;
            }
            PathCheck::Ready(path) => path,
        };
        let added = match self.store.add(&path) {
            Ok(added) => added,
            Err(err) => {
                dialog.status = DialogStatus::Error(format!("{err:#}").into());
                cx.notify();
                return;
            }
        };
        self.close_dialog(window, cx);
        let verb = if added.is_new { "Added" } else { "Already saved" };
        self.notice = Some(Notice::Info(format!("{verb} {}", added.dir.name).into()));
        self.input.update(cx, |input, cx| input.set_content("", cx));
        self.refresh(Keep::Id(added.dir.id), cx);
    }

    fn browse(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn(async move |this, cx| {
            let picked = picked.await.map_err(anyhow::Error::from).and_then(|r| r);
            this.update(cx, |this, cx| this.use_picked(picked, cx)).ok();
        })
        .detach();
    }

    fn use_picked(&mut self, picked: Result<Option<Vec<PathBuf>>>, cx: &mut Context<Self>) {
        let Some(dialog) = &mut self.add_dialog else {
            return;
        };
        match picked {
            Ok(Some(paths)) => {
                let Some(path) = paths.into_iter().next() else {
                    return;
                };
                let text = path.to_string_lossy().into_owned();
                dialog.input.update(cx, |input, cx| input.set_content(text, cx));
                self.recheck_dialog(cx);
            }
            Ok(None) => {}
            Err(err) => {
                dialog.status =
                    DialogStatus::Error(format!("Could not open folder picker: {err:#}").into());
                cx.notify();
            }
        }
    }

    fn remove_selected(&mut self, _: &RemoveSelected, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.selected {
            self.remove_at(ix, cx);
        }
    }

    fn run_row_action(&mut self, ix: usize, action: RowAction, cx: &mut Context<Self>) {
        match action {
            RowAction::Open(with) => self.open_dir(ix, with, cx),
            RowAction::CopyPath => self.copy_path(ix, cx),
            RowAction::Remove => self.remove_at(ix, cx),
        }
    }

    fn remove_at(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(dir) = self.results.get(ix).cloned() else {
            return;
        };
        self.notice = Some(match self.store.remove(dir.id) {
            Ok(()) => Notice::Info(format!("Removed {}", dir.name).into()),
            Err(err) => Notice::Error(format!("Could not remove: {err:#}").into()),
        });
        self.selected = Some(ix);
        self.refresh(Keep::Index, cx);
    }

    fn display_path(&self, path: &Path) -> String {
        match self.home.as_deref().and_then(|home| path.strip_prefix(home).ok()) {
            Some(rest) if rest.as_os_str().is_empty() => "~".into(),
            Some(rest) => format!("~/{}", rest.display()),
            None => path.display().to_string(),
        }
    }

    fn render_row(&self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let dir = &self.results[ix];
        let missing = !dir.path.is_dir();
        let selected = self.selected == Some(ix);
        let show_actions = selected || self.hovered == Some(ix);
        let initial: String = dir.name.chars().next().map_or_else(String::new, |c| c.to_uppercase().collect());
        let meta = match dir.last_opened_at {
            Some(at) => format!("opened {}", relative_time(store::now() - at)),
            None => "never opened".into(),
        };

        div()
            .id(("row", dir.id as u64))
            .w_full()
            .h(px(ROW_HEIGHT))
            .px(px(8.))
            .flex()
            .items_center()
            .child(
                div()
                    .size_full()
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .rounded(px(6.))
                    .relative()
                    .when(selected, |el| el.bg(rgb(SELECTED)))
                    .when(!selected, |el| el.hover(|s| s.bg(rgb(HOVER))))
                    .when(selected, |el| {
                        el.child(
                            div()
                                .absolute()
                                .left_0()
                                .top(px(10.))
                                .bottom(px(10.))
                                .w(px(2.))
                                .rounded(px(1.))
                                .bg(rgb(ACCENT)),
                        )
                    })
                    .child(
                        div()
                            .size(px(24.))
                            .flex_none()
                            .rounded(px(6.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .when(missing, |el| el.bg(rgb(0x1E1F23)).text_color(rgb(SUBTLE)))
                            .when(!missing, |el| {
                                el.bg(rgba(0x5E6AD240)).text_color(rgb(0xB4BAF5))
                            })
                            .child(initial),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(1.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(if missing { MUTED } else { TEXT }))
                                            .child(dir.name.clone()),
                                    )
                                    .when(missing, |el| {
                                        el.child(
                                            div()
                                                .flex_none()
                                                .px(px(5.))
                                                .rounded(px(4.))
                                                .border_1()
                                                .border_color(rgb(BORDER))
                                                .text_size(px(11.))
                                                .line_height(px(16.))
                                                .text_color(rgb(SUBTLE))
                                                .child("missing"),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.))
                                    .text_color(rgb(if missing { SUBTLE } else { MUTED }))
                                    .child(self.display_path(&dir.path)),
                            ),
                    )
                    .when(!show_actions, |el| {
                        el.child(
                            div()
                                .flex_none()
                                .text_size(px(12.))
                                .text_color(rgb(SUBTLE))
                                .child(meta),
                        )
                    })
                    .when(show_actions, |el| {
                        el.child(
                            div()
                                .flex_none()
                                .flex()
                                .items_center()
                                .gap(px(2.))
                                .children(ROW_ACTIONS.map(|(action, label)| {
                                    self.render_row_button(ix, dir.id, action, label, cx)
                                })),
                        )
                    }),
            )
            // Hover is tracked in state rather than with hover styles because it changes
            // layout and text color, which gpui only applies from state.
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.hovered = Some(ix);
                } else if this.hovered == Some(ix) {
                    this.hovered = None;
                }
                cx.notify();
            }))
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                this.select(ix, cx);
                if event.click_count() >= 2 {
                    this.open_dir(ix, OpenWith::Files, cx);
                }
            }))
    }

    fn render_row_button(
        &self,
        ix: usize,
        id: i64,
        action: RowAction,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let hovered = self.hovered_action == Some((ix, action));
        let hover_text = if action == RowAction::Remove { DANGER } else { TEXT };
        div()
            .id((label, id as u64))
            .h(px(24.))
            .px(px(8.))
            .flex()
            .items_center()
            .rounded(px(6.))
            .text_size(px(12.))
            .text_color(rgb(if hovered { hover_text } else { MUTED }))
            .when(hovered, |el| el.bg(rgb(0x26272C)))
            .cursor_pointer()
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.hovered_action = Some((ix, action));
                } else if this.hovered_action == Some((ix, action)) {
                    this.hovered_action = None;
                }
                cx.notify();
            }))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                this.run_row_action(ix, action, cx);
            }))
            .child(label)
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(48.))
            .px(px(16.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().size(px(14.)).rounded(px(4.)).bg(rgb(ACCENT)))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(TEXT))
                            .child("Directory Saver"),
                    ),
            )
            .child(
                button("add-folder")
                    .bg(rgb(ACCENT))
                    .hover(|s| s.bg(rgb(0x6C78E0)))
                    .text_color(rgb(0xFFFFFF))
                    .child("Add folder")
                    .child(kbd("Ctrl N", rgba(0xFFFFFF26), rgba(0xFFFFFFCC)))
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.add_folder(&AddFolder, window, cx)
                    })),
            )
    }

    fn render_search(&self, cx: &App) -> impl IntoElement {
        let show_esc = !self.input.read(cx).content().is_empty();
        div().px(px(16.)).pb(px(12.)).child(
            field()
                .child(search_glyph())
                .child(self.input.clone())
                .when(show_esc, |el| {
                    el.child(kbd("Esc", rgb(0x1E1F23), rgb(SUBTLE)))
                }),
        )
    }

    fn render_dialog(&self, dialog: &AddDialog, cx: &mut Context<Self>) -> impl IntoElement {
        let ready = matches!(dialog.status, DialogStatus::Check(PathCheck::Ready(_)));
        let validation = match &dialog.status {
            DialogStatus::Check(PathCheck::Empty) => None,
            DialogStatus::Check(PathCheck::Ready(path)) => Some(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_color(rgb(MUTED))
                    .child(div().size(px(6.)).flex_none().rounded_full().bg(rgb(ACCENT)))
                    .child(div().truncate().child(self.display_path(path))),
            ),
            DialogStatus::Check(PathCheck::Invalid(reason)) => {
                Some(div().text_color(rgb(SUBTLE)).child(*reason))
            }
            DialogStatus::Error(msg) => {
                Some(div().truncate().text_color(rgb(DANGER)).child(msg.clone()))
            }
        };

        let card = div()
            .id("add-dialog")
            .key_context(DIALOG_CONTEXT)
            .on_action(cx.listener(Self::dialog_confirm))
            .on_action(cx.listener(Self::dialog_cancel))
            .occlude()
            .w(px(440.))
            .p(px(16.))
            .flex()
            .flex_col()
            .rounded(px(8.))
            .bg(rgb(SURFACE))
            .border_1()
            .border_color(rgb(BORDER))
            .shadow_lg()
            .child(
                div()
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(TEXT))
                    .child("Add folder"),
            )
            .child(
                div()
                    .mt(px(2.))
                    .text_size(px(12.))
                    .text_color(rgb(MUTED))
                    .child("Paste a path or browse for a folder."),
            )
            .child(field().mt(px(14.)).bg(rgb(BG)).child(dialog.input.clone()))
            .child(
                div()
                    .h(px(18.))
                    .mt(px(6.))
                    .text_size(px(12.))
                    .children(validation),
            )
            .child(
                div()
                    .mt(px(10.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        dialog_button("browse")
                            .border_1()
                            .border_color(rgb(BORDER))
                            .hover(|s| s.bg(rgb(HOVER)))
                            .cursor_pointer()
                            .text_color(rgb(TEXT))
                            .child("Browse…")
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.browse(cx))),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(
                                dialog_button("cancel")
                                    .hover(|s| s.bg(rgb(HOVER)))
                                    .cursor_pointer()
                                    .text_color(rgb(MUTED))
                                    .child("Cancel")
                                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                        this.close_dialog(window, cx)
                                    })),
                            )
                            .child(
                                dialog_button("add")
                                    .bg(rgb(ACCENT))
                                    .text_color(rgb(0xFFFFFF))
                                    .when(!ready, |el| el.opacity(0.4))
                                    .when(ready, |el| {
                                        el.cursor_pointer()
                                            .hover(|s| s.bg(rgb(0x6C78E0)))
                                            .on_click(cx.listener(
                                                |this, _: &ClickEvent, window, cx| {
                                                    this.dialog_confirm(&DialogConfirm, window, cx)
                                                },
                                            ))
                                    })
                                    .child("Add"),
                            ),
                    ),
            );

        div()
            .id("add-dialog-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .bg(rgba(0x00000099))
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.close_dialog(window, cx)))
            .child(card)
    }
}

fn field() -> gpui::Div {
    div()
        .h(px(36.))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(8.))
        .overflow_hidden()
        .rounded(px(6.))
        .bg(rgb(SURFACE))
        .border_1()
        .border_color(rgb(BORDER))
        .text_size(px(13.))
        .line_height(px(20.))
}

fn button(id: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .h(px(28.))
        .pl(px(10.))
        .pr(px(4.))
        .flex()
        .items_center()
        .gap(px(8.))
        .rounded(px(6.))
        .cursor_pointer()
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
}

fn dialog_button(id: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .h(px(28.))
        .px(px(12.))
        .flex()
        .items_center()
        .rounded(px(6.))
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
}

fn kbd(label: &'static str, bg: impl Into<Hsla>, fg: impl Into<Hsla>) -> impl IntoElement {
    div()
        .h(px(20.))
        .px(px(6.))
        .flex()
        .items_center()
        .rounded(px(4.))
        .bg(bg.into())
        .text_size(px(11.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(fg.into())
        .child(label)
}

fn search_glyph() -> impl IntoElement {
    div()
        .relative()
        .size(px(14.))
        .flex_none()
        .child(
            div()
                .absolute()
                .top(px(1.))
                .left(px(1.))
                .size(px(9.))
                .rounded_full()
                .border(px(1.5))
                .border_color(rgb(SUBTLE)),
        )
        // gpui divs cannot rotate, so the handle is a short staircase of pixels.
        .children((0..3).map(|i| {
            div()
                .absolute()
                .top(px(9. + i as f32 * 1.5))
                .left(px(9. + i as f32 * 1.5))
                .size(px(2.))
                .bg(rgb(SUBTLE))
        }))
}

fn relative_time(seconds: i64) -> String {
    const UNITS: [(i64, &str); 6] = [
        (365 * 86400, "y"),
        (30 * 86400, "mo"),
        (7 * 86400, "w"),
        (86400, "d"),
        (3600, "h"),
        (60, "m"),
    ];
    UNITS
        .iter()
        .find(|(unit, _)| seconds >= *unit)
        .map_or_else(|| "just now".into(), |(unit, suffix)| format!("{}{suffix} ago", seconds / unit))
}

impl Focusable for DirectorySaver {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl Render for DirectorySaver {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = if !self.results.is_empty() {
            uniform_list(
                "results",
                self.results.len(),
                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                    range.map(|ix| this.render_row(ix, cx)).collect::<Vec<_>>()
                }),
            )
            .track_scroll(self.scroll.clone())
            .py(px(4.))
            .flex_1()
            .into_any_element()
        } else {
            let (title, hint) = if self.input.read(cx).content().is_empty() {
                ("No directories yet", "Press Ctrl N to add a folder")
            } else {
                ("No results", "Try a different search, or press Esc to clear it")
            };
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(4.))
                .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).text_color(rgb(MUTED)).child(title))
                .child(div().text_size(px(12.)).text_color(rgb(SUBTLE)).child(hint))
                .into_any_element()
        };

        let (status, status_color): (SharedString, _) = match &self.notice {
            Some(Notice::Error(msg)) => (msg.clone(), rgb(DANGER)),
            Some(Notice::Info(msg)) => (msg.clone(), rgb(MUTED)),
            None if self.results.len() == 1 => ("1 directory".into(), rgb(SUBTLE)),
            None => (format!("{} directories", self.results.len()).into(), rgb(SUBTLE)),
        };

        // The dialog sits outside the DirectorySaver key context so Enter, Esc and
        // Ctrl ⌫ typed into it never reach the list behind it.
        div()
            .size_full()
            .relative()
            .bg(rgb(BG))
            .font_family(FONT)
            .text_size(px(13.))
            .text_color(rgb(TEXT))
            .child(
                div()
                    .key_context(CONTEXT)
                    .on_action(cx.listener(Self::select_prev))
                    .on_action(cx.listener(Self::select_next))
                    .on_action(cx.listener(Self::confirm))
                    .on_action(cx.listener(Self::open_terminal))
                    .on_action(cx.listener(Self::copy_selected_path))
                    .on_action(cx.listener(Self::cancel))
                    .on_action(cx.listener(Self::add_folder))
                    .on_action(cx.listener(Self::remove_selected))
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(self.render_header(cx))
                    .child(self.render_search(cx))
                    .child(div().h(px(1.)).bg(rgb(BORDER)))
                    .child(body)
                    .child(
                        div()
                            .h(px(32.))
                            .px(px(16.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .border_t_1()
                            .border_color(rgb(BORDER))
                            .text_size(px(12.))
                            .child(div().truncate().text_color(status_color).child(status))
                            .child(
                                div()
                                    .flex()
                                    .gap(px(14.))
                                    .text_color(rgb(SUBTLE))
                                    .children(["↑↓ navigate", "↵ open", "Ctrl ↵ terminal", "Ctrl ⌫ remove"]),
                            ),
                    ),
            )
            .children(self.add_dialog.as_ref().map(|dialog| self.render_dialog(dialog, cx)))
    }
}

fn db_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("DIRECTORY_SAVER_DB") {
        return Ok(path.into());
    }
    Ok(dirs::data_dir()
        .context("no data directory for this user")?
        .join("directory-saver/directories.db"))
}

fn main() {
    let store = match db_path().and_then(|path| Store::open(&path)) {
        Ok(store) => store,
        Err(err) => {
            eprintln!("directory-saver: {err:#}");
            std::process::exit(1);
        }
    };

    Application::new().run(move |cx: &mut App| {
        text_input::bind_keys(cx);
        cx.bind_keys([
            KeyBinding::new("up", SelectPrev, Some(CONTEXT)),
            KeyBinding::new("down", SelectNext, Some(CONTEXT)),
            KeyBinding::new("enter", Confirm, Some(CONTEXT)),
            KeyBinding::new("ctrl-enter", OpenTerminal, Some(CONTEXT)),
            KeyBinding::new("ctrl-shift-c", CopyPath, Some(CONTEXT)),
            KeyBinding::new("escape", Cancel, Some(CONTEXT)),
            KeyBinding::new("ctrl-n", AddFolder, Some(CONTEXT)),
            KeyBinding::new("ctrl-backspace", RemoveSelected, Some(CONTEXT)),
            KeyBinding::new("enter", DialogConfirm, Some(DIALOG_CONTEXT)),
            KeyBinding::new("escape", DialogCancel, Some(DIALOG_CONTEXT)),
            KeyBinding::new("ctrl-q", Quit, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());

        let bounds = Bounds::centered(None, size(px(760.), px(520.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Directory Saver".into()),
                        ..Default::default()
                    }),
                    app_id: Some("directory-saver".into()),
                    window_background: gpui::WindowBackgroundAppearance::Opaque,
                    ..Default::default()
                },
                |_, cx| cx.new(|cx| DirectorySaver::new(store, cx)),
            )
            .expect("failed to open window");
        window
            .update(cx, |view, window, cx| {
                window.focus(&view.focus_handle(cx));
                cx.activate(true);
            })
            .ok();
    });
}
