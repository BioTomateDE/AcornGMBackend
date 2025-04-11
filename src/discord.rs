use serenity;
use serenity::all::{GatewayIntents, Message};
use serenity::Client;
use serenity::client::{Context, EventHandler};

struct Handler;

#[serenity::async_trait]
impl EventHandler for Handler {
    // async fn message(&self, context: Context, msg: Message) {
    //     if msg.content == "!ping" {
    //         let _ = msg.channel_id.say(&context, "Pong!");
    //     }
    // }
}

pub async fn initialize_discord_app() -> Result<Client, String> {
    let discord_token: String = match std::env::var("DISCORD_APP_TOKEN") {
        Ok(token) => token,
        Err(error) => return Err(format!("Could not load Discord Token from env: {error}")),
    };

    let client: Client = match Client::builder(discord_token, GatewayIntents::default())
        .event_handler(Handler)
        .await {
        Ok(client) => client,
        Err(error) => return Err(format!("Could not initialize Discord Application: {error}"))
    };

    client.
}
