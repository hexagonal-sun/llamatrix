use std::{
    collections::HashMap,
    fs::{self, File},
    path::PathBuf,
    time::Duration,
};

use anyhow::{Context, Result};
use clap::Parser;
use llama::Chat;
use log::{error, warn};
use matrix_sdk::{
    Client, Room, ServerName,
    config::SyncSettings,
    event_handler::Ctx,
    matrix_auth::MatrixSession,
    ruma::{
        OwnedRoomId, UserId,
        events::room::{
            member::StrippedRoomMemberEvent,
            message::{MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent},
        },
    },
};
use reqwest::Url;
use tokio::{
    select,
    sync::{
        mpsc::{self, Receiver},
        oneshot::{self, channel},
    },
    time::sleep,
};

mod llama;

#[derive(Parser)]
/// An ollama bridge bot for Matrix
struct Args {
    /// The Matrix username of the account that the bot should use.
    #[clap(long, short)]
    username: String,

    /// The password of the Matrix account.
    #[clap(long, short)]
    password: String,

    /// The homeserver upon which the Matrix acounts resides.
    #[clap(long, short = 's', default_value = "matrix.org")]
    homeserver: String,

    /// The LLM to use with ollama.
    #[clap(long, short)]
    model: String,

    /// The URL of the ollama server
    #[clap(long, short = 'o', default_value = "http://localhost:11434", value_parser = Url::parse)]
    url: Url,
}

fn get_data_dir() -> PathBuf {
    dirs::data_dir().unwrap().join("llamatrix")
}

enum LlamaReq {
    Chat(LlamaChatReq),
    ClrCtx(OwnedRoomId),
}

struct LlamaChatReq {
    room_id: OwnedRoomId,
    prompt: String,
    reply_tx: oneshot::Sender<String>,
}

impl LlamaChatReq {
    fn new(room_id: OwnedRoomId, prompt: impl ToString) -> (LlamaReq, oneshot::Receiver<String>) {
        let (tx, rx) = channel();
        (
            LlamaReq::Chat(Self {
                room_id,
                prompt: prompt.to_string(),
                reply_tx: tx,
            }),
            rx,
        )
    }
}

async fn llama_task(mut rx: Receiver<LlamaReq>, url: Url, model: String) {
    let mut state: HashMap<OwnedRoomId, Chat> = HashMap::new();

    loop {
        match rx.recv().await {
            Some(LlamaReq::Chat(chat_req)) => {
                let mut chat = state
                    .remove(&chat_req.room_id)
                    .unwrap_or_else(|| Chat::new(model.clone(), url.clone()));

                match chat.message(chat_req.prompt).await {
                    Ok(resp) => {
                        chat_req.reply_tx.send(resp).unwrap();
                    }
                    Err(e) => {
                        error!("Failed to generate response from ollama: {}", e);
                    }
                }
                state.insert(chat_req.room_id, chat);
            }
            Some(LlamaReq::ClrCtx(rm)) => {
                state.remove(&rm);
            }
            None => {
                return;
            }
        }
    }
}

async fn accept_invites(evt: StrippedRoomMemberEvent, client: Client, rm: Room) {
    dbg!(&evt);
    if evt.state_key != client.user_id().unwrap() {
        return;
    }

    if let Err(e) = rm.join().await {
        warn!(
            "Failed to join invited room: {} ({})",
            rm.room_id(),
            e.to_string()
        );

        let _ = rm.leave().await;
    }
}

async fn handle_msg_event(
    evt: OriginalSyncRoomMessageEvent,
    rm: Room,
    client: Client,
    ctx: Ctx<mpsc::Sender<LlamaReq>>,
) {
    // Don't respond to our own messages.
    if evt.sender == client.user_id().unwrap() {
        return;
    }

    match evt.content.msgtype {
        MessageType::Text(txt) => {
            let matched = txt.body.strip_prefix("!llama");

            if !rm.is_direct().await.unwrap() && !matched.is_some() {
                return;
            }

            let prompt = matched.unwrap_or_else(|| txt.body.as_str());

            if prompt == "!llamaclear" {
                ctx.send(LlamaReq::ClrCtx(rm.room_id().into()))
                    .await
                    .unwrap();

                rm.send(RoomMessageEventContent::text_plain("Context cleared"))
                    .await
                    .unwrap();

                return;
            }

            let _ = rm.typing_notice(true).await;

            let (req, mut rx) = LlamaChatReq::new(rm.room_id().into(), prompt);

            ctx.send(req).await.unwrap();

            loop {
                select! {
                _ = sleep(Duration::from_secs(3)) => {
                    let _ = rm.typing_notice(true).await;
                },
                resp = &mut rx => {
                    let _ = rm.typing_notice(false).await;
                    rm.send(RoomMessageEventContent::text_plain(resp.unwrap()))
                      .await
                      .unwrap();
                    break;
                }
                }
            }
        }
        _ => {
            warn!("Could not reply to non-text based message");
        }
    }
}

fn write_session(session: MatrixSession) -> Result<()> {
    let f =
        File::create(get_data_dir().join("session")).context("Could not create session file")?;

    Ok(serde_json::to_writer(f, &session)?)
}

fn read_session() -> Option<MatrixSession> {
    let f = File::open(get_data_dir().join("session")).ok()?;

    serde_json::from_reader(f).ok()?
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    let server = ServerName::parse(args.homeserver).context("Could not parse homeserver")?;

    let userid = UserId::parse_with_server_name(args.username, &server)
        .context("could not parse user ID")?;

    fs::create_dir_all(get_data_dir()).context("Could not create data dir")?;

    let client = Client::builder()
        .server_name(&server)
        .sqlite_store(get_data_dir().join("db"), None)
        .build()
        .await?;

    match read_session() {
        Some(session) => client
            .restore_session(session)
            .await
            .context("Failed to restore session")?,
        None => {
            let response = client
                .matrix_auth()
                .login_username(userid, &args.password)
                .send()
                .await
                .context("Failed to login")?;

            write_session((&response).into())?;
        }
    }

    let (tx, rx) = mpsc::channel(1024);

    tokio::spawn(llama_task(rx, args.url, args.model));

    client.add_event_handler(accept_invites);

    let token = client
        .sync_once(SyncSettings::default().timeout(Duration::from_millis(500)))
        .await?;

    client.add_event_handler_context(tx);
    client.add_event_handler(handle_msg_event);

    client
        .sync(SyncSettings::default().token(token.next_batch))
        .await?;

    Ok(())
}
