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
    guild::Permissions,
    http::interaction::{InteractionResponse, InteractionResponseType},
};
use twilight_util::builder::{
    command::CommandBuilder,
    embed::{EmbedBuilder, EmbedFieldBuilder},
    InteractionResponseDataBuilder,
};

use crate::{randstring, upload::upload_raw, AppState, Error};

const UPLOAD_COMMAND_NAME: &str = "Save Attached Images";

struct Response {
    description: String,
    fields: Vec<(String, String)>,
}

impl Response {
    pub fn new(description: String) -> Self {
        Self {
            description,
            fields: Vec::new(),
        }
    }

    pub fn add_field(&mut self, title: String, content: String) {
        self.fields.push((title, content));
    }

    pub fn interaction_response(self) -> InteractionResponse {
        let mut embed = EmbedBuilder::new().description(self.description);
        for (title, content) in self.fields {
            let field = EmbedFieldBuilder::new(title, content).inline().build();
            embed = embed.field(field);
        }
        let embed = embed.build();

        let data = InteractionResponseDataBuilder::new()
            .flags(MessageFlags::EPHEMERAL)
            .embeds([embed])
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

#[instrument(skip(state))]
async fn upload_attachments(state: AppState, interaction: Interaction) -> Result<Response, Error> {
    if !interaction.guild_id.is_some_and(|g| g == state.guild)
        || !interaction
            .app_permissions
            .is_some_and(|p| p.contains(Permissions::MODERATE_MEMBERS))
    {
        return Ok(Response::new("There's no good way to enforce that only users who have permissions to use a user command can use it, so this has been disabled for now outside the main server.".to_string()));
    }
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
    let mut uploaded = 0;

    while let Some(res) = set.join_next().await {
        if let Ok(Ok(_)) = res {
            uploaded += 1;
        } else {
            failures += 1;
        }
        match res {
            Err(source) => error!(?source, "S3 uploader panicked"),
            Ok(Err(source)) => error!(?source, "S3 uploader failed"),
            Ok(Ok(())) => {}
        };
    }

    let content = if uploaded == 0 {
        "Found no attachments".to_string()
    } else {
        format!(
            "Uploaded {uploaded} attachments: <{}/{upload_id}/>",
            state.root_url
        )
    };

    let mut response = Response::new(content);
    if skipped_ctype != 0 {
        response.add_field("Skipped".to_string(), skipped_ctype.to_string());
    }
    if failures != 0 {
        response.add_field("Failed".to_string(), failures.to_string());
    }

    Ok(response)
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
