use std::cell::RefCell;
use std::os::unix::net::UnixListener;
use std::rc::Rc;
use std::sync::mpsc;

use gtk4::prelude::*;
use gtk4::{glib, Application, ApplicationWindow, TextView, CssProvider, ScrolledWindow};

use crate::ai::AiClient;
use crate::db::Database;

const APP_ID: &str = "com.notepad.NoTePaD";

#[derive(PartialEq, Clone, Copy)]
enum AppState {
    Input,
    ApiKeyInput,
    AiWaiting,
    AiResult,
    Archive,
}

pub fn run(listener: UnixListener) -> Result<(), Box<dyn std::error::Error>> {
    let app = Application::builder()
        .application_id(APP_ID)
        .build();

    let listener = Rc::new(RefCell::new(Some(listener)));

    app.connect_activate(move |app| {
        let listener = listener.borrow_mut().take();
        build_ui(app, listener);
    });

    app.run_with_args::<String>(&[]);
    Ok(())
}

fn build_ui(app: &Application, listener: Option<UnixListener>) {
    // Database
    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("notepad");
    std::fs::create_dir_all(&data_dir).ok();
    let db = Rc::new(Database::new(
        data_dir.join("memos.db").to_str().unwrap(),
    ).expect("Failed to open database"));

    // Window
    let window = ApplicationWindow::builder()
        .application(app)
        .title("NoTePaD")
        .default_width(400)
        .default_height(200)
        .decorated(false)
        .build();

    // CSS
    let css = CssProvider::new();
    css.load_from_data(
        "window { background-color: rgba(25, 25, 25, 0.9); border-radius: 12px; }
         scrolledwindow { background-color: transparent; }
         textview { background-color: transparent; color: #dcdcdc; padding: 16px; }
         textview text { background-color: transparent; }"
    );
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().unwrap(),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    // Text input
    let text_view = TextView::builder()
        .wrap_mode(gtk4::WrapMode::Word)
        .build();
    let scrolled = ScrolledWindow::builder()
        .child(&text_view)
        .vexpand(true)
        .hexpand(true)
        .build();
    window.set_child(Some(&scrolled));

    // State
    let state = Rc::new(RefCell::new(AppState::Input));

    // AI channels
    let (query_tx, query_rx) = mpsc::channel::<(String, Vec<(String, String)>)>();
    let (response_tx, response_rx) = mpsc::channel::<String>();

    // AI worker thread
    std::thread::spawn(move || {
        while let Ok((question, memos)) = query_rx.recv() {
            let result = match crate::ai::load_api_key() {
                Some(key) => {
                    let client = AiClient::new(key);
                    match client.query(&question, &memos) {
                        Ok(r) => r,
                        Err(e) => e,
                    }
                }
                None => "API 키가 설정되지 않았습니다. /api-key 로 설정해주세요.".to_string(),
            };
            let _ = response_tx.send(result);
        }
    });

    // Poll AI responses
    let ai_state = state.clone();
    let ai_text_view = text_view.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        if let Ok(response) = response_rx.try_recv() {
            ai_text_view.buffer().set_text(&response);
            ai_text_view.set_editable(false);
            ai_text_view.set_cursor_visible(false);
            *ai_state.borrow_mut() = AppState::AiResult;
        }
        glib::ControlFlow::Continue
    });

    // --- Save and hide (ESC / focus lost) ---
    let save_db = db.clone();
    let save_window = window.clone();
    let save_text_view = text_view.clone();
    let save_state = state.clone();
    let save_and_hide = Rc::new(move || {
        let current = *save_state.borrow();
        match current {
            AppState::Input => {
                let buffer = save_text_view.buffer();
                let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
                let trimmed = text.trim();
                if !trimmed.is_empty() && !trimmed.starts_with('/') {
                    let _ = save_db.save_memo(trimmed);
                }
                buffer.set_text("");
                save_window.set_visible(false);
            }
            AppState::ApiKeyInput => {
                save_text_view.buffer().set_text("");
                save_text_view.set_editable(true);
                save_text_view.set_cursor_visible(true);
                *save_state.borrow_mut() = AppState::Input;
                save_window.set_visible(false);
            }
            _ => {
                save_text_view.buffer().set_text("");
                save_text_view.set_editable(true);
                save_text_view.set_cursor_visible(true);
                *save_state.borrow_mut() = AppState::Input;
                save_window.set_visible(false);
            }
        }
    });

    // --- Enter handler for / commands ---
    let enter_db = db.clone();
    let enter_text_view = text_view.clone();
    let enter_state = state.clone();
    let key_controller = gtk4::EventControllerKey::new();
    let esc_handler = save_and_hide.clone();
    key_controller.connect_key_pressed(move |_, key, _, _| {
        if key == gtk4::gdk::Key::Escape {
            esc_handler();
            return glib::Propagation::Stop;
        }

        if key == gtk4::gdk::Key::Return || key == gtk4::gdk::Key::KP_Enter {
            let current = *enter_state.borrow();

            if current == AppState::Input {
                let buffer = enter_text_view.buffer();
                let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
                let trimmed = text.trim().to_string();

                if trimmed == "/api-key" {
                    buffer.set_text("API 키를 입력하세요:\n");
                    // Place cursor at end
                    let end = buffer.end_iter();
                    buffer.place_cursor(&end);
                    *enter_state.borrow_mut() = AppState::ApiKeyInput;
                    return glib::Propagation::Stop;
                }

                if trimmed == "/list" {
                    let memos = enter_db.get_recent_memos(30).unwrap_or_default();
                    if memos.is_empty() {
                        buffer.set_text("저장된 메모가 없습니다.");
                    } else {
                        let mut display = String::new();
                        for (content, created_at) in &memos {
                            let date = if created_at.len() >= 16 {
                                &created_at[5..16]
                            } else {
                                created_at.as_str()
                            };
                            display.push_str(&format!("[{}]\n{}\n\n", date, content));
                        }
                        buffer.set_text(display.trim_end());
                    }
                    enter_text_view.set_editable(false);
                    enter_text_view.set_cursor_visible(false);
                    *enter_state.borrow_mut() = AppState::Archive;
                    return glib::Propagation::Stop;
                }

                if trimmed.starts_with("/ai ") {
                    let question = trimmed[4..].to_string();
                    let memos = enter_db.get_recent_memos(50).unwrap_or_default();
                    buffer.set_text("생각 중...");
                    enter_text_view.set_editable(false);
                    enter_text_view.set_cursor_visible(false);
                    *enter_state.borrow_mut() = AppState::AiWaiting;
                    let _ = query_tx.send((question, memos));
                    return glib::Propagation::Stop;
                }
            }

            if current == AppState::ApiKeyInput {
                let buffer = enter_text_view.buffer();
                let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
                let key_text = text.lines().last().unwrap_or("").trim().to_string();
                if !key_text.is_empty() && key_text != "API 키를 입력하세요:" {
                    match crate::ai::save_api_key(&key_text) {
                        Ok(_) => buffer.set_text("API 키가 저장되었습니다."),
                        Err(e) => buffer.set_text(&format!("저장 실패: {}", e)),
                    }
                    enter_text_view.set_editable(false);
                    enter_text_view.set_cursor_visible(false);
                    *enter_state.borrow_mut() = AppState::AiResult;
                    return glib::Propagation::Stop;
                }
            }
        }

        glib::Propagation::Proceed
    });
    window.add_controller(key_controller);

    // Focus lost: just hide, keep text
    window.connect_is_active_notify(move |w| {
        if !w.is_active() && w.is_visible() {
            w.set_visible(false);
        }
    });

    // Socket toggle
    if let Some(sock_listener) = listener {
        let window_toggle = window.clone();
        let text_view_focus = text_view.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            if sock_listener.accept().is_ok() {
                if window_toggle.is_visible() {
                    window_toggle.set_visible(false);
                } else {
                    window_toggle.set_visible(true);
                    window_toggle.present();
                    text_view_focus.grab_focus();
                }
            }
            glib::ControlFlow::Continue
        });
    }

    // Global shortcut
    let window_shortcut = window.clone();
    let text_view_shortcut = text_view.clone();
    setup_global_shortcut(window_shortcut, text_view_shortcut);

    // Start
    window.present();
    text_view.grab_focus();
}

