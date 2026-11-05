use std::io::{ErrorKind, Write};

use rpassword::ConfigBuilder;
use scorarium_engine::Engine;
use scorarium_engine::auth::PasswordState;

/// Prompt for a new password (with confirmation) and replace the current one with it.
///
/// The prompts read from and draw on the controlling terminal rather than stdin and stdout, so
/// neither the password nor its feedback can reach the shell's output or history.
pub(crate) async fn set(engine: &Engine, out: &mut impl Write) -> eyre::Result<()> {
    if engine.password_state().await? == PasswordState::NotNeeded {
        eyre::bail!("the demo has no password to set");
    }
    let entered = tokio::task::spawn_blocking(|| {
        let prompt = |text: &str| {
            let config = ConfigBuilder::new().password_feedback_mask('*').build();
            rpassword::prompt_password_with_config(text, config)
        };
        let password = prompt("New password: ")?;
        // An empty password is refused without asking for a confirmation.
        if password.is_empty() {
            return Ok((password, String::new()));
        }
        let confirmation = prompt("Confirm password: ")?;
        std::io::Result::Ok((password, confirmation))
    })
    .await?;
    let (password, confirmation) = match entered {
        Ok(entered) => entered,
        // Ctrl-D on an empty prompt is end of input, and ctrl-c is an interrupt.
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::UnexpectedEof | ErrorKind::Interrupted
            ) =>
        {
            writeln!(out, "cancelled")?;
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    if password.is_empty() {
        eyre::bail!("the password must not be empty");
    }
    if password != confirmation {
        eyre::bail!("the passwords do not match");
    }
    engine.replace_password(&password).await?;
    writeln!(out, "Password set.")?;
    Ok(())
}
