use mikrotik_rs::{CommandBuilder, Event, ReplyResponse};

use crate::rwispros::connection::RouterClient;
use crate::rwispros::errors::{Result, RouterOsError};
use crate::rwispros::types::{Identity, Resources};

impl RouterClient {
    pub async fn get_identity(&self) -> Result<Identity> {
        let replies = self.run_read_command("/system/identity/print").await?;
        let reply = first_reply(&replies, "/system/identity/print")?;

        Ok(Identity {
            name: attr(reply, "name").unwrap_or_default(),
            revision: attr(reply, "revision"),
        })
    }

    pub async fn get_resources(&self) -> Result<Resources> {
        let replies = self.run_read_command("/system/resource/print").await?;
        let reply = first_reply(&replies, "/system/resource/print")?;

        Ok(Resources {
            uptime: attr(reply, "uptime"),
            version: attr(reply, "version"),
            board_name: attr(reply, "board-name"),
            architecture_name: attr(reply, "architecture-name"),
            cpu: attr(reply, "cpu"),
            cpu_count: parse_u32(attr(reply, "cpu-count")),
            cpu_frequency: parse_u32(attr(reply, "cpu-frequency")),
            cpu_load: parse_u32(attr(reply, "cpu-load")),
            free_memory: parse_u64(attr(reply, "free-memory")),
            total_memory: parse_u64(attr(reply, "total-memory")),
            free_hdd_space: parse_u64(attr(reply, "free-hdd-space")),
            total_hdd_space: parse_u64(attr(reply, "total-hdd-space")),
        })
    }

    async fn run_read_command(&self, command_path: &str) -> Result<Vec<ReplyResponse>> {
        let command = CommandBuilder::new().command(command_path).build();
        let mut receiver = self.device().send_command(command).await?;
        let mut replies = Vec::new();

        while let Some(event) = receiver.recv().await {
            match event {
                Event::Reply { response, .. } => replies.push(response),
                Event::Done { .. } | Event::Empty { .. } => return Ok(replies),
                Event::Trap { response, .. } => {
                    return Err(RouterOsError::Command {
                        message: response.message,
                    });
                }
                Event::Fatal { reason } => {
                    return Err(RouterOsError::Protocol { message: reason });
                }
            }
        }

        Err(RouterOsError::ConnectionClosed)
    }
}

fn first_reply<'a>(replies: &'a [ReplyResponse], command_path: &str) -> Result<&'a ReplyResponse> {
    replies.first().ok_or_else(|| RouterOsError::Command {
        message: format!("{command_path} returned no data"),
    })
}

fn attr(reply: &ReplyResponse, key: &str) -> Option<String> {
    reply.attributes.get(key).cloned().flatten()
}

fn parse_u32(value: Option<String>) -> Option<u32> {
    value.and_then(|value| value.trim().parse().ok())
}

fn parse_u64(value: Option<String>) -> Option<u64> {
    value.and_then(|value| value.trim().parse().ok())
}
