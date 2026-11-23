use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita::prelude::*;

use crate::ai::chatgpt::{AccountStatus, ChatGpt, Model, Problem, SettingsState};

type Catalog = (Vec<Model>, Option<String>);

enum Action {
    Load,
    Refresh,
    Resume,
    SignIn(Option<String>),
    Activate(String),
    Disconnect(String),
    Model(String, String),
}

struct Panel {
    group: libadwaita::PreferencesGroup,
    window: gtk4::glib::WeakRef<libadwaita::PreferencesWindow>,
    accounts: libadwaita::ComboRow,
    status: libadwaita::ActionRow,
    models: libadwaita::ComboRow,
    sign_in: gtk4::Button,
    activate: gtk4::Button,
    disconnect: gtk4::Button,
    refresh: gtk4::Button,
    resume: gtk4::Button,
    state: RefCell<Option<SettingsState>>,
    catalog: RefCell<Vec<Model>>,
    busy: Cell<bool>,
    rendering: Cell<bool>,
    local_whisper: Cell<bool>,
}

pub fn install(
    page: &libadwaita::PreferencesPage,
    engine: &libadwaita::ComboRow,
    recognition: &libadwaita::ComboRow,
    window: &libadwaita::PreferencesWindow,
) {
    let group = libadwaita::PreferencesGroup::builder()
        .title("ChatGPT")
        .description("Use your ChatGPT plan to format text transcribed by local Whisper.")
        .build();
    let accounts = libadwaita::ComboRow::builder()
        .title("Account")
        .tooltip_text("Choose a saved ChatGPT account or add another account or workspace")
        .build();
    let status = libadwaita::ActionRow::builder().title("Connection").build();
    let models = libadwaita::ComboRow::builder().title("Model").build();

    let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    buttons.set_margin_start(12);
    buttons.set_margin_end(12);
    buttons.set_margin_top(8);
    buttons.set_margin_bottom(8);
    let sign_in = gtk4::Button::with_label("Continue with ChatGPT");
    sign_in.add_css_class("suggested-action");
    let activate = gtk4::Button::with_label("Use this account");
    let disconnect = gtk4::Button::with_label("Disconnect");
    for button in [&sign_in, &activate, &disconnect] {
        buttons.append(button);
    }
    let account_actions = gtk4::ListBoxRow::builder()
        .child(&buttons)
        .activatable(false)
        .selectable(false)
        .build();

    let usage = libadwaita::ActionRow::builder()
        .title("ChatGPT plan usage")
        .build();
    let manage = gtk4::LinkButton::with_label("https://chatgpt.com/settings/usage", "Manage usage");
    let refresh = gtk4::Button::with_label("Refresh");
    refresh.set_tooltip_text(Some("Refresh connection status and available models"));
    let resume = gtk4::Button::with_label("Resume formatting");
    for button in [
        refresh.upcast_ref::<gtk4::Widget>(),
        resume.upcast_ref(),
        manage.upcast_ref(),
    ] {
        button.set_valign(gtk4::Align::Center);
        usage.add_suffix(button);
    }

    group.add(&accounts);
    group.add(&account_actions);
    group.add(&status);
    group.add(&models);
    group.add(&usage);
    page.add(&group);

    let panel = Rc::new(Panel {
        group,
        window: window.downgrade(),
        accounts,
        status,
        models,
        sign_in,
        activate,
        disconnect,
        refresh,
        resume,
        state: RefCell::new(None),
        catalog: RefCell::new(Vec::new()),
        busy: Cell::new(false),
        rendering: Cell::new(false),
        local_whisper: Cell::new(recognition.selected() == 0),
    });
    connect(&panel);
    let weak = Rc::downgrade(&panel);
    recognition.connect_selected_notify(move |recognition| {
        if let Some(panel) = weak.upgrade() {
            panel.local_whisper.set(recognition.selected() == 0);
            panel.render_buttons();
        }
    });
    panel.group.set_visible(engine.selected() == 2);
    if engine.selected() == 2 {
        panel.start(Action::Load);
    } else {
        panel.render_buttons();
    }
    engine.connect_selected_notify(move |engine| {
        let show = engine.selected() == 2;
        panel.group.set_visible(show);
        if show && panel.state.borrow().is_none() {
            panel.start(Action::Load);
        }
    });
}

