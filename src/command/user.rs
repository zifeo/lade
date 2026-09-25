use anyhow::Result;

use crate::exit_codes;
use crate::global_config::GlobalConfig;
use crate::message_box::{self, Report};

pub async fn run(username: Option<String>, reset: bool) -> Result<()> {
    if reset {
        GlobalConfig::update(|c| c.user = None).await?;
        Report::new()
            .line("User reset. Per-user keys will use the OS user.")
            .print();
        return Ok(());
    }
    if let Some(user) = username {
        if user.is_empty() {
            message_box::MessageBox::new()
                .error()
                .line("No user provided.")
                .print_stderr();
            std::process::exit(exit_codes::FAILURE);
        }
        GlobalConfig::update(|c| c.user = Some(user.clone())).await?;
        Report::new().line(format!("User set to {user}")).print();
        return Ok(());
    }
    let config = GlobalConfig::load().await?;
    if let Some(user) = config.user {
        println!("{}", user);
    } else {
        Report::new()
            .line("No user set. Lade will use the current OS user.")
            .print();
    }
    Ok(())
}
