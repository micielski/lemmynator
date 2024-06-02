use std::sync::{Arc, Mutex};

use lemmy_api_common::{
    person::{Login, LoginResponse},
    sensitive::Sensitive,
};
use ln_config::Config;
use ratatui_image::picker::Picker;
use reqwest::{
    header::{HeaderMap, HeaderValue},
    Client,
};

use crate::{
    action::{event_to_action, Action, Mode},
    tui::Tui,
    ui::{components::Component, main_ui::MainWindow},
};

use anyhow::Result;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

pub struct App {
    should_quit: bool,
    action_tx: UnboundedSender<Action>,
    action_rx: UnboundedReceiver<Action>,
    main_window: MainWindow,
    mode: Mode,
}

pub struct Ctx {
    pub action_tx: UnboundedSender<Action>,
    pub client: Client,
    pub picker: Mutex<Picker>,
    pub config: Config,
}

impl App {
    pub async fn new(config: Config) -> Result<Self> {
        let (action_tx, action_rx) = mpsc::unbounded_channel();
        let user_agent = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

        let client = Client::builder().user_agent(user_agent).build()?;

        let login_req = Login {
            username_or_email: Sensitive::new(config.connection.username.clone()),
            password: Sensitive::new(config.connection.password.clone()),
            ..Default::default()
        };

        let res: LoginResponse = client
            .post(format!(
                "https://{}/api/v3/user/login",
                config.connection.instance
            ))
            .json(&login_req)
            .send()
            .await?
            .json()
            .await?;

        let mut header_map = HeaderMap::new();
        header_map.insert(
            reqwest::header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", &res.jwt.as_ref().unwrap()[..]))?,
        );
        let client = Client::builder()
            .user_agent(user_agent)
            .default_headers(header_map)
            .build()?;

        let mut picker = Picker::from_termios().unwrap();
        picker.guess_protocol();

        let ctx = Arc::new(Ctx {
            action_tx: action_tx.clone(),
            client,
            picker: Mutex::new(picker),
            config,
        });

        Ok(Self {
            should_quit: false,
            main_window: MainWindow::new(Arc::clone(&ctx)).await?,
            action_tx,
            action_rx,
            mode: Mode::Normal,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut tui = Tui::new()?;

        tui.enter()?;

        self.render(&mut tui)?;
        self.main_loop(&mut tui).await?;

        tui.exit()?;
        Ok(())
    }

    async fn main_loop(&mut self, tui: &mut Tui) -> Result<()> {
        loop {
            let tui_event = tui.next();
            let action = self.action_rx.recv();

            tokio::select! {
                event = tui_event => {
                    if let Some(action) = event_to_action(self.mode, event.unwrap()) {
                        if let Some(action) = self.update(action) {
                            self.action_tx.send(action).unwrap();
                        }
                    };
                },

                action = action => {
                    if let Some(action) = action {
                        if action.is_render() {
                            self.render(tui)?;
                        } else if let Some(action) = self.update(action) {
                            self.action_tx.send(action).unwrap();
                        }
                    }
                }
            }

            if self.should_quit {
                break Ok(());
            }
        }
    }

    fn render(&mut self, tui: &mut Tui) -> Result<()> {
        tui.terminal.draw(|f| {
            self.main_window.render(f, f.size());
        })?;
        Ok(())
    }

    #[must_use]
    fn update(&mut self, action: Action) -> Option<Action> {
        use Action as A;
        match &action {
            A::Quit => {
                self.should_quit = true;
                None
            }

            A::Render => Some(A::Render),

            A::SwitchToInputMode => {
                self.mode = Mode::Input;
                Some(A::Render)
            }

            A::SwitchToNormalMode => {
                self.mode = Mode::Normal;
                Some(A::Render)
            }

            _ => self.main_window.handle_actions(action),
        }
    }
}