fn connect(panel: &Rc<Panel>) {
    let weak = Rc::downgrade(panel);
    panel.accounts.connect_selected_notify(move |_| {
        if let Some(panel) = weak.upgrade() {
            if !panel.rendering.get() {
                panel.render_buttons();
            }
        }
    });
    let weak = Rc::downgrade(panel);
    panel.models.connect_selected_notify(move |row| {
        let Some(panel) = weak.upgrade() else { return };
        if panel.rendering.get() {
            return;
        }
        let active = panel
            .state
            .borrow()
            .as_ref()
            .and_then(|state| state.active.clone());
        let model = panel
            .catalog
            .borrow()
            .get(row.selected() as usize)
            .map(|m| m.slug.clone());
        if let (Some(active), Some(model)) = (active, model) {
            panel.start(Action::Model(active, model));
        }
    });
    let weak = Rc::downgrade(panel);
    panel.sign_in.connect_clicked(move |_| {
        if let Some(panel) = weak.upgrade() {
            panel.start(Action::SignIn(panel.selected()));
        }
    });
    for (button, action) in [
        (&panel.activate, Action::Activate as fn(String) -> Action),
        (
            &panel.disconnect,
            Action::Disconnect as fn(String) -> Action,
        ),
    ] {
        let weak = Rc::downgrade(panel);
        button.connect_clicked(move |_| {
            if let Some(panel) = weak.upgrade() {
                if let Some(id) = panel.selected() {
                    panel.start(action(id));
                }
            }
        });
    }
    let weak = Rc::downgrade(panel);
    panel.refresh.connect_clicked(move |_| {
        if let Some(panel) = weak.upgrade() {
            panel.start(Action::Refresh);
        }
    });
    let weak = Rc::downgrade(panel);
    panel.resume.connect_clicked(move |_| {
        if let Some(panel) = weak.upgrade() {
            panel.start(Action::Resume);
        }
    });
}

impl Panel {
    fn selected(&self) -> Option<String> {
        self.state
            .borrow()
            .as_ref()?
            .accounts
            .get(self.accounts.selected() as usize)
            .map(|account| account.client_id.clone())
    }

    fn render_buttons(&self) {
        let state = self.state.borrow();
        let selected = state
            .as_ref()
            .and_then(|state| state.accounts.get(self.accounts.selected() as usize));
        let active = state.as_ref().and_then(|state| {
            state
                .accounts
                .iter()
                .find(|account| Some(&account.client_id) == state.active.as_ref())
        });
        let selected_active =
            matches!((selected, active), (Some(s), Some(a)) if s.client_id == a.client_id);
        self.accounts.set_subtitle(
            &active
                .map(|account| format!("Active: {}", account.label))
                .unwrap_or_default(),
        );
        let active_status = active.map(|account| account.status);
        let local_whisper = self.local_whisper.get();
        self.status.set_subtitle(if !local_whisper {
            "Select Whisper Local on the Recognition page"
        } else if let Some(account) = selected.filter(|_| !selected_active) {
            account.status.label()
        } else {
            active_status.map_or("Disconnected", AccountStatus::label)
        });
        let ready = !self.busy.get();
        let usable = active_status == Some(AccountStatus::Connected);
        self.accounts.set_sensitive(ready);
        self.sign_in
            .set_visible(selected.is_none_or(|account| !account.status.can_use()));
        self.sign_in.set_sensitive(ready && local_whisper);
        self.activate.set_visible(
            !selected_active && selected.is_some_and(|account| account.status.can_use()),
        );
        self.activate.set_sensitive(ready && local_whisper);
        self.disconnect
            .set_visible(selected.is_some_and(|account| account.has_tokens));
        self.disconnect.set_sensitive(ready);
        self.refresh.set_sensitive(ready);
        self.resume.set_visible(matches!(
            active_status,
            Some(AccountStatus::UsageLimit | AccountStatus::Restricted)
        ));
        self.resume.set_sensitive(ready && local_whisper);
        self.models.set_sensitive(
            ready
                && local_whisper
                && usable
                && selected_active
                && !self.catalog.borrow().is_empty(),
        );
    }

