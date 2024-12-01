use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Message {
    role: Role,
    content: String,
}

pub struct Chat {
    ctx: ChatCtx,
    client: Client,
    url: Url,
}

#[derive(Serialize)]
pub struct ChatCtx {
    model: String,
    stream: bool,
    messages: Vec<Message>,
}

#[derive(Deserialize, Debug)]
pub struct ChatResponse {
    message: Message,
}

impl Chat {
    pub fn new(model: impl ToString, url: Url) -> Self {
        Self {
            client: Client::new(),
            ctx: ChatCtx {
                model: model.to_string(),
                messages: Vec::new(),
                stream: false,
            },
            url,
        }
    }

    pub async fn message(&mut self, prompt: impl ToString) -> anyhow::Result<String> {
        self.ctx.messages.push(Message {
            role: Role::User,
            content: prompt.to_string(),
        });

        let resp = self
            .client
            .post(self.url.join("/api/chat").unwrap())
            .json(&self.ctx)
            .send()
            .await?
            .json::<ChatResponse>()
            .await?;

        assert_eq!(resp.message.role, Role::Assistant);

        let response = resp.message.content.clone();

        self.ctx.messages.push(resp.message);

        Ok(response)
    }
}