fn setup_global_shortcut(window: ApplicationWindow, text_view: TextView) {
    let (tx, rx) = mpsc::channel::<()>();

    std::thread::spawn(move || {
        if let Err(e) = futures_lite::future::block_on(shortcut_loop(tx)) {
            eprintln!("Global shortcut failed: {}", e);
        }
    });

    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        if rx.try_recv().is_ok() {
            if window.is_visible() {
                window.set_visible(false);
            } else {
                window.set_visible(true);
                window.present();
                text_view.grab_focus();
            }
        }
        glib::ControlFlow::Continue
    });
}

async fn shortcut_loop(tx: mpsc::Sender<()>) -> ashpd::Result<()> {
    use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
    use ashpd::WindowIdentifier;
    use futures_lite::StreamExt;

    let proxy = GlobalShortcuts::new().await?;
    let session = proxy.create_session().await?;

    let shortcuts = &[
        NewShortcut::new("toggle", "Toggle NoTePaD")
            .preferred_trigger(Some("CTRL+SHIFT+Space")),
    ];
    let _ = proxy.bind_shortcuts(&session, shortcuts, &WindowIdentifier::default()).await?;

    eprintln!("[NoTePaD] Global shortcut registered.");

    let mut stream = proxy.receive_activated().await?;
    while let Some(_) = stream.next().await {
        let _ = tx.send(());
    }

    Ok(())
}
