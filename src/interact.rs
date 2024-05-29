use std::sync::Arc;

use serde_json::json;
use tokio::task::JoinSet;
use twilight_http::{request::Request, routing::Route};
use twilight_model::{
    application::{
        command::{Command, CommandType},
        interaction::{Interaction, InteractionData, InteractionType},
    },
    channel::message::MessageFlags,
    http::interaction::{InteractionResponse, InteractionResponseType},
};
use twilight_util::builder::{command::CommandBuilder, InteractionResponseDataBuilder};

use crate::{randstring, upload::upload_raw, AppState, Error};

const UPLOAD_COMMAND_NAME: &str = "Save Attached Images";

struct Response {
    content: String,
}

impl Response {
    pub fn new(content: String) -> Self {
        Self { content }
    }

    pub fn interaction_response(self) -> InteractionResponse {
        let data = InteractionResponseDataBuilder::new()
            .flags(MessageFlags::EPHEMERAL)
            .content(self.content)
            .build();
        InteractionResponse {
            kind: InteractionResponseType::ChannelMessageWithSource,
            data: Some(data),
        }
    }
}

pub async fn interact(state: AppState, interaction: Interaction) -> InteractionResponse {
    match interaction.kind {
        InteractionType::Ping => InteractionResponse {
            kind: InteractionResponseType::Pong,
            data: None,
        },
        InteractionType::ApplicationCommand => {
            command(state, interaction).await.interaction_response()
        }
        _ => unsupported(),
    }
}

fn unsupported() -> InteractionResponse {
    Response::new("Unsupported interaction kind".to_string()).interaction_response()
}

async fn command(state: AppState, interaction: Interaction) -> Response {
    upload_attachments(state, interaction)
        .await
        .unwrap_or_else(|source| {
            error!(?source, "Failed to process interaction");
            Response::new(format!("Failed to process your request: {source}"))
        })
}

#[instrument(skip(state))]
async fn upload_link(
    state: AppState,
    id: Arc<str>,
    upload_seq: u64,
    url: String,
) -> Result<(), Error> {
    let data = state.http.get(url).send().await?.bytes().await?;
    upload_raw(state, id.as_ref(), upload_seq, data).await?;
    Ok(())
}

#[instrument(skip_all)]
async fn upload_attachments(state: AppState, interaction: Interaction) -> Result<Response, Error> {
    let Some(InteractionData::ApplicationCommand(data)) = interaction.data else {
        return Err(Error::MissingCommandData);
    };
    let target = data.target_id.ok_or(Error::MissingTarget)?;
    let resolved = data.resolved.ok_or(Error::NoResolvedData)?;
    let message = resolved
        .messages
        .get(&target.cast())
        .ok_or(Error::MessageNotFound)?;

    let mut set = JoinSet::new();

    let upload_id: Arc<str> = randstring(16).into();
    let mut skipped_ctype = 0;

    for (upload_seq, attachment) in message
        .attachments
        .iter()
        .filter(|v| {
            let is_image = v.content_type.as_ref().is_some_and(|v| v.contains("image"));
            if !is_image {
                skipped_ctype += 1;
            }
            is_image
        })
        .enumerate()
    {
        let state = state.clone();
        let id = upload_id.clone();
        set.spawn(upload_link(
            state,
            id,
            upload_seq as u64,
            attachment.url.clone(),
        ));
    }

    let mut failures = 0;
    while let Some(res) = set.join_next().await {
        if res.is_err() || res.as_ref().is_ok_and(|output| output.is_err()) {
            failures += 1;
        }
        match res {
            Err(source) => error!(?source, "S3 uploader panicked"),
            Ok(Err(source)) => error!(?source, "S3 uploader failed"),
            Ok(Ok(())) => {}
        };
    }

    Ok(Response::new(format!(
        "Uploaded attachments: {}/{upload_id}/ ({failures} failed. {skipped_ctype} skipped.)",
        state.root_url
    )))
}

#[instrument(skip_all)]
pub async fn register_commands(state: &AppState) -> Result<(), Error> {
    // This horribleness brought to you by Advaith and Discord's fucking horrendous GA policies
    let command_struct = CommandBuilder::new(UPLOAD_COMMAND_NAME, "", CommandType::Message).build();
    let mut command_value = serde_json::to_value(command_struct)?;
    let Some(cmd_value_object) = command_value.as_object_mut() else {
        unreachable!("Serializing a struct and getting not-a-map should be impossible");
    };

    cmd_value_object.insert("integration_types".to_string(), json!([1]));
    cmd_value_object.insert("contexts".to_string(), json!([0, 1, 2]));

    let request = Request::builder(&Route::SetGlobalCommands {
        application_id: state.discord.application_id.get(),
    })
    .json(&json!([cmd_value_object]))
    .build()?;
    state
        .discord
        .client
        .request::<Vec<Command>>(request)
        .await?;
    Ok(())
}