    fn start(self: &Rc<Self>, action: Action) {
        if self.busy.replace(true) {
            return;
        }
        let progress = if matches!(action, Action::SignIn(_)) {
            "Waiting for you to finish in your browser…"
        } else {
            "Checking ChatGPT…"
        };
        let selected = self.selected();
        let switch_selection = matches!(
            action,
            Action::Load | Action::SignIn(_) | Action::Activate(_)
        );
        self.render_buttons();
        self.status.set_subtitle(progress);
        let (send, receive) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let result = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map(|runtime| runtime.block_on(run(action)));
            let _ = send.send_blocking(result);
        });
        let panel = Rc::downgrade(self);
        gtk4::glib::spawn_future_local(async move {
            let result = receive.recv().await;
            let Some(panel) = panel.upgrade() else { return };
            panel.busy.set(false);
            match result {
                Ok(Ok((state, catalog, notice, welcome))) => {
                    if let Some(state) = state {
                        panel.rendering.set(true);
                        panel.refresh_widgets(
                            state,
                            catalog,
                            if switch_selection {
                                None
                            } else {
                                selected.as_deref()
                            },
                        );
                        panel.rendering.set(false);
                    }
                    panel.render_buttons();
                    if let Some(notice) = notice {
                        panel.status.set_subtitle(&notice.to_string());
                    }
                    if welcome {
                        panel.welcome();
                    }
                }
                _ => {
                    panel.render_buttons();
                    panel
                        .status
                        .set_subtitle("Could not check the ChatGPT connection");
                }
            }
        });
    }

    fn refresh_widgets(
        &self,
        state: SettingsState,
        catalog: Option<Catalog>,
        selected: Option<&str>,
    ) {
        let selected = selected.or(state.active.as_deref());
        let index = state
            .accounts
            .iter()
            .position(|account| Some(account.client_id.as_str()) == selected)
            .unwrap_or(state.accounts.len());
        let mut labels: Vec<_> = state
            .accounts
            .iter()
            .map(|account| account.label.as_str())
            .collect();
        labels.push("Add account");
        self.accounts
            .set_model(Some(&gtk4::StringList::new(&labels)));
        self.accounts.set_selected(index as u32);

        if let Some((models, chosen)) = catalog {
            let labels: Vec<_> = models
                .iter()
                .map(|model| model.display_name.as_str())
                .collect();
            self.models.set_model(Some(&gtk4::StringList::new(&labels)));
            let selected = models
                .iter()
                .position(|model| Some(&model.slug) == chosen.as_ref());
            self.models
                .set_selected(selected.map_or(gtk4::INVALID_LIST_POSITION, |n| n as u32));
            self.catalog.replace(models);
        } else if state.active != self.state.borrow().as_ref().and_then(|s| s.active.clone())
            || !state.accounts.iter().any(|a| {
                Some(&a.client_id) == state.active.as_ref() && a.status == AccountStatus::Connected
            })
        {
            self.models.set_model(None::<&gtk4::StringList>);
            self.catalog.borrow_mut().clear();
        } else if let Some(index) = self
            .catalog
            .borrow()
            .iter()
            .position(|m| Some(&m.slug) == state.model.as_ref())
        {
            self.models.set_selected(index as u32);
        }
        self.state.replace(Some(state));
    }

    fn welcome(&self) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let dialog = libadwaita::MessageDialog::new(
            Some(&window),
            Some("You’re using your ChatGPT plan"),
            Some("Koe uses your ChatGPT plan to format text transcribed by local Whisper. You can review or change usage in ChatGPT settings."),
        );
        dialog.add_response("ok", "Got it");
        dialog.set_default_response(Some("ok"));
        dialog.set_close_response("ok");
        dialog.present();
    }
}

async fn run(
    action: Action,
) -> (
    Option<SettingsState>,
    Option<Catalog>,
    Option<Problem>,
    bool,
) {
    let chat = ChatGpt::default();
    let load_models = matches!(
        action,
        Action::Load | Action::Refresh | Action::Resume | Action::SignIn(_) | Action::Activate(_)
    );
    let resume = matches!(action, Action::Resume);
    let result = match action {
        Action::SignIn(id) => {
            chat.sign_in(id.as_deref(), |url| {
                // Use GIO's default browser without logging the authorization URL.
                gtk4::gio::AppInfo::launch_default_for_uri(url, gtk4::gio::AppLaunchContext::NONE)
                    .map_err(|_| Problem::Browser)
            })
            .await
        }
        Action::Activate(id) => chat.activate(&id).await.map(|()| false),
        Action::Disconnect(id) => chat.disconnect(&id).await.map(|()| false),
        Action::Model(id, slug) => chat.set_model(&id, slug).await.map(|()| false),
        _ => Ok(false),
    };
    let welcome = matches!(result, Ok(true));
    let mut notice = result.err();
    let mut state = chat.settings().await;
    let mut catalog = None;
    if load_models
        && notice.is_none()
        && state.as_ref().is_ok_and(|state| {
            state.accounts.iter().any(|a| {
                Some(&a.client_id) == state.active.as_ref()
                    && (a.status == AccountStatus::Connected || (resume && a.status.can_use()))
            })
        })
    {
        match chat.models(resume).await {
            Ok((models, selected)) => {
                if models.is_empty() {
                    notice = Some(Problem::ModelUnavailable);
                }
                catalog = Some((models, selected));
            }
            Err(error) => notice = Some(error),
        }
        state = chat.settings().await;
    }
    if let Err(error) = state {
        notice.get_or_insert(error);
        return (None, None, notice, welcome);
    }
    (state.ok(), catalog, notice, welcome)
}
