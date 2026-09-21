use anyhow::Result;

use crate::exit_codes;
use crate::global_config::GlobalConfig;
use crate::message_box;

pub async fn run(username: Option<String>, reset: bool) -> Result<()> {
    if reset {
        GlobalConfig::update(|c| c.user = None).await?;
        message_box::MessageBox::new()
            .info()
            .line("User reset. Per-user keys will use the OS user.")
            .print_plain_stderr();
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
        message_box::MessageBox::new()
            .info()
            .line(format!("User set to {user}"))
            .print_plain_stderr();
        return Ok(());
    }
    let config = GlobalConfig::load().await?;
    if let Some(user) = config.user {
        println!("{}", user);
    } else {
        message_box::MessageBox::new()
            .info()
            .line("No user set. Lade will use the current OS user.")
            .print_plain_stderr();
    }
    Ok(())
}
